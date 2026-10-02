//! The managed set of templates. Missing ones are created; existing ones are edited
//! only when category or BODY text differ (an edit resubmits for review). Templates
//! absent from the manifests are never deleted.

use paloma_core::{Error, Result};
use paloma_graph::api::templates::{self, ListFilter};
use paloma_graph::paging;
use serde_json::{Value, json};

use crate::diff;
use crate::kind::{Cx, KindPlan, read};

pub fn template() -> Value {
    json!({ "name": "", "language": "", "category": "", "components": [] })
}

pub async fn export(cx: Cx<'_>) -> Result<Vec<Value>> {
    let waba = cx.ctx.require_waba_id()?;
    // Follow every page: an export that silently dropped templates would mislead a later diff.
    let all = paging::collect_all(cx.client, None, |after| {
        templates::list(waba, &ListFilter::default(), after)
    })
    .await?;
    Ok(all
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|t| {
            let mut spec = json!({ "name": t.get("name"), "language": t.get("language"), "category": t.get("category") });
            if let Some(c) = t.get("components") {
                spec["components"] = c.clone();
            }
            spec
        })
        .collect())
}

pub async fn plan(cx: Cx<'_>, spec: &Value) -> Result<KindPlan> {
    let s = |k: &str| {
        spec.get(k)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_owned()
    };
    let (name, language) = (s("name"), s("language"));
    if name.is_empty() || language.is_empty() {
        return Err(Error::validation(
            "Template spec needs both 'name' and 'language'",
        ));
    }
    let create_body = create_body(spec, &name, &language)?;
    let desired = normalize(&create_body);
    let waba = cx.ctx.require_waba_id()?;

    let found = read(cx, &templates::lookup(waba, &name, Some(&language))).await?;
    let Some(current) = templates::exact_matches(&found, &name, Some(&language))
        .first()
        .map(|v| (*v).clone())
    else {
        let category = desired
            .get("category")
            .and_then(Value::as_str)
            .unwrap_or("");
        return Ok(KindPlan {
            changes: vec![format!("create {name}/{language} (category {category})")],
            preview: Some(create_body.clone()),
            requests: vec![templates::create(waba, create_body)],
        });
    };
    let current_norm = normalize(&current);
    if diff::in_sync(&desired, Some(&current_norm)) {
        return Ok(KindPlan::synced());
    }
    let mut changes = diff::changes(&desired, Some(&current_norm));
    changes.push(format!("(edit resubmits '{name}/{language}' for approval)"));
    let category = spec
        .get("category")
        .or_else(|| current_norm.get("category"))
        .cloned()
        .unwrap_or_default();
    let edit_body = json!({ "category": category, "components": create_body.get("components") });
    let id = current
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::other("template has no id"))?;
    Ok(KindPlan {
        changes,
        preview: Some(edit_body.clone()),
        requests: vec![templates::edit(id, edit_body)],
    })
}

/// The create body: from `components`, or from the `body`/`example` shorthand.
fn create_body(spec: &Value, name: &str, language: &str) -> Result<Value> {
    let category = spec.get("category").and_then(Value::as_str).unwrap_or("");
    if let Some(components) = spec.get("components") {
        return Ok(
            json!({ "name": name, "category": category, "language": language, "components": components }),
        );
    }
    let text = spec.get("body").and_then(Value::as_str).unwrap_or("");
    if text.trim().is_empty() {
        return Err(Error::validation(format!(
            "Template '{name}' needs either 'components' or a 'body' string"
        )));
    }
    let example: Vec<String> = match spec.get("example") {
        Some(Value::String(s)) => s.split(',').map(|v| v.trim().to_owned()).collect(),
        Some(Value::Array(a)) => a
            .iter()
            .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned))
            .collect(),
        _ => Vec::new(),
    };
    Ok(templates::inline_body(
        name, category, language, text, &example,
    ))
}

/// The fields compared for drift: category and BODY text.
fn normalize(t: &Value) -> Value {
    let body = t
        .get("components")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|c| {
            c.get("type")
                .and_then(Value::as_str)
                .is_some_and(|ty| ty.eq_ignore_ascii_case("BODY"))
        })
        .and_then(|c| c.get("text"))
        .and_then(Value::as_str)
        .unwrap_or("");
    json!({ "category": t.get("category").and_then(Value::as_str).unwrap_or(""), "body": body })
}
