use paloma_core::{Error, Result};
use paloma_graph::api::profile;
use serde_json::{Value, json};

use crate::kind::{Cx, KindPlan, read, singleton_plan};

pub fn template() -> Value {
    json!({ "about": "", "address": "", "description": "", "email": "", "vertical": "", "websites": [] })
}

async fn current(cx: Cx<'_>) -> Result<Value> {
    let phone = cx.ctx.require_phone_number_id()?;
    Ok(read(cx, &profile::get(phone))
        .await?
        .pointer("/data/0")
        .cloned()
        .unwrap_or_else(|| json!({})))
}

pub async fn export(cx: Cx<'_>) -> Result<Vec<Value>> {
    let mut spec = current(cx).await?;
    if let Some(m) = spec.as_object_mut() {
        m.remove("profile_picture_url");
        m.remove("messaging_product");
    }
    Ok(vec![spec])
}

pub async fn plan(cx: Cx<'_>, spec: &Value) -> Result<KindPlan> {
    let fields = spec
        .as_object()
        .ok_or_else(|| Error::validation("Profile spec must be a mapping"))?
        .clone();
    let phone = cx.ctx.require_phone_number_id()?;
    let cur = current(cx).await?;
    let req = profile::set(phone, fields);
    let preview = req.json_body().cloned().unwrap_or_default();
    Ok(singleton_plan(spec, &cur, preview, req))
}
