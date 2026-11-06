//! Commerce settings (cart and catalog visibility).

use serde_json::Value;

use crate::Request;

pub fn get(phone_number_id: &str) -> Request {
    Request::get(format!("{phone_number_id}/whatsapp_commerce_settings"))
}

/// What turning the cart or the catalog off confirms: customers notice it.
pub const DISABLE_CONFIRM: &str = "disable shopping cart or catalog visibility";

/// Whether a change turns the cart or the catalog off.
pub fn turns_off(cart_enabled: Option<bool>, catalog_visible: Option<bool>) -> bool {
    cart_enabled == Some(false) || catalog_visible == Some(false)
}

/// Graph takes these as query parameters, not a JSON body.
pub fn set(
    phone_number_id: &str,
    is_cart_enabled: Option<bool>,
    is_catalog_visible: Option<bool>,
) -> Request {
    Request::post(format!("{phone_number_id}/whatsapp_commerce_settings"))
        .query_opt("is_cart_enabled", is_cart_enabled)
        .query_opt("is_catalog_visible", is_catalog_visible)
}

/// The one settings object a `GET` returns, which Graph wraps in `data[0]`.
pub fn first(response: &Value) -> Option<&Value> {
    response.pointer("/data/0")
}
