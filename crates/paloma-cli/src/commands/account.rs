use clap::Subcommand;
use paloma_core::Result;

use super::planned;
use crate::app::App;

#[derive(Debug, Subcommand)]
pub enum AccountCommand {
    /// WABA details (planned v1.0).
    Get,
    /// Messaging limits and tier (planned v1.0).
    Limits,
    /// Messaging analytics (planned v1.0).
    Analytics,
    /// Apps subscribed to the WABA (planned v1.0).
    SubscribedApps,
}

impl AccountCommand {
    pub fn run(self, _app: &App) -> Result<()> {
        let verb = match self {
            Self::Get => "get",
            Self::Limits => "limits",
            Self::Analytics => "analytics",
            Self::SubscribedApps => "subscribed-apps",
        };
        planned(&format!("account {verb}"), "v1.0")
    }
}
