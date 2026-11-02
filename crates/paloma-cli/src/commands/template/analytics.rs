//! Template analytics and the side-by-side comparison of two templates.

#[allow(clippy::wildcard_imports)] // part of `template`: shares its imports and helpers
use super::*;

#[derive(Debug, Args)]
pub struct Compare {
    /// Template id.
    pub(super) id: String,
    /// The template id to compare it with, on the same WABA.
    pub(super) other_id: String,
    /// Days to look back from now: 7, 30, 60 or 90.
    #[arg(long, value_parser = compare_window, value_name = "DAYS")]
    pub(super) days: u64,
}

pub(super) fn compare_window(value: &str) -> std::result::Result<u64, String> {
    value
        .parse()
        .ok()
        .filter(|days| limits::TEMPLATE_COMPARE_WINDOW_DAYS.contains(days))
        .ok_or_else(|| format!("must be one of {:?}", limits::TEMPLATE_COMPARE_WINDOW_DAYS))
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum ProductType {
    CloudApi,
    MarketingMessagesLiteApi,
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("subject").required(true).args(["ids", "groups"])))]
pub struct Analytics {
    /// Template id; repeat it or separate ids with commas (up to 10).
    #[arg(long = "id", value_delimiter = ',', value_name = "ID,...")]
    pub(super) ids: Vec<String>,
    /// Template group id instead; repeat it or separate ids with commas (up to 10).
    #[arg(long = "group", value_delimiter = ',', value_name = "ID,...")]
    pub(super) groups: Vec<String>,
    /// Start of the range: UNIX seconds or YYYY-MM-DD, within the last 90 days.
    #[arg(long, value_name = "TIME")]
    pub(super) start: String,
    /// End of the range: UNIX seconds or YYYY-MM-DD.
    #[arg(long, value_name = "TIME")]
    pub(super) end: String,
    /// Metric to return, e.g. sent; repeat it or separate metrics with commas. All of them
    /// by default.
    #[arg(long = "metric", value_delimiter = ',', value_name = "METRIC,...")]
    pub(super) metrics: Vec<String>,
    /// Which API's sends to count. Meta counts only the Cloud API's by default.
    #[arg(long, value_enum, conflicts_with = "groups")]
    pub(super) product_type: Option<ProductType>,
    /// Use the WABA's timezone instead of UTC; --start and --end must then be YYYY-MM-DD.
    #[arg(long)]
    pub(super) waba_timezone: bool,
    /// Start after this cursor.
    #[arg(long)]
    pub(super) after: Option<String>,
}

pub(super) async fn compare(app: &App, a: &Compare) -> Result<()> {
    validate::template_id(&a.id)?;
    validate::template_id(&a.other_id)?;
    let end = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| Error::other(format!("the system clock is before 1970: {e}")))?
        .as_secs();
    let start = end.saturating_sub(a.days * 86_400);
    let req = templates::compare(&a.id, &a.other_id, start, end);
    if let Some(resp) = app.send(&req).await? {
        app.out
            .emit(&Output::response(&resp)?.table("/data", COMPARE_COLUMNS));
    }
    Ok(())
}

pub(super) async fn analytics(app: &App, a: &Analytics) -> Result<()> {
    validate::template_analytics_range(&a.start, &a.end, a.waba_timezone)?;
    validate::template_ids_within(&a.ids, limits::MAX_TEMPLATE_ANALYTICS_IDS)?;
    if a.groups.len() > limits::MAX_TEMPLATE_GROUP_ANALYTICS_IDS {
        return Err(Error::validation(format!(
            "{} template group ids given; max is {} per request",
            a.groups.len(),
            limits::MAX_TEMPLATE_GROUP_ANALYTICS_IDS
        )));
    }
    for id in &a.groups {
        validate::numeric_id(id, "template group id")?;
    }
    for metric in &a.metrics {
        validate::not_blank(metric, "--metric")?;
    }
    let waba = app.ctx()?.require_waba_id()?.to_owned();
    let query = AnalyticsQuery {
        start: &a.start,
        end: &a.end,
        metrics: &a.metrics,
        waba_timezone: a.waba_timezone,
    };
    let after = a.after.as_deref();
    let (req, columns) = if a.groups.is_empty() {
        let product_type = a.product_type.as_ref().map(enum_name);
        (
            templates::analytics(&waba, &a.ids, product_type.as_deref(), &query, after),
            ANALYTICS_COLUMNS,
        )
    } else {
        (
            templates::group_analytics(&waba, &a.groups, &query, after),
            GROUP_ANALYTICS_COLUMNS,
        )
    };
    if let Some(resp) = app.send(&req).await? {
        let out = Output::response(&resp)?;
        if let Some(next) = paging::next_cursor(&out.data) {
            app.out
                .note(format!("more results exist: page with --after {next}"));
        }
        if app.out.format() == Format::Table {
            let rows: Vec<Value> = out
                .data
                .get("data")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .flat_map(|group| {
                    group
                        .get("data_points")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                })
                .cloned()
                .collect();
            app.out
                .emit(&Output::new(json!({"data": rows})).table("/data", columns));
        } else {
            app.out.emit(&out);
        }
    }
    Ok(())
}
