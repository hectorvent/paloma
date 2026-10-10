//! Translates a non-2xx Graph API response into the right [`Error`], so API failures
//! honor the exit-code contract and the user sees Meta's own message.

use paloma_core::Error;
use serde_json::Value;

/// Meta error codes meaning the message was blocked by the 24-hour window.
const WINDOW_CODES: [i64; 2] = [131_047, 470];
/// Meta error codes for other messaging-policy blocks (per-user frequency cap, etc.).
const POLICY_CODES: [i64; 1] = [131_049];
/// Meta error codes for throttling. Meta says to build on codes rather than HTTP statuses,
/// and documents no status for some of these, so they count whatever the status is.
const RATE_LIMIT_CODES: [i64; 4] = [4, 80_007, 130_429, 131_056];

pub fn map_error(status: u16, body: &str) -> Error {
    let root: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let error = root.get("error").unwrap_or(&root);
    let code = error.get("code").and_then(Value::as_i64).filter(|c| *c > 0);
    let message = match code.and_then(hint) {
        Some(hint) => format!("{} (hint: {hint})", describe(error)),
        None => describe(error),
    };

    if status == 429 || code.is_some_and(|c| RATE_LIMIT_CODES.contains(&c)) {
        return Error::RateLimited(format!("rate limited ({status}): {message}"));
    }
    if status == 401 || status == 403 {
        return Error::auth(format!("Graph API auth error ({status}): {message}"));
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

/// The next step for these codes, which Meta's text may only partly cover. Keeps Meta's
/// text and the exit code as they are, so scripts that match on either keep working.
const fn hint(code: i64) -> Option<&'static str> {
    match code {
        131_049 => Some(
            "this user may have reached the marketing-message limit, and Meta doesn't deliver \
             marketing messages to US users; otherwise wait at least 24 hours before resending, \
             because repeated attempts can block delivery to this user for up to 24 hours",
        ),
        131_061 => Some(
            "templates with a max price can't be sent through /messages; call the Marketing \
             Messages API (/marketing_messages) directly, because Paloma can't send through it yet",
        ),
        131_062 => Some(
            "authentication and max-price templates can't go to a business-scoped user ID; \
             address the message with the user's phone number in `to`, or for max price use a \
             template without optimization_spec",
        ),
        131_063 => Some(
            "this account has disable_marketing_messages_on_cloud_api set; re-enable marketing \
             templates on the Cloud API, or once onboarded with the Terms of Service signed, call \
             the Marketing Messages API (/marketing_messages) directly, because Paloma can't send \
             through it yet",
        ),
        131_065 => Some(
            "max-price delivery isn't available in this user's country; send to a supported \
             country or use a template without optimization_spec",
        ),
        _ => None,
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
    fn throttling_codes_exit_4_whatever_the_status() {
        for code in [4, 80_007, 130_429, 131_056] {
            for status in [400, 403, 429, 500] {
                let e = map_error(status, &body(code, "slow"));
                assert_eq!(e.exit_code().code(), 4, "#{code} with {status}");
                assert!(e.to_string().contains(&format!("rate limited ({status})")));
            }
        }
    }

    #[test]
    fn known_codes_add_a_hint_and_keep_meta_text_and_exit_code() {
        for (code, needle, exit) in [
            (131_049, "24 hours", 6),
            (131_061, "/marketing_messages", 1),
            (131_062, "phone number", 1),
            (131_063, "disable_marketing_messages_on_cloud_api", 1),
            (131_065, "optimization_spec", 1),
        ] {
            let e = map_error(400, &body(code, "Meta says no"));
            let text = e.to_string();
            assert!(text.contains("Meta says no: more"), "{text}");
            assert!(text.contains(needle), "{text}");
            assert_eq!(e.exit_code().code(), exit, "{text}");
        }
        assert!(!map_error(400, &body(100, "x")).to_string().contains("hint"));
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
