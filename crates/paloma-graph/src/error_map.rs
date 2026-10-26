//! Translates a non-2xx Graph API response into the right [`Error`], so API failures
//! honor the exit-code contract and the user sees Meta's own message.

use paloma_core::Error;
use serde_json::Value;

/// Meta error codes meaning the message was blocked by the 24-hour window.
const WINDOW_CODES: [i64; 2] = [131_047, 470];
/// Meta error codes for other policy blocks: the per-user frequency cap, a marketing opt-out,
/// the integrity codes for an account that's restricted or disabled, and a suspended, disabled
/// or ineligible group. Meta sends 131207 with a 403, so these are checked before the status.
const POLICY_CODES: [i64; 8] = [
    131_049, 131_050, 368, 130_497, 131_031, 131_207, 131_214, 131_215,
];
/// Meta error codes for throttling. Meta says to build on codes rather than HTTP statuses,
/// and documents no status for some of these, so they count whatever the status is.
const RATE_LIMIT_CODES: [i64; 8] = [
    4, 80_007, 130_429, 131_048, 131_056, 131_064, 131_208, 133_016,
];

/// Meta's authorization codes. Graph often sends these with a 400, so the status isn't enough.
const fn is_auth_code(code: i64) -> bool {
    matches!(code, 0 | 3 | 10 | 190 | 131_005 | 200..=299)
}

pub fn map_error(status: u16, body: &str) -> Error {
    let root: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let error = root.get("error").unwrap_or(&root);
    let raw_code = error.get("code").and_then(Value::as_i64);
    let code = raw_code.filter(|c| *c > 0);
    let message = match code.and_then(hint) {
        Some(hint) => format!("{} (hint: {hint})", describe(error)),
        None => describe(error),
    };

    if status == 429 || code.is_some_and(|c| RATE_LIMIT_CODES.contains(&c)) {
        return Error::RateLimited(format!("rate limited ({status}): {message}"));
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
    if status == 401 || status == 403 || raw_code.is_some_and(is_auth_code) {
        return Error::auth(format!("Graph API auth error ({status}): {message}"));
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
        131_215 => Some(
            "the Groups API needs an Official Business Account; check with paloma number oba get",
        ),
        131_061 => Some(
            "templates with a max price can't be sent through /messages; use paloma send template \
             --via marketing to call /marketing_messages after completing MM API onboarding",
        ),
        131_062 => Some(
            "authentication and max-price templates can't go to a business-scoped user ID; \
             address the message with the user's phone number in `to`, or for max price use a \
             template without optimization_spec",
        ),
        131_063 => Some(
            "this account has disable_marketing_messages_on_cloud_api set; use paloma send template \
             --via auto to refresh the category, or --via marketing after completing MM API \
             onboarding with the Terms of Service signed",
        ),
        131_065 => Some(
            "max-price delivery isn't available in this user's country; send to a supported \
             country or use a template without optimization_spec",
        ),
        _ => None,
    }
}

/// Meta's message, plus whichever fuller explanations it sent: for many validation errors the
/// message is only "Invalid parameter", and the reason is in `error_user_msg`.
fn describe(error: &Value) -> String {
    let text = |pointer: &str| {
        error
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
    };
    let Some(message) = text("/message") else {
        return "(no error message)".into();
    };
    let mut described = message.to_owned();
    for extra in ["/error_data/details", "/error_user_msg"] {
        if let Some(extra) = text(extra).filter(|t| !described.contains(t)) {
            described.push_str(": ");
            described.push_str(extra);
        }
    }
    described
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
        for code in [
            4, 80_007, 130_429, 131_048, 131_056, 131_064, 131_208, 133_016,
        ] {
            for status in [400, 403, 429, 500] {
                let e = map_error(status, &body(code, "slow"));
                assert_eq!(e.exit_code().code(), 4, "#{code} with {status}");
                assert!(e.to_string().contains(&format!("rate limited ({status})")));
            }
        }
    }

    #[test]
    fn authorization_codes_exit_3_whatever_the_status() {
        for code in [0, 3, 10, 190, 200, 250, 299, 131_005] {
            for status in [400, 403, 500] {
                let e = map_error(status, &body(code, "denied"));
                assert_eq!(e.exit_code().code(), 3, "#{code} with {status}");
                assert!(e.to_string().contains("denied"));
            }
        }
        // Close neighbours and the template-insights codes aren't authorization errors.
        for code in [100, 33, 199, 300, 200_005] {
            assert_eq!(
                map_error(400, &body(code, "x")).exit_code().code(),
                1,
                "#{code}"
            );
        }
    }

    #[test]
    fn integrity_and_opt_out_codes_exit_6() {
        for code in [368, 130_497, 131_031, 131_050, 131_207, 131_214, 131_215] {
            assert_eq!(
                map_error(400, &body(code, "x")).exit_code().code(),
                6,
                "#{code}"
            );
        }
    }

    #[test]
    fn a_suspended_group_is_a_policy_block_despite_its_403() {
        assert_eq!(map_error(403, &body(131_207, "x")).exit_code().code(), 6);
        assert!(
            map_error(400, &body(131_215, "x"))
                .to_string()
                .contains("number oba get")
        );
    }

    #[test]
    fn a_body_without_a_code_stays_generic() {
        assert_eq!(
            map_error(400, r#"{"error":{"message":"x"}}"#)
                .exit_code()
                .code(),
            1
        );
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
    fn surfaces_the_user_message_graph_sends_for_validation_errors() {
        let e = map_error(
            400,
            r#"{"error":{"message":"Invalid parameter","code":100,"error_user_title":"Not eligible","error_user_msg":"Templates must have been sent at least 1,000 times."}}"#,
        );
        assert_eq!(
            e.to_string(),
            "Graph API error (400, #100): Invalid parameter: Templates must have been sent at least 1,000 times."
        );
        let repeated = map_error(
            400,
            r#"{"error":{"message":"Bad","code":100,"error_data":{"details":"more"},"error_user_msg":"more"}}"#,
        );
        assert_eq!(
            repeated.to_string(),
            "Graph API error (400, #100): Bad: more"
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
