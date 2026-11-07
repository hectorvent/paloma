//! Webhook configuration and local receive tools share validation and secret handling.

use std::path::PathBuf;

use super::util::{SecretSource, read_secret};
use crate::app::{App, no_id};
use crate::output::Output;
use clap::{Args, Subcommand, ValueEnum};
use paloma_core::{Error, ExitCode, Result, limits};
use paloma_graph::api::webhook::{self, AVAILABLE_FIELDS};
use paloma_graph::callback::{self, Probe, Url};
use paloma_graph::request::redact_url;
use paloma_graph::webhook_samples::{self, SAMPLE_EVENTS, SampleContext};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
mod fields;
mod listen;
mod probe;
mod version;

use fields::Fields;
use probe::{SimulateArgs, VerifyUrlArgs};

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
                    let phone = app.phone()?;
                    webhook::number_callback(
                        phone,
                        callback_url.as_deref().unwrap_or(""),
                        token.as_ref().map(ExposeSecret::expose_secret),
                    )
                } else {
                    let waba = app.waba()?;
                    match (&callback_url, &token) {
                        (Some(url), Some(token)) => {
                            webhook::subscribe(waba, url, token.expose_secret())
                        }
                        _ => webhook::subscribe_default(waba),
                    }
                };
                if !app.confirm_or_abort(if number {
                    "change the phone-number webhook callback"
                } else {
                    webhook::SUBSCRIBE_CONFIRM
                })? {
                    return Ok(());
                }
                app.send_and_emit(&request, no_id).await
            }
            Self::Unsubscribe => {
                let waba = app.waba()?;
                if !app.confirm_or_abort(&format!("unsubscribe this app from WABA {waba}"))? {
                    return Ok(());
                }
                app.send_and_emit(&webhook::unsubscribe(waba), no_id).await
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
    webhook::check_callback_url(url, "--callback-url")
}
