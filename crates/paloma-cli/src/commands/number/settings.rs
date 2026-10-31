//! Non-calling settings have different safety constraints: storage changes require an
//! unregistered number, while identity checks apply to future message delivery. So one
//! `set` changes one of them, and a failed storage change never blocks an identity change.

use crate::app::{App, no_id};
use clap::{ArgGroup, Args, Subcommand};
use paloma_core::{Result, validate};
use paloma_graph::api::phone_numbers;

#[derive(Debug, Subcommand)]
pub enum SettingsCommand {
    /// Show the number's settings, without SIP credentials.
    Get,
    /// Change local storage or the identity check, one per call.
    Set(Set),
}

#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("setting")
        .required(true)
        .args(["storage_region", "no_storage", "identity_check", "no_identity_check"])
))]
pub struct Set {
    /// Keep data at rest in this country. The number must be deregistered first; register it
    /// again afterward.
    #[arg(long, value_name = "CC")]
    storage_region: Option<String>,
    /// Turn in-country storage off. The number must be deregistered first.
    #[arg(long)]
    no_storage: bool,
    /// Deliver a message only if the customer's identity hash still matches.
    #[arg(long)]
    identity_check: bool,
    /// Deliver messages whether or not the customer's identity hash changed.
    #[arg(long)]
    no_identity_check: bool,
}

impl SettingsCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        if let Self::Set(Set {
            storage_region: Some(region),
            ..
        }) = &self
        {
            validate::data_localization_region(region)?;
        }
        let phone = app.ctx()?.require_phone_number_id()?.to_owned();
        let a = match self {
            Self::Get => {
                return app
                    .send_and_emit(&phone_numbers::settings(&phone), no_id)
                    .await;
            }
            Self::Set(a) => a,
        };
        let req = if a.identity_check || a.no_identity_check {
            phone_numbers::set_identity_check(&phone, a.identity_check)
        } else {
            app.out.note(
                "local storage changes require a deregistered number; register it again afterward",
            );
            phone_numbers::set_storage(&phone, !a.no_storage, a.storage_region.as_deref())
        };
        if !app.confirm_or_abort(&format!("change settings for number {phone}"))? {
            return Ok(());
        }
        app.send_and_emit(&req, no_id).await
    }
}
