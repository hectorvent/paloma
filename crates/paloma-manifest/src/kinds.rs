//! One module per manifest kind: `template()`, `export()`, `plan()`.

pub mod account;
pub mod calling;
pub mod commerce;
pub mod conversational_automation;
pub mod number;
pub mod profile;
pub mod qr_code;
pub mod template;
pub mod webhook;

pub mod phone_settings;

#[cfg(test)]
mod plan_tests;
