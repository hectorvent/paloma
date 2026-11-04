//! The WABA's name and timezone.
//!
//! The Cloud API marketing toggle, insights and billing are left out on purpose: each stops
//! sends, can't be undone or moves money, so none should flip because a file said so. A
//! spec naming anything else is refused rather than ignored, so nobody believes a manifest
//! manages one of those.

use paloma_core::{Error, Result, validate};
use paloma_graph::api::account::{self, SETTINGS_FIELDS};
use serde_json::{Map, Value, json};

use crate::kind::{Cx, KindPlan, read, singleton_plan};

pub fn template() -> Value {
    json!({ "name": "", "timezoneId": "" })
}

/// Meta returns the timezone as a string, and YAML reads a bare `1` as a number. Without
/// this, that manifest would show drift forever.
fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn settings_of(value: &Value) -> Map<String, Value> {
    ["name", "timezone_id"]
        .into_iter()
        .filter_map(|key| {
            let value = value.get(key).and_then(text)?;
            Some((key.to_owned(), Value::String(value)))
        })
        .collect()
}

/// The spec as a checked, normalized object.
fn desired(spec: &Value) -> Result<Map<String, Value>> {
    let fields = spec
        .as_object()
        .ok_or_else(|| Error::validation("an Account spec must be a mapping"))?;
    for (key, value) in fields {
        let known = matches!(key.as_str(), "name" | "timezone_id");
        if !known {
            return Err(Error::validation(format!(
                "an Account manifest manages name and timezoneId, not '{key}'; \
                 see `paloma account set`"
            )));
        }
        if !value.is_null() && (text(value).is_none() || (key == "name" && !value.is_string())) {
            return Err(Error::validation(format!(
                "Account '{key}' must be a string"
            )));
        }
    }
    let desired = settings_of(spec);
    if let Some(name) = desired.get("name").and_then(Value::as_str) {
        validate::not_blank(name, "name")?;
    }
    if let Some(id) = desired.get("timezone_id").and_then(Value::as_str) {
        validate::timezone_id(id)?;
    }
    Ok(desired)
}

async fn current(cx: Cx<'_>) -> Result<Value> {
    let waba = cx.scope.require_waba_id()?;
    let live = read(cx, &account::get(waba, SETTINGS_FIELDS)).await?;
    Ok(Value::Object(settings_of(&live)))
}

pub async fn export(cx: Cx<'_>) -> Result<Vec<Value>> {
    Ok(vec![current(cx).await?])
}

pub async fn plan(cx: Cx<'_>, spec: &Value) -> Result<KindPlan> {
    let desired = Value::Object(desired(spec)?);
    let waba = cx.scope.require_waba_id()?;
    let cur = current(cx).await?;
    let request = account::set(waba, desired.clone());
    Ok(singleton_plan(&desired, &cur, desired.clone(), request))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_numeric_timezone_reads_like_metas_string() {
        let spec = json!({ "timezone_id": 7 });
        assert_eq!(desired(&spec).unwrap()["timezone_id"], json!("7"));
    }

    #[test]
    fn the_marketing_toggle_and_insights_are_refused() {
        for key in [
            "disable_marketing_messages_on_cloud_api",
            "is_enabled_for_insights",
            "currency",
        ] {
            let err = desired(&json!({ key: true })).unwrap_err();
            assert_eq!(err.exit_code().code(), 5, "{key}");
        }
    }

    #[test]
    fn a_spec_that_is_not_a_mapping_is_refused() {
        for spec in [json!("name"), json!(7), json!(["name"]), json!(true)] {
            let err = desired(&spec).unwrap_err();
            assert_eq!(err.exit_code().code(), 5, "{spec}");
        }
        assert!(desired(&json!({})).unwrap().is_empty());
    }

    #[test]
    fn blank_or_malformed_values_are_refused_and_null_is_unset() {
        assert!(desired(&json!({ "name": " " })).is_err());
        assert!(desired(&json!({ "timezone_id": "UTC" })).is_err());
        assert!(desired(&json!({ "name": ["a"] })).is_err());
        assert!(desired(&json!({ "name": null })).unwrap().is_empty());
    }
}
