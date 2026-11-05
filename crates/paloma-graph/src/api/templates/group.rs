//! Template groups, which analytics reports on together.

use super::{Request, json};

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
