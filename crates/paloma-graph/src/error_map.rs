//! Translates a non-2xx Graph API response into the right [`Error`], so API failures
//! honor the exit-code contract and the user sees Meta's own message.

use paloma_core::Error;
use serde_json::Value;

/// Meta error codes meaning the message was blocked by the 24-hour window.
const WINDOW_CODES: [i64; 2] = [131_047, 470];
/// Meta error codes for other messaging-policy blocks (per-user frequency cap, etc.).
const POLICY_CODES: [i64; 1] = [131_049];

pub fn map_error(status: u16, body: &str) -> Error {
    let root: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let error = root.get("error").unwrap_or(&root);
    let code = error.get("code").and_then(Value::as_i64).filter(|c| *c > 0);
    let message = describe(error);

    if status == 401 || status == 403 {
        return Error::auth(format!("Graph API auth error ({status}): {message}"));
    }
    if status == 429 {
        return Error::RateLimited(format!("rate limited (429): {message}"));
    }
    if let Some(c) = code {
        if WINDOW_CODES.contains(&c) {
            return Error::policy(format!(
                "outside the 24-hour customer-service window (Meta #{c}): send an approved template instead: {message}"
            ));
        }
        if POLICY_CODES.contains(&c) {
            return Error::policy(format!(
                "blocked by a messaging policy (Meta #{c}): {message}"
            ));
        }
    }
    Error::Api {
        status,
        code,
        message,
    }
}

fn describe(error: &Value) -> String {
    let msg = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if msg.is_empty() {
        return "(no error message)".into();
    }
    match error
        .pointer("/error_data/details")
        .and_then(Value::as_str)
        .filter(|d| !d.trim().is_empty())
    {
        Some(details) => format!("{msg}: {details}"),
        None => msg.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(code: i64, msg: &str) -> String {
        format!(
            r#"{{"error":{{"message":"{msg}","code":{code},"error_data":{{"details":"more"}}}}}}"#
        )
    }

    #[test]
    fn maps_status_and_meta_codes_to_exit_codes() {
        assert_eq!(map_error(401, &body(190, "expired")).exit_code().code(), 3);
        assert_eq!(map_error(403, "").exit_code().code(), 3);
        assert_eq!(map_error(429, &body(4, "slow")).exit_code().code(), 4);
        assert_eq!(
            map_error(400, &body(131_047, "window")).exit_code().code(),
            6
        );
        assert_eq!(map_error(400, &body(470, "window")).exit_code().code(), 6);
        assert_eq!(map_error(400, &body(131_049, "cap")).exit_code().code(), 6);
        assert_eq!(
            map_error(400, &body(100, "bad param")).exit_code().code(),
            1
        );
    }

    #[test]
    fn surfaces_message_and_details() {
        let e = map_error(400, &body(100, "Invalid parameter"));
        assert_eq!(
            e.to_string(),
            "Graph API error (400, #100): Invalid parameter: more"
        );
        assert_eq!(
            map_error(502, "<html>").to_string(),
            "Graph API error (502): (no error message)"
        );
    }
}
