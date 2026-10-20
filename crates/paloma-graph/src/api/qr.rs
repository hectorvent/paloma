//! QR codes are scoped to a phone number; updates POST the collection with a code,
//! while reads and deletes address the code beneath that collection.

use crate::Request;
use paloma_core::{Error, Result};
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

/// A reconcile lookup must identify exactly the requested code before comparing state.
pub fn details<'a>(value: &'a Value, requested_code: &str) -> Result<Option<&'a Value>> {
    let invalid = || {
        Error::other(
            "invalid QR lookup response: expected exactly the requested code and its prefilled message",
        )
    };
    let node = match (value.get("code"), value.get("data")) {
        (Some(_), None) => value,
        (None, Some(Value::Array(rows))) => match rows.as_slice() {
            [] => return Ok(None),
            [node] => node,
            _ => return Err(invalid()),
        },
        _ => return Err(invalid()),
    };
    if node.get("code").and_then(Value::as_str) != Some(requested_code)
        || node
            .get("prefilled_message")
            .and_then(Value::as_str)
            .is_none()
    {
        return Err(invalid());
    }
    Ok(Some(node))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconcile_lookup_requires_one_exact_complete_code() {
        let code = "WOMVT6TJ2BP7A1";
        let node = json!({"code":code,"prefilled_message":"Hi"});
        assert_eq!(details(&node, code).unwrap(), Some(&node));
        assert_eq!(
            details(&json!({"data":[node.clone()]}), code).unwrap(),
            Some(&node)
        );
        assert_eq!(details(&json!({"data":[]}), code).unwrap(), None);
        for invalid in [
            json!({"code":"4O4YGZEG3RIVE1","prefilled_message":"Hi"}),
            json!({"data":[{"code":"4O4YGZEG3RIVE1","prefilled_message":"Hi"}]}),
            json!({"data":[node.clone(),node.clone()]}),
            json!({"data":null}),
            json!({"data":[{}]}),
            json!({"code":code,"data":[],"prefilled_message":"Hi"}),
            json!({"code":code}),
            json!({"code":code,"prefilled_message":false}),
        ] {
            assert!(details(&invalid, code).is_err(), "{invalid}");
        }
    }
}
