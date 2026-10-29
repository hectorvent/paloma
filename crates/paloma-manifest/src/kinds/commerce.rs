//! The `Commerce` kind: cart and catalog visibility. Turning either off confirms, like
//! `commerce set`, because customers notice it.

use paloma_core::Result;
use paloma_graph::api::commerce;
use serde_json::{Value, json};

use crate::kind::{Cx, KindPlan, read, singleton_plan};

pub fn template() -> Value {
    json!({ "isCartEnabled": "", "isCatalogVisible": "" })
}

async fn current(cx: Cx<'_>) -> Result<Value> {
    let phone = cx.ctx.require_phone_number_id()?;
    let mut cur = read(cx, &commerce::get(phone))
        .await?
        .pointer("/data/0")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if let Some(m) = cur.as_object_mut() {
        m.remove("id");
    }
    Ok(cur)
}

pub async fn export(cx: Cx<'_>) -> Result<Vec<Value>> {
    Ok(vec![current(cx).await?])
}

pub async fn plan(cx: Cx<'_>, spec: &Value) -> Result<KindPlan> {
    let phone = cx.ctx.require_phone_number_id()?;
    let cur = current(cx).await?;
    let flag = |k: &str| spec.get(k).and_then(Value::as_bool);
    let req = commerce::set(phone, flag("is_cart_enabled"), flag("is_catalog_visible"));
    let plan = singleton_plan(spec, &cur, spec.clone(), req);
    // Matches `commerce set`: turning a storefront feature off is what customers notice.
    let disables =
        flag("is_cart_enabled") == Some(false) || flag("is_catalog_visible") == Some(false);
    Ok(if disables && !plan.in_sync() {
        plan.confirming("disable shopping cart or catalog visibility")
    } else {
        plan
    })
}
