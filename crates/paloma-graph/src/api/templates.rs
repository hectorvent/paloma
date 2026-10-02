//! Message templates (`/{waba-id}/message_templates`).

use serde_json::{Value, json};

use crate::Request;

/// Filters for listing templates.
#[derive(Debug, Clone, Default)]
pub struct ListFilter {
    pub name: Option<String>,
    pub status: Option<String>,
    pub category: Option<String>,
    pub language: Option<String>,
    pub limit: Option<u32>,
}

pub fn list(waba_id: &str, filter: &ListFilter, after: Option<&str>) -> Request {
    Request::get(format!("{waba_id}/message_templates"))
        .query_opt("name", filter.name.as_deref())
        .query_opt("status", filter.status.as_deref())
        .query_opt("category", filter.category.as_deref())
        .query_opt("language", filter.language.as_deref())
        .query_opt("limit", filter.limit)
        .query_opt("after", after)
}

/// Look up templates by exact name (and optionally language); Graph's `name` filter is a
/// prefix/contains match, so callers still filter on exact equality.
pub fn lookup(waba_id: &str, name: &str, language: Option<&str>) -> Request {
    let filter = ListFilter {
        name: Some(name.to_owned()),
        language: language.map(str::to_owned),
        limit: Some(100),
        ..ListFilter::default()
    };
    list(waba_id, &filter, None)
}

/// The entries of a lookup response whose name matches exactly (and language, if given).
pub fn exact_matches<'a>(
    response: &'a Value,
    name: &str,
    language: Option<&str>,
) -> Vec<&'a Value> {
    response
        .get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|t| t.get("name").and_then(Value::as_str) == Some(name))
                .filter(|t| {
                    language.is_none_or(|l| t.get("language").and_then(Value::as_str) == Some(l))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn create(waba_id: &str, body: Value) -> Request {
    Request::post(format!("{waba_id}/message_templates")).json(body)
}

/// A minimal BODY-only template, with optional example values for its variables.
pub fn inline_body(
    name: &str,
    category: &str,
    language: &str,
    body: &str,
    example: &[String],
) -> Value {
    let mut component = json!({ "type": "BODY", "text": body });
    if !example.is_empty() {
        component["example"] = json!({ "body_text": [example] });
    }
    json!({ "name": name, "category": category, "language": language, "components": [component] })
}

/// `POST /{template-id}`: edit an existing template.
pub fn edit(template_id: &str, body: Value) -> Request {
    Request::post(template_id).json(body)
}

/// Delete by name (every language), or one language via its `hsm_id`.
pub fn delete(waba_id: &str, name: &str, hsm_id: Option<&str>) -> Request {
    Request::delete(format!("{waba_id}/message_templates"))
        .query("name", name)
        .query_opt("hsm_id", hsm_id)
}

/// Find the single template matching `name` (and `language`, if given) by executing a
/// lookup. Several languages without `--language` is a validation error, because acting
/// on an arbitrary one would be a surprise.
pub async fn find_one(
    client: &crate::Client,
    waba_id: &str,
    name: &str,
    language: Option<&str>,
) -> paloma_core::Result<Value> {
    use paloma_core::Error;
    let resp = client
        .execute(&lookup(waba_id, name, language))
        .await?
        .json()?;
    let matches = exact_matches(&resp, name, language);
    match matches.as_slice() {
        [] => Err(Error::other(format!(
            "no template named '{name}'{} on this WABA",
            language.map(|l| format!(" ({l})")).unwrap_or_default()
        ))),
        [one] => Ok((*one).clone()),
        _ if language.is_none() => Err(Error::validation(format!(
            "template '{name}' exists in multiple languages; pass --language"
        ))),
        [first, ..] => Ok((*first).clone()),
    }
}
