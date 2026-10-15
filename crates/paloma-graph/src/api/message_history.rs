//! `GET /{phone-number-id}/message_history` and `GET /{message-history-id}/events`: a sent
//! message's delivery status trail, not its content.
//!
//! The trail takes two calls because `events` is keyed by a history id that only the first
//! call can turn a wamid into. `message_history` can also nest the events, but only its
//! v23.0 spec says so, with a different timestamp field and nested paging, so the events
//! edge is the one to rely on.

use paloma_core::limits::MAX_MESSAGE_HISTORY_EVENTS_PAGE;
use serde_json::Value;

use crate::Request;

/// Everything the events edge offers; the default leaves out errors and webhook state.
const EVENT_FIELDS: &str = "cursor,node{id,delivery_status,error_description,occurrence_timestamp,status_timestamp,application,webhook_update_state,webhook_uri}";

/// One page of the history entries for a wamid.
pub fn lookup(phone_number_id: &str, wamid: &str, after: Option<&str>) -> Request {
    Request::get(format!("{phone_number_id}/message_history"))
        .query("message_id", wamid)
        .query("fields", "id,message_id")
        .query_opt("after", after)
}

/// The id of this wamid's history entry on a page of [`lookup`]. It checks the wamid rather
/// than taking the first entry, because another message's trail would look just as valid.
pub fn history_id(page: &Value, wamid: &str) -> Option<String> {
    let entry = page
        .get("data")?
        .as_array()?
        .iter()
        .find(|entry| entry.get("message_id").and_then(Value::as_str) == Some(wamid))?;
    match entry.get("id")? {
        Value::String(id) => Some(id.clone()).filter(|id| !id.trim().is_empty()),
        Value::Number(id) => Some(id.to_string()),
        _ => None,
    }
}

/// One page of a history entry's delivery status events.
pub fn events(history_id: &str, after: Option<&str>) -> Request {
    Request::get(format!("{history_id}/events"))
        .query("fields", EVENT_FIELDS)
        .query("limit", MAX_MESSAGE_HISTORY_EVENTS_PAGE)
        .query_opt("after", after)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn history_id_is_the_entry_for_this_wamid() {
        let page = json!({"data": [
            {"id": "1", "message_id": "wamid.OTHER"},
            {"id": "2", "message_id": "wamid.X"},
        ]});
        assert_eq!(history_id(&page, "wamid.X").as_deref(), Some("2"));
        assert_eq!(history_id(&page, "wamid.Y"), None);
        assert_eq!(history_id(&json!({"data": []}), "wamid.X"), None);
        let blank = json!({"data": [{"id": " ", "message_id": "wamid.X"}]});
        assert_eq!(history_id(&blank, "wamid.X"), None);
    }
}
