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

/// Make an uploaded file the profile photo. `handle` comes from the Resumable Upload API.
pub fn set_photo(phone_number_id: &str, handle: &str) -> Request {
    let mut fields = Map::new();
    fields.insert("profile_picture_handle".into(), handle.into());
    set(phone_number_id, fields)
}

/// The one settings object a `GET` returns, which Graph wraps in `data[0]`.
pub fn first(response: &Value) -> Option<&Value> {
    response.pointer("/data/0")
}
