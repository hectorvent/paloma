//! PINs and pending display names cannot be read back, so explicit values always
//! count as drift. Search visibility is compared before writing; live status is read-only.
use crate::kind::{Cx, KindPlan, read};
use paloma_core::{Error, Result, validate};
use paloma_graph::api::phone_numbers::{self, EXPORT_FIELDS};
use serde_json::{Map, Value, json};

pub fn template() -> Value {
    json!({"pin":"","newDisplayName":"","searchVisibility":""})
}
pub async fn export(cx: Cx<'_>) -> Result<Vec<Value>> {
    let phone = cx.ctx.require_phone_number_id()?;
    Ok(vec![
        read(
            cx,
            &phone_numbers::get(phone, &format!("{EXPORT_FIELDS},search_visibility")),
        )
        .await?,
    ])
}
pub async fn plan(cx: Cx<'_>, spec: &Value) -> Result<KindPlan> {
    let object = spec
        .as_object()
        .ok_or_else(|| Error::validation("Number spec must be a mapping"))?;
    let mut desired = Map::new();
    for (key, value) in object {
        if value.is_null() {
            continue;
        }
        match key.as_str() {
            "pin" => {
                let pin = value
                    .as_str()
                    .map_or_else(|| value.to_string(), str::to_owned);
                validate::pin(&pin)?;
                desired.insert(key.clone(), json!(pin));
            }
            "new_display_name" => {
                let name = value
                    .as_str()
                    .ok_or_else(|| Error::validation("newDisplayName must be a string"))?;
                validate::not_blank(name, "display name")?;
                desired.insert(key.clone(), value.clone());
            }
            "search_visibility" => {
                if !matches!(value.as_str(), Some("VISIBLE" | "HIDDEN")) {
                    return Err(Error::validation(
                        "searchVisibility must be VISIBLE or HIDDEN",
                    ));
                }
                desired.insert(key.clone(), value.clone());
            }
            // These observational fields were exported by the original Number kind.
            "id"
            | "display_phone_number"
            | "verified_name"
            | "quality_rating"
            | "code_verification_status"
            | "name_status"
            | "whatsapp_business_manager_messaging_limit"
            | "throughput"
            | "platform_type" => (),
            _ => return Err(Error::validation(format!("Number does not manage {key}"))),
        }
    }
    if desired.is_empty() {
        return Ok(KindPlan::synced());
    }
    let phone = cx.ctx.require_phone_number_id()?;
    let mut changes = Vec::new();
    if let Some(visibility) = desired.get("search_visibility") {
        let live = read(cx, &phone_numbers::get(phone, "search_visibility")).await?;
        if live.get("search_visibility") == Some(visibility) {
            desired.remove("search_visibility");
        } else {
            changes.extend(crate::diff::changes(
                &json!({"search_visibility":visibility}),
                Some(&live),
            ));
        }
    }
    if desired.contains_key("pin") {
        changes.push("pin: (write-only) -> ******".into());
    }
    if desired.contains_key("new_display_name") {
        changes.push("new_display_name: (submitted for review)".into());
    }
    if desired.is_empty() {
        return Ok(KindPlan::synced());
    }
    let body = Value::Object(desired);
    Ok(KindPlan {
        changes,
        preview: Some(paloma_graph::request::redact_json(&body)),
        requests: vec![phone_numbers::update(phone, body)],
    })
}
