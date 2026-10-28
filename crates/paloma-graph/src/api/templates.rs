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
    pub fields: Option<String>,
    pub limit: Option<u32>,
}

pub fn list(waba_id: &str, filter: &ListFilter, after: Option<&str>) -> Request {
    Request::get(format!("{waba_id}/message_templates"))
        .query_opt("name", filter.name.as_deref())
        .query_opt("status", filter.status.as_deref())
        .query_opt("category", filter.category.as_deref())
        .query_opt("language", filter.language.as_deref())
        .query_opt("fields", filter.fields.as_deref())
        .query_opt("limit", filter.limit)
        .query_opt("after", after)
}

/// Look up templates by exact name (and optionally language); Graph's `name` filter is a
/// prefix/contains match, so callers still filter on exact equality.
pub fn lookup(waba_id: &str, name: &str, language: Option<&str>) -> Request {
    lookup_page(waba_id, name, language, None)
}

/// One page of a [`lookup`], for following its cursor.
pub fn lookup_page(
    waba_id: &str,
    name: &str,
    language: Option<&str>,
    after: Option<&str>,
) -> Request {
    lookup_fields_page(waba_id, name, language, None, after)
}

/// The fields behind `template get --quality`.
pub const QUALITY_FIELDS: &str = "name,language,status,quality_score,health_status";
/// The fields a send needs to route a template by its latest category.
pub const CATEGORY_FIELDS: &str = "name,language,category";

/// [`lookup_page`] asking for specific fields.
pub fn lookup_fields_page(
    waba_id: &str,
    name: &str,
    language: Option<&str>,
    fields: Option<&str>,
    after: Option<&str>,
) -> Request {
    let filter = ListFilter {
        name: Some(name.to_owned()),
        language: language.map(str::to_owned),
        fields: fields.map(str::to_owned),
        limit: Some(100),
        ..ListFilter::default()
    };
    list(waba_id, &filter, after)
}

/// The latest category of the template named exactly `name` in `language`, from a lookup
/// made with [`CATEGORY_FIELDS`].
pub fn latest_category<'a>(response: &'a Value, name: &str, language: &str) -> Option<&'a str> {
    exact_matches(response, name, Some(language))
        .first()?
        .get("category")?
        .as_str()
}

/// Routing needs the completed category, not an impending `correct_category` change.
pub fn routing_lookup_page(
    account_id: &str,
    name: &str,
    language: &str,
    after: Option<&str>,
) -> Request {
    lookup_page(account_id, name, Some(language), after)
        .query("fields", "id,name,language,category")
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

/// Variable examples, in the shape the template's `parameter_format` calls for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Examples {
    Positional(Vec<String>),
    /// `(parameter name, example value)` pairs.
    Named(Vec<(String, String)>),
}

impl Examples {
    fn is_empty(&self) -> bool {
        match self {
            Self::Positional(values) => values.is_empty(),
            Self::Named(pairs) => pairs.is_empty(),
        }
    }

    fn named_params(pairs: &[(String, String)]) -> Value {
        pairs
            .iter()
            .map(|(name, value)| json!({ "param_name": name, "example": value }))
            .collect()
    }

    fn header(&self) -> Value {
        match self {
            Self::Positional(values) => json!({ "header_text": values }),
            Self::Named(pairs) => {
                json!({ "header_text_named_params": Self::named_params(pairs) })
            }
        }
    }

    fn body(&self) -> Value {
        match self {
            Self::Positional(values) => json!({ "body_text": [values] }),
            Self::Named(pairs) => json!({ "body_text_named_params": Self::named_params(pairs) }),
        }
    }
}

/// A template header. Media headers carry a Resumable Upload handle, not a media id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Header {
    Text { text: String, example: Examples },
    Media { format: String, handle: String },
}

/// A button with a typed inline form; the other button types stay with `--file`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Button {
    QuickReply {
        text: String,
    },
    Url {
        text: String,
        url: String,
        example: Option<String>,
    },
    PhoneNumber {
        text: String,
        phone_number: String,
    },
    CopyCode {
        example: String,
    },
}

impl Button {
    fn to_json(&self) -> Value {
        match self {
            Self::QuickReply { text } => json!({ "type": "QUICK_REPLY", "text": text }),
            Self::Url {
                text,
                url,
                example: None,
            } => json!({ "type": "URL", "text": text, "url": url }),
            Self::Url {
                text,
                url,
                example: Some(example),
            } => json!({ "type": "URL", "text": text, "url": url, "example": [example] }),
            Self::PhoneNumber { text, phone_number } => {
                json!({ "type": "PHONE_NUMBER", "text": text, "phone_number": phone_number })
            }
            Self::CopyCode { example } => json!({ "type": "COPY_CODE", "example": example }),
        }
    }
}

/// A template described by typed flags.
#[derive(Debug, Clone)]
pub struct InlineTemplate {
    pub name: String,
    pub category: String,
    pub language: String,
    pub parameter_format: Option<String>,
    pub header: Option<Header>,
    pub body: String,
    pub body_example: Examples,
    pub footer: Option<String>,
    pub buttons: Vec<Button>,
    pub allow_category_change: bool,
    pub ttl_seconds: Option<i64>,
}

/// The create payload for an [`InlineTemplate`]. Optional fields are left out when unset,
/// so Meta's defaults apply.
pub fn inline_template(t: &InlineTemplate) -> Value {
    let mut components = Vec::new();
    match &t.header {
        Some(Header::Text { text, example }) => {
            let mut header = json!({ "type": "HEADER", "format": "TEXT", "text": text });
            if !example.is_empty() {
                header["example"] = example.header();
            }
            components.push(header);
        }
        Some(Header::Media { format, handle }) => components.push(json!({
            "type": "HEADER",
            "format": format,
            "example": { "header_handle": [handle] },
        })),
        None => {}
    }
    let mut body = json!({ "type": "BODY", "text": t.body });
    if !t.body_example.is_empty() {
        body["example"] = t.body_example.body();
    }
    components.push(body);
    if let Some(footer) = &t.footer {
        components.push(json!({ "type": "FOOTER", "text": footer }));
    }
    if !t.buttons.is_empty() {
        let buttons: Vec<Value> = t.buttons.iter().map(Button::to_json).collect();
        components.push(json!({ "type": "BUTTONS", "buttons": buttons }));
    }
    let mut template = json!({
        "name": t.name,
        "category": t.category,
        "language": t.language,
        "components": components,
    });
    if let Some(format) = &t.parameter_format {
        template["parameter_format"] = json!(format);
    }
    if t.allow_category_change {
        template["allow_category_change"] = json!(true);
    }
    if let Some(ttl) = t.ttl_seconds {
        template["message_send_ttl_seconds"] = json!(ttl);
    }
    template
}

/// `POST /{template-id}`: edit an existing template.
pub fn edit(template_id: &str, body: Value) -> Request {
    Request::post(template_id).json(body)
}

/// Delete by name (every language), or one template via its `hsm_id`. Meta requires the
/// name alongside the id.
pub fn delete(waba_id: &str, name: &str, hsm_id: Option<&str>) -> Request {
    Request::delete(format!("{waba_id}/message_templates"))
        .query("name", name)
        .query_opt("hsm_id", hsm_id)
}

/// Delete several templates by id. `hsm_ids` is a JSON array and can't be combined with
/// `name` or `hsm_id`.
pub fn delete_many(waba_id: &str, ids: &[String]) -> Request {
    Request::delete(format!("{waba_id}/message_templates"))
        .query("hsm_ids", format!("[{}]", ids.join(",")))
}

/// `GET /{waba-id}?fields=message_template_namespace`.
pub fn namespace(waba_id: &str) -> Request {
    Request::get(waba_id).query("fields", "message_template_namespace")
}

fn hsm_ids(ids: &[String]) -> Value {
    json!({ "hsm_ids": ids })
}

/// `POST /{waba-id}/message_templates/archive`. Meta deletes an archived template after 28
/// days unless it is unarchived.
pub fn archive(waba_id: &str, ids: &[String]) -> Request {
    Request::post(format!("{waba_id}/message_templates/archive")).json(hsm_ids(ids))
}

/// `POST /{waba-id}/message_templates/unarchive`.
pub fn unarchive(waba_id: &str, ids: &[String]) -> Request {
    Request::post(format!("{waba_id}/message_templates/unarchive")).json(hsm_ids(ids))
}

/// How many templates a bulk response reports as failed. These requests return 200 when
/// only some fail. The migration page writes `failed_templates` as both a list and a map,
/// so either shape counts.
pub fn failed_count(response: &Value) -> usize {
    match response.get("failed_templates") {
        Some(Value::Object(failed)) => failed.len(),
        Some(Value::Array(failed)) => failed.len(),
        _ => 0,
    }
}

/// `POST /{template-id}/unpause`.
pub fn unpause(template_id: &str) -> Request {
    Request::post(format!("{template_id}/unpause"))
}

/// Filters for browsing the Template Library.
#[derive(Debug, Clone, Default)]
pub struct LibraryFilter {
    pub search: Option<String>,
    pub topic: Option<String>,
    pub usecase: Option<String>,
    pub industry: Option<String>,
    pub language: Option<String>,
    pub name: Option<String>,
}

/// `GET /message_template_library`. The library is Meta's, so the path has no WABA.
pub fn library_list(filter: &LibraryFilter, after: Option<&str>) -> Request {
    Request::get("message_template_library")
        .query_opt("search", filter.search.as_deref())
        .query_opt("topic", filter.topic.as_deref())
        .query_opt("usecase", filter.usecase.as_deref())
        .query_opt("industry", filter.industry.as_deref())
        .query_opt("language", filter.language.as_deref())
        .query_opt("name", filter.name.as_deref())
        .query_opt("after", after)
}

/// A template to create from a Template Library entry.
#[derive(Debug, Clone)]
pub struct LibraryTemplate {
    pub name: String,
    pub language: String,
    pub library_template: String,
    pub button_inputs: Option<Value>,
    pub body_inputs: Option<Value>,
}

/// The create payload for a [`LibraryTemplate`], for [`create`]. Meta's page writes
/// `library_template_button_inputs` as a quoted string that isn't valid JSON, but types it
/// as an array of objects, as its reference does, so that's what is sent. The same page says
/// the category must be `UTILITY` when creating from the library.
pub fn library_template(t: &LibraryTemplate) -> Value {
    let mut template = json!({
        "name": t.name,
        "category": "UTILITY",
        "language": t.language,
        "library_template_name": t.library_template,
    });
    if let Some(inputs) = &t.button_inputs {
        template["library_template_button_inputs"] = inputs.clone();
    }
    if let Some(inputs) = &t.body_inputs {
        template["library_template_body_inputs"] = inputs.clone();
    }
    template
}

/// `GET /{template-id}/compare`, against one other template, over `start..end` in UNIX
/// seconds.
pub fn compare(template_id: &str, other_id: &str, start: u64, end: u64) -> Request {
    Request::get(format!("{template_id}/compare"))
        .query("template_ids", format!("[{other_id}]"))
        .query("start", start)
        .query("end", end)
}

/// What to migrate. Unset fields are left to Meta's defaults.
#[derive(Debug, Clone, Default)]
pub struct Migration<'a> {
    pub source_waba_id: &'a str,
    pub page_number: Option<u32>,
    pub count: Option<u32>,
    pub template_ids: &'a [String],
}

/// `POST /{destination-waba-id}/migrate_message_templates`. This recreates the templates
/// in the destination; the source keeps its own.
pub fn migrate(destination_waba_id: &str, migration: &Migration<'_>) -> Request {
    let mut body = json!({ "source_waba_id": migration.source_waba_id });
    if let Some(page) = migration.page_number {
        body["page_number"] = json!(page);
    }
    if let Some(count) = migration.count {
        body["count"] = json!(count);
    }
    if !migration.template_ids.is_empty() {
        body["template_ids"] = json!(migration.template_ids);
    }
    Request::post(format!("{destination_waba_id}/migrate_message_templates")).json(body)
}

/// `GET /{waba-id}/message_template_previews` for authentication templates. Meta's table
/// marks `button_types` required and its syntax block optional, so it is always sent.
/// Meta's page names the language filter both `language` and `languages`, so neither is
/// sent; every language comes back and the caller narrows the result.
pub fn auth_previews(
    waba_id: &str,
    add_security_recommendation: bool,
    code_expiration_minutes: Option<u32>,
    after: Option<&str>,
) -> Request {
    Request::get(format!("{waba_id}/message_template_previews"))
        .query("category", "AUTHENTICATION")
        .query_opt(
            "add_security_recommendation",
            add_security_recommendation.then_some(true),
        )
        .query_opt("code_expiration_minutes", code_expiration_minutes)
        .query("button_types", "OTP")
        .query_opt("after", after)
}

/// `POST /{waba-id}/upsert_message_templates`: updates what exists by name and language,
/// and creates the rest.
pub fn upsert(waba_id: &str, body: Value) -> Request {
    Request::post(format!("{waba_id}/upsert_message_templates")).json(body)
}

/// `POST /{waba-id}/template_groups`. Unlike other template id lists, Meta types these as
/// integers.
pub fn group_create(
    waba_id: &str,
    name: &str,
    description: Option<&str>,
    template_ids: &[u64],
) -> Request {
    let mut body = json!({ "name": name, "whatsapp_business_templates": template_ids });
    if let Some(description) = description {
        body["description"] = json!(description);
    }
    Request::post(format!("{waba_id}/template_groups")).json(body)
}

/// `GET /{group-id}`.
pub fn group_get(group_id: &str) -> Request {
    Request::get(group_id)
}

/// A partial update of a template group. Unset fields stay as they are.
#[derive(Debug, Clone, Copy, Default)]
pub struct GroupUpdate<'a> {
    pub name: Option<&'a str>,
    pub description: Option<&'a str>,
    pub add_templates: &'a [u64],
    pub remove_templates: &'a [u64],
}

/// `POST /{group-id}`.
pub fn group_update(group_id: &str, update: &GroupUpdate<'_>) -> Request {
    let mut body = json!({});
    if let Some(name) = update.name {
        body["name"] = json!(name);
    }
    if let Some(description) = update.description {
        body["description"] = json!(description);
    }
    if !update.add_templates.is_empty() {
        body["add_templates"] = json!(update.add_templates);
    }
    if !update.remove_templates.is_empty() {
        body["remove_templates"] = json!(update.remove_templates);
    }
    Request::post(group_id).json(body)
}

/// `DELETE /{group-id}`. The templates in the group stay.
pub fn group_delete(group_id: &str) -> Request {
    Request::delete(group_id)
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
fn analytics_request(node: String, query: &AnalyticsQuery<'_>, after: Option<&str>) -> Request {
    let metrics = (!query.metrics.is_empty()).then(|| query.metrics.join(","));
    Request::get(node)
        .query("granularity", "daily")
        .query("start", query.start)
        .query("end", query.end)
        .query_opt("metric_types", metrics)
        .query_opt("use_waba_timezone", query.waba_timezone.then_some(true))
        .query_opt("after", after)
}

fn id_array(ids: &[String]) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal() -> InlineTemplate {
        InlineTemplate {
            name: "t".into(),
            category: "UTILITY".into(),
            language: "en_US".into(),
            parameter_format: None,
            header: None,
            body: "Hi".into(),
            body_example: Examples::Positional(Vec::new()),
            footer: None,
            buttons: Vec::new(),
            allow_category_change: false,
            ttl_seconds: None,
        }
    }

    #[test]
    fn minimal_inline_template_matches_the_body_only_shape() {
        assert_eq!(
            inline_template(&minimal()),
            inline_body("t", "UTILITY", "en_US", "Hi", &[])
        );
    }

    #[test]
    fn named_examples_use_the_named_param_shape() {
        let mut t = minimal();
        t.body = "Hi {{first_name}}".into();
        t.body_example = Examples::Named(vec![("first_name".into(), "Pablo".into())]);
        t.header = Some(Header::Text {
            text: "For {{who}}".into(),
            example: Examples::Named(vec![("who".into(), "you".into())]),
        });
        let components = &inline_template(&t)["components"];
        assert_eq!(
            components[0]["example"],
            json!({ "header_text_named_params": [{ "param_name": "who", "example": "you" }] })
        );
        assert_eq!(
            components[1]["example"],
            json!({ "body_text_named_params": [{ "param_name": "first_name", "example": "Pablo" }] })
        );
    }

    #[test]
    fn failed_templates_count_as_a_list_or_a_map() {
        assert_eq!(
            failed_count(&json!({ "failed_templates": { "1": "x" } })),
            1
        );
        assert_eq!(failed_count(&json!({ "failed_templates": ["1", "2"] })), 2);
        assert_eq!(failed_count(&json!({ "archived_templates": ["1"] })), 0);
    }

    #[test]
    fn latest_category_needs_the_exact_name_and_language() {
        let page = json!({ "data": [
            { "name": "sale_2", "language": "en_US", "category": "UTILITY" },
            { "name": "sale", "language": "es", "category": "UTILITY" },
            { "name": "sale", "language": "en_US", "category": "MARKETING" },
        ]});
        assert_eq!(latest_category(&page, "sale", "en_US"), Some("MARKETING"));
        assert_eq!(latest_category(&page, "sale", "fr"), None);
    }

    #[test]
    fn a_group_update_sends_only_what_changes() {
        let update = GroupUpdate {
            add_templates: &[1],
            ..GroupUpdate::default()
        };
        assert_eq!(
            group_update("9", &update).json_body(),
            Some(&json!({ "add_templates": [1] }))
        );
    }

    #[test]
    fn bulk_delete_sends_only_hsm_ids() {
        let r = delete_many("222", &["1".into(), "2".into()]);
        assert_eq!(
            r.url("https://g", "v1"),
            "https://g/v1/222/message_templates?hsm_ids=%5B1%2C2%5D"
        );
    }
}
