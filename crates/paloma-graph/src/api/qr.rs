//! QR codes are scoped to a phone number; updates POST the collection with a code,
//! while reads and deletes address the code beneath that collection.

use crate::Request;
use serde_json::{Value, json};

pub fn create(phone: &str, message: &str, image: Option<&str>) -> Request {
    let mut body = json!({"prefilled_message":message});
    if let Some(image) = image {
        body["generate_qr_image"] = json!(image);
    }
    Request::post(format!("{phone}/message_qrdls")).json(body)
}

pub fn update(phone: &str, code: &str, message: &str) -> Request {
    Request::post(format!("{phone}/message_qrdls"))
        .json(json!({"code":code, "prefilled_message":message}))
}

pub fn list(phone: &str, limit: Option<u32>, after: Option<&str>) -> Request {
    Request::get(format!("{phone}/message_qrdls"))
        .query_opt("limit", limit)
        .query_opt("after", after)
}

pub fn get(phone: &str, code: &str, image: Option<&str>) -> Request {
    let req = Request::get(format!("{phone}/message_qrdls/{code}"));
    match image {
        Some(format) => req.query(
            "fields",
            format!("code,prefilled_message,deep_link_url,qr_image_url.format({format})"),
        ),
        None => req,
    }
}

pub fn delete(phone: &str, code: &str) -> Request {
    Request::delete(format!("{phone}/message_qrdls/{code}"))
}

pub fn code(value: &Value) -> Option<String> {
    value
        .get("code")
        .or_else(|| value.pointer("/data/0/code"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

pub fn image_url(value: &Value) -> Option<&str> {
    value
        .get("qr_image_url")
        .or_else(|| value.pointer("/data/0/qr_image_url"))
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
}
