use clap::{Args, Subcommand};
use paloma_core::{Result, validate};
use paloma_graph::api::messages;

use crate::app::{App, no_id};

#[derive(Debug, Subcommand)]
pub enum MessageCommand {
    /// Mark an inbound message as read (blue ticks).
    Read(Target),
    /// Mark an inbound message as read and show a typing indicator.
    Typing(Target),
}

#[derive(Debug, Args)]
pub struct Target {
    /// The inbound message id (wamid…).
    message_id: String,
}

impl MessageCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        let (payload, id) = match &self {
            Self::Read(t) => (messages::mark_read(&t.message_id), &t.message_id),
            Self::Typing(t) => (messages::typing(&t.message_id), &t.message_id),
        };
        validate::not_blank(id, "message id")?;
        let phone = app.ctx()?.require_phone_number_id()?.to_owned();
        app.send_and_emit(&messages::send(&phone, payload), no_id)
            .await
    }
}
