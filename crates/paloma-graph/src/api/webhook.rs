//! Webhook callbacks: the per-WABA override (`subscribed_apps`) and the app-level field
//! subscriptions (`/{app-id}/subscriptions`, app-token authorized).

use serde_json::{Value, json};

use crate::Request;

/// Every `whatsapp_business_account` webhook field an app can subscribe to.
pub const AVAILABLE_FIELDS: [&str; 28] = [
    "messages",
    "calls",
    "message_template_status_update",
    "message_template_quality_update",
    "template_category_update",
    "account_update",
    "account_alerts",
    "account_review_update",
    "business_capability_update",
    "phone_number_name_update",
    "phone_number_quality_update",
    "security",
    "messaging_handovers",
    "user_preferences",
    "history",
    "smb_message_echoes",
    "flows",
    "group_lifecycle_update",
    "group_participants_update",
    "group_settings_update",
    "group_status_update",
    "business_username_updates",
    "message_template_components_update",
    "account_settings_update",
    "automatic_events",
    "partner_solutions",
    "payment_configuration_update",
    "smb_app_state_sync",
];

pub fn subscribed_apps(waba_id: &str) -> Request {
    Request::get(format!("{waba_id}/subscribed_apps"))
}

/// Subscribe this app to the WABA, overriding its callback URL.
pub fn subscribe(waba_id: &str, callback_url: &str, verify_token: &str) -> Request {
    Request::post(format!("{waba_id}/subscribed_apps"))
        .json(json!({ "override_callback_uri": callback_url, "verify_token": verify_token }))
}

pub fn unsubscribe(waba_id: &str) -> Request {
    Request::delete(format!("{waba_id}/subscribed_apps"))
}

/// The phone number's effective webhook configuration.
pub fn configuration(phone_number_id: &str) -> Request {
    Request::get(phone_number_id).query("fields", "webhook_configuration")
}

/// The app id of the first app subscribed to the WABA, from a `subscribed_apps` response.
pub fn subscribed_app_id(subscribed: &Value) -> Option<String> {
    subscribed
        .pointer("/data/0/whatsapp_business_api_data/id")
        .and_then(|v| {
            v.as_str()
                .map(str::to_owned)
                .or_else(|| v.as_i64().map(|n| n.to_string()))
        })
}

pub fn app_subscriptions(app_id: &str) -> Request {
    Request::get(format!("{app_id}/subscriptions")).app_auth(app_id)
}

/// Replace the app's `whatsapp_business_account` field subscription. Graph takes these as
/// query parameters.
pub fn set_app_subscriptions(
    app_id: &str,
    fields: &[String],
    callback_url: Option<&str>,
    verify_token: Option<&str>,
) -> Request {
    Request::post(format!("{app_id}/subscriptions"))
        .app_auth(app_id)
        .query("object", "whatsapp_business_account")
        .query("fields", fields.join(","))
        .query_opt("callback_url", callback_url)
        .query_opt("verify_token", verify_token)
}

/// Each `whatsapp_business_account` field and the Graph version Meta delivers it in, from a
/// subscriptions response. Other objects on the same app, such as `page`, are left out.
pub fn field_versions(subscriptions: &Value) -> Vec<(String, Option<String>)> {
    subscriptions
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|s| s.get("object").and_then(Value::as_str) == Some("whatsapp_business_account"))
        .flat_map(|s| {
            s.get("fields")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|f| {
            let name = f.get("name").and_then(Value::as_str)?.to_owned();
            let version = f.get("version").and_then(Value::as_str).map(str::to_owned);
            Some((name, version))
        })
        .collect()
}

/// The subscribed fields for `whatsapp_business_account` from a subscriptions response.
pub fn subscribed_fields(subscriptions: &Value) -> Vec<String> {
    subscriptions
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|s| s.get("object").and_then(Value::as_str) == Some("whatsapp_business_account"))
        .flat_map(|s| {
            s.get("fields")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|f| {
            f.get("name")
                .and_then(Value::as_str)
                .or_else(|| f.as_str())
                .map(str::to_owned)
        })
        .collect()
}

/// `data[0].override_callback_uri` of a `subscribed_apps` response.
pub fn override_callback(subscribed: &Value) -> Option<String> {
    subscribed
        .pointer("/data/0/override_callback_uri")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// `webhook_configuration.application` of a phone-number response.
pub fn application_callback(config: &Value) -> Option<String> {
    config
        .pointer("/webhook_configuration/application")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// The app-level callback URL for `whatsapp_business_account`, from a subscriptions response.
pub fn app_callback_url(subscriptions: &Value) -> Option<String> {
    subscriptions
        .get("data")?
        .as_array()?
        .iter()
        .find(|s| s.get("object").and_then(Value::as_str) == Some("whatsapp_business_account"))?
        .get("callback_url")?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Subscribe using the app-level callback, removing a WABA alternate callback.
pub fn subscribe_default(waba_id: &str) -> Request {
    Request::post(format!("{waba_id}/subscribed_apps"))
}

/// A blank URL removes the number's alternate callback without unsubscribing the app.
pub fn number_callback(phone: &str, url: &str, token: Option<&str>) -> Request {
    let mut config = json!({"override_callback_uri":url});
    if let Some(token) = token {
        config["verify_token"] = json!(token);
    }
    Request::post(phone).json(json!({"webhook_configuration":config}))
}

/// The active phone number's alternate callback, when configured.
pub fn number_override_callback(config: &Value) -> Option<String> {
    config
        .pointer("/webhook_configuration/phone_number")
        .and_then(Value::as_str)
        .filter(|url| !url.is_empty())
        .map(str::to_owned)
}

/// Effective delivery prefers a number override, then the WABA override, then the app.
pub fn effective_callback(config: &Value, waba_override: Option<&str>) -> Option<String> {
    [
        config
            .pointer("/webhook_configuration/phone_number")
            .and_then(Value::as_str),
        config
            .pointer("/webhook_configuration/whatsapp_business_account")
            .and_then(Value::as_str),
        waba_override,
        config
            .pointer("/webhook_configuration/application")
            .and_then(Value::as_str),
    ]
    .into_iter()
    .flatten()
    .find(|v| !v.is_empty())
    .map(str::to_owned)
}
