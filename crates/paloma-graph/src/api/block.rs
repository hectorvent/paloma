//! The block list (`/{phone-number-id}/block_users`).

use paloma_core::validate;
use serde_json::{Value, json};

use super::MESSAGING_PRODUCT;
use crate::Request;

/// A phone number goes in `user` and a business-scoped user ID in `user_id`. One entry never
/// carries both, a case where Meta would use `user`.
fn body(users: &[String]) -> Value {
    let users: Vec<Value> = users
        .iter()
        .map(|u| {
            let u = u.trim();
            let field = if validate::bsuid(u).is_some() {
                "user_id"
            } else {
                "user"
            };
            json!({ field: u })
        })
        .collect();
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

/// Where a block or unblock response lists the users it acted on.
pub const ADDED_USERS: &str = "/block_users/added_users";
pub const REMOVED_USERS: &str = "/block_users/removed_users";

/// How many users a block or unblock response says failed.
pub fn failed_count(response: &Value) -> usize {
    response
        .pointer("/block_users/failed_users")
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_user_is_addressed_by_its_own_kind() {
        let users = [" 15551234567 ".to_owned(), "US.1349".to_owned()];
        assert_eq!(
            body(&users),
            json!({
                "messaging_product": "whatsapp",
                "block_users": [{ "user": "15551234567" }, { "user_id": "US.1349" }]
            })
        );
    }
}
