//! Read receipts, typing indicators and delivery history. History follows every page of
//! events and refuses entries for another message, which a lookup can return.

use clap::{Args, Subcommand};
use paloma_core::{Error, Result, validate};
use paloma_graph::api::{message_history, messages};
use paloma_graph::paging;

use crate::app::{App, no_id};
use crate::output::Output;

/// Pointers because each row is an edge that wraps the event in `node`.
const HISTORY_COLUMNS: &[&str] = &[
    "/node/delivery_status",
    "/node/occurrence_timestamp",
    "/node/status_timestamp",
    "/node/error_description",
    "/node/webhook_update_state",
    "/node/webhook_uri",
];

#[derive(Debug, Subcommand)]
pub enum MessageCommand {
    /// Mark an inbound message as read (blue ticks).
    Read(Target),
    /// Mark an inbound message as read and show a typing indicator.
    Typing(Target),
    /// Show a message's delivery status trail.
    History(History),
}

#[derive(Debug, Args)]
pub struct Target {
    /// The inbound message id (wamid…).
    message_id: String,
}

#[derive(Debug, Args)]
pub struct History {
    /// The message id (wamid…).
    message_id: String,
}

impl MessageCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        let (payload, id) = match &self {
            Self::Read(t) => (messages::mark_read(&t.message_id), &t.message_id),
            Self::Typing(t) => (messages::typing(&t.message_id), &t.message_id),
            Self::History(h) => return history(app, &h.message_id).await,
        };
        validate::not_blank(id, "message id")?;
        let phone = app.ctx()?.require_phone_number_id()?.to_owned();
        app.send_and_emit(&messages::send(&phone, payload), no_id)
            .await
    }
}

/// The second call needs an id only the first returns, so a dry run shows a placeholder.
async fn history(app: &App, wamid: &str) -> Result<()> {
    let wamid = wamid.trim();
    validate::not_blank(wamid, "message id")?;
    let phone = app.ctx()?.require_phone_number_id()?.to_owned();
    let lookup = |after: Option<&str>| message_history::lookup(&phone, wamid, after);
    if app.dry_run() {
        return app.print_dry_run(&[lookup(None), message_history::events("<history-id>", None)]);
    }
    let client = app.client()?;
    let page = paging::find_page(client, lookup, |page| {
        message_history::history_id(page, wamid).is_some()
    })
    .await?;
    let id = page
        .and_then(|page| message_history::history_id(&page, wamid))
        .ok_or_else(|| {
            Error::other(format!(
                "Meta returned no message history for {wamid}. This can happen for a message \
                 it delivered, so it doesn't mean the message doesn't exist"
            ))
        })?;
    let trail =
        paging::collect_all(client, None, |after| message_history::events(&id, after)).await?;
    app.out.emit(
        &Output::new(trail)
            .id(Some(id))
            .table("/data", HISTORY_COLUMNS),
    );
    Ok(())
}
