//! Token inspection. The token under inspection travels in the query string, as Meta
//! defines it, so it relies on the redaction every rendered URL goes through.

use crate::Request;

/// `GET /debug_token`: what a token is, who granted it, and when it expires.
pub fn debug(input_token: &str) -> Request {
    Request::get("debug_token").query("input_token", input_token)
}
