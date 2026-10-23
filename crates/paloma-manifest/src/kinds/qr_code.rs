//! QR codes already on the number, one manifest per code. Meta generates the code and
//! offers no name or other key a manifest could use to find one it created, so `code` is
//! the identity and `apply` only reconciles the prefilled message. It never creates a code
//! (`paloma qr create` does, and `export` picks it up) and never deletes one.

use paloma_core::{Error, Result, validate};
use paloma_graph::api::qr;
use paloma_graph::paging;
use serde_json::{Value, json};

use crate::kind::{Cx, KindPlan, read, singleton_plan};

pub fn template() -> Value {
    json!({ "code": "", "prefilledMessage": "" })
}

pub async fn export(cx: Cx<'_>) -> Result<Vec<Value>> {
    let phone = cx.ctx.require_phone_number_id()?;
    let all = paging::collect_all(cx.client, None, |after| qr::list(phone, None, after)).await?;
    // Live rows are reported as Graph returned them: the input rules in `spec_fields` belong
    // to `plan`, and one unusual row mustn't hide the rest of the export.
    Ok(all
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|q| json!({"code": q.get("code"), "prefilled_message": q.get("prefilled_message")}))
        .collect())
}

pub async fn plan(cx: Cx<'_>, spec: &Value) -> Result<KindPlan> {
    let (code, message) = spec_fields(spec)?;
    let phone = cx.ctx.require_phone_number_id()?;
    let response = read(cx, &qr::get(phone, code, None)).await?;
    let current = qr::details(&response, code)?.ok_or_else(|| {
        Error::validation(format!(
            "no QR code {code} on this number; a new one gets a new code, so create it with \
             `paloma qr create` and declare that"
        ))
    })?;
    let request = qr::update(phone, code, message);
    let preview = request.json_body().cloned().unwrap_or_default();
    let mut plan = singleton_plan(
        &json!({ "prefilled_message": message }),
        current,
        preview,
        request,
    );
    // Every QrCode manifest of a number has the same label, so name the code.
    for change in &mut plan.changes {
        *change = format!("{code} {change}");
    }
    Ok(plan)
}

fn spec_fields(spec: &Value) -> Result<(&str, &str)> {
    let code = spec
        .get("code")
        .and_then(Value::as_str)
        .filter(|code| !code.is_empty())
        .ok_or_else(|| {
            Error::validation(
                "QrCode spec needs a 'code': Meta generates it, so create the code with \
                 `paloma qr create`, then declare it (`paloma export qrcode` lists them)",
            )
        })?;
    validate::qr_code(code)?;
    let message = spec
        .get("prefilled_message")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::validation(format!("QrCode {code} needs a 'prefilledMessage'")))?;
    validate::qr_message(message)?;
    Ok((code, message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spec_needs_a_valid_code_and_a_message_within_the_limit() {
        let ok = json!({ "code": "WOMVT6TJ2BP7A1", "prefilled_message": "Hi" });
        assert_eq!(spec_fields(&ok).unwrap(), ("WOMVT6TJ2BP7A1", "Hi"));
        for bad in [
            json!({ "prefilled_message": "Hi" }),
            json!({ "code": "", "prefilled_message": "Hi" }),
            json!({ "code": "a/b", "prefilled_message": "Hi" }),
            json!({ "code": "WOMVT6TJ2BP7A1" }),
            json!({ "code": "WOMVT6TJ2BP7A1", "prefilled_message": "a".repeat(141) }),
        ] {
            assert_eq!(
                spec_fields(&bad).unwrap_err().exit_code().code(),
                5,
                "{bad}"
            );
        }
    }
}
