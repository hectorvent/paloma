//! WhatsApp Flows.

use std::fmt::Write as _;
use std::path::Path;

use serde_json::{Map, Value};

use crate::{FormPart, Request};

pub const DEFAULT_GET_FIELDS: &str = "id,name,status,categories,validation_errors,json_version,endpoint_uri,whatsapp_business_account,application";
pub const PREVIEW_FIELD: &str = "preview.invalidate(false)";
pub const FLOW_JSON_NAME: &str = "flow.json";

pub fn list(
    waba_id: &str,
    fields: Option<&str>,
    limit: Option<u32>,
    after: Option<&str>,
) -> Request {
    Request::get(format!("{waba_id}/flows"))
        .query_opt("fields", fields)
        .query_opt("limit", limit)
        .query_opt("after", after)
}

pub fn get(flow_id: &str, fields: &str) -> Request {
    Request::get(flow_id).query("fields", fields)
}

pub fn create(waba_id: &str, body: Value) -> Request {
    Request::post(format!("{waba_id}/flows")).json(body)
}

/// Update metadata (name, categories, endpoint URI).
pub fn update(flow_id: &str, body: Map<String, Value>) -> Request {
    Request::post(flow_id).json(Value::Object(body))
}

/// Upload a Flow JSON asset. Graph requires the part to be named `flow.json`, whatever
/// the local file is called.
pub fn upload_json(flow_id: &str, path: &Path) -> Request {
    Request::post(format!("{flow_id}/assets")).multipart(vec![
        FormPart::text("name", FLOW_JSON_NAME),
        FormPart::text("asset_type", "FLOW_JSON"),
        FormPart::file("file", path, FLOW_JSON_NAME, "application/json"),
    ])
}

pub fn publish(flow_id: &str) -> Request {
    Request::post(format!("{flow_id}/publish"))
}

pub fn deprecate(flow_id: &str) -> Request {
    Request::post(format!("{flow_id}/deprecate"))
}

pub fn delete(flow_id: &str) -> Request {
    Request::delete(flow_id)
}

pub fn assets(flow_id: &str) -> Request {
    Request::get(format!("{flow_id}/assets"))
}

/// The `download_url` of the FLOW_JSON asset in an assets response.
pub fn flow_json_download_url(assets: &Value) -> Option<String> {
    assets
        .get("data")?
        .as_array()?
        .iter()
        .find(|a| a.get("asset_type").and_then(Value::as_str) == Some("FLOW_JSON"))?
        .get("download_url")?
        .as_str()
        .filter(|u| !u.is_empty())
        .map(str::to_owned)
}

/// The `validation_errors` of a response, or of each direct sub-response when a command
/// merged several (as `flow update` does for metadata plus Flow JSON), each rendered as
/// `line 14:3 INVALID_PROPERTY_VALUE: <message>`.
pub fn validation_errors(response: &Value) -> Vec<String> {
    let errors_of = |v: &Value| -> Vec<String> {
        v.get("validation_errors")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(describe_error)
            .collect()
    };
    let mut errors = errors_of(response);
    if let Value::Object(m) = response {
        m.values()
            .filter(|c| c.is_object())
            .for_each(|c| errors.extend(errors_of(c)));
    }
    errors
}

fn describe_error(error: &Value) -> String {
    let mut s = String::new();
    let int = |k: &str| error.get(k).and_then(Value::as_i64).filter(|n| *n > 0);
    if let Some(line) = int("line_start") {
        let _ = write!(s, "line {line}");
        if let Some(col) = int("column_start") {
            let _ = write!(s, ":{col}");
        }
        s.push(' ');
    }
    if let Some(code) = error
        .get("error")
        .and_then(Value::as_str)
        .filter(|c| !c.trim().is_empty())
    {
        s.push_str(code);
        s.push_str(": ");
    }
    let msg = error
        .get("message")
        .and_then(Value::as_str)
        .filter(|m| !m.trim().is_empty());
    s.push_str(msg.unwrap_or("(no message)"));
    s
}

pub fn migrate(destination: &str, source: &str, names: &[String]) -> Request {
    Request::post(format!("{destination}/migrate_flows"))
        .query("source_waba_id", source)
        .query_opt(
            "source_flow_names",
            (!names.is_empty()).then(|| serde_json::to_string(names).expect("string array")),
        )
}
pub fn get_key(phone: &str) -> Request {
    Request::get(format!("{phone}/whatsapp_business_encryption"))
}
pub fn set_key(phone: &str, key: &str) -> Request {
    Request::post(format!("{phone}/whatsapp_business_encryption"))
        .multipart(vec![FormPart::text("business_public_key", key)])
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn validation_errors_from_top_level_and_sub_responses() {
        let body = json!({
            "validation_errors": [{"error": "INVALID_PROPERTY_VALUE", "message": "bad", "line_start": 14, "column_start": 3}],
            "flow_json": {"validation_errors": [{"message": "oops"}]}
        });
        assert_eq!(
            validation_errors(&body),
            vec![
                "line 14:3 INVALID_PROPERTY_VALUE: bad".to_owned(),
                "oops".to_owned()
            ]
        );
        assert!(validation_errors(&json!({"success": true})).is_empty());
    }
}
