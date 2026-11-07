//! The app-level webhook field subscriptions, which need an app token and so an app id
//! and the app secret.

use super::{
    AVAILABLE_FIELDS, App, Args, Error, ExposeSecret, Output, Result, Subcommand, VerifyTokenInput,
    check_callback_url, json, no_id, webhook,
};

#[derive(Debug, Args)]
pub struct Fields {
    #[command(subcommand)]
    pub(super) command: FieldsCommand,
}

#[derive(Debug, Subcommand)]
pub(super) enum FieldsCommand {
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

impl Fields {
    pub(super) async fn run(self, app: &App) -> Result<()> {
        match self.command {
            FieldsCommand::List => {
                if app.out.is_quiet() {
                    app.out
                        .stdout(&format!("{}\n", AVAILABLE_FIELDS.join("\n")));
                } else {
                    app.out
                        .emit(&Output::new(json!({ "fields": AVAILABLE_FIELDS })));
                }
                Ok(())
            }
            FieldsCommand::Get => {
                let app_id = crate::commands::upload::app_id(app).await?;
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
                app.ensure_can_confirm(webhook::FIELDS_CONFIRM)?;
                let app_id = crate::commands::upload::app_id(app).await?;
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
