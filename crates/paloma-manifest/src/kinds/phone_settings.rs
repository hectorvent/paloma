//! Settings API accepts one feature per write. Calling stays in its existing kind;
//! these non-calling settings are compared independently and never deregister a number.
use crate::kind::{Cx, KindPlan, read};
use paloma_core::{Error, Result, validate};
use paloma_graph::api::phone_numbers;
use serde_json::{Map, Value, json};

pub fn template() -> Value {
    json!({"storageConfiguration":{"status":"","dataLocalizationRegion":""},"userIdentityChange":{"enableIdentityKeyCheck":""}})
}

fn desired(spec: &Value) -> Result<Value> {
    let fields = spec
        .as_object()
        .ok_or_else(|| Error::validation("PhoneSettings spec must be a mapping"))?;
    let mut desired = Map::new();
    for (key, value) in fields {
        if !matches!(
            key.as_str(),
            "storage_configuration" | "user_identity_change"
        ) {
            return Err(Error::validation(
                "PhoneSettings manages storageConfiguration and userIdentityChange only",
            ));
        }
        if value.is_null() {
            continue;
        }
        let object = value
            .as_object()
            .ok_or_else(|| Error::validation("each phone setting must be a mapping"))?;
        if key == "user_identity_change" {
            if object.len() != 1
                || !object
                    .get("enable_identity_key_check")
                    .is_some_and(Value::is_boolean)
            {
                return Err(Error::validation(
                    "userIdentityChange requires only boolean enableIdentityKeyCheck",
                ));
            }
        } else {
            if object
                .keys()
                .any(|k| !matches!(k.as_str(), "status" | "data_localization_region"))
            {
                return Err(Error::validation(
                    "storageConfiguration accepts status and dataLocalizationRegion",
                ));
            }
            let status = value.get("status").and_then(Value::as_str);
            match status {
                Some("IN_COUNTRY_STORAGE_ENABLED") => {
                    let region = value
                        .get("data_localization_region")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            Error::validation("enabled storage requires dataLocalizationRegion")
                        })?;
                    validate::data_localization_region(region)?;
                }
                Some("IN_COUNTRY_STORAGE_DISABLED")
                    if !object.contains_key("data_localization_region") => {}
                _ => {
                    return Err(Error::validation(
                        "storage status must be IN_COUNTRY_STORAGE_ENABLED with a region or IN_COUNTRY_STORAGE_DISABLED without one",
                    ));
                }
            }
        }
        desired.insert(key.clone(), value.clone());
    }
    Ok(Value::Object(desired))
}

fn writable(live: &Value) -> Value {
    let mut fields = Map::new();
    if let Some(storage) = live.get("storage_configuration").and_then(Value::as_object)
        && let Some(status) = storage.get("status").and_then(Value::as_str)
    {
        let enabled = status.eq_ignore_ascii_case("IN_COUNTRY_STORAGE_ENABLED");
        if enabled
            || status.eq_ignore_ascii_case("DEFAULT")
            || status.eq_ignore_ascii_case("IN_COUNTRY_STORAGE_DISABLED")
        {
            let mut value = json!({"status": if enabled {"IN_COUNTRY_STORAGE_ENABLED"} else {"IN_COUNTRY_STORAGE_DISABLED"}});
            if enabled && let Some(region) = storage.get("data_localization_region") {
                value["data_localization_region"] = region.clone();
            }
            fields.insert("storage_configuration".into(), value);
        }
    }
    if let Some(flag) = live
        .pointer("/user_identity_change/enable_identity_key_check")
        .filter(|v| v.is_boolean())
    {
        fields.insert(
            "user_identity_change".into(),
            json!({"enable_identity_key_check":flag}),
        );
    }
    Value::Object(fields)
}

pub async fn export(cx: Cx<'_>) -> Result<Vec<Value>> {
    let phone = cx.scope.require_phone_number_id()?;
    Ok(vec![writable(
        &read(cx, &phone_numbers::settings(phone)).await?,
    )])
}

pub async fn plan(cx: Cx<'_>, spec: &Value) -> Result<KindPlan> {
    let desired = desired(spec)?;
    if desired.as_object().is_some_and(Map::is_empty) {
        return Ok(KindPlan::synced());
    }
    let phone = cx.scope.require_phone_number_id()?;
    let current = writable(&read(cx, &phone_numbers::settings(phone)).await?);
    let mut plan = KindPlan::synced();
    let mut bodies = Vec::new();
    // Identity first: storage only succeeds on a deregistered number, and its failure
    // must not block an unrelated identity change.
    for key in ["user_identity_change", "storage_configuration"] {
        let Some(value) = desired.get(key) else {
            continue;
        };
        let body = json!({key:value});
        if crate::diff::in_sync(&body, Some(&current)) {
            continue;
        }
        // Meta's settings schema doesn't list this field as readable. Say so instead of
        // implying the live value is off, since the write can never converge in that case.
        if key == "user_identity_change" && current.get(key).is_none() {
            plan.changes.push(format!(
                "user_identity_change.enable_identity_key_check: (not returned by Graph) -> {}",
                value["enable_identity_key_check"]
            ));
        } else {
            plan.changes
                .extend(crate::diff::changes(&body, Some(&current)));
        }
        plan.requests
            .push(phone_numbers::set_settings(phone, body.clone()));
        bodies.push(body);
    }
    if !plan.in_sync() {
        // One body per request, so a dry-run shows exactly what would be sent.
        plan.preview = Some(match <[Value; 1]>::try_from(bodies) {
            Ok([body]) => body,
            Err(bodies) => Value::Array(bodies),
        });
        plan.confirm = Some(
            "change number identity or storage settings; storage changes require deregistration and re-registration",
        );
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn future_storage_status_does_not_hide_known_identity_settings() {
        let live = json!({"storage_configuration":{"status":"FUTURE_META_STATE"},"user_identity_change":{"enable_identity_key_check":true}});
        assert_eq!(
            writable(&live),
            json!({"user_identity_change":{"enable_identity_key_check":true}})
        );
        assert_eq!(
            writable(&json!({"storage_configuration":{"status":"default"}})),
            json!({"storage_configuration":{"status":"IN_COUNTRY_STORAGE_DISABLED"}})
        );
    }

    #[test]
    fn unknown_and_unsafe_settings_are_rejected() {
        for spec in [
            json!({"calling":{}}),
            json!({"storage_configuration":{"status":"IN_COUNTRY_STORAGE_ENABLED"}}),
            json!({"user_identity_change":{"enabled":true}}),
            json!({"storage_configuration":{"status":"IN_COUNTRY_STORAGE_DISABLED","data_localization_region":"IN"}}),
        ] {
            assert!(desired(&spec).is_err());
        }
        assert!(
            desired(&json!({"user_identity_change":{"enable_identity_key_check":false}})).is_ok()
        );
    }
}
