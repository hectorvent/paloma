//! Assigned-user reads require business context; welcome sequences are WABA-scoped
//! ad assets, with explicit identifiers and confirmation on changes or deletion.
use crate::commands::listing::{Pagination, Rows, list};
use crate::output::Output;
use crate::{
    app::{App, no_id},
    commands::util::read_json,
};
use clap::{Args, Subcommand, ValueEnum};
use paloma_core::{Error, Result, validate};
use paloma_graph::api::account;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The lookback for Meta's reach estimate. Its names are Meta's, also exposed by clap.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ReachInterval {
    #[value(name = "L1D")]
    OneDay,
    #[value(name = "L7D")]
    SevenDays,
    #[value(name = "L14D")]
    FourteenDays,
    #[value(name = "L28D")]
    TwentyEightDays,
}

impl ReachInterval {
    const fn api(self) -> &'static str {
        match self {
            Self::OneDay => "L1D",
            Self::SevenDays => "L7D",
            Self::FourteenDays => "L14D",
            Self::TwentyEightDays => "L28D",
        }
    }
}

#[derive(Debug, Args)]
pub struct Reach {
    /// Destination country as an ISO 3166-1 alpha-2 code; repeatable.
    #[arg(long = "country", required = true, value_name = "CC")]
    countries: Vec<String>,
    /// How much history the estimate draws on.
    #[arg(long, value_enum)]
    interval: ReachInterval,
}

impl Reach {
    pub async fn run(self, app: &App) -> Result<()> {
        self.countries
            .iter()
            .try_for_each(|c| validate::country_code(c))?;
        let waba = app.ctx()?.require_waba_id()?.to_owned();
        let req = account::reach_estimate(&waba, &self.countries, self.interval.api());
        let Some(response) = app.send(&req).await? else {
            return Ok(());
        };
        app.out.emit(&Output::response(&response)?.table(
            "/estimates",
            &[
                "bid_amount",
                "users",
                "deliveries_lower_bound",
                "deliveries_upper_bound",
                "cost_lower_bound",
                "cost_upper_bound",
            ],
        ));
        Ok(())
    }
}

#[derive(Debug, Args)]
pub struct Users {
    #[arg(long, required_unless_present = "system", conflicts_with = "system")]
    business_id: Option<String>,
    /// List WABA system users instead of assigned users.
    #[arg(long)]
    system: bool,
    #[command(flatten)]
    page: Pagination,
}
impl Users {
    pub async fn run(self, app: &App) -> Result<()> {
        if let Some(id) = &self.business_id {
            validate::numeric_id(id, "business id")?;
        }
        validate::page_size(
            self.page.limit,
            Some(paloma_core::limits::MAX_ASSIGNED_USERS_PAGE),
        )?;
        let waba = app.ctx()?.require_waba_id()?.to_owned();
        let page = |after: Option<&str>| {
            account::users(
                &waba,
                self.system,
                self.business_id.as_deref(),
                self.page.limit,
                after,
            )
        };
        list(
            app,
            &self.page,
            page,
            Rows {
                columns: &["id", "name", "tasks"],
                id: "id",
            },
        )
        .await
    }
}
#[derive(Debug, Subcommand)]
pub enum WelcomeSequence {
    /// List the WABA's welcome sequences.
    List {
        /// Sequences to return. Meta documents no paging for this edge.
        #[arg(long, value_name = "N")]
        limit: Option<u32>,
    },
    /// Show one welcome sequence.
    Get {
        /// Sequence id.
        id: String,
    },
    /// Create a sequence from its JSON content.
    Create {
        /// Sequence name.
        #[arg(long)]
        name: String,
        /// The sequence content: text, autofill_message and ice_breakers (JSON).
        #[arg(short = 'f', long)]
        file: PathBuf,
    },
    /// Change a sequence's name, content or both.
    Update {
        /// Sequence id.
        id: String,
        /// New name.
        #[arg(long)]
        name: Option<String>,
        /// New content (JSON), as for `create`.
        #[arg(short = 'f', long)]
        file: Option<PathBuf>,
    },
    /// Delete a sequence (Meta refuses sequences used in active ads).
    Delete {
        /// Sequence id.
        id: String,
    },
}
fn content(path: &Path) -> Result<Value> {
    let body = read_json(path)?;
    let object = body
        .as_object()
        .filter(|m| !m.is_empty())
        .ok_or_else(|| Error::validation("welcome sequence must be a nonempty JSON object"))?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "text" | "autofill_message" | "ice_breakers"))
    {
        return Err(Error::validation(
            "sequence accepts text, autofill_message and ice_breakers",
        ));
    }
    if let Some(value) = body.get("text") {
        validate::not_blank(
            value
                .as_str()
                .ok_or_else(|| Error::validation("sequence text must be a string"))?,
            "sequence text",
        )?;
    }
    if let Some(value) = body.get("autofill_message") {
        let text = value
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::validation("autofill_message requires string content"))?;
        validate::not_blank(text, "autofill message")?;
    }
    if let Some(value) = body.get("ice_breakers") {
        let list = value
            .as_array()
            .ok_or_else(|| Error::validation("ice_breakers must be an array"))?;
        for item in list {
            let title = item
                .get("title")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::validation("each ice breaker requires string title"))?;
            validate::not_blank(title, "ice breaker title")?;
        }
    }
    Ok(body)
}
/// Meta documents a bare array here; `data` is accepted too, since other edges wrap lists.
async fn list_sequences(app: &App, req: &paloma_graph::Request) -> Result<()> {
    let Some(response) = app.send(req).await? else {
        return Ok(());
    };
    let mut output = Output::response(&response)?;
    let rows = if output.data.is_array() { "" } else { "/data" };
    let ids = output
        .data
        .pointer(rows)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| v.get("sequence_id").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    output.primary_id = (!ids.is_empty()).then_some(ids);
    app.out
        .emit(&output.table(rows, &["sequence_id", "name", "is_used_in_ad"]));
    Ok(())
}

const IN_ADS: &str = "change or delete a welcome sequence used by click-to-WhatsApp ads";

impl WelcomeSequence {
    pub async fn run(self, app: &App) -> Result<()> {
        let req = match self {
            Self::List { limit } => {
                validate::page_size(limit, None)?;
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                let req = account::welcome_sequences(&waba, None, limit);
                return list_sequences(app, &req).await;
            }
            Self::Get { id } => {
                validate::numeric_id(&id, "sequence id")?;
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                let req = account::welcome_sequences(&waba, Some(&id), None);
                return list_sequences(app, &req).await;
            }
            Self::Delete { id } => {
                validate::numeric_id(&id, "sequence id")?;
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                if !app.confirm_or_abort(IN_ADS)? {
                    return Ok(());
                }
                account::delete_welcome_sequence(&waba, &id)
            }
            Self::Create { name, file } => {
                validate::not_blank(&name, "sequence name")?;
                let sequence = content(&file)?;
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                account::create_welcome_sequence(&waba, &name, &sequence)
            }
            Self::Update { id, name, file } => {
                validate::numeric_id(&id, "sequence id")?;
                if name.is_none() && file.is_none() {
                    return Err(Error::validation("pass --name or --file"));
                }
                if let Some(name) = &name {
                    validate::not_blank(name, "sequence name")?;
                }
                let sequence = file.as_deref().map(content).transpose()?;
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                if !app.confirm_or_abort(IN_ADS)? {
                    return Ok(());
                }
                account::update_welcome_sequence(&waba, &id, name.as_deref(), sequence.as_ref())
            }
        };
        app.send_and_emit(&req, |v| {
            v.get("sequence_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| no_id(v))
        })
        .await
    }
}
