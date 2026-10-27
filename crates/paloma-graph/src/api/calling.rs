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

/// A phone number goes in `user_wa_id` and a business-scoped user ID in `recipient`.
pub fn permissions(phone_number_id: &str, user: &str) -> Request {
    let user = user.trim();
    let param = if paloma_core::validate::bsuid(user).is_some() {
        "recipient"
    } else {
        "user_wa_id"
    };
    Request::get(format!("{phone_number_id}/call_permissions"))
        .query(param, user.trim_start_matches('+'))
}

/// Partially update the `calling` settings object.
#[allow(clippy::needless_pass_by_value)] // takes ownership like every other body builder
pub fn set(phone_number_id: &str, calling: Value) -> Request {
    Request::post(format!("{phone_number_id}/settings"))
        .json(json!({ "messaging_product": MESSAGING_PRODUCT, "calling": calling }))
}
