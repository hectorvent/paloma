use paloma_core::Result;
use paloma_graph::api::{MESSAGING_PRODUCT, calling};
use serde_json::{Map, Value, json};

use crate::kind::{Cx, KindPlan, read, singleton_plan};

pub fn template() -> Value {
    json!({
        "status": "",
        "callIconVisibility": "",
        "callbackPermissionStatus": "",
        "callIcons": { "restrictToUserCountries": [] },
        "sip": { "status": "", "servers": [] },
        "srtpKeyExchangeProtocol": "",
        "audio": { "additionalCodecs": [] },
        "voicemail": {
            "status": "",
            "triggers": [],
            "audio": { "default": { "announcementMediaId": "", "timeoutSeconds": "" } }
        }
    })
}

/// The writable calling settings, minus server-assigned and secret SIP fields.
async fn current(cx: Cx<'_>) -> Result<Value> {
    let phone = cx.ctx.require_phone_number_id()?;
    let mut node = read(cx, &calling::get(phone, false))
        .await?
        .get("calling")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if !node.is_object() {
        node = Value::Object(Map::new());
    }
    if let Some(servers) = node
        .pointer_mut("/sip/servers")
        .and_then(Value::as_array_mut)
    {
        for s in servers.iter_mut().filter_map(Value::as_object_mut) {
            s.remove("app_id");
            s.remove("sip_user_password");
        }
    }
    Ok(node)
}

pub async fn export(cx: Cx<'_>) -> Result<Vec<Value>> {
    Ok(vec![current(cx).await?])
}

pub async fn plan(cx: Cx<'_>, spec: &Value) -> Result<KindPlan> {
    let phone = cx.ctx.require_phone_number_id()?;
    let cur = current(cx).await?;
    let preview = json!({ "messaging_product": MESSAGING_PRODUCT, "calling": spec });
    Ok(singleton_plan(
        spec,
        &cur,
        preview,
        calling::set(phone, spec.clone()),
    ))
}
