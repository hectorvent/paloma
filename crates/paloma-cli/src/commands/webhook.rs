use clap::{Args, Subcommand};
use paloma_core::{Error, Result};
use paloma_graph::api::webhook::{self, AVAILABLE_FIELDS};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};

use super::{SecretSource, planned, read_secret};
use crate::app::{App, no_id};
use crate::output::Output;

#[derive(Debug, Subcommand)]
pub enum WebhookCommand {
    /// Show the effective callback URL and subscribed apps.
    Get,
    /// Subscribe this app to the WABA with an override callback URL.
    Subscribe {
        /// HTTPS endpoint Meta will call.
        #[arg(long)]
        callback_url: String,
        #[command(flatten)]
        verify_token: VerifyTokenInput,
    },
    /// Remove this app's subscription (and override) from the WABA.
    Unsubscribe,
    /// Show or change the app-level webhook field subscriptions.
    Fields(Fields),
    /// Run a local webhook receiver (planned v1.0).
    Listen,
    /// Verify an endpoint's hub.challenge handshake (planned v1.0).
    VerifyUrl,
    /// Send signed sample payloads to an endpoint (planned v1.0).
    Simulate,
}

#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct Fields {
    #[command(subcommand)]
    command: Option<FieldsCommand>,
}

/// The token echoed back during Meta's verification handshake: from
/// PALOMA_VERIFY_TOKEN, stdin or a prompt.
#[derive(Debug, Args)]
pub struct VerifyTokenInput {
    /// Deprecated: prefer PALOMA_VERIFY_TOKEN or --verify-token-stdin.
    #[arg(long, hide = true)]
    verify_token: Option<String>,
    /// Read the verify token from stdin. Otherwise uses PALOMA_VERIFY_TOKEN, or prompts.
    #[arg(long, conflicts_with = "verify_token")]
    verify_token_stdin: bool,
}

impl VerifyTokenInput {
    fn read(&self, app: &App) -> Result<SecretString> {
        read_secret(
            app,
            &SecretSource {
                what: "verify token",
                flag: "--verify-token",
                stdin_flag: "--verify-token-stdin",
                env: "PALOMA_VERIFY_TOKEN",
            },
            self.verify_token.as_deref(),
            self.verify_token_stdin,
        )
    }

    fn given(&self) -> bool {
        self.verify_token.is_some() || self.verify_token_stdin
    }
}

#[derive(Debug, Subcommand)]
enum FieldsCommand {
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
    /// List every field that can be subscribed (offline).
    Available,
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
                let effective = webhook::application_callback(&cfg);
                let override_uri = webhook::override_callback(&apps);
                app.out.emit(
                    &Output::new(json!({
                        "effective_callback_url": effective,
                        "override_callback_uri": override_uri,
                        "subscribed_apps": apps.get("data").cloned().unwrap_or(Value::Array(vec![])),
                    }))
                    .id(override_uri.or(effective)),
                );
                Ok(())
            }
            Self::Subscribe {
                callback_url,
                verify_token,
            } => {
                let verify_token = verify_token.read(app)?;
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                if !app
                    .confirm_or_abort(&format!("change the webhook callback to {callback_url}"))?
                {
                    return Ok(());
                }
                app.send_and_emit(
                    &webhook::subscribe(&waba, &callback_url, verify_token.expose_secret()),
                    no_id,
                )
                .await
            }
            Self::Unsubscribe => {
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                if !app.confirm_or_abort(&format!("unsubscribe this app from WABA {waba}"))? {
                    return Ok(());
                }
                app.send_and_emit(&webhook::unsubscribe(&waba), no_id).await
            }
            Self::Fields(f) => f.run(app).await,
            Self::Listen => planned("webhook listen", "v1.0"),
            Self::VerifyUrl => planned("webhook verify-url", "v1.0"),
            Self::Simulate => planned("webhook simulate", "v1.0"),
        }
    }
}

impl Fields {
    async fn run(self, app: &App) -> Result<()> {
        match self.command {
            Some(FieldsCommand::Available) => {
                if app.out.quiet() {
                    app.out
                        .stdout(&format!("{}\n", AVAILABLE_FIELDS.join("\n")));
                } else {
                    app.out
                        .emit(&Output::new(json!({ "fields": AVAILABLE_FIELDS })));
                }
                Ok(())
            }
            None => {
                let app_id = super::app_id(app).await?;
                app.send_and_emit(&webhook::app_subscriptions(&app_id), no_id)
                    .await
            }
            Some(FieldsCommand::Set {
                fields,
                callback_url,
                verify_token,
            }) => {
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
                let app_id = super::app_id(app).await?;
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
