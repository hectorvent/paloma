//! The Resumable Upload API: a file becomes a handle, which template creation and profile
//! photos take instead of a media id.
//!
//! The ids it returns are used as they come, because they can carry their own query string.

use std::path::Path;

use serde_json::Value;

use crate::Request;

/// `POST /{app-id}/uploads`: open a session for a file of this size and type.
pub fn start(app_id: &str, file_name: &str, file_length: u64, file_type: &str) -> Request {
    Request::post(format!("{app_id}/uploads"))
        .query("file_name", file_name)
        .query("file_length", file_length)
        .query("file_type", file_type)
}

/// `POST /{session}`: send the file from `offset` on. Meta fails this call unless the token
/// is in the `Authorization` header under the `OAuth` scheme.
pub fn transfer(session: &str, path: &Path, offset: u64) -> Request {
    Request::post(session)
        .oauth_auth()
        .header("file_offset", offset)
        .file(path, offset)
}

/// `GET /{session}`: how much of the file Meta has, to resume an interrupted transfer.
pub fn status(session: &str) -> Request {
    Request::get(session).oauth_auth()
}

/// The session id from [`start`]'s response.
pub fn session_id(response: &Value) -> Option<&str> {
    response
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
}

/// The handle from a finished [`transfer`].
pub fn handle(response: &Value) -> Option<&str> {
    response
        .get("h")
        .and_then(Value::as_str)
        .filter(|handle| !handle.is_empty())
}

/// The offset from [`status`]. Meta's example gives it as a string.
pub fn offset(response: &Value) -> Option<u64> {
    match response.get("file_offset")? {
        Value::String(text) => text.parse().ok(),
        other => other.as_u64(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn an_empty_id_or_handle_counts_as_missing() {
        assert_eq!(session_id(&json!({ "id": "" })), None);
        assert_eq!(handle(&json!({ "h": "" })), None);
        assert_eq!(handle(&json!({ "h": "4::H" })), Some("4::H"));
    }

    #[test]
    fn offset_reads_a_string_or_a_number_and_nothing_else() {
        assert_eq!(offset(&json!({ "file_offset": "3" })), Some(3));
        assert_eq!(offset(&json!({ "file_offset": 3 })), Some(3));
        assert_eq!(offset(&json!({ "file_offset": "x" })), None);
        assert_eq!(offset(&json!({ "file_offset": -1 })), None);
        assert_eq!(offset(&json!({})), None);
    }

    #[test]
    fn a_session_id_keeps_its_own_query_string_and_logs_hide_it() {
        let url = status("upload:S1?sig=abc").url("https://graph.test", "v26.0");
        assert_eq!(url, "https://graph.test/v26.0/upload:S1?sig=abc");
        let described = status("upload:S1?sig=abc").describe("https://graph.test", "v26.0");
        assert!(!described.to_string().contains("abc"), "{described}");
        let logged = crate::request::diagnostic_body(r#"{"id":"upload:S1?sig=abc"}"#);
        assert!(!logged.contains("abc"), "{logged}");
        assert!(logged.contains("upload:S1"), "{logged}");
    }
}
