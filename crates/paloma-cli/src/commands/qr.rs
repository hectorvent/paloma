use clap::Subcommand;
use paloma_core::Result;

use super::planned;
use crate::app::App;

#[derive(Debug, Subcommand)]
pub enum QrCommand {
    /// Create a message QR code (planned v1.0).
    Create,
    /// List message QR codes (planned v1.0).
    List,
    /// Show a message QR code (planned v1.0).
    Get,
    /// Change a QR code's prefilled message (planned v1.0).
    Update,
    /// Delete a message QR code (planned v1.0).
    Delete,
}

impl QrCommand {
    pub fn run(self, _app: &App) -> Result<()> {
        let verb = match self {
            Self::Create => "create",
            Self::List => "list",
            Self::Get => "get",
            Self::Update => "update",
            Self::Delete => "delete",
        };
        planned(&format!("qr {verb}"), "v1.0")
    }
}
