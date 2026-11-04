//! Template analytics and the side-by-side comparison of two templates.

#[allow(clippy::wildcard_imports)] // part of `templates`: shares its imports and helpers
use super::*;

/// `GET /{template-id}/compare`, against one other template, over `start..end` in UNIX
/// seconds.
pub fn compare(template_id: &str, other_id: &str, start: u64, end: u64) -> Request {
    Request::get(format!("{template_id}/compare"))
        .query("template_ids", format!("[{other_id}]"))
        .query("start", start)
        .query("end", end)
}

/// The window and metrics of an analytics request.
#[derive(Debug, Clone, Copy, Default)]
pub struct AnalyticsQuery<'a> {
    pub start: &'a str,
    pub end: &'a str,
    pub metrics: &'a [String],
    pub waba_timezone: bool,
}

/// The parameters both analytics edges share. Meta's table says `DAILY` and its example
/// requests say `daily`, on the same page; the requests are what is sent.
pub(super) fn analytics_request(
    node: String,
    query: &AnalyticsQuery<'_>,
    after: Option<&str>,
) -> Request {
    let metrics = (!query.metrics.is_empty()).then(|| query.metrics.join(","));
    Request::get(node)
        .query("granularity", "daily")
        .query("start", query.start)
        .query("end", query.end)
        .query_opt("metric_types", metrics)
        .query_opt("use_waba_timezone", query.waba_timezone.then_some(true))
        .query_opt("after", after)
}

pub(super) fn id_array(ids: &[String]) -> String {
    format!("[{}]", ids.join(","))
}

/// `GET /{waba-id}/template_analytics`.
pub fn analytics(
    waba_id: &str,
    template_ids: &[String],
    product_type: Option<&str>,
    query: &AnalyticsQuery<'_>,
    after: Option<&str>,
) -> Request {
    analytics_request(format!("{waba_id}/template_analytics"), query, after)
        .query("template_ids", id_array(template_ids))
        .query_opt("product_type", product_type)
}

/// `GET /{waba-id}/template_group_analytics`.
pub fn group_analytics(
    waba_id: &str,
    group_ids: &[String],
    query: &AnalyticsQuery<'_>,
    after: Option<&str>,
) -> Request {
    analytics_request(format!("{waba_id}/template_group_analytics"), query, after)
        .query("template_group_ids", id_array(group_ids))
}
