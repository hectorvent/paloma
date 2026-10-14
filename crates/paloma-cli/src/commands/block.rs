use clap::{Args, Subcommand};
use paloma_core::limits::MAX_BLOCKLIST_USERS;
use paloma_core::{Result, validate};
use paloma_graph::api::block;
use paloma_graph::paging;

use crate::app::{App, no_id};
use crate::output::Output;

/// `wa_id` is missing for a user whose phone number Meta can't share.
const LIST_COLUMNS: &[&str] = &["wa_id", "user_id"];

#[derive(Debug, Subcommand)]
pub enum BlockCommand {
    /// Block one or more users.
    Add {
        /// E.164 number or business-scoped user ID (BSUID). Parent BSUIDs aren't accepted.
        #[arg(required = true, value_name = "USER")]
        users: Vec<String>,
    },
    /// Unblock one or more users.
    Remove {
        /// E.164 number or business-scoped user ID (BSUID). Parent BSUIDs aren't accepted.
        #[arg(required = true, value_name = "USER")]
        users: Vec<String>,
    },
    /// List blocked users.
    List(ListArgs),
}

#[derive(Debug, Args)]
pub struct ListArgs {
    /// Page size.
    #[arg(long)]
    limit: Option<u32>,
    /// Follow every page.
    #[arg(long)]
    all: bool,
    /// Start after this cursor.
    #[arg(long)]
    after: Option<String>,
}

impl BlockCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        match &self {
            Self::Add { users } | Self::Remove { users } => {
                users.iter().try_for_each(|u| validate::block_user(u))?;
            }
            Self::List(a) => validate::page_size(a.limit, None)?,
        }
        let phone = app.ctx()?.require_phone_number_id()?.to_owned();
        match self {
            Self::Add { users } => app.send_and_emit(&block::add(&phone, &users), no_id).await,
            Self::Remove { users } => {
                app.send_and_emit(&block::remove(&phone, &users), no_id)
                    .await
            }
            Self::List(a) => list(app, &phone, &a).await,
        }
    }
}

/// `template::list` trusts `paging.next`, which this edge doesn't document: without it a
/// full page and the last page look the same, so `--all` walks to an empty page, and a
/// single page can only say that more may exist.
async fn list(app: &App, phone: &str, a: &ListArgs) -> Result<()> {
    let page = |after: Option<&str>| block::list(phone, a.limit, after);
    if app.dry_run() {
        return app.print_dry_run(&[page(a.after.as_deref())]);
    }
    if a.all {
        // Every page before the empty one holds at least one user.
        let max_pages = MAX_BLOCKLIST_USERS + 1;
        let merged =
            paging::collect_until_empty(app.client()?, a.after.clone(), max_pages, page).await?;
        app.out
            .emit(&Output::new(merged).table("/data", LIST_COLUMNS));
    } else {
        let resp = app.fetch(&page(a.after.as_deref())).await?;
        let out = Output::response(&resp)?.table("/data", LIST_COLUMNS);
        if let Some(next) = paging::next_cursor(&out.data) {
            app.out.note(format!(
                "more results exist: re-run with --all, or page with --after {next}"
            ));
        } else if let Some(next) = paging::cursor_unless_empty(&out.data) {
            app.out.note(format!(
                "more results may exist: re-run with --all, or page with --after {next}"
            ));
        }
        app.out.emit(&out);
    }
    Ok(())
}
