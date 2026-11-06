//! Templates built from typed fields, and the example checks Meta's review applies to
//! them, shared by `template create` and the Template manifest's body shorthand.

use super::{Error, Result, Value, json, validate};

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
    pub(super) fn is_empty(&self) -> bool {
        match self {
            Self::Positional(values) => values.is_empty(),
            Self::Named(pairs) => pairs.is_empty(),
        }
    }

    pub(super) fn named_params(pairs: &[(String, String)]) -> Value {
        pairs
            .iter()
            .map(|(name, value)| json!({ "param_name": name, "example": value }))
            .collect()
    }

    pub(super) fn header(&self) -> Value {
        match self {
            Self::Positional(values) => json!({ "header_text": values }),
            Self::Named(pairs) => {
                json!({ "header_text_named_params": Self::named_params(pairs) })
            }
        }
    }

    pub(super) fn body(&self) -> Value {
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
    pub(super) fn to_json(&self) -> Value {
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

/// The distinct `{{...}}` variables in template text, sorted.
pub fn distinct_variables(text: &str) -> Vec<&str> {
    let mut variables = validate::template_variables(text);
    variables.sort_unstable();
    variables.dedup();
    variables
}

/// Check `given` covers the variables in `text`, as Meta requires an example for each, and
/// shape them for the parameter format.
pub fn examples(text: &str, given: &[String], named: bool, flag: &str) -> Result<Examples> {
    let variables = distinct_variables(text);
    if named {
        return named_examples(&variables, given, flag);
    }
    let mut numbers = Vec::with_capacity(variables.len());
    for variable in &variables {
        let Ok(number) = variable.parse::<usize>() else {
            return Err(Error::validation(format!(
                "variable {{{{{variable}}}}} isn't positional{}",
                if flag.starts_with("--") {
                    "; pass --parameter-format named"
                } else {
                    "; number the variables {{1}}, {{2}}, ... in order"
                }
            )));
        };
        numbers.push(number);
    }
    numbers.sort_unstable();
    if numbers.iter().copied().ne(1..=numbers.len()) {
        return Err(Error::validation(
            "positional variables must be numbered from {{1}} without gaps",
        ));
    }
    if given.len() != numbers.len() {
        return Err(Error::validation(format!(
            "{flag} has {} value(s) for {} variable(s)",
            given.len(),
            numbers.len()
        )));
    }
    for value in given {
        validate::not_blank(value, flag)?;
    }
    Ok(Examples::Positional(given.to_vec()))
}

pub(super) fn named_examples(variables: &[&str], given: &[String], flag: &str) -> Result<Examples> {
    let mut pairs = Vec::with_capacity(given.len());
    for example in given {
        let Some((name, value)) = example.split_once('=') else {
            return Err(Error::validation(format!(
                "{flag} must use name=value with --parameter-format named"
            )));
        };
        validate::template_parameter_name(name)?;
        validate::not_blank(value, flag)?;
        if !variables.contains(&name) {
            return Err(Error::validation(format!(
                "{flag} names '{name}', which isn't a variable in the text"
            )));
        }
        pairs.push((name.to_owned(), value.to_owned()));
    }
    for variable in variables {
        validate::template_parameter_name(variable)?;
        if !pairs.iter().any(|(name, _)| name == variable) {
            return Err(Error::validation(format!(
                "{flag} has no example for {{{{{variable}}}}}"
            )));
        }
    }
    Ok(Examples::Named(pairs))
}

/// A template described by typed flags.
#[derive(Debug, Clone)]
pub struct InlineTemplate {
    pub name: String,
    pub category: paloma_core::TemplateCategory,
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
        "category": t.category.as_str(),
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
