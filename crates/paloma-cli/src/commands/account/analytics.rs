//! Report arguments are validated before account resolution. Only table output flattens
//! pricing groups; structured formats keep Meta's envelope and missing costs intact.

use crate::app::App;
use crate::cli::Format;
use crate::output::Output;
use clap::{Args, ValueEnum};
use paloma_core::{Error, Result, validate};
use paloma_graph::api::analytics::{self, PricingFilters, Query};
use serde_json::{Value, json};

const MESSAGE_COLUMNS: &[&str] = &["start", "end", "sent", "delivered"];
const PRICING_COLUMNS: &[&str] = &[
    "start",
    "end",
    "phone_number",
    "country",
    "pricing_category",
    "pricing_type",
    "tier",
    "volume",
    "cost",
];

#[derive(Debug, Args)]
pub struct Analytics {
    /// Start boundary: UNIX seconds or YYYY-MM-DD (midnight UTC).
    #[arg(long, value_name = "TIME", allow_hyphen_values = true)]
    start: String,
    /// End boundary: UNIX seconds or YYYY-MM-DD; must be later than --start.
    #[arg(long, value_name = "TIME", allow_hyphen_values = true)]
    end: String,
    /// Reporting interval; translated to the API-specific granularity value.
    #[arg(long, value_enum, default_value = "day")]
    granularity: Granularity,
    /// Query pricing/cost and delivered volume instead of sent/delivered counts.
    #[arg(long)]
    pricing: bool,
    /// Only this business phone number, in E.164 (not its id); repeat it or use commas.
    #[arg(long = "phone", value_delimiter = ',', value_name = "E164,...")]
    phone_number: Vec<String>,
    /// Uppercase two-letter country code; repeat or comma-separate.
    #[arg(long, value_delimiter = ',')]
    country: Vec<String>,
    /// Messaging-only message class; repeat or comma-separate.
    #[arg(long, value_enum, value_delimiter = ',', conflicts_with = "pricing")]
    product_type: Vec<ProductType>,
    /// Pricing-only metrics; omitted means all available metrics.
    #[arg(long, value_enum, value_delimiter = ',', requires = "pricing")]
    metric: Vec<Metric>,
    /// Pricing-only charge type; repeat or comma-separate.
    #[arg(long, value_enum, value_delimiter = ',', requires = "pricing")]
    pricing_type: Vec<PricingType>,
    /// Pricing-only category; repeat or comma-separate.
    #[arg(long, value_enum, value_delimiter = ',', requires = "pricing")]
    pricing_category: Vec<PricingCategory>,
    /// Pricing-only breakdown; repeat or comma-separate.
    #[arg(long, value_enum, value_delimiter = ',', requires = "pricing")]
    dimension: Vec<Dimension>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Granularity {
    HalfHour,
    Day,
    Month,
}
impl From<Granularity> for analytics::Granularity {
    fn from(value: Granularity) -> Self {
        match value {
            Granularity::HalfHour => Self::HalfHour,
            Granularity::Day => Self::Day,
            Granularity::Month => Self::Month,
        }
    }
}
#[derive(Debug, Clone, Copy, ValueEnum)]
enum ProductType {
    Template,
    NonTemplate,
    Incoming,
}
impl ProductType {
    const fn api(self) -> u16 {
        match self {
            Self::Template => 0,
            Self::NonTemplate => 2,
            Self::Incoming => 100,
        }
    }
}
#[derive(Debug, Clone, Copy, ValueEnum)]
enum Metric {
    Cost,
    Volume,
}
#[derive(Debug, Clone, Copy, ValueEnum)]
enum PricingType {
    FreeCustomerService,
    FreeEntryPoint,
    Regular,
}
#[derive(Debug, Clone, Copy, ValueEnum)]
enum PricingCategory {
    Authentication,
    AuthenticationInternational,
    Marketing,
    MarketingLite,
    MarketingLiteDynamic,
    Service,
    Utility,
    ReferralConversion,
}
#[derive(Debug, Clone, Copy, ValueEnum)]
enum Dimension {
    Country,
    Phone,
    PricingCategory,
    PricingType,
    Tier,
}

fn api_values(values: &[impl ValueEnum]) -> Vec<String> {
    values
        .iter()
        .map(|v| {
            crate::commands::util::enum_name(v)
                .replace('-', "_")
                .to_ascii_uppercase()
        })
        .collect()
}

impl Analytics {
    pub async fn run(self, app: &App) -> Result<()> {
        let start = crate::commands::util::timestamp(&self.start, "--start")?;
        let end = crate::commands::util::timestamp(&self.end, "--end")?;
        validate::analytics_range(start, end)?;
        self.phone_number
            .iter()
            .try_for_each(|p| validate::analytics_phone_number(p))?;
        self.country
            .iter()
            .try_for_each(|c| validate::country_code(c))?;
        let query = Query {
            start,
            end,
            granularity: self.granularity.into(),
            phone_numbers: self.phone_number,
            country_codes: self.country,
        };
        let waba = app.ctx()?.require_waba_id()?;
        let request = if self.pricing {
            analytics::pricing(
                waba,
                &query,
                &PricingFilters {
                    metric_types: api_values(&self.metric),
                    pricing_types: api_values(&self.pricing_type),
                    pricing_categories: api_values(&self.pricing_category),
                    dimensions: api_values(&self.dimension),
                },
            )
        } else {
            analytics::messaging(
                waba,
                &query,
                &self
                    .product_type
                    .into_iter()
                    .map(ProductType::api)
                    .collect::<Vec<_>>(),
            )
        };
        if let Some(response) = app.send(&request).await? {
            let mut out = Output::response(&response)?;
            if self.pricing {
                let rows = pricing_rows(&out.data)?;
                if app.out.format() == Format::Table {
                    out.data = json!({"data":rows});
                    out = out.table("/data", PRICING_COLUMNS);
                }
            } else {
                points(out.data.pointer("/analytics/data_points"))?;
                out = out.table("/analytics/data_points", MESSAGE_COLUMNS);
            }
            app.out.emit(&out);
        }
        Ok(())
    }
}

fn points(value: Option<&Value>) -> Result<&Vec<Value>> {
    value
        .and_then(Value::as_array)
        .filter(|rows| rows.iter().all(Value::is_object))
        .ok_or_else(|| {
            Error::other(
                "Graph returned malformed analytics data_points; expected an array of objects",
            )
        })
}

fn pricing_rows(value: &Value) -> Result<Vec<Value>> {
    let groups = value
        .pointer("/pricing_analytics/data")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            Error::other("Graph returned malformed pricing_analytics.data; expected an array")
        })?;
    let mut rows = Vec::new();
    for group in groups {
        rows.extend(points(group.get("data_points"))?.iter().cloned());
    }
    Ok(rows)
}
