//! A PIN cannot be read back, so an explicit value always counts as drift. Display names
//! and search visibility are compared first: resubmitting a name restarts Meta's review.
//! Live status is read-only.
use crate::kind::{Cx, KindPlan, read};
use paloma_core::{Error, Result, validate};
use paloma_graph::api::phone_numbers::{self, EXPORT_FIELDS};
use serde_json::{Map, Value, json};

const NAME_FIELDS: &str = "verified_name,new_display_name,new_name_status";

/// Already the verified name, or already submitted and not turned down. A declined or
/// expired request is the one case where submitting the same name again makes sense.
fn name_is_settled(name: &Value, live: &Value) -> bool {
    live.get("verified_name") == Some(name)
        || (live.get("new_display_name") == Some(name)
            && !matches!(
                live.get("new_name_status").and_then(Value::as_str),
                Some("DECLINED" | "EXPIRED")
            ))
}

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
    let mut fields = Vec::new();
    if desired.contains_key("search_visibility") {
        fields.push("search_visibility");
    }
    if desired.contains_key("new_display_name") {
        fields.push(NAME_FIELDS);
    }
    let live = if fields.is_empty() {
        Value::Null
    } else {
        read(cx, &phone_numbers::get(phone, &fields.join(","))).await?
    };
    if let Some(name) = desired.get("new_display_name")
        && name_is_settled(name, &live)
    {
        desired.remove("new_display_name");
    }
    if let Some(visibility) = desired.get("search_visibility") {
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
    let disruptive =
        desired.contains_key("new_display_name") || desired.contains_key("search_visibility");
    let body = Value::Object(desired);
    let plan = KindPlan {
        changes,
        preview: Some(paloma_graph::request::redact_json(&body)),
        requests: vec![phone_numbers::update(phone, body)],
        confirm: None,
    };
    Ok(if disruptive {
        plan.confirming("change this number's display name or search visibility")
    } else {
        plan
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_resubmitted_only_when_it_differs_or_was_turned_down() {
        let name = json!("Acme");
        for live in [
            json!({"verified_name":"Acme"}),
            json!({"new_display_name":"Acme","new_name_status":"PENDING_REVIEW"}),
            json!({"new_display_name":"Acme","new_name_status":"APPROVED"}),
        ] {
            assert!(name_is_settled(&name, &live), "{live}");
        }
        for live in [
            json!({"verified_name":"Old"}),
            json!({"verified_name":"Old","new_display_name":"Other","new_name_status":"PENDING_REVIEW"}),
            json!({"new_display_name":"Acme","new_name_status":"DECLINED"}),
            json!({"new_display_name":"Acme","new_name_status":"EXPIRED"}),
        ] {
            assert!(!name_is_settled(&name, &live), "{live}");
        }
    }
}
