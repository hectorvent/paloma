//! Message templates (`/{waba-id}/message_templates`).

use paloma_core::{Error, Result, validate};

use crate::Request;
use serde_json::{Value, json};
mod analytics;
mod group;
mod inline;
mod library;
pub use analytics::*;
pub use group::*;
pub use inline::*;
pub use library::*;

/// Filters for listing templates.
#[derive(Debug, Clone, Default)]
pub struct ListFilter {
    pub name: Option<String>,
    pub status: Option<String>,
    pub category: Option<paloma_core::TemplateCategory>,
    pub language: Option<String>,
    pub fields: Option<String>,
    pub limit: Option<u32>,
}

pub fn list(waba_id: &str, filter: &ListFilter, after: Option<&str>) -> Request {
    Request::get(format!("{waba_id}/message_templates"))
        .query_opt("name", filter.name.as_deref())
        .query_opt("status", filter.status.as_deref())
        .query_opt(
            "category",
            filter.category.map(paloma_core::TemplateCategory::as_str),
        )
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

/// Rows per lookup page. Paloma's choice, not Meta's limit (none is documented): the name
/// filter also matches other names, so a bigger page means fewer round trips.
const LOOKUP_PAGE_SIZE: u32 = 100;

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
        limit: Some(LOOKUP_PAGE_SIZE),
        ..ListFilter::default()
    };
    list(waba_id, &filter, after)
}

/// The latest category of the template named exactly `name` in `language`, from a lookup
/// that asked for `category`.
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

/// What editing a live template confirms: an approved one goes back to review.
pub const EDIT_CONFIRM: &str = "edit a live template; an approved template goes back to review";

pub fn create(waba_id: &str, body: Value) -> Request {
    Request::post(format!("{waba_id}/message_templates")).json(body)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positional_examples_need_one_value_per_variable_numbered_from_one() {
        let ok = examples(
            "Hi {{1}}, order {{2}}",
            &["Ana".into(), "7".into()],
            false,
            "e",
        );
        assert_eq!(
            ok.unwrap(),
            Examples::Positional(vec!["Ana".into(), "7".into()])
        );
        assert!(
            examples("Hi {{1}}", &[], false, "e").is_err(),
            "a value is missing"
        );
        assert!(
            examples("Hi {{2}}", &["x".into()], false, "e").is_err(),
            "numbering has a gap"
        );
        assert!(
            examples("Hi {{name}}", &["x".into()], false, "e").is_err(),
            "not positional"
        );
        assert!(
            examples("Hi {{1}}", &[" ".into()], false, "e").is_err(),
            "blank value"
        );
    }

    #[test]
    fn named_examples_need_a_pair_for_each_variable_in_the_text() {
        let ok = examples("Hi {{name}}", &["name=Ana".into()], true, "e").unwrap();
        assert_eq!(ok, Examples::Named(vec![("name".into(), "Ana".into())]));
        assert!(examples("Hi {{name}}", &["other=Ana".into()], true, "e").is_err());
        assert!(examples("Hi {{name}}", &["Ana".into()], true, "e").is_err());
        assert_eq!(distinct_variables("{{b}} {{a}} {{b}}"), ["a", "b"]);
    }

    fn minimal() -> InlineTemplate {
        InlineTemplate {
            name: "t".into(),
            category: paloma_core::TemplateCategory::Utility,
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
