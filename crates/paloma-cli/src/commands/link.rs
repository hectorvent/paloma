//! wa.me links are built offline, with no token or API call, so they work anywhere.

use clap::{Args, Subcommand};
use paloma_core::{Error, Result};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::json;

use crate::app::App;
use crate::output::Output;

#[derive(Debug, Subcommand)]
pub enum LinkCommand {
    /// Build a `https://wa.me` click-to-chat link.
    Create(Create),
}

#[derive(Debug, Args)]
pub struct Create {
    /// Phone number in international format; non-digits are ignored.
    #[arg(long)]
    phone: String,
    /// Pre-filled message text.
    #[arg(long)]
    text: Option<String>,
}

/// RFC 3986 unreserved characters stay literal; spaces become `%20`, not `+`.
const QUERY: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

impl LinkCommand {
    pub fn run(self, app: &App) -> Result<()> {
        let Self::Create(args) = self;
        let digits: String = args.phone.chars().filter(char::is_ascii_digit).collect();
        if digits.is_empty() {
            return Err(Error::validation("--phone must contain digits"));
        }
        let mut url = format!("https://wa.me/{digits}");
        if let Some(text) = args.text.as_deref().filter(|t| !t.is_empty()) {
            url.push_str("?text=");
            url.push_str(&utf8_percent_encode(text, QUERY).to_string());
        }
        app.out
            .emit(&Output::new(json!({ "url": url, "phone": digits })).id(Some(url)));
        Ok(())
    }
}
