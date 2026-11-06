//! Token inspection. The token under inspection travels in the query string, as Meta
//! defines it, so it relies on the redaction every rendered URL goes through.

use serde_json::Value;

use crate::Request;

/// `GET /debug_token`: what a token is, who granted it, and when it expires.
pub fn debug(input_token: &str) -> Request {
    Request::get("debug_token").query("input_token", input_token)
}

/// The token's details from a [`debug`] response.
pub fn details(response: &Value) -> Option<&Value> {
    response.get("data")
}

/// Whether Meta reports the token as valid.
pub fn is_valid(details: &Value) -> bool {
    details.get("is_valid") == Some(&Value::Bool(true))
}
