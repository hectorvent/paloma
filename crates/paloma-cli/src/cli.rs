//! The clap command tree and the global flags.

use std::path::PathBuf;

use clap::{Args, Parser, ValueEnum};
use secrecy::SecretString;

use crate::commands::Command;

/// An unofficial CLI for the WhatsApp Cloud API.
///
/// Shape: `paloma <resource> <verb> [args] [flags]`. Data goes to stdout, diagnostics to
/// stderr; exit codes: 0 ok, 1 error, 2 usage, 3 auth, 4 rate-limited, 5 validation,
/// 6 policy.
#[derive(Debug, Parser)]
#[command(
    name = "paloma",
    version,
    propagate_version = true,
    max_term_width = 100
)]
pub struct Cli {
    #[command(flatten)]
    pub globals: GlobalArgs,

    #[command(subcommand)]
    pub command: Command,
}

/// `--output` renderings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum Format {
    /// Aligned columns for lists, key/value for single objects.
    #[default]
    Table,
    /// Pretty JSON (the raw Graph response where there is one).
    Json,
    /// YAML.
    Yaml,
    /// The response body exactly as received.
    Raw,
}

/// Parse a secret flag straight into a [`SecretString`], so it never sits in a plain
/// `String` that a `Debug` print could show.
#[allow(clippy::unnecessary_wraps)] // clap value parsers return a Result
fn secret(value: &str) -> Result<SecretString, std::convert::Infallible> {
    Ok(SecretString::from(value))
}

/// Flags accepted at any level of the command tree.
#[derive(Debug, Clone, Args)]
#[command(next_help_heading = "Global options")]
pub struct GlobalArgs {
    /// Named credential/context profile.
    #[arg(short = 'p', long, global = true, env = "PALOMA_PROFILE")]
    pub profile: Option<String>,

    /// Access token. Prefer the keyring (`paloma auth login`) or PALOMA_TOKEN: argv is
    /// visible to other processes.
    #[arg(
        long,
        global = true,
        env = "PALOMA_TOKEN",
        hide = true,
        hide_env_values = true,
        value_parser = secret
    )]
    pub token: Option<SecretString>,

    /// Meta app secret (app-level webhook subscriptions, webhook signatures).
    #[arg(
        long,
        global = true,
        env = "PALOMA_APP_SECRET",
        hide = true,
        hide_env_values = true,
        value_parser = secret
    )]
    pub app_secret: Option<SecretString>,

    /// Output format.
    #[arg(
        short = 'o',
        long,
        global = true,
        value_enum,
        default_value_t,
        env = "PALOMA_OUTPUT"
    )]
    pub output: Format,

    /// Override the active sender phone number id.
    #[arg(long, global = true, env = "PALOMA_PHONE_NUMBER_ID")]
    pub phone_number_id: Option<String>,

    /// Override the active WhatsApp Business Account id.
    #[arg(long, global = true, env = "PALOMA_WABA_ID")]
    pub waba_id: Option<String>,

    /// Messaging account to bill for sends; set it if your token reaches several on one number.
    #[arg(long, global = true, env = "PALOMA_MESSAGING_ACCOUNT_ID")]
    pub messaging_account_id: Option<String>,

    /// Meta app id (webhook field subscriptions and upload handles).
    #[arg(long, global = true, env = "PALOMA_APP_ID")]
    pub app_id: Option<String>,

    /// Override the Graph API version, e.g. v26.0.
    #[arg(long, global = true, env = "PALOMA_API_VERSION")]
    pub api_version: Option<String>,

    /// Print the request (method, URL, body) with secrets masked instead of sending it.
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// Print only the essential id (wamid, media id, ...) for scripting.
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Log requests to stderr; repeat (-vv) to include bodies.
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Request timeout, e.g. 30s or 2m.
    #[arg(long, global = true, default_value = "60s", value_parser = humantime::parse_duration)]
    pub timeout: std::time::Duration,

    /// Disable ANSI colors (also honors NO_COLOR).
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Config file path [default: ~/.config/paloma/config.toml].
    #[arg(long, global = true, env = "PALOMA_CONFIG")]
    pub config: Option<PathBuf>,

    /// Skip confirmation prompts for destructive operations.
    #[arg(short, long, global = true)]
    pub yes: bool,

    /// Graph API base URL (for tests and proxies).
    #[arg(long, global = true, env = "PALOMA_GRAPH_URL", hide = true, default_value = paloma_graph::GRAPH_BASE)]
    pub graph_url: String,
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn command_tree_is_valid() {
        Cli::command().debug_assert();
    }
}
