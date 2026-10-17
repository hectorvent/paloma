//! Analytics is a field expansion on the account node. Its two APIs use different
//! granularity names, and filter arrays must be JSON inside the expansion expression.

use crate::Request;
use serde::Serialize;
use std::fmt::Write as _;

#[derive(Debug, Clone, Copy)]
pub enum Granularity {
    HalfHour,
    Day,
    Month,
}

impl Granularity {
    const fn api(self, pricing: bool) -> &'static str {
        match (self, pricing) {
            (Self::HalfHour, _) => "HALF_HOUR",
            (Self::Day, false) => "DAY",
            (Self::Month, false) => "MONTH",
            (Self::Day, true) => "DAILY",
            (Self::Month, true) => "MONTHLY",
        }
    }
}

#[derive(Debug)]
pub struct Query {
    pub start: u64,
    pub end: u64,
    pub granularity: Granularity,
    pub phone_numbers: Vec<String>,
    pub country_codes: Vec<String>,
}

#[derive(Debug, Default)]
pub struct PricingFilters {
    pub metric_types: Vec<String>,
    pub pricing_types: Vec<String>,
    pub pricing_categories: Vec<String>,
    pub dimensions: Vec<String>,
}

fn base(query: &Query, pricing: bool) -> String {
    let field = if pricing {
        "pricing_analytics"
    } else {
        "analytics"
    };
    let mut fields = format!(
        "{field}.start({}).end({}).granularity({})",
        query.start,
        query.end,
        query.granularity.api(pricing)
    );
    let phones: Vec<_> = query
        .phone_numbers
        .iter()
        .map(|p| p.trim().trim_start_matches('+'))
        .collect();
    append(&mut fields, "phone_numbers", &phones);
    fields
}

fn append<T: Serialize>(fields: &mut String, name: &str, values: &[T]) {
    if !values.is_empty() {
        let array =
            serde_json::to_string(values).expect("analytics filter arrays contain only primitives");
        let _ = write!(fields, ".{name}({array})");
    }
}

pub fn messaging(account: &str, query: &Query, product_types: &[u16]) -> Request {
    let mut fields = base(query, false);
    append(&mut fields, "product_types", product_types);
    append(&mut fields, "country_codes", &query.country_codes);
    Request::get(account).query("fields", fields)
}

pub fn pricing(account: &str, query: &Query, filters: &PricingFilters) -> Request {
    let mut fields = base(query, true);
    append(&mut fields, "country_codes", &query.country_codes);
    append(&mut fields, "metric_types", &filters.metric_types);
    append(&mut fields, "pricing_types", &filters.pricing_types);
    append(
        &mut fields,
        "pricing_categories",
        &filters.pricing_categories,
    );
    append(&mut fields, "dimensions", &filters.dimensions);
    Request::get(account).query("fields", fields)
}
