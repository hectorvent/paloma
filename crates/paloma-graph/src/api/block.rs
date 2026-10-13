//! The block list (`/{phone-number-id}/block_users`).

use serde_json::{Value, json};

use super::MESSAGING_PRODUCT;
use crate::Request;

fn body(users: &[String]) -> Value {
    let users: Vec<Value> = users.iter().map(|u| json!({ "user": u })).collect();
    json!({ "messaging_product": MESSAGING_PRODUCT, "block_users": users })
}

pub fn add(phone_number_id: &str, users: &[String]) -> Request {
    Request::post(format!("{phone_number_id}/block_users")).json(body(users))
}

/// Unblock: a DELETE that carries a JSON body.
pub fn remove(phone_number_id: &str, users: &[String]) -> Request {
    Request::delete(format!("{phone_number_id}/block_users")).json(body(users))
}

pub fn list(phone_number_id: &str, limit: Option<u32>, after: Option<&str>) -> Request {
    Request::get(format!("{phone_number_id}/block_users"))
        .query_opt("limit", limit)
        .query_opt("after", after)
}
