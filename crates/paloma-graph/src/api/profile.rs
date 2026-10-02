//! The WhatsApp Business profile.

use serde_json::{Map, Value};

use super::MESSAGING_PRODUCT;
use crate::Request;

pub const FIELDS: &str =
    "about,address,description,email,profile_picture_url,websites,vertical,messaging_product";

pub fn get(phone_number_id: &str) -> Request {
    Request::get(format!("{phone_number_id}/whatsapp_business_profile")).query("fields", FIELDS)
}

/// Update the profile with the given (snake_case) fields.
pub fn set(phone_number_id: &str, fields: Map<String, Value>) -> Request {
    let mut body = Map::new();
    body.insert("messaging_product".into(), MESSAGING_PRODUCT.into());
    body.extend(fields);
    Request::post(format!("{phone_number_id}/whatsapp_business_profile")).json(Value::Object(body))
}
