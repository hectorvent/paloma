//! The block list takes phone numbers or business-scoped user IDs, but Meta fails a parent
//! BSUID here. Its pages don't promise `paging.next`, so `--all` walks to an empty page.

use clap::{Args, Subcommand};
use paloma_core::limits::MAX_BLOCKLIST_USERS;
use paloma_core::{Result, validate};
use paloma_graph::api::block;
use paloma_graph::paging;

use super::listing::Pagination;
use crate::app::App;
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
    #[command(flatten)]
    page: Pagination,
}

impl BlockCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        match &self {
            Self::Add { users } | Self::Remove { users } => {
                users.iter().try_for_each(|u| validate::block_user(u))?;
            }
            Self::List(a) => validate::page_size(a.page.limit, None)?,
        }
        let phone = app.phone()?;
        match self {
            Self::Add { users } => {
                if !app.confirm_or_abort(&format!(
                    "block {} user(s), so they can't message or call the number",
                    users.len()
                ))? {
                    return Ok(());
                }
                write(app, &block::add(phone, &users), block::ADDED_USERS).await
            }
            Self::Remove { users } => {
                write(app, &block::remove(phone, &users), block::REMOVED_USERS).await
            }
            Self::List(a) => list(app, phone, &a).await,
        }
    }
}

/// Meta answers 200 even when some users fail, listing them in `failed_users`, so `-q`
/// prints only the users it acted on and a partial failure exits 1.
async fn write(app: &App, req: &paloma_graph::Request, ok: &str) -> Result<()> {
    let Some(out) = app.send_output(req).await? else {
        return Ok(());
    };
    let ids = super::util::acted_on(&out.data, ok, "input");
    let failed = block::failed_count(&out.data);
    app.out.emit(&out.id(ids));
    if failed > 0 {
        app.out
            .warn(format!("{failed} user(s) failed; see failed_users"));
        return Err(paloma_core::Error::Silent(paloma_core::ExitCode::Generic));
    }
    Ok(())
}

/// `listing::list` trusts `paging.next`, which this edge doesn't document: without it a
/// full page and the last page look the same, so `--all` walks to an empty page, and a
/// single page can only say that more may exist.
async fn list(app: &App, phone: &str, a: &ListArgs) -> Result<()> {
    let page = |after: Option<&str>| block::list(phone, a.page.limit, after);
    if app.is_dry_run() {
        return app.print_dry_run(&[page(a.page.after.as_deref())]);
    }
    if a.page.all {
        // Every page before the empty one holds at least one user.
        let max_pages = MAX_BLOCKLIST_USERS + 1;
        let merged =
            paging::collect_until_empty(app.client()?, a.page.after.clone(), max_pages, page)
                .await?;
        let ids = user_ids(&merged);
        app.out
            .emit(&Output::new(merged).table("/data", LIST_COLUMNS).id(ids));
    } else {
        let resp = app.fetch(&page(a.page.after.as_deref())).await?;
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
        let ids = user_ids(&out.data);
        app.out.emit(&out.id(ids));
    }
    Ok(())
}

/// Each blocked user as `block remove` takes them: the phone number, or the BSUID of a user
/// whose number isn't shared.
fn user_ids(data: &serde_json::Value) -> Option<String> {
    let ids: Vec<&str> = data
        .get("data")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| {
            ["wa_id", "user_id"]
                .iter()
                .find_map(|key| row.get(*key).and_then(serde_json::Value::as_str))
                .filter(|id| !id.is_empty())
        })
        .collect();
    (!ids.is_empty()).then(|| ids.join("\n"))
}
