//! One module per top-level resource. Commands stay thin: parse, validate locally,
//! build a request with `paloma_graph::api`, hand it to [`App`], emit the result.
//!
//! To add a command: add an `Args` struct and a variant to the group's enum, and match
//! it in the group's `run`.

use clap::Subcommand;
use paloma_core::Result;

use crate::app::App;

mod account;
mod auth;
mod automation;
mod block;
mod calling;
mod commerce;
mod config;
mod flow;
mod group;
mod link;
mod listing;
mod manifest;
mod media;
mod message;
mod number;
mod profile;
mod qr;
mod send;
mod template;
mod upload;
mod util;
mod version;
mod webhook;

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Send a message (text, template, media, interactive, ...).
    #[command(subcommand)]
    Send(send::SendCommand),
    /// Act on messages (read receipts, typing indicator, delivery history).
    #[command(subcommand)]
    Message(message::MessageCommand),
    /// Upload, inspect, download and delete media.
    #[command(subcommand)]
    Media(media::MediaCommand),
    /// Manage message templates.
    #[command(subcommand)]
    Template(template::TemplateCommand),
    /// Manage business phone numbers.
    #[command(subcommand)]
    Number(number::NumberCommand),
    /// Read and update the business profile.
    #[command(subcommand)]
    Profile(profile::ProfileCommand),
    /// Read and update shopping cart and catalog settings.
    #[command(subcommand)]
    Commerce(commerce::CommerceCommand),
    /// Configure welcome messages, ice breakers and bot commands.
    #[command(subcommand)]
    Automation(automation::AutomationCommand),
    /// Manage the block list.
    #[command(subcommand)]
    Block(block::BlockCommand),
    /// Create and manage WhatsApp groups.
    #[command(subcommand)]
    Group(group::GroupCommand),
    /// Manage webhook callbacks and field subscriptions.
    #[command(subcommand)]
    Webhook(webhook::WebhookCommand),
    /// Configure voice calling.
    #[command(subcommand)]
    Calling(calling::CallingCommand),
    /// Manage WhatsApp Flows.
    #[command(subcommand)]
    Flow(flow::FlowCommand),
    /// Manage WABA settings, users and billing, and read its analytics.
    #[command(subcommand)]
    Account(account::AccountCommand),
    /// Manage message QR codes.
    #[command(subcommand)]
    Qr(qr::QrCommand),
    /// Build wa.me click-to-chat links (offline).
    #[command(subcommand)]
    Link(link::LinkCommand),
    /// Log in, inspect and log out of a profile.
    #[command(subcommand)]
    Auth(auth::AuthCommand),
    /// Read and edit the config file.
    #[command(subcommand)]
    Config(config::ConfigCommand),
    /// Interactive first-run setup.
    Configure(config::Configure),
    /// Export live configuration as YAML manifests.
    Export(manifest::Export),
    /// Apply YAML manifests (idempotent; only drift is written).
    Apply(manifest::Apply),
    /// Show drift between manifests and live configuration (exit 1 on drift).
    Diff(manifest::Diff),
    /// Generate shell completions.
    Completion(version::Completion),
    /// Print version information.
    Version,
}

impl Command {
    pub async fn run(self, app: &App) -> Result<()> {
        match self {
            Self::Send(c) => c.run(app).await,
            Self::Message(c) => c.run(app).await,
            Self::Media(c) => c.run(app).await,
            Self::Template(c) => c.run(app).await,
            Self::Number(c) => c.run(app).await,
            Self::Profile(c) => c.run(app).await,
            Self::Commerce(c) => c.run(app).await,
            Self::Block(c) => c.run(app).await,
            Self::Group(c) => c.run(app).await,
            Self::Automation(c) => c.run(app).await,
            Self::Webhook(c) => c.run(app).await,
            Self::Calling(c) => c.run(app).await,
            Self::Flow(c) => c.run(app).await,
            Self::Account(c) => c.run(app).await,
            Self::Qr(c) => c.run(app).await,
            Self::Link(c) => c.run(app),
            Self::Auth(c) => c.run(app).await,
            Self::Config(c) => c.run(app),
            Self::Configure(_) => config::Configure::run(app),
            Self::Export(c) => c.run(app).await,
            Self::Apply(c) => c.run(app).await,
            Self::Diff(c) => c.run(app).await,
            Self::Completion(c) => {
                c.run(app);
                Ok(())
            }
            Self::Version => {
                version::version(app);
                Ok(())
            }
        }
    }
}
