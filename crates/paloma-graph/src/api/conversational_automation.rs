//! Conversational components: welcome message, ice breakers (prompts), commands.

use serde_json::Value;

use crate::Request;

pub fn get(phone_number_id: &str) -> Request {
    Request::get(phone_number_id).query("fields", "conversational_automation")
}

pub fn set(phone_number_id: &str, spec: Value) -> Request {
    Request::post(format!("{phone_number_id}/conversational_automation")).json(spec)
}

/// Typed common settings; empty lists explicitly clear the corresponding component.
#[derive(Debug, Default, serde::Serialize)]
pub struct Settings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_welcome_message: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commands: Option<Vec<Command>>,
}

#[derive(Debug, serde::Serialize)]
pub struct Command {
    pub command_name: String,
    pub command_description: String,
}

impl Settings {
    pub fn body(&self) -> Value {
        // All members are JSON primitives; serialization cannot fail.
        serde_json::to_value(self).expect("automation settings are JSON primitives")
    }
}

pub fn bot(id: &str) -> Request {
    Request::get(id)
}
