//! Graph API endpoints, one module per resource. Every function is pure and returns a
//! [`crate::Request`]; payload shaping lives here so commands and manifest kinds share
//! it.

pub mod block;
pub mod calling;
pub mod commerce;
pub mod conversational_automation;
pub mod flows;
pub mod media;
pub mod messages;
pub mod phone_numbers;
pub mod profile;
pub mod templates;
pub mod uploads;
pub mod webhook;

/// The constant every Cloud API write carries.
pub const MESSAGING_PRODUCT: &str = "whatsapp";
