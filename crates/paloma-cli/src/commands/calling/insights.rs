//! Read-only calling views. Permission depends on the person being asked about, so it's
//! addressed by the same phone-or-BSUID rules as a send.

use clap::{Args, ValueEnum};
use paloma_core::{Result, validate};
use paloma_graph::api::analytics::{self, CallFilters, Query};
use paloma_graph::api::calling;
use serde_json::Value;

use crate::app::App;
use crate::output::Output;

const COLUMNS: &[&str] = &["start", "end", "count", "cost", "average_duration"];

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Granularity {
    HalfHour,
    Day,
    Month,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Direction {
    UserInitiated,
    BusinessInitiated,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Dimension {
    Phone,
    Direction,
    Country,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Metric {
    Count,
    Cost,
    AverageDuration,
}

#[derive(Debug, Args)]
pub struct Analytics {
    /// Start boundary: UNIX seconds or YYYY-MM-DD (midnight UTC).
    #[arg(long, value_name = "TIME")]
    start: String,
    /// End boundary: UNIX seconds or YYYY-MM-DD; must be later than --start.
    #[arg(long, value_name = "TIME")]
    end: String,
    #[arg(long, value_enum, default_value = "day")]
    granularity: Granularity,
    /// Only this business phone number, in E.164 (not its id); repeat it or use commas.
    #[arg(long = "phone", value_delimiter = ',', value_name = "E164,...")]
    phone_number: Vec<String>,
    /// Uppercase two-letter country code; repeat or comma-separate.
    #[arg(long, value_delimiter = ',')]
    country: Vec<String>,
    /// Call direction; repeat or comma-separate.
    #[arg(long, value_enum, value_delimiter = ',')]
    direction: Vec<Direction>,
    /// Break results down; repeat or comma-separate.
    #[arg(long, value_enum, value_delimiter = ',')]
    dimension: Vec<Dimension>,
    /// Metrics to return, all of them when omitted; repeat or comma-separate.
    #[arg(long, value_enum, value_delimiter = ',')]
    metric: Vec<Metric>,
}

impl Analytics {
    pub async fn run(self, app: &App) -> Result<()> {
        let start = super::super::util::timestamp(&self.start, "--start")?;
        let end = super::super::util::timestamp(&self.end, "--end")?;
        validate::analytics_range(start, end)?;
        self.phone_number
            .iter()
            .try_for_each(|p| validate::analytics_phone_number(p))?;
        self.country
            .iter()
            .try_for_each(|c| validate::country_code(c))?;
        let filters = CallFilters {
            directions: self
                .direction
                .iter()
                .map(|d| match d {
                    Direction::UserInitiated => "USER_INITIATED",
                    Direction::BusinessInitiated => "BUSINESS_INITIATED",
                })
                .collect(),
            dimensions: self
                .dimension
                .iter()
                .map(|d| match d {
                    Dimension::Phone => "phone",
                    Dimension::Direction => "direction",
                    Dimension::Country => "country",
                })
                .collect(),
            metric_types: self
                .metric
                .iter()
                .map(|m| match m {
                    Metric::Count => "COUNT",
                    Metric::Cost => "COST",
                    Metric::AverageDuration => "AVERAGE_DURATION",
                })
                .collect(),
        };
        let query = Query {
            start,
            end,
            granularity: match self.granularity {
                Granularity::HalfHour => analytics::Granularity::HalfHour,
                Granularity::Day => analytics::Granularity::Day,
                Granularity::Month => analytics::Granularity::Month,
            },
            phone_numbers: self.phone_number,
            country_codes: self.country,
        };
        let waba = app.ctx()?.require_waba_id()?;
        let request = analytics::calls(waba, &query, &filters);
        if let Some(response) = app.send(&request).await? {
            app.out
                .emit(&Output::response(&response)?.table("/call_analytics/data_points", COLUMNS));
        }
        Ok(())
    }
}

/// Read whether this person can be called or asked for permission.
pub async fn permission(app: &App, user: &str) -> Result<()> {
    validate::recipient(user)?;
    let phone = app.ctx()?.require_phone_number_id()?.to_owned();
    app.send_and_emit(&calling::permissions(&phone, user), |v: &Value| {
        v.pointer("/permission/status")
            .and_then(Value::as_str)
            .map(str::to_owned)
    })
    .await
}
