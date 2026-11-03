//! The WABA callback override plus the app-level field subscription.

use std::collections::BTreeSet;

use paloma_core::{Error, Result};
use paloma_graph::api::webhook;
use serde_json::{Map, Value, json};

use paloma_graph::lookup::{AppChoice, find_app};

use crate::kind::{Cx, KindPlan, read};

/// The app a fields change goes to, named like `webhook fields set` does, since a WABA can
/// have several subscribed apps.
fn describe(app: &AppChoice) -> String {
    match app {
        AppChoice::Given(id) => format!("app {id}"),
        AppChoice::Discovered { id, subscribed } if *subscribed > 1 => format!(
            "app {id}, the first of {subscribed} subscribed to the WABA; pick one with --app-id"
        ),
        AppChoice::Discovered { id, .. } => format!("app {id}, subscribed to the WABA"),
    }
}

pub fn template() -> Value {
    json!({ "callbackUrl": "", "verifyToken": "", "fields": [] })
}

/// The app, its subscriptions response, and how the app was picked; needs an app id and
/// the app secret.
async fn subscriptions(cx: Cx<'_>) -> Result<(AppChoice, Value)> {
    let waba = cx.ctx.require_waba_id()?;
    let app = find_app(cx.client, cx.ctx.app_id.as_deref(), Some(waba)).await?;
    let subs = read(cx, &webhook::app_subscriptions(app.id())).await?;
    Ok((app, subs))
}

pub async fn export(cx: Cx<'_>) -> Result<Vec<Value>> {
    let waba = cx.ctx.require_waba_id()?;
    let mut spec = Map::new();
    if let Some(url) = paloma_graph::lookup::current_callback_url(
        cx.client,
        waba,
        cx.ctx.phone_number_id.as_deref(),
    )
    .await?
    {
        spec.insert("callback_url".into(), url.into());
    }
    // Fields need app credentials; omit them rather than fail the export.
    if let Ok((_, subs)) = subscriptions(cx).await {
        let fields = webhook::subscribed_fields(&subs);
        if !fields.is_empty() {
            spec.insert("fields".into(), fields.into());
        }
    }
    Ok(if spec.is_empty() {
        vec![]
    } else {
        vec![Value::Object(spec)]
    })
}

pub async fn plan(cx: Cx<'_>, spec: &Value) -> Result<KindPlan> {
    let waba = cx.ctx.require_waba_id()?;
    let text = |k: &str| {
        spec.get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    let verify_token = text("verify_token");
    let mut plan = KindPlan::default();
    let mut preview = Map::new();
    let mut callback_changes = false;
    let mut fields_change = false;

    if let Some(desired) = text("callback_url") {
        // The same checks as `webhook subscribe`, before any read.
        webhook::check_callback_url(desired, "callbackUrl")?;
        let current = paloma_graph::lookup::current_callback_url(
            cx.client,
            waba,
            cx.ctx.phone_number_id.as_deref(),
        )
        .await?;
        if current.as_deref() != Some(desired) {
            let token = verify_token.ok_or_else(|| {
                Error::validation("Webhook change needs a 'verifyToken' in the spec (a secret), or use `paloma webhook subscribe`")
            })?;
            plan.changes.push(format!(
                "callback_url {} -> {desired}",
                current.as_deref().unwrap_or("(none)")
            ));
            preview.insert(
                "override".into(),
                json!({ "override_callback_uri": desired, "verify_token": "******" }),
            );
            plan.requests.push(webhook::subscribe(waba, desired, token));
            callback_changes = true;
        }
    }

    if let Some(Value::Array(items)) = spec.get("fields") {
        let mut desired: Vec<String> = Vec::new();
        for f in items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|f| !f.is_empty())
        {
            if !desired.iter().any(|d| d == f) {
                desired.push(f.to_owned());
            }
        }
        let (app, subs) = subscriptions(cx).await?;
        let app_id = app.id().to_owned();
        let current: BTreeSet<String> = webhook::subscribed_fields(&subs).into_iter().collect();
        let wanted: BTreeSet<String> = desired.iter().cloned().collect();
        if wanted != current {
            let added: Vec<_> = wanted.difference(&current).cloned().collect();
            let removed: Vec<_> = current.difference(&wanted).cloned().collect();
            // Only new names: a field the app already has, which `export` wrote out, may be
            // one Meta added after this list was made, and must not break a re-apply.
            let unknown: Vec<&str> = added
                .iter()
                .map(String::as_str)
                .filter(|f| !webhook::AVAILABLE_FIELDS.contains(f))
                .collect();
            if !unknown.is_empty() {
                return Err(Error::validation(format!(
                    "unknown webhook field: {}; `paloma webhook fields list` lists them",
                    unknown.join(", ")
                )));
            }
            let callback = webhook::app_callback_url(&subs)
                .or_else(|| text("callback_url").map(str::to_owned));
            plan.changes.push(format!(
                "fields +[{}] -[{}] on {}",
                added.join(", "),
                removed.join(", "),
                describe(&app)
            ));
            fields_change = true;
            let mut body = json!({ "object": "whatsapp_business_account", "fields": desired });
            if let Some(c) = &callback {
                body["callback_url"] = c.clone().into();
            }
            preview.insert("subscription".into(), body);
            plan.requests.push(webhook::set_app_subscriptions(
                &app_id,
                &desired,
                callback.as_deref(),
                verify_token,
            ));
        }
    }

    if !plan.requests.is_empty() {
        plan.preview = Some(Value::Object(preview));
        // The same guards as `webhook subscribe` and `webhook fields set`: either changes
        // what production receives.
        plan.confirm = Some(match (callback_changes, fields_change) {
            (true, true) => webhook::SUBSCRIBE_AND_FIELDS_CONFIRM,
            (true, false) => webhook::SUBSCRIBE_CONFIRM,
            _ => webhook::FIELDS_CONFIRM,
        });
    }
    Ok(plan)
}
