//! `verify-url` and `simulate`: local checks of your own endpoint, not Graph calls, so
//! they honor `--dry-run` themselves instead of going through `App::send`.

use super::{
    App, Args, Error, ExitCode, ExposeSecret, Output, PathBuf, Probe, Result, SAMPLE_EVENTS,
    SampleContext, Url, Value, ValueEnum, VerifyTokenInput, callback, json, limits, redact_url,
    webhook_samples,
};

#[derive(Debug, Args)]
pub struct VerifyUrlArgs {
    /// The endpoint to check.
    pub(super) url: String,
    #[command(flatten)]
    pub(super) verify_token: VerifyTokenInput,
}

#[derive(Debug, Args)]
pub struct SimulateArgs {
    /// The endpoint to POST to.
    pub(super) url: String,
    /// Which sample delivery to send.
    #[arg(
        long,
        default_value = "text",
        conflicts_with = "file",
        value_parser = clap::builder::PossibleValuesParser::new(SAMPLE_EVENTS)
    )]
    pub(super) event: String,
    /// Send this JSON file instead of a sample, byte for byte.
    #[arg(short = 'f', long, value_name = "PATH")]
    pub(super) file: Option<PathBuf>,
    /// valid: sign with the app secret. wrong and missing test that the endpoint rejects it,
    /// so they pass when it doesn't answer 200, and don't need the app secret.
    #[arg(long, value_enum, default_value_t)]
    pub(super) signature: SignatureMode,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub(super) enum SignatureMode {
    #[default]
    Valid,
    Wrong,
    Missing,
}

/// Meta needs a real certificate; plain HTTP is only good for trying things out locally.
pub(super) fn warn_unless_https(app: &App, url: &Url) {
    if url.scheme() != "https" {
        app.out.warn(
            "this isn't HTTPS: Meta only calls endpoints with a valid TLS certificate, so this \
             check says nothing about the certificate",
        );
    }
}

impl VerifyUrlArgs {
    pub(super) async fn run(self, app: &App) -> Result<()> {
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
    pub(super) async fn run(self, app: &App) -> Result<()> {
        let url = callback::parse_url(&self.url)?;
        warn_unless_https(app, &url);
        let file_body = self.file.as_deref().map(read_payload).transpose()?;
        let ctx = app.ctx()?;
        let (body, source) = match file_body {
            Some(body) => (body, "file"),
            None => {
                let sample = SampleContext {
                    waba_id: ctx
                        .waba_id()
                        .unwrap_or(webhook_samples::EXAMPLE_WABA_ID)
                        .to_owned(),
                    phone_number_id: ctx
                        .phone_number_id()
                        .unwrap_or(webhook_samples::EXAMPLE_PHONE_NUMBER_ID)
                        .to_owned(),
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
pub(super) fn read_payload(path: &std::path::Path) -> Result<Vec<u8>> {
    let body = crate::commands::util::read_file(path)?.into_bytes();
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
