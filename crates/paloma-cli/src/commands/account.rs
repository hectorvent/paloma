//! Account reads keep WABA health separate from the portfolio limit exposed by each
//! phone number. Analytics keeps its reporting filters separate from account settings.

use std::path::PathBuf;

use clap::{Args, Subcommand};
use paloma_core::{Error, Result, validate};
use paloma_graph::api::{account, webhook};

use super::read_json;

mod analytics;
mod billing_currency;
use crate::app::{App, field_id, no_id};
use analytics::Analytics;
use billing_currency::BillingCurrency;

const LIMIT_COLUMNS: &[&str] = &[
    "id",
    "display_phone_number",
    "whatsapp_business_manager_messaging_limit",
];
const APP_COLUMNS: &[&str] = &[
    "/whatsapp_business_api_data/id",
    "/whatsapp_business_api_data/name",
    "override_callback_uri",
];

#[derive(Debug, Subcommand)]
pub enum AccountCommand {
    /// Show the active WABA's details.
    Get {
        /// Comma-separated Graph fields (replaces the default selection).
        #[arg(long)]
        fields: Option<String>,
    },
    /// Update account settings.
    Set(SetArgs),
    /// List account activity history.
    Activities(PageArgs),
    /// Clone an account and explicitly switch billing currency.
    #[command(subcommand)]
    BillingCurrency(BillingCurrency),
    /// Show the shared portfolio messaging limit reported by each WABA number.
    Limits(PageArgs),
    /// Show the WABA's messaging and calling health.
    Health,
    /// Report messaging counts or pricing analytics.
    Analytics(Analytics),
    /// List apps subscribed to the WABA.
    SubscribedApps(PageArgs),
}

#[derive(Debug, Args)]
pub struct PageArgs {
    /// Follow every page.
    #[arg(long)]
    all: bool,
    /// Start after this cursor.
    #[arg(long)]
    after: Option<String>,
}

#[derive(Debug, Args)]
pub struct SetArgs {
    #[arg(long)]
    name: Option<String>,
    /// Graph timezone identifier (not an IANA timezone name).
    #[arg(long)]
    timezone_id: Option<String>,
    /// Enable or disable template insights.
    #[arg(long, action = clap::ArgAction::Set)]
    insights: Option<bool>,
    /// Reject marketing templates on Cloud API; onboard to MM API first.
    #[arg(long, action = clap::ArgAction::Set)]
    disable_cloud_marketing: Option<bool>,
    /// Full writable account object (JSON), exclusive with typed settings.
    #[arg(long, conflicts_with_all = ["name", "timezone_id", "insights", "disable_cloud_marketing"])]
    file: Option<PathBuf>,
}

impl SetArgs {
    fn body(self) -> Result<serde_json::Value> {
        if let Some(name) = &self.name {
            validate::not_blank(name, "name")?;
        }
        if let Some(timezone) = &self.timezone_id {
            validate::not_blank(timezone, "timezone id")?;
        }
        let body = if let Some(file) = self.file {
            read_json(&file)?
        } else {
            account::Settings {
                name: self.name,
                timezone_id: self.timezone_id,
                insights: self.insights,
                disable_cloud_marketing: self.disable_cloud_marketing,
            }
            .body()
        };
        if body.as_object().is_none_or(serde_json::Map::is_empty) {
            return Err(Error::validation(
                "account settings must be a nonempty JSON object; pass a setting or --file",
            ));
        }
        for key in ["name", "timezone_id"] {
            if let Some(value) = body.get(key) {
                let text = value
                    .as_str()
                    .ok_or_else(|| Error::validation(format!("{key} must be a string")))?;
                validate::not_blank(text, key)?;
            }
        }
        for key in [
            "is_enabled_for_insights",
            "disable_marketing_messages_on_cloud_api",
        ] {
            if body.get(key).is_some_and(|v| !v.is_boolean()) {
                return Err(Error::validation(format!("{key} must be boolean")));
            }
        }
        Ok(body)
    }
}

impl AccountCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        if let Self::BillingCurrency(command) = self {
            return command.run(app).await;
        }
        if let Self::Set(args) = self {
            let body = args.body()?;
            let waba = app.ctx()?.require_waba_id()?.to_owned();
            if body
                .get("disable_marketing_messages_on_cloud_api")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
                && !app.confirm_or_abort(
                    "disable Cloud API marketing sends (MM API onboarding must be complete)",
                )?
            {
                return Ok(());
            }
            return app.send_and_emit(&account::set(&waba, body), no_id).await;
        }
        if let Self::Get {
            fields: Some(fields),
        } = &self
        {
            validate::not_blank(fields, "fields")?;
        }
        if let Self::Analytics(args) = self {
            return args.run(app).await;
        }
        let waba = app.ctx()?.require_waba_id()?.to_owned();
        match self {
            Self::Activities(a) => {
                super::template::list(
                    app,
                    a.all,
                    a.after,
                    |after| account::activities(&waba, after),
                    &[
                        "event_type",
                        "timestamp",
                        "actor_id",
                        "actor_business_id",
                        "category",
                    ],
                )
                .await
            }
            Self::Set(_) | Self::BillingCurrency(_) | Self::Analytics(_) => {
                unreachable!("handled before context resolution")
            }
            Self::Get { fields } => {
                app.send_and_emit(
                    &account::get(&waba, fields.as_deref().unwrap_or(account::DETAIL_FIELDS)),
                    field_id,
                )
                .await
            }
            Self::Health => {
                app.send_and_emit(&account::get(&waba, account::HEALTH_FIELDS), field_id)
                    .await
            }
            Self::Limits(a) => {
                super::template::list(
                    app,
                    a.all,
                    a.after,
                    |after| account::limits(&waba, after),
                    LIMIT_COLUMNS,
                )
                .await
            }
            Self::SubscribedApps(a) => {
                super::template::list(
                    app,
                    a.all,
                    a.after,
                    |after| webhook::subscribed_apps(&waba).query_opt("after", after),
                    APP_COLUMNS,
                )
                .await
            }
        }
    }
}
