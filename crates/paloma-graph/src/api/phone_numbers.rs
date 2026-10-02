//! Business phone numbers.

use serde_json::json;

use super::MESSAGING_PRODUCT;
use crate::Request;

pub const DETAIL_FIELDS: &str = "id,display_phone_number,verified_name,quality_rating,code_verification_status,\
name_status,messaging_limit_tier,throughput,platform_type";
pub const QUALITY_FIELDS: &str =
    "display_phone_number,quality_rating,messaging_limit_tier,throughput";
pub const STATUS_FIELDS: &str =
    "display_phone_number,verified_name,quality_rating,code_verification_status";

pub fn list(waba_id: &str) -> Request {
    Request::get(format!("{waba_id}/phone_numbers"))
}

pub fn get(phone_number_id: &str, fields: &str) -> Request {
    Request::get(phone_number_id).query("fields", fields)
}

pub fn register(phone_number_id: &str, pin: &str) -> Request {
    Request::post(format!("{phone_number_id}/register"))
        .json(json!({ "messaging_product": MESSAGING_PRODUCT, "pin": pin }))
}

pub fn deregister(phone_number_id: &str) -> Request {
    Request::post(format!("{phone_number_id}/deregister"))
}

/// Set the two-step verification PIN.
pub fn set_pin(phone_number_id: &str, pin: &str) -> Request {
    Request::post(phone_number_id).json(json!({ "pin": pin }))
}
