//! The managed set of templates. Missing ones are created; existing ones are edited
//! only when they drift (an edit resubmits for review). Templates absent from the
//! manifests are never deleted.
//!
//! Drift is what the manifest declares. A `components` spec compares every declared
//! component and field, but not `example` blocks: those are review samples Meta rewrites
//! on read (an uploaded header handle comes back as a CDN URL), so comparing them would
//! drift forever. Fields Meta adds that the spec doesn't declare are ignored too. The
//! `body` shorthand declares only the BODY text, so it leaves other live components alone.

use paloma_core::{Error, Result};
use paloma_graph::api::templates::{self, ListFilter};
use paloma_graph::paging;
use serde_json::{Map, Value, json};

use crate::diff;
use crate::kind::{Cx, KindPlan};

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
    let shorthand = spec.get("components").is_none();
    let desired = normalize(&create_body, shorthand, None);
    let waba = cx.ctx.require_waba_id()?;

    // Follow the cursor: stopping short would plan a create for a template that exists.
    let found = paloma_graph::lookup::find_page(cx.client, waba, &name, Some(&language)).await?;
    let Some(current) = found.as_ref().and_then(|page| {
        templates::exact_matches(page, &name, Some(&language))
            .first()
            .map(|v| (*v).clone())
    }) else {
        let category = desired
            .get("category")
            .and_then(Value::as_str)
            .unwrap_or("");
        return Ok(KindPlan {
            changes: vec![format!("create {name}/{language} (category {category})")],
            preview: Some(create_body.clone()),
            requests: vec![templates::create(waba, create_body)],
            ..KindPlan::default()
        });
    };
    let current_norm = normalize(&current, shorthand, Some(&desired));
    if diff::in_sync(&desired, Some(&current_norm)) {
        return Ok(KindPlan::synced());
    }
    let category_changed =
        spec.get("category").is_some() && desired.get("category") != current_norm.get("category");
    let content_key = if shorthand { "body" } else { "components" };
    let content_changed = desired.get(content_key) != current_norm.get(content_key);
    if !category_changed && !content_changed {
        return Ok(KindPlan::synced());
    }
    let mut changes = component_changes(&desired, &current_norm);
    changes.push(format!("(edit resubmits '{name}/{language}' for approval)"));
    // Send only what changed: Meta's examples edit each separately, and an approved
    // template's category can't be edited at all.
    let mut edit_body = Map::new();
    if category_changed {
        edit_body.insert("category".into(), desired["category"].clone());
    }
    if content_changed {
        // An edit replaces every component, so a shorthand edit must carry the live
        // non-BODY components forward or it would delete them.
        let components = if shorthand {
            shorthand_edit_components(&current, &create_body, &name, &language)?
        } else {
            create_body.get("components").cloned().unwrap_or_default()
        };
        edit_body.insert("components".into(), components);
    }
    let edit_body = Value::Object(edit_body);
    let id = current
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::other("template has no id"))?;
    // The same guard as `template update`.
    Ok(KindPlan {
        changes,
        preview: Some(edit_body.clone()),
        requests: vec![templates::edit(id, edit_body)],
        refusal: edit_refusal(&current, &name, &language, category_changed),
        ..KindPlan::default()
    }
    .confirming(templates::EDIT_CONFIRM))
}

/// Meta's limits on edits, which a live template's status is enough to check.
fn edit_refusal(
    current: &Value,
    name: &str,
    language: &str,
    category_changed: bool,
) -> Option<String> {
    let status = current.get("status").and_then(Value::as_str)?;
    if !matches!(status, "APPROVED" | "REJECTED" | "PAUSED") {
        return Some(format!(
            "template {name}/{language} is {status}; only APPROVED, REJECTED or PAUSED templates can be edited"
        ));
    }
    let live = current
        .get("category")
        .and_then(Value::as_str)
        .unwrap_or("?");
    (category_changed && status == "APPROVED").then(|| {
        format!(
            "template {name}/{language} is APPROVED, so Meta won't change its category; declare {live} or create a new template"
        )
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

/// The live components with BODY swapped for the shorthand's. Refused when the live
/// template has media (a media header or a carousel): Meta returns those examples as
/// CDN URLs, but an edit needs upload handles, so they can't be resent as read.
fn shorthand_edit_components(
    current: &Value,
    create_body: &Value,
    name: &str,
    language: &str,
) -> Result<Value> {
    let is_type = |c: &Value, ty: &str| {
        c.get("type")
            .and_then(Value::as_str)
            .is_some_and(|t| t.eq_ignore_ascii_case(ty))
    };
    let body = create_body
        .get("components")
        .and_then(Value::as_array)
        .and_then(|cs| cs.iter().find(|c| is_type(c, "BODY")))
        .cloned()
        .unwrap_or_default();
    let live = current
        .get("components")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let has_media = live.iter().any(|c| {
        is_type(c, "CAROUSEL")
            || (is_type(c, "HEADER")
                && c.get("format").and_then(Value::as_str).is_some_and(|f| {
                    ["IMAGE", "VIDEO", "DOCUMENT", "GIF"]
                        .iter()
                        .any(|m| f.eq_ignore_ascii_case(m))
                }))
    });
    if has_media {
        return Err(Error::validation(format!(
            "Template '{name}/{language}' has a media header or carousel, which the 'body' \
             shorthand can't edit without losing it. Declare its full 'components' instead \
             (start from 'paloma export template')"
        )));
    }
    let mut out: Vec<Value> = live
        .into_iter()
        .map(|c| if is_type(&c, "BODY") { body.clone() } else { c })
        .collect();
    if !out.iter().any(|c| is_type(c, "BODY")) {
        out.push(body);
    }
    Ok(Value::Array(out))
}

/// The state compared for drift. With `like` (the normalized desired state), the
/// current state keeps only the fields the spec declares.
fn normalize(t: &Value, shorthand: bool, like: Option<&Value>) -> Value {
    let category = t.get("category").and_then(Value::as_str).unwrap_or("");
    if shorthand {
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
        return json!({ "category": category, "body": body });
    }
    let components = canonical(t.get("components").unwrap_or(&Value::Null));
    let components = match like.and_then(|l| l.get("components")) {
        Some(declared) => project(&components, declared),
        None => components,
    };
    json!({ "category": category, "components": components })
}

/// Drops `example` blocks and upper-cases `type`/`format`, which Meta returns in
/// upper case whatever case they were created in.
fn canonical(v: &Value) -> Value {
    match v {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(k, _)| k.as_str() != "example")
                .map(|(k, v)| {
                    let v = match (k.as_str(), v) {
                        ("type" | "format", Value::String(s)) => Value::String(s.to_uppercase()),
                        _ => canonical(v),
                    };
                    (k.clone(), v)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

/// `current` reduced to the shape of `declared`: object fields the spec doesn't name
/// are dropped, arrays of equal length are projected element by element. Arrays of
/// different length stay whole, so an added or removed component still drifts.
fn project(current: &Value, declared: &Value) -> Value {
    match (current, declared) {
        (Value::Object(c), Value::Object(d)) => Value::Object(
            c.iter()
                .filter_map(|(k, v)| d.get(k).map(|dv| (k.clone(), project(v, dv))))
                .collect(),
        ),
        (Value::Array(c), Value::Array(d)) if c.len() == d.len() => {
            Value::Array(c.iter().zip(d).map(|(cv, dv)| project(cv, dv)).collect())
        }
        _ => current.clone(),
    }
}

/// Readable drift lines, with array items addressed by index
/// (`components.2.buttons.0.text: "Shop" -> "Buy"`).
fn component_changes(desired: &Value, current: &Value) -> Vec<String> {
    let count = |v: &Value| {
        v.get("components")
            .and_then(Value::as_array)
            .map_or(0, Vec::len)
    };
    let mut out = Vec::new();
    if count(desired) != count(current) {
        out.push(format!(
            "components: {} live -> {} in manifest",
            count(current),
            count(desired)
        ));
    }
    out.extend(diff::changes(&indexed(desired), Some(&indexed(current))));
    out
}

fn indexed(v: &Value) -> Value {
    match v {
        Value::Array(items) => Value::Object(
            items
                .iter()
                .enumerate()
                .map(|(i, item)| (i.to_string(), indexed(item)))
                .collect(),
        ),
        Value::Object(map) => {
            Value::Object(map.iter().map(|(k, v)| (k.clone(), indexed(v))).collect())
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn spec_components() -> Value {
        json!({
            "category": "MARKETING",
            "components": [
                { "type": "header", "format": "image", "example": { "header_handle": ["4::upload-handle"] } },
                { "type": "BODY", "text": "Hi {{1}}", "example": { "body_text": [["Ana"]] } },
                { "type": "FOOTER", "text": "Reply STOP to opt out" },
                { "type": "BUTTONS", "buttons": [{ "type": "URL", "text": "Shop", "url": "https://example.com/{{1}}" }] }
            ]
        })
    }

    /// What Meta returns for the same template: upper-case types, the handle rewritten
    /// to a CDN URL, and server-side fields the spec never declared.
    fn live_components() -> Value {
        json!({
            "id": "123",
            "status": "APPROVED",
            "category": "MARKETING",
            "components": [
                { "type": "HEADER", "format": "IMAGE", "example": { "header_handle": ["https://scontent.example/img.jpg"] } },
                { "type": "BODY", "text": "Hi {{1}}", "example": { "body_text": [["Ana"]] } },
                { "type": "FOOTER", "text": "Reply STOP to opt out" },
                { "type": "BUTTONS", "buttons": [{ "type": "URL", "text": "Shop", "url": "https://example.com/{{1}}", "example": ["https://example.com/a"] }] }
            ]
        })
    }

    fn drifts(spec: &Value, live: &Value) -> bool {
        let desired = normalize(spec, false, None);
        !diff::in_sync(&desired, Some(&normalize(live, false, Some(&desired))))
    }

    #[test]
    fn same_template_is_in_sync_despite_examples_case_and_server_fields() {
        assert!(!drifts(&spec_components(), &live_components()));
    }

    #[test]
    fn footer_only_change_drifts() {
        let mut spec = spec_components();
        spec["components"][2]["text"] = json!("Reply STOP to unsubscribe");
        assert!(drifts(&spec, &live_components()));
    }

    #[test]
    fn button_only_change_drifts_and_names_the_path() {
        let mut spec = spec_components();
        spec["components"][3]["buttons"][0]["text"] = json!("Buy");
        let desired = normalize(&spec, false, None);
        let current = normalize(&live_components(), false, Some(&desired));
        assert_eq!(
            component_changes(&desired, &current),
            vec![r#"components.3.buttons.0.text: "Shop" -> "Buy""#.to_owned()]
        );
    }

    #[test]
    fn header_format_change_drifts() {
        let mut spec = spec_components();
        spec["components"][0]["format"] = json!("VIDEO");
        assert!(drifts(&spec, &live_components()));
    }

    #[test]
    fn removed_component_drifts() {
        let mut spec = spec_components();
        spec["components"].as_array_mut().unwrap().remove(2);
        let desired = normalize(&spec, false, None);
        let current = normalize(&live_components(), false, Some(&desired));
        assert!(!diff::in_sync(&desired, Some(&current)));
        assert_eq!(
            component_changes(&desired, &current)[0],
            "components: 4 live -> 3 in manifest"
        );
    }

    #[test]
    fn shorthand_compares_only_category_and_body() {
        let spec = json!({ "category": "MARKETING", "components": [{ "type": "BODY", "text": "Hi {{1}}" }] });
        let desired = normalize(&spec, true, None);
        assert!(diff::in_sync(
            &desired,
            Some(&normalize(&live_components(), true, Some(&desired)))
        ));
    }
}
