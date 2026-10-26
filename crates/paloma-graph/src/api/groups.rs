//! Groups (`/{phone-number-id}/groups`, `/{group-id}`). Meta's management guide and reference
//! pages disagree on the invite link, and the guide's GET and POST are used. Participants can
//! only join through that link, so there's no call for adding them, despite the reference.

use std::path::Path;

use serde_json::{Map, Value, json};

use super::{MESSAGING_PRODUCT, media};
use crate::Request;
use crate::request::FormPart;

/// Without `fields`, Graph returns only the group's id.
pub const DETAIL_FIELDS: &str = "subject,description,suspended,creation_timestamp,\
    total_participant_count,participants,join_approval_mode";

pub fn create(
    phone: &str,
    subject: &str,
    description: Option<&str>,
    join_approval_mode: Option<&str>,
) -> Request {
    let mut body = json!({ "messaging_product": MESSAGING_PRODUCT, "subject": subject.trim() });
    if let Some(description) = description {
        body["description"] = json!(description);
    }
    if let Some(mode) = join_approval_mode {
        body["join_approval_mode"] = json!(mode);
    }
    Request::post(format!("{phone}/groups")).json(body)
}

pub fn list(phone: &str, limit: Option<u32>, after: Option<&str>) -> Request {
    Request::get(format!("{phone}/groups"))
        .query_opt("limit", limit)
        .query_opt("after", after)
}

/// The groups on a page of [`list`], which Graph nests under `data.groups`.
pub fn page_groups(page: &Value) -> Vec<Value> {
    page.pointer("/data/groups")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

pub fn get(group: &str, fields: &str) -> Request {
    Request::get(group).query("fields", fields)
}

/// Update text settings as JSON.
pub fn update(group: &str, subject: Option<&str>, description: Option<&str>) -> Request {
    let mut body = Map::new();
    body.insert("messaging_product".into(), MESSAGING_PRODUCT.into());
    if let Some(subject) = subject {
        body.insert("subject".into(), subject.trim().into());
    }
    if let Some(description) = description {
        body.insert("description".into(), description.into());
    }
    Request::post(group).json(Value::Object(body))
}

/// Update settings with a new photo, which Meta takes as a multipart `file`.
pub fn update_with_photo(
    group: &str,
    subject: Option<&str>,
    description: Option<&str>,
    photo: &Path,
) -> Request {
    let mut parts = vec![FormPart::text("messaging_product", MESSAGING_PRODUCT)];
    if let Some(subject) = subject {
        parts.push(FormPart::text("subject", subject.trim()));
    }
    if let Some(description) = description {
        parts.push(FormPart::text("description", description));
    }
    let name = photo.file_name().map_or_else(
        || "photo.jpg".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );
    parts.push(FormPart::file(
        "file",
        photo,
        name,
        media::guess_mime(photo),
    ));
    Request::post(group).multipart(parts)
}

pub fn delete(group: &str) -> Request {
    Request::delete(group)
}

pub fn invite_link(group: &str) -> Request {
    Request::get(format!("{group}/invite_link"))
}

pub fn reset_invite_link(group: &str) -> Request {
    Request::post(format!("{group}/invite_link"))
        .json(json!({ "messaging_product": MESSAGING_PRODUCT }))
}

pub fn remove_participants(group: &str, users: &[String]) -> Request {
    let participants: Vec<Value> = users.iter().map(|u| json!({ "user": u.trim() })).collect();
    Request::delete(format!("{group}/participants")).json(json!({
        "messaging_product": MESSAGING_PRODUCT,
        "participants": participants,
    }))
}

pub fn join_requests(group: &str, limit: Option<u32>, after: Option<&str>) -> Request {
    Request::get(format!("{group}/join_requests"))
        .query_opt("limit", limit)
        .query_opt("after", after)
}

fn join_request_body(ids: &[String]) -> Value {
    json!({ "messaging_product": MESSAGING_PRODUCT, "join_requests": ids })
}

pub fn approve_join_requests(group: &str, ids: &[String]) -> Request {
    Request::post(format!("{group}/join_requests")).json(join_request_body(ids))
}

pub fn reject_join_requests(group: &str, ids: &[String]) -> Request {
    Request::delete(format!("{group}/join_requests")).json(join_request_body(ids))
}

/// A pin message. Unpinning takes no duration.
pub fn pin_payload(group: &str, message_id: &str, days: Option<u32>) -> Value {
    let mut pin = json!({
        "type": if days.is_some() { "pin" } else { "unpin" },
        "message_id": message_id.trim(),
    });
    if let Some(days) = days {
        pin["expiration_days"] = json!(days);
    }
    json!({
        "messaging_product": MESSAGING_PRODUCT,
        "recipient_type": "group",
        "to": group,
        "type": "pin",
        "pin": pin,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_carries_a_duration_and_unpin_does_not() {
        assert_eq!(
            pin_payload("G", "wamid.1", Some(4))["pin"],
            json!({"type":"pin","message_id":"wamid.1","expiration_days":4})
        );
        assert_eq!(
            pin_payload("G", "wamid.1", None)["pin"],
            json!({"type":"unpin","message_id":"wamid.1"})
        );
    }

    #[test]
    fn groups_are_read_from_their_nested_page() {
        let page = json!({"data":{"groups":[{"id":"a"},{"id":"b"}]}});
        assert_eq!(page_groups(&page).len(), 2);
        assert!(page_groups(&json!({"data":[]})).is_empty());
    }
}
