//! Account reads keep WABA health separate from the portfolio limit exposed by each
//! phone number. Analytics keeps its reporting filters separate from account settings.

use std::path::PathBuf;

use clap::{Args, Subcommand};
use paloma_core::{Error, Result, limits, validate};
use paloma_graph::api::{account, webhook};

use super::util::{read_json, toggle};
use crate::{cli::Format, output::Output};
use paloma_graph::Request;
use serde_json::json;

mod administration;
mod analytics;
mod billing_currency;
use super::listing::{Pagination, Rows};
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
        #[arg(long, value_name = "FIELD,...")]
        fields: Option<String>,
    },
    /// Update account settings.
    Set(SetArgs),
    /// Estimate marketing deliveries and costs at several max prices.
    Reach(administration::Reach),
    /// List the people and system users with access to the WABA.
    #[command(subcommand)]
    Users(UsersCommand),
    /// Manage click-to-WhatsApp welcome message sequences.
    #[command(subcommand)]
    WelcomeSequence(administration::WelcomeSequence),
    /// Show the account's activity history.
    #[command(subcommand)]
    Activity(ActivityCommand),
    /// Clone an account and explicitly switch billing currency.
    #[command(subcommand)]
    BillingCurrency(BillingCurrency),
    /// Show the shared portfolio messaging limit reported by each WABA number.
    Limits(LimitsArgs),
    /// Show the WABA's messaging and calling health.
    Health,
    /// Report messaging counts or pricing analytics.
    Analytics(Analytics),
    /// Show the apps subscribed to the WABA's webhooks.
    #[command(subcommand)]
    Apps(AppsCommand),
}

#[derive(Debug, Subcommand)]
pub enum UsersCommand {
    /// List assigned users, or the WABA's system users with --system.
    List(administration::Users),
}

#[derive(Debug, Subcommand)]
pub enum ActivityCommand {
    /// List account activity, newest first.
    List(ActivitiesArgs),
}

#[derive(Debug, Subcommand)]
pub enum AppsCommand {
    /// List the apps subscribed to the WABA.
    List(Pagination),
}

#[derive(Debug, Args)]
pub struct ActivitiesArgs {
    #[command(flatten)]
    page: Pagination,
}

#[derive(Debug, Args)]
pub struct LimitsArgs {
    #[command(flatten)]
    page: Pagination,
    /// Read the shared limit directly from this business portfolio.
    #[arg(long, conflicts_with_all = ["all", "after", "limit"])]
    business_id: Option<String>,
}

#[derive(Debug, Args)]
pub struct SetArgs {
    /// The WABA's name.
    #[arg(long)]
    name: Option<String>,
    /// Graph timezone identifier (not an IANA timezone name).
    #[arg(long)]
    timezone_id: Option<String>,
    /// Enroll in template insights. Meta can't undo it, so it goes alone and confirms.
    #[arg(long, conflicts_with_all = ["name", "timezone_id", "cloud_marketing", "no_cloud_marketing"])]
    insights: bool,
    /// Let marketing templates go out through Cloud API again.
    #[arg(long, overrides_with = "no_cloud_marketing")]
    cloud_marketing: bool,
    /// Refuse marketing templates on Cloud API, so they go through MM API; onboard there first.
    #[arg(long, overrides_with = "cloud_marketing")]
    no_cloud_marketing: bool,
    /// Full writable account object (JSON), exclusive with typed settings.
    #[arg(short = 'f', long, conflicts_with_all = ["name", "timezone_id", "insights", "cloud_marketing", "no_cloud_marketing"])]
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
                insights: self.insights.then_some(true),
                disable_cloud_marketing: toggle(self.cloud_marketing, self.no_cloud_marketing)
                    .map(|allowed| !allowed),
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
    pub async fn run(self, app: &App) -> Result<()> {
        match self {
            Self::Reach(command) => command.run(app).await,
            Self::Users(UsersCommand::List(command)) => command.run(app).await,
            Self::WelcomeSequence(command) => command.run(app).await,
            Self::BillingCurrency(command) => command.run(app).await,
            Self::Analytics(args) => args.run(app).await,
            Self::Set(args) => args.run(app).await,
            Self::Get { fields } => {
                if let Some(fields) = &fields {
                    validate::not_blank(fields, "fields")?;
                }
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                let fields = fields.as_deref().unwrap_or(account::DETAIL_FIELDS);
                app.send_and_emit(&account::get(&waba, fields), field_id)
                    .await
            }
            Self::Health => {
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                show_health(app, &account::get(&waba, account::HEALTH_FIELDS)).await
            }
            Self::Limits(args) => args.run(app).await,
            Self::Apps(AppsCommand::List(page)) => {
                page.check(None)?;
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                super::listing::list(
                    app,
                    &page,
                    |after| {
                        webhook::subscribed_apps(&waba)
                            .query_opt("after", after)
                            .query_opt("limit", page.limit)
                    },
                    Rows {
                        columns: APP_COLUMNS,
                        id: "/whatsapp_business_api_data/id",
                    },
                )
                .await
            }
            Self::Activity(ActivityCommand::List(args)) => args.run(app).await,
        }
    }
}

impl SetArgs {
    async fn run(self, app: &App) -> Result<()> {
        let body = self.body()?;
        let waba = app.ctx()?.require_waba_id()?.to_owned();
        if body.get("is_enabled_for_insights") == Some(&serde_json::Value::Bool(true)) {
            if !app
                .confirm_or_abort("enable template insights permanently (this cannot be undone)")?
            {
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
        app.send_and_emit(&account::set(&waba, body), no_id).await
    }
}

impl LimitsArgs {
    /// Every number's limit on the WABA, or the shared limit of one business portfolio.
    async fn run(self, app: &App) -> Result<()> {
        self.page.check(None)?;
        if let Some(id) = &self.business_id {
            validate::numeric_id(id, "business id")?;
            return app
                .send_and_emit(&account::portfolio_limit(id), no_id)
                .await;
        }
        let waba = app.ctx()?.require_waba_id()?.to_owned();
        super::listing::list(
            app,
            &self.page,
            |after| account::limits(&waba, after).query_opt("limit", self.page.limit),
            Rows {
                columns: LIMIT_COLUMNS,
                id: "id",
            },
        )
        .await
    }
}

impl ActivitiesArgs {
    async fn run(self, app: &App) -> Result<()> {
        self.page.check(Some(limits::MAX_ACTIVITIES_PAGE))?;
        let waba = app.ctx()?.require_waba_id()?.to_owned();
        super::listing::list(
            app,
            &self.page,
            |after| account::activities_page(&waba, after, self.page.limit),
            Rows {
                columns: &[
                    "event_type",
                    "timestamp",
                    "actor_id",
                    "actor_business_id",
                    "category",
                    "id",
                ],
                id: "id",
            },
        )
        .await
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
