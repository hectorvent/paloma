//! Webhook configuration and local receive tools share validation and secret handling.

use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use paloma_core::{Error, ExitCode, Result, limits, validate};
use paloma_graph::api::webhook::{self, AVAILABLE_FIELDS};
use paloma_graph::callback::{self, Probe, Url};
use paloma_graph::request::redact_url;
use paloma_graph::webhook_samples::{self, SAMPLE_EVENTS, SampleContext};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};

use super::util::{SecretSource, read_secret};
use crate::app::{App, no_id};
use crate::output::Output;

mod listen;
mod version;

#[derive(Debug, Subcommand)]
pub enum WebhookCommand {
    /// Show the effective callback URL and subscribed apps.
    Get,
    /// Subscribe the app, or set/remove a number-level alternate callback.
    Subscribe {
        /// HTTPS endpoint Meta will call.
        #[arg(long)]
        callback_url: Option<String>,
        /// Manage the active phone number's alternate callback instead of the WABA.
        #[arg(long)]
        number: bool,
        #[command(flatten)]
        verify_token: VerifyTokenInput,
    },
    /// Remove this app's subscription (and override) from the WABA.
    Unsubscribe,
    /// Show, list or change the app-level webhook field subscriptions.
    Fields(Fields),
    /// Show or change the Graph version of the app's WhatsApp webhooks.
    Version(version::Version),
    /// Run a local webhook receiver that prints each event as a line of JSON.
    Listen(listen::ListenArgs),
    /// Run Meta's hub.challenge handshake against an endpoint and report pass or fail.
    VerifyUrl(VerifyUrlArgs),
    /// POST a signed sample webhook to an endpoint and report pass or fail.
    Simulate(SimulateArgs),
}

#[derive(Debug, Args)]
pub struct VerifyUrlArgs {
    /// The endpoint to check.
    url: String,
    #[command(flatten)]
    verify_token: VerifyTokenInput,
}

#[derive(Debug, Args)]
pub struct SimulateArgs {
    /// The endpoint to POST to.
    url: String,
    /// Which sample delivery to send.
    #[arg(
        long,
        default_value = "text",
        conflicts_with = "file",
        value_parser = clap::builder::PossibleValuesParser::new(SAMPLE_EVENTS)
    )]
    event: String,
    /// Send this JSON file instead of a sample, byte for byte.
    #[arg(short = 'f', long, value_name = "PATH")]
    file: Option<PathBuf>,
    /// valid: sign with the app secret. wrong and missing test that the endpoint rejects it,
    /// so they pass when it doesn't answer 200, and don't need the app secret.
    #[arg(long, value_enum, default_value_t)]
    signature: SignatureMode,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
enum SignatureMode {
    #[default]
    Valid,
    Wrong,
    Missing,
}

#[derive(Debug, Args)]
pub struct Fields {
    #[command(subcommand)]
    command: FieldsCommand,
}

/// The token echoed back during Meta's verification handshake: from
/// PALOMA_VERIFY_TOKEN, stdin or a prompt.
#[derive(Debug, Args)]
pub struct VerifyTokenInput {
    /// Read the verify token from stdin. Otherwise uses PALOMA_VERIFY_TOKEN, or prompts.
    #[arg(long)]
    verify_token_stdin: bool,
}

impl VerifyTokenInput {
    fn read(&self, app: &App) -> Result<SecretString> {
        read_secret(
            app,
            &SecretSource {
                what: "verify token",
                flag: None,
                stdin_flag: "--verify-token-stdin",
                env: "PALOMA_VERIFY_TOKEN",
                missing_is_auth: false,
            },
            None,
            self.verify_token_stdin,
        )
    }

    fn given(&self) -> bool {
        self.verify_token_stdin
    }
}

#[derive(Debug, Subcommand)]
enum FieldsCommand {
    /// Show the fields the app is subscribed to.
    Get,
    /// List every field that can be subscribed (offline).
    List,
    /// Replace the subscribed fields.
    Set {
        #[arg(required = true, value_name = "FIELD", value_parser = clap::builder::PossibleValuesParser::new(AVAILABLE_FIELDS))]
        fields: Vec<String>,
        /// Change the app-level callback URL at the same time.
        #[arg(long)]
        callback_url: Option<String>,
        #[command(flatten)]
        verify_token: VerifyTokenInput,
    },
}

impl WebhookCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        match self {
            Self::Get => {
                let ctx = app.ctx()?;
                let waba = ctx.require_waba_id()?.to_owned();
                let phone = ctx.require_phone_number_id()?.to_owned();
                let (apps_req, cfg_req) = (
                    webhook::subscribed_apps(&waba),
                    webhook::configuration(&phone),
                );
                if app.dry_run() {
                    return app.print_dry_run(&[apps_req, cfg_req]);
                }
                let apps = app.fetch(&apps_req).await?.json()?;
                let cfg = app.fetch(&cfg_req).await?.json()?;
                let override_uri = webhook::override_callback(&apps);
                let effective = webhook::effective_callback(&cfg, override_uri.as_deref());
                app.out.emit(
                    &Output::new(json!({
                        "effective_callback_url": effective,
                        "number_override_callback_uri": webhook::number_override_callback(&cfg),
                        "override_callback_uri": override_uri,
                        "subscribed_apps": apps.get("data").cloned().unwrap_or(Value::Array(vec![])),
                    }))
                    .id(effective),
                );
                Ok(())
            }
            Self::Subscribe {
                callback_url,
                number,
                verify_token,
            } => {
                if let Some(url) = &callback_url {
                    check_callback_url(url)?;
                } else if verify_token.given() {
                    return Err(Error::validation("a verify token requires --callback-url"));
                }
                let token = callback_url
                    .as_ref()
                    .map(|_| verify_token.read(app))
                    .transpose()?;
                let request = if number {
                    let phone = app.ctx()?.require_phone_number_id()?;
                    webhook::number_callback(
                        phone,
                        callback_url.as_deref().unwrap_or(""),
                        token.as_ref().map(ExposeSecret::expose_secret),
                    )
                } else {
                    let waba = app.ctx()?.require_waba_id()?;
                    match &callback_url {
                        Some(url) => webhook::subscribe(
                            waba,
                            url,
                            token
                                .as_ref()
                                .expect("callback token was read")
                                .expose_secret(),
                        ),
                        None => webhook::subscribe_default(waba),
                    }
                };
                if !app.confirm_or_abort(if number {
                    "change the phone-number webhook callback"
                } else {
                    "change the WABA webhook subscription and callback"
                })? {
                    return Ok(());
                }
                app.send_and_emit(&request, no_id).await
            }
            Self::Unsubscribe => {
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                if !app.confirm_or_abort(&format!("unsubscribe this app from WABA {waba}"))? {
                    return Ok(());
                }
                app.send_and_emit(&webhook::unsubscribe(&waba), no_id).await
            }
            Self::Fields(f) => f.run(app).await,
            Self::Version(v) => v.run(app).await,
            Self::Listen(args) => listen::run(args, app).await,
            Self::VerifyUrl(args) => args.run(app).await,
            Self::Simulate(args) => args.run(app).await,
        }
    }
}

/// Explicit callback changes must be usable by Meta before a token is requested.
fn check_callback_url(url: &str) -> Result<()> {
    validate::not_blank(url, "--callback-url")?;
    validate::max_chars(url, limits::MAX_WEBHOOK_CALLBACK_URL, "callback URL")?;
    if callback::parse_url(url)?.scheme() != "https" {
        return Err(Error::validation(format!(
            "--callback-url must be an https URL: {}",
            redact_url(url)
        )));
    }
    Ok(())
}

/// Meta needs a real certificate; plain HTTP is only good for trying things out locally.
fn warn_unless_https(app: &App, url: &Url) {
    if url.scheme() != "https" {
        app.out.warn(
            "this isn't HTTPS: Meta only calls endpoints with a valid TLS certificate, so this \
             check says nothing about the certificate",
        );
    }
}

impl VerifyUrlArgs {
    async fn run(self, app: &App) -> Result<()> {
        let url = callback::parse_url(&self.url)?;
        warn_unless_https(app, &url);
        let token = self.verify_token.read(app)?;
        let safe_url = redact_url(url.as_str());
        let challenge = callback::challenge()?;
        if app.dry_run() {
            app.out.note("dry-run: no request sent");
            app.out.emit(&Output::new(json!({
                "method": "GET",
                "url": safe_url,
                "query": {
                    "hub.mode": "subscribe",
                    "hub.challenge": challenge,
                    "hub.verify_token": "<redacted>",
                },
            })));
            return Ok(());
        }
        let result = Probe::new(app.globals.timeout)?
            .handshake(&url, token.expose_secret(), &challenge)
            .await?;
        app.out.emit(&Output::new(json!({
            "url": safe_url,
            "passed": result.passed(),
            "status": result.status,
            "challenge_echoed": result.echoed_challenge,
        })));
        if result.passed() {
            app.out.info("handshake passed");
            return Ok(());
        }
        app.out.warn(if result.status == 200 {
            "handshake failed: the endpoint answered 200, but the body isn't exactly the \
             hub.challenge value"
                .to_owned()
        } else {
            format!(
                "handshake failed: the endpoint answered {}. Meta needs 200 and the \
                 hub.challenge value, and redirects aren't followed",
                result.status
            )
        });
        Err(Error::Silent(ExitCode::Generic))
    }
}

impl SimulateArgs {
    async fn run(self, app: &App) -> Result<()> {
        let url = callback::parse_url(&self.url)?;
        warn_unless_https(app, &url);
        let file_body = self.file.as_deref().map(read_payload).transpose()?;
        let ctx = app.ctx()?;
        let (body, source) = match file_body {
            Some(body) => (body, "file"),
            None => {
                let sample = SampleContext {
                    waba_id: ctx
                        .waba_id
                        .clone()
                        .unwrap_or_else(|| webhook_samples::EXAMPLE_WABA_ID.into()),
                    phone_number_id: ctx
                        .phone_number_id
                        .clone()
                        .unwrap_or_else(|| webhook_samples::EXAMPLE_PHONE_NUMBER_ID.into()),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_secs()),
                    nonce: callback::nonce()?,
                };
                let payload = webhook_samples::payload(&self.event, &sample)
                    .ok_or_else(|| Error::validation(format!("unknown event: {}", self.event)))?;
                (payload.to_string().into_bytes(), self.event.as_str())
            }
        };
        let safe_url = redact_url(url.as_str());
        if app.dry_run() {
            app.out.note("dry-run: no request sent");
            let mut headers = json!({ "Content-Type": "application/json" });
            if self.signature != SignatureMode::Missing {
                headers[callback::SIGNATURE_HEADER] = "sha256=<computed when sent>".into();
            }
            let parsed: Value = serde_json::from_slice(&body).unwrap_or_default();
            // `-o raw` shows just the body, as for any dry run; the rest the whole request.
            let raw = matches!(app.out.format(), crate::cli::Format::Raw)
                .then(|| String::from_utf8_lossy(&body).into_owned());
            app.out.emit(&Output {
                raw,
                ..Output::new(json!({
                    "method": "POST",
                    "url": safe_url,
                    "headers": headers,
                    "body": parsed,
                }))
            });
            return Ok(());
        }
        let signature = match self.signature {
            SignatureMode::Valid => Some(callback::sign(
                ctx.require_app_secret()?.expose_secret(),
                &body,
            )),
            SignatureMode::Wrong => Some(callback::wrong_signature(&body)?),
            SignatureMode::Missing => None,
        };
        let status = Probe::new(app.globals.timeout)?
            .deliver(&url, body, signature.as_deref())
            .await?;
        let expects_200 = self.signature == SignatureMode::Valid;
        let passed = (status == 200) == expects_200;
        app.out.emit(&Output::new(json!({
            "url": safe_url,
            "event": source,
            "signature": self.signature.to_possible_value().map(|v| v.get_name().to_owned()),
            "status": status,
            "passed": passed,
        })));
        match (passed, expects_200) {
            (true, true) => app.out.info("delivered: the endpoint answered 200"),
            (true, false) if status >= 500 => app.out.warn(format!(
                "rejected, but with a server error ({status}): check it isn't crashing instead \
                 of checking the signature"
            )),
            (true, false) => app.out.info(format!(
                "rejected as it should be: the endpoint answered {status}"
            )),
            (false, true) => app.out.warn(format!(
                "the endpoint answered {status}. Meta treats anything but 200 as a failed \
                 delivery and retries it for up to 7 days"
            )),
            (false, false) => app.out.warn(
                "the endpoint answered 200 to a request with a bad signature: it isn't checking \
                 X-Hub-Signature-256",
            ),
        }
        if passed {
            Ok(())
        } else {
            Err(Error::Silent(ExitCode::Generic))
        }
    }
}

/// A custom payload, checked but unchanged: the signature covers these exact bytes.
fn read_payload(path: &std::path::Path) -> Result<Vec<u8>> {
    validate::readable_file(path)?;
    let body = std::fs::read(path)
        .map_err(|e| Error::validation(format!("could not read {}: {e}", path.display())))?;
    if body.len() > limits::MAX_WEBHOOK_PAYLOAD {
        return Err(Error::validation(format!(
            "{} is {} bytes; webhook payloads are at most {}",
            path.display(),
            body.len(),
            limits::MAX_WEBHOOK_PAYLOAD
        )));
    }
    callback::compact_json(&body)
        .map_err(|e| Error::validation(format!("{} is not valid JSON: {e}", path.display())))?;
    Ok(body)
}

impl Fields {
    async fn run(self, app: &App) -> Result<()> {
        match self.command {
            FieldsCommand::List => {
                if app.out.quiet() {
                    app.out
                        .stdout(&format!("{}\n", AVAILABLE_FIELDS.join("\n")));
                } else {
                    app.out
                        .emit(&Output::new(json!({ "fields": AVAILABLE_FIELDS })));
                }
                Ok(())
            }
            FieldsCommand::Get => {
                let app_id = super::upload::app_id(app).await?;
                app.send_and_emit(&webhook::app_subscriptions(&app_id), no_id)
                    .await
            }
            FieldsCommand::Set {
                fields,
                callback_url,
                verify_token,
            } => {
                if let Some(url) = &callback_url {
                    check_callback_url(url)?;
                }
                // Only a new callback URL uses a token, so a PALOMA_VERIFY_TOKEN that's merely
                // set in the environment isn't sent without one.
                let verify_token = if callback_url.is_some() {
                    Some(verify_token.read(app)?)
                } else if verify_token.given() {
                    return Err(Error::validation("a verify token needs --callback-url"));
                } else {
                    None
                };
                app.ensure_can_confirm("change the app's webhook subscriptions")?;
                let app_id = super::upload::app_id(app).await?;
                if !app.confirm_or_abort(&format!(
                    "subscribe app {app_id} to: {}",
                    fields.join(", ")
                ))? {
                    return Ok(());
                }
                let req = webhook::set_app_subscriptions(
                    &app_id,
                    &fields,
                    callback_url.as_deref(),
                    verify_token.as_ref().map(ExposeSecret::expose_secret),
                );
                app.send_and_emit(&req, no_id).await
            }
        }
    }
}
