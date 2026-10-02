//! Commerce settings (cart and catalog visibility).

use crate::Request;

pub fn get(phone_number_id: &str) -> Request {
    Request::get(format!("{phone_number_id}/whatsapp_commerce_settings"))
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
