//! Conversational components: welcome message, ice breakers (prompts), commands.

use serde_json::Value;

use crate::Request;

pub fn get(phone_number_id: &str) -> Request {
    Request::get(phone_number_id).query("fields", "conversational_automation")
}

pub fn set(phone_number_id: &str, spec: Value) -> Request {
    Request::post(format!("{phone_number_id}/conversational_automation")).json(spec)
}
