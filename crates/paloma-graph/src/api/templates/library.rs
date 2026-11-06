//! Meta's template library: ready-made templates that skip review once created.

use super::{Request, Value, json};

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

/// The create payload for a [`LibraryTemplate`], for [`create`](super::create). Meta's page writes
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
