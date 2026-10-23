//! Business phone numbers.
//!
//! `messaging_limit_tier` is deprecated; the messaging limit now belongs to the business
//! portfolio and is read through `whatsapp_business_manager_messaging_limit`.

use serde_json::{Value, json};

use super::MESSAGING_PRODUCT;
use crate::Request;

pub const DETAIL_FIELDS: &str = "id,display_phone_number,verified_name,quality_rating,code_verification_status,\
name_status,status,health_status,whatsapp_business_manager_messaging_limit,throughput,platform_type";
/// What a manifest export records: the number's setup, without `status` and `health_status`,
/// which change on their own and would make every export look like a change.
pub const EXPORT_FIELDS: &str = "id,display_phone_number,verified_name,quality_rating,code_verification_status,\
name_status,whatsapp_business_manager_messaging_limit,throughput,platform_type";
pub const QUALITY_FIELDS: &str =
    "display_phone_number,quality_rating,whatsapp_business_manager_messaging_limit,throughput";
pub const STATUS_FIELDS: &str =
    "display_phone_number,verified_name,quality_rating,code_verification_status";

/// The value the guide gives for `filtering` on `account_mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountMode {
    Sandbox,
    Live,
}

impl AccountMode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Sandbox => "SANDBOX",
            Self::Live => "LIVE",
        }
    }
}

/// Filters for listing phone numbers.
#[derive(Debug, Clone, Default)]
pub struct ListFilter {
    pub account_mode: Option<AccountMode>,
    /// Oldest onboarding first. Graph's default is newest first, so that order has no
    /// parameter to send.
    pub oldest_first: bool,
    pub limit: Option<u32>,
}

pub fn list(waba_id: &str, filter: &ListFilter, after: Option<&str>) -> Request {
    let filtering = filter.account_mode.map(|mode| {
        Value::to_string(&json!([{
            "field": "account_mode",
            "operator": "EQUAL",
            "value": mode.as_str(),
        }]))
    });
    Request::get(format!("{waba_id}/phone_numbers"))
        .query_opt("filtering", filtering)
        .query_opt(
            "sort",
            filter
                .oldest_first
                .then_some("['last_onboarded_time_ascending']"),
        )
        .query_opt("limit", filter.limit)
        .query_opt("after", after)
}

pub fn get(phone_number_id: &str, fields: &str) -> Request {
    Request::get(phone_number_id).query("fields", fields)
}

/// `data_localization_region` enables local storage as part of the registration.
pub fn register(phone_number_id: &str, pin: &str, region: Option<&str>) -> Request {
    let mut body = json!({ "messaging_product": MESSAGING_PRODUCT, "pin": pin });
    if let Some(region) = region {
        body["data_localization_region"] = json!(region);
    }
    Request::post(format!("{phone_number_id}/register")).json(body)
}

pub fn deregister(phone_number_id: &str) -> Request {
    Request::post(format!("{phone_number_id}/deregister"))
}

/// Set the two-step verification PIN.
pub fn set_pin(phone_number_id: &str, pin: &str) -> Request {
    Request::post(phone_number_id).json(json!({ "pin": pin }))
}

/// Verification requests use the same phone node as registration.
pub fn request_code(phone: &str, method: &str, language: &str) -> Request {
    Request::post(format!("{phone}/request_code"))
        .json(json!({"code_method": method, "language": language}))
}

pub fn verify_code(phone: &str, code: &str) -> Request {
    Request::post(format!("{phone}/verify_code")).json(json!({"code": code}))
}

pub fn add(waba: &str, cc: &str, phone_number: &str, name: &str) -> Request {
    Request::post(format!("{waba}/phone_numbers"))
        .json(json!({"cc": cc, "phone_number": phone_number, "verified_name": name}))
}

pub fn set_name(phone: &str, name: &str) -> Request {
    Request::post(phone).json(json!({"new_display_name": name}))
}

pub fn search_visibility(phone: &str, value: &str) -> Request {
    Request::post(phone).json(json!({"search_visibility": value}))
}

pub fn settings(phone: &str) -> Request {
    // Do not request SIP credentials while reading non-calling settings.
    Request::get(format!("{phone}/settings"))
}

pub fn set_settings(phone: &str, body: Value) -> Request {
    Request::post(format!("{phone}/settings")).json(body)
}

pub fn username(phone: &str) -> Request {
    Request::get(format!("{phone}/username"))
}
pub fn reserved_usernames(phone: &str) -> Request {
    Request::get(format!("{phone}/username_suggestions"))
}
pub fn set_username(phone: &str, username: &str, transfer: bool) -> Request {
    Request::post(format!("{phone}/username")).json(json!({
        "username": username, "transfer_action": if transfer {"force_transfer"} else {"none"}
    }))
}
pub fn delete_username(phone: &str) -> Request {
    Request::delete(format!("{phone}/username"))
}

pub fn contact(phone: &str, bsuid: &str, delete: bool) -> Request {
    let node = format!("{phone}/contact_book");
    let req = if delete {
        Request::delete(node)
    } else {
        Request::get(node)
    };
    req.query("messaging_product", MESSAGING_PRODUCT)
        .query("bsuid", bsuid)
}

/// The OBA and regional compliance edges share the phone-scoped read/submit shape.
pub fn administration(phone: &str, edge: &str, body: Option<Value>) -> Request {
    let node = format!("{phone}/{edge}");
    match body {
        Some(body) => Request::post(node).json(body),
        None => Request::get(node),
    }
}

/// Partially update writable phone-node fields.
pub fn update(phone: &str, body: Value) -> Request {
    Request::post(phone).json(body)
}
