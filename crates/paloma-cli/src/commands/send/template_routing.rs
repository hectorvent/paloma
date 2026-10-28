//! A fresh category chooses the send edge. Only an explicit Graph rejection permits
//! refreshing and rerouting once; ambiguous transport/server failures must not duplicate sends.

use paloma_core::{Error, Result};
use paloma_graph::api::messages::TemplateEndpoint;
use paloma_graph::api::{messages, templates};
use paloma_graph::{Response, error_map, paging};
use serde_json::Value;

use super::TemplateVia;
use crate::app::App;
use crate::output::Output;

pub(super) async fn send(
    app: &App,
    payload: Value,
    name: &str,
    language: &str,
    via: TemplateVia,
) -> Result<()> {
    let ctx = app.ctx()?;
    let phone = ctx.require_phone_number_id()?;
    let payload = messages::with_messaging_account(payload, ctx.messaging_account_id.as_deref());
    if let Some(endpoint) = via.endpoint() {
        return app
            .send_and_emit(
                &messages::send_template(phone, payload, endpoint),
                messages::message_id,
            )
            .await;
    }
    let account = ctx.messaging_account_id.as_deref().or(ctx.waba_id.as_deref()).ok_or_else(|| {
        Error::auth("automatic template routing needs --messaging-account-id or --waba-id; set one on the profile, or select --via cloud|marketing to skip category lookup")
    })?;
    if app.dry_run() {
        app.out.note("automatic routing: the POST endpoint depends on the category lookup; use --via cloud or --via marketing for a fixed endpoint preview");
        return app.print_dry_run(&[
            templates::routing_lookup_page(account, name, language, None),
            messages::template_routing_preview(phone, payload),
        ]);
    }
    let endpoint = lookup_endpoint(app, account, name, language).await?;
    let request = messages::send_template(phone, payload.clone(), endpoint);
    let response = match app.send(&request).await {
        Ok(response) => response,
        Err(original) if category_may_have_changed(&original, endpoint) => {
            let refreshed = lookup_endpoint(app, account, name, language).await?;
            if refreshed == endpoint {
                return Err(original);
            }
            app.out.note(format!(
                "template category changed; retrying once via /{}",
                refreshed.edge()
            ));
            app.send(&messages::send_template(phone, payload, refreshed))
                .await?
        }
        Err(error) => return Err(error),
    };
    emit(app, response)
}

async fn lookup_endpoint(
    app: &App,
    account: &str,
    name: &str,
    language: &str,
) -> Result<TemplateEndpoint> {
    let page = paging::find_page(
        app.client()?,
        |after| templates::routing_lookup_page(account, name, language, after),
        |page| !templates::exact_matches(page, name, Some(language)).is_empty(),
    )
    .await?
    .ok_or_else(|| {
        Error::other(format!(
            "no template named '{name}' ({language}) on Messaging account {account}"
        ))
    })?;
    let matches = templates::exact_matches(&page, name, Some(language));
    let [template] = matches.as_slice() else {
        return Err(Error::other(
            "Graph returned multiple exact template matches; refusing to choose a category",
        ));
    };
    template.get("category").and_then(Value::as_str)
        .and_then(TemplateEndpoint::for_category)
        .ok_or_else(|| Error::other("template has a missing or unsupported category; select --via cloud|marketing explicitly if you know its current category"))
}

/// Only Meta's explicit category refusals justify a fresh category check; any other
/// rejection is final.
fn category_may_have_changed(error: &Error, endpoint: TemplateEndpoint) -> bool {
    match endpoint {
        TemplateEndpoint::Cloud => error_map::is_cloud_marketing_refusal(error),
        TemplateEndpoint::Marketing => error_map::is_marketing_parameter_refusal(error),
    }
}

fn emit(app: &App, response: Option<Response>) -> Result<()> {
    if let Some(response) = response {
        let out = Output::response(&response)?;
        let id = messages::message_id(&out.data);
        app.out.emit(&out.id(id));
    }
    Ok(())
}
