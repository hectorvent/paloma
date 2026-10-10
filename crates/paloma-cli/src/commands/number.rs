use clap::{Args, Subcommand};
use paloma_core::{Result, validate};
use paloma_graph::api::phone_numbers::{self, DETAIL_FIELDS, QUALITY_FIELDS};
use secrecy::{ExposeSecret, SecretString};

use super::{SecretSource, read_secret};
use crate::app::{App, field_id, no_id};
use crate::output::Output;

const LIST_COLUMNS: &[&str] = &[
    "id",
    "display_phone_number",
    "verified_name",
    "quality_rating",
    "code_verification_status",
];

#[derive(Debug, Subcommand)]
pub enum NumberCommand {
    /// List the WABA's phone numbers.
    List,
    /// Show a phone number's details [default: the active number].
    Get { phone_number_id: Option<String> },
    /// Show quality rating, messaging tier and throughput.
    Quality { phone_number_id: Option<String> },
    /// Register the number for Cloud API use.
    Register(PinInput),
    /// Deregister the number.
    Deregister,
    /// Set the two-step verification PIN.
    SetPin(PinInput),
}

/// The six-digit two-step verification PIN: from PALOMA_PIN, stdin or a prompt.
#[derive(Debug, Args)]
pub struct PinInput {
    /// Deprecated: prefer PALOMA_PIN or --pin-stdin.
    #[arg(long, hide = true)]
    pin: Option<String>,
    /// Read the six-digit PIN from stdin. Otherwise uses PALOMA_PIN, or prompts.
    #[arg(long, conflicts_with = "pin")]
    pin_stdin: bool,
}

impl PinInput {
    fn read(&self, app: &App) -> Result<SecretString> {
        let pin = read_secret(
            app,
            &SecretSource {
                what: "PIN",
                flag: "--pin",
                stdin_flag: "--pin-stdin",
                env: "PALOMA_PIN",
            },
            self.pin.as_deref(),
            self.pin_stdin,
        )?;
        validate::pin(pin.expose_secret())?;
        Ok(pin)
    }
}

impl NumberCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        match self {
            Self::List => {
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                if let Some(resp) = app.send(&phone_numbers::list(&waba)).await? {
                    app.out
                        .emit(&Output::response(&resp)?.table("/data", LIST_COLUMNS));
                }
                Ok(())
            }
            Self::Get { phone_number_id } => {
                let id = target(app, phone_number_id)?;
                app.send_and_emit(&phone_numbers::get(&id, DETAIL_FIELDS), field_id)
                    .await
            }
            Self::Quality { phone_number_id } => {
                let id = target(app, phone_number_id)?;
                app.send_and_emit(&phone_numbers::get(&id, QUALITY_FIELDS), |v| {
                    v.get("quality_rating")
                        .and_then(|q| q.as_str())
                        .map(str::to_owned)
                })
                .await
            }
            Self::Register(input) => {
                let pin = input.read(app)?;
                let phone = app.ctx()?.require_phone_number_id()?.to_owned();
                app.send_and_emit(&phone_numbers::register(&phone, pin.expose_secret()), no_id)
                    .await
            }
            Self::Deregister => {
                let phone = app.ctx()?.require_phone_number_id()?.to_owned();
                if !app.confirm_or_abort(&format!("deregister number {phone}"))? {
                    return Ok(());
                }
                app.send_and_emit(&phone_numbers::deregister(&phone), no_id)
                    .await
            }
            Self::SetPin(input) => {
                let pin = input.read(app)?;
                let phone = app.ctx()?.require_phone_number_id()?.to_owned();
                app.send_and_emit(&phone_numbers::set_pin(&phone, pin.expose_secret()), no_id)
                    .await
            }
        }
    }
}

fn target(app: &App, explicit: Option<String>) -> Result<String> {
    match explicit.filter(|s| !s.trim().is_empty()) {
        Some(id) => Ok(id),
        None => Ok(app.ctx()?.require_phone_number_id()?.to_owned()),
    }
}
