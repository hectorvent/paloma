//! Meta delivers each webhook field in the Graph version its subscription was saved with, and
//! doesn't document a parameter for changing it. So `set` resubmits the same fields through the
//! target version and reads the result back, rather than trusting Graph's success response.

use super::VerifyTokenInput;
use crate::app::App;
use crate::output::Output;
use clap::{Args, Subcommand};
use paloma_core::{Error, Result, validate};
use paloma_graph::api::webhook;
use secrecy::ExposeSecret;
use serde_json::{Value, json};

#[derive(Debug, Args)]
pub struct Version {
    #[command(subcommand)]
    command: Option<VersionCommand>,
}

#[derive(Debug, Subcommand)]
enum VersionCommand {
    /// Move the app's WhatsApp webhooks to a Graph version, such as v26.0.
    Set {
        /// The Graph version to move to.
        #[arg(value_name = "VERSION")]
        target: String,
        /// Also resend the current callback URL and a verify token, if Meta asks for them.
        #[arg(long)]
        reverify: bool,
        #[command(flatten)]
        verify_token: VerifyTokenInput,
    },
}

/// The versions in use across the subscribed fields, sorted and without repeats.
fn versions(subscriptions: &Value) -> Vec<String> {
    let mut versions: Vec<String> = webhook::field_versions(subscriptions)
        .into_iter()
        .filter_map(|(_, version)| version)
        .collect();
    versions.sort();
    versions.dedup();
    versions
}

fn emit(app: &App, subscriptions: &Value) {
    let found = versions(subscriptions);
    let version = match found.as_slice() {
        [one] => Some(one.clone()),
        _ => None,
    };
    let fields: Vec<Value> = webhook::field_versions(subscriptions)
        .into_iter()
        .map(|(name, version)| json!({"name": name, "version": version}))
        .collect();
    let data = json!({
        "object": "whatsapp_business_account",
        "version": version,
        "fields": fields,
    });
    app.out.emit(
        &Output::new(data)
            .id(version)
            .table("/fields", &["name", "version"]),
    );
}

impl Version {
    pub async fn run(self, app: &App) -> Result<()> {
        let Some(VersionCommand::Set {
            target,
            reverify,
            verify_token,
        }) = self.command
        else {
            let app_id = super::super::app_id(app).await?;
            let Some(response) = app.send(&webhook::app_subscriptions(&app_id)).await? else {
                return Ok(());
            };
            emit(app, &response.json()?);
            return Ok(());
        };
        validate::graph_version(&target)?;
        // Only a resent callback URL uses a token, as in `webhook fields set`.
        let token = if reverify {
            Some(verify_token.read(app)?)
        } else if verify_token.given() {
            return Err(Error::validation("a verify token needs --reverify"));
        } else {
            None
        };
        let app_id = super::super::app_id(app).await?;
        let read = webhook::app_subscriptions(&app_id);
        let current = app.fetch(&read).await?.json()?;
        let fields = webhook::subscribed_fields(&current);
        if fields.is_empty() {
            return Err(Error::validation(format!(
                "app {app_id} has no WhatsApp webhook subscription; add one with `paloma webhook fields set`"
            )));
        }
        let before = versions(&current);
        if before == [target.as_str()] {
            app.out
                .info(format!("WhatsApp webhooks are already on {target}"));
            emit(app, &current);
            return Ok(());
        }
        let callback = if reverify {
            Some(webhook::app_callback_url(&current).ok_or_else(|| {
                Error::validation("the WhatsApp subscription has no callback URL to resend")
            })?)
        } else {
            None
        };
        if !app.confirm_or_abort(&format!(
            "move app {app_id}'s WhatsApp webhooks from {} to {target}; the payloads your endpoint receives may change",
            before.join(", ")
        ))? {
            return Ok(());
        }
        let write = webhook::set_app_subscriptions(
            &app_id,
            &fields,
            callback.as_deref(),
            token.as_ref().map(ExposeSecret::expose_secret),
        )
        .at_version(&target);
        if app.send(&write).await?.is_none() {
            return Ok(());
        }
        let after = app.fetch(&read).await?.json()?;
        let now = versions(&after);
        if now != [target.as_str()] {
            return Err(Error::other(format!(
                "Graph accepted the change, but still reports {}; change it in the App Dashboard",
                now.join(", ")
            )));
        }
        emit(app, &after);
        Ok(())
    }
}
