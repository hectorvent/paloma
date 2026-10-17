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
    let filter = ListFilter {
        name: Some(name.to_owned()),
        language: language.map(str::to_owned),
        limit: Some(100),
        ..ListFilter::default()
    };
    list(waba_id, &filter, after)
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

/// The first lookup page holding `name` exactly (and `language`, if given), following the
/// cursor only as far as needed. Graph's `name` filter also returns other names, so a
/// page can be full without holding the one asked for.
pub async fn find_page(
    client: &crate::Client,
    waba_id: &str,
    name: &str,
    language: Option<&str>,
) -> paloma_core::Result<Option<Value>> {
    crate::paging::find_page(
        client,
        |after| lookup_page(waba_id, name, language, after),
        |page| !exact_matches(page, name, language).is_empty(),
    )
    .await
}

/// Find the single template matching `name` (and `language`, if given) by executing a
/// lookup. With a language, the first match ends the search. Without one, every page is
/// read first, because a translation on a later page makes the name ambiguous. Several
/// languages without `--language` is a validation error, because acting on an arbitrary
/// one would be a surprise.
pub async fn find_one(
    client: &crate::Client,
    waba_id: &str,
    name: &str,
    language: Option<&str>,
) -> paloma_core::Result<Value> {
    use paloma_core::Error;
    let page = match language {
        Some(_) => find_page(client, waba_id, name, language).await?,
        None => Some(
            crate::paging::collect_all(client, None, |after| {
                lookup_page(waba_id, name, None, after)
            })
            .await?,
        ),
    }
    .unwrap_or_default();
    let matches = exact_matches(&page, name, language);
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

/// Template namespace belongs to the WABA node, not a particular template.
pub fn namespace(waba_id: &str) -> Request {
    Request::get(waba_id).query("fields", "message_template_namespace")
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
    fn bulk_delete_sends_only_hsm_ids() {
        let r = delete_many("222", &["1".into(), "2".into()]);
        assert_eq!(
            r.url("https://g", "v1"),
            "https://g/v1/222/message_templates?hsm_ids=%5B1%2C2%5D"
        );
    }
}
