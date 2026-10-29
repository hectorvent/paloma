//! Account reads keep WABA health separate from the portfolio limit exposed by each
//! phone number. Analytics keeps its reporting filters separate from account settings.

use std::path::PathBuf;

use clap::{Args, Subcommand};
use paloma_core::{Error, Result, limits, validate};
use paloma_graph::api::{account, webhook};

use super::util::read_json;
use crate::{cli::Format, output::Output};
use paloma_graph::Request;
use serde_json::json;

mod administration;
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
    "/whatsapp_business_api_data/link",
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
    /// Estimate marketing deliveries and costs at several max prices.
    Reach(administration::Reach),
    /// List assigned users or WABA system users.
    Users(administration::Users),
    /// Manage click-to-WhatsApp welcome message sequences.
    #[command(subcommand)]
    WelcomeSequence(administration::WelcomeSequence),
    /// List account activity history.
    Activities(ActivitiesArgs),
    /// Clone an account and explicitly switch billing currency.
    #[command(subcommand)]
    BillingCurrency(BillingCurrency),
    /// Show the shared portfolio messaging limit reported by each WABA number.
    Limits(LimitsArgs),
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
pub struct ActivitiesArgs {
    #[command(flatten)]
    page: PageArgs,
    /// Page size, at most 100.
    #[arg(long)]
    limit: Option<u32>,
}

#[derive(Debug, Args)]
pub struct LimitsArgs {
    #[command(flatten)]
    page: PageArgs,
    /// Read the shared limit directly from this business portfolio.
    #[arg(long, conflicts_with_all = ["all", "after"])]
    business_id: Option<String>,
}

#[derive(Debug, Args)]
pub struct SetArgs {
    #[arg(long)]
    name: Option<String>,
    /// Graph timezone identifier (not an IANA timezone name).
    #[arg(long)]
    timezone_id: Option<String>,
    /// Enroll in template insights (irreversible); false is refused.
    #[arg(long, action = clap::ArgAction::Set)]
    insights: Option<bool>,
    /// Enroll in template insights (irreversible), exclusive with other settings.
    #[arg(long, conflicts_with_all = ["name", "timezone_id", "insights", "disable_cloud_marketing", "file"])]
    enable_insights: bool,
    /// Reject marketing templates on Cloud API; onboard to MM API first.
    #[arg(long, visible_alias = "disable-marketing-messages-on-cloud-api", action = clap::ArgAction::Set)]
    disable_cloud_marketing: Option<bool>,
    /// Full writable account object (JSON), exclusive with typed settings.
    #[arg(long, conflicts_with_all = ["name", "timezone_id", "insights", "disable_cloud_marketing", "enable_insights"])]
    file: Option<PathBuf>,
}

impl SetArgs {
    fn body(self) -> Result<serde_json::Value> {
        if let Some(name) = &self.name {
            validate::not_blank(name, "name")?;
        }
        if let Some(timezone) = &self.timezone_id {
            validate::timezone_id(timezone)?;
        }
        let body = if let Some(file) = self.file {
            read_json(&file)?
        } else {
            account::Settings {
                name: self.name,
                timezone_id: self.timezone_id,
                insights: if self.enable_insights {
                    Some(true)
                } else {
                    self.insights
                },
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
                if key == "timezone_id" {
                    validate::timezone_id(text)?;
                } else {
                    validate::not_blank(text, key)?;
                }
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
        if let Some(enabled) = body.get("is_enabled_for_insights") {
            if enabled != &serde_json::Value::Bool(true) {
                return Err(Error::validation(
                    "insights enrollment is irreversible and cannot be disabled",
                ));
            }
            if body.as_object().is_some_and(|object| object.len() != 1) {
                return Err(Error::validation(
                    "enable insights separately from other account settings",
                ));
            }
        }
        Ok(body)
    }
}

impl AccountCommand {
    #[allow(clippy::too_many_lines)] // one short arm per verb; each arm calls into its module
    pub async fn run(self, app: &App) -> Result<()> {
        if let Self::Reach(command) = self {
            return command.run(app).await;
        }
        if let Self::Users(command) = self {
            return command.run(app).await;
        }
        if let Self::WelcomeSequence(command) = self {
            return command.run(app).await;
        }
        if let Self::BillingCurrency(command) = self {
            return command.run(app).await;
        }
        if let Self::Set(args) = self {
            let body = args.body()?;
            let waba = app.ctx()?.require_waba_id()?.to_owned();
            if body.get("is_enabled_for_insights") == Some(&serde_json::Value::Bool(true)) {
                if !app.confirm_or_abort(
                    "enable template insights permanently (this cannot be undone)",
                )? {
                    return Ok(());
                }
                return app
                    .send_and_emit(&account::enable_insights(&waba), no_id)
                    .await;
            }
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
        if let Self::Limits(args) = &self
            && let Some(id) = &args.business_id
        {
            validate::numeric_id(id, "business id")?;
            return app
                .send_and_emit(&account::portfolio_limit(id), no_id)
                .await;
        }
        if let Self::Activities(args) = &self {
            validate::page_size(args.limit, Some(limits::MAX_ACTIVITIES_PAGE))?;
        }
        let waba = app.ctx()?.require_waba_id()?.to_owned();
        match self {
            Self::Activities(a) => {
                super::listing::list(
                    app,
                    a.page.all,
                    a.page.after,
                    |after| account::activities_page(&waba, after, a.limit),
                    &[
                        "event_type",
                        "timestamp",
                        "actor_id",
                        "actor_business_id",
                        "category",
                        "id",
                    ],
                )
                .await
            }
            Self::Reach(_)
            | Self::Users(_)
            | Self::WelcomeSequence(_)
            | Self::Set(_)
            | Self::BillingCurrency(_)
            | Self::Analytics(_) => {
                unreachable!("handled before context resolution")
            }
            Self::Get { fields } => {
                app.send_and_emit(
                    &account::get(&waba, fields.as_deref().unwrap_or(account::DETAIL_FIELDS)),
                    field_id,
                )
                .await
            }
            Self::Health => show_health(app, &account::get(&waba, account::HEALTH_FIELDS)).await,
            Self::Limits(a) => {
                super::listing::list(
                    app,
                    a.page.all,
                    a.page.after,
                    |after| account::limits(&waba, after),
                    LIMIT_COLUMNS,
                )
                .await
            }
            Self::SubscribedApps(a) => {
                super::listing::list(
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

/// Only table output flattens health entities; scripting keeps the Graph response.
pub(super) async fn show_health(app: &App, req: &Request) -> Result<()> {
    let Some(response) = app.send(req).await? else {
        return Ok(());
    };
    let mut output = Output::response(&response)?;
    let id = field_id(&output.data);
    if app.out.format() == Format::Table {
        output.data = json!({"rows": account::health_rows(&output.data)});
        output = output.table(
            "/rows",
            &[
                "entity_type",
                "id",
                "can_send_message",
                "can_receive_call_sip",
                "details",
            ],
        );
    }
    app.out.emit(&output.id(id));
    Ok(())
}
