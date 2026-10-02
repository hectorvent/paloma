//! Only the two-step verification PIN is writable, and it cannot be read back, so a spec
//! with a `pin` always counts as drift.

use paloma_core::{Result, validate};
use paloma_graph::api::phone_numbers::{self, DETAIL_FIELDS};
use serde_json::{Value, json};

use crate::kind::{Cx, KindPlan, read};

pub fn template() -> Value {
    json!({ "pin": "" })
}

pub async fn export(cx: Cx<'_>) -> Result<Vec<Value>> {
    let phone = cx.ctx.require_phone_number_id()?;
    Ok(vec![
        read(cx, &phone_numbers::get(phone, DETAIL_FIELDS)).await?,
    ])
}

pub fn plan(cx: Cx<'_>, spec: &Value) -> Result<KindPlan> {
    let Some(pin) = spec.get("pin").filter(|p| !p.is_null()) else {
        return Ok(KindPlan::synced());
    };
    let pin = pin.as_str().map_or_else(|| pin.to_string(), str::to_owned);
    validate::pin(&pin)?;
    let phone = cx.ctx.require_phone_number_id()?;
    Ok(KindPlan {
        changes: vec!["pin: (write-only) -> ******".into()],
        preview: Some(json!({ "pin": "******" })),
        requests: vec![phone_numbers::set_pin(phone, &pin)],
    })
}
