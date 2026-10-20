//! WABA reads; portfolio messaging limits are exposed on its phone numbers, not the
//! account node itself.

use crate::Request;
use serde_json::{Value, json};

pub const DETAIL_FIELDS: &str = "id,name,timezone_id,message_template_namespace,account_review_status,business_verification_status";
pub const SETTINGS_FIELDS: &str = "name,timezone_id";
pub const HEALTH_FIELDS: &str = "health_status";
pub const LIMIT_FIELDS: &str = "id,display_phone_number,whatsapp_business_manager_messaging_limit";

pub fn get(waba_id: &str, fields: &str) -> Request {
    Request::get(waba_id).query("fields", fields)
}

pub fn limits(waba_id: &str, after: Option<&str>) -> Request {
    Request::get(format!("{waba_id}/phone_numbers"))
        .query("fields", LIMIT_FIELDS)
        .query_opt("after", after)
}

pub fn activities(waba_id: &str, after: Option<&str>) -> Request {
    Request::get(format!("{waba_id}/activities"))
        .query(
            "fields",
            "id,actor_business_id,actor_id,category,event_type,timestamp",
        )
        .query_opt("after", after)
}

/// Common writable settings, shared by typed CLI flags and future manifests.
#[derive(Debug, Default)]
pub struct Settings {
    pub name: Option<String>,
    pub timezone_id: Option<String>,
    pub insights: Option<bool>,
    pub disable_cloud_marketing: Option<bool>,
}

impl Settings {
    pub fn body(&self) -> Value {
        let mut body = json!({});
        for (key, value) in [
            ("name", self.name.as_ref().map(|v| json!(v))),
            ("timezone_id", self.timezone_id.as_ref().map(|v| json!(v))),
            ("is_enabled_for_insights", self.insights.map(|v| json!(v))),
            (
                "disable_marketing_messages_on_cloud_api",
                self.disable_cloud_marketing.map(|v| json!(v)),
            ),
        ] {
            if let Some(value) = value {
                body[key] = value;
            }
        }
        body
    }
}

pub fn set(waba_id: &str, body: Value) -> Request {
    Request::post(waba_id).json(body)
}

pub fn start_currency_migration(waba_id: &str, currency: &str, credit_id: Option<&str>) -> Request {
    let mut body = json!({"currency": currency});
    if let Some(id) = credit_id {
        body["extended_credit_id"] = json!(id);
    }
    Request::post(format!("{waba_id}/set_payment_method_migration_intent")).json(body)
}

pub fn currency_migration_status(migration_id: &str) -> Request {
    Request::get(migration_id)
}

pub fn complete_currency_migration(migration_id: &str) -> Request {
    Request::post(format!("{migration_id}/resume_migration"))
}

/// Insights enrollment is one-way and uses a query parameter, not account JSON.
pub fn enable_insights(waba_id: &str) -> Request {
    Request::post(waba_id).query("is_enabled_for_insights", true)
}

pub fn portfolio_limit(business_id: &str) -> Request {
    get(business_id, "whatsapp_business_manager_messaging_limit")
}

pub fn activities_page(waba_id: &str, after: Option<&str>, limit: Option<u32>) -> Request {
    activities(waba_id, after).query_opt("limit", limit)
}

/// The overall `can_send_message` of a `health_status` response.
pub fn overall_health(response: &Value) -> Option<String> {
    response
        .pointer("/health_status/can_send_message")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// A `health_status` response as flat rows: the overall verdict, then one per node, with
/// its limitations and errors spelled out because those say what to do about it.
pub fn health_rows(response: &Value) -> Vec<Value> {
    let overall = overall_health(response).map(|status| {
        json!({
            "entity_type": "OVERALL",
            "id": response.get("id"),
            "can_send_message": status,
        })
    });
    let entities = response
        .pointer("/health_status/entities")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|entity| {
            json!({
                "entity_type": entity.get("entity_type"),
                "id": entity.get("id"),
                "can_send_message": entity.get("can_send_message"),
                "can_receive_call_sip": entity.get("can_receive_call_sip"),
                "details": health_details(entity),
            })
        });
    overall.into_iter().chain(entities).collect()
}

fn health_details(entity: &Value) -> String {
    let list = |key: &str| {
        entity
            .get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let info = list("additional_info")
        .into_iter()
        .filter_map(|line| line.as_str().map(str::to_owned));
    let errors = list("errors").into_iter().map(|error| {
        let text = |key: &str| error.get(key).and_then(Value::as_str).unwrap_or_default();
        let code = error
            .get("error_code")
            .map_or(String::new(), Value::to_string);
        format!(
            "{code}: {} {}",
            text("error_description"),
            text("possible_solution")
        )
        .trim_end()
        .to_owned()
    });
    info.chain(errors).collect::<Vec<_>>().join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_preserve_false_and_omit_unset_fields() {
        assert_eq!(Settings::default().body(), json!({}));
        assert_eq!(
            Settings {
                name: Some("Example".into()),
                insights: Some(false),
                disable_cloud_marketing: Some(true),
                ..Settings::default()
            }
            .body(),
            json!({"name":"Example", "is_enabled_for_insights":false, "disable_marketing_messages_on_cloud_api":true})
        );
    }
}
