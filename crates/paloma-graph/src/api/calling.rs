//! Calling settings (`/{phone-number-id}/settings`).

use serde_json::{Value, json};

use super::MESSAGING_PRODUCT;
use crate::Request;

pub fn get(phone_number_id: &str, include_sip_credentials: bool) -> Request {
    let req = Request::get(format!("{phone_number_id}/settings")).query("fields", "calling");
    if include_sip_credentials {
        req.query("include_sip_credentials", "true")
    } else {
        req
    }
}

/// Partially update the `calling` settings object.
#[allow(clippy::needless_pass_by_value)] // takes ownership like every other body builder
pub fn set(phone_number_id: &str, calling: Value) -> Request {
    Request::post(format!("{phone_number_id}/settings"))
        .json(json!({ "messaging_product": MESSAGING_PRODUCT, "calling": calling }))
}
