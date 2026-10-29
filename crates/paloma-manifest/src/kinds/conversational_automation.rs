//! The `ConversationalAutomation` kind: welcome message, ice breakers and bot commands. The
//! spec has the shape Graph returns, so an export applies back unchanged.

use paloma_core::Result;
use paloma_graph::api::conversational_automation as automation;
use serde_json::{Value, json};

use crate::kind::{Cx, KindPlan, read, singleton_plan};

pub fn template() -> Value {
    json!({ "enableWelcomeMessage": "", "prompts": [], "commands": [] })
}

async fn current(cx: Cx<'_>) -> Result<Value> {
    let phone = cx.ctx.require_phone_number_id()?;
    let node = read(cx, &automation::get(phone))
        .await?
        .get("conversational_automation")
        .cloned();
    Ok(node.filter(Value::is_object).unwrap_or_else(|| json!({})))
}

pub async fn export(cx: Cx<'_>) -> Result<Vec<Value>> {
    Ok(vec![current(cx).await?])
}

pub async fn plan(cx: Cx<'_>, spec: &Value) -> Result<KindPlan> {
    let phone = cx.ctx.require_phone_number_id()?;
    let cur = current(cx).await?;
    Ok(singleton_plan(
        spec,
        &cur,
        spec.clone(),
        automation::set(phone, spec.clone()),
    ))
}
