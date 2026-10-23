//! Non-calling settings have different safety constraints: storage changes require an
//! unregistered number, while identity checks apply to future message delivery.

use crate::app::{App, no_id};
use clap::Subcommand;
use paloma_core::{Result, validate};
use paloma_graph::api::phone_numbers;
use serde_json::json;

#[derive(Debug, Subcommand)]
pub enum SettingsCommand {
    /// Read current settings without requesting SIP credentials.
    Get,
    /// Change local storage on an already deregistered number; re-register afterward.
    Storage {
        /// Country for data at rest; omit with --disable.
        #[arg(long, required_unless_present = "disable", conflicts_with = "disable")]
        region: Option<String>,
        /// Disable in-country storage on a deregistered number.
        #[arg(long)]
        disable: bool,
    },
    /// Require the customer's identity hash to match before delivering messages.
    IdentityCheck {
        #[arg(long, required = true, action = clap::ArgAction::Set)]
        enabled: bool,
    },
}

impl SettingsCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        if let Self::Storage {
            region: Some(region),
            ..
        } = &self
        {
            validate::data_localization_region(region)?;
        }
        let phone = app.ctx()?.require_phone_number_id()?.to_owned();
        let body = match self {
            Self::Get => {
                return app
                    .send_and_emit(&phone_numbers::settings(&phone), no_id)
                    .await;
            }
            Self::Storage { region, disable } => {
                app.out.note("local storage changes require a deregistered number; register it again afterward");
                let mut storage = json!({"status": if disable {"IN_COUNTRY_STORAGE_DISABLED"} else {"IN_COUNTRY_STORAGE_ENABLED"}});
                if let Some(region) = region {
                    storage["data_localization_region"] = json!(region);
                }
                json!({"storage_configuration": storage})
            }
            Self::IdentityCheck { enabled } => {
                json!({"user_identity_change": {"enable_identity_key_check": enabled}})
            }
        };
        if !app.confirm_or_abort(&format!("change settings for number {phone}"))? {
            return Ok(());
        }
        app.send_and_emit(&phone_numbers::set_settings(&phone, body), no_id)
            .await
    }
}
