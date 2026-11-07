//! Groups need an Official Business Account and report much of their outcome by webhook, so
//! create returns a request id rather than a group, and writes that act on several people can
//! partly fail inside a 2xx response. Those failures must not read as success.

use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use paloma_core::{Error, ExitCode, Result, limits, validate};
use paloma_graph::api::{groups, messages};
use paloma_graph::paging;
use serde_json::{Value, json};

use super::listing::{Pagination, Rows};
use crate::app::{App, field_id};
use crate::output::Output;

const LIST_COLUMNS: &[&str] = &["id", "subject", "created_at"];
const REQUEST_COLUMNS: &[&str] = &["join_request_id", "wa_id", "creation_timestamp"];

#[derive(Debug, Subcommand)]
pub enum GroupCommand {
    /// Create a group. Its id and invite link arrive by webhook.
    Create(Create),
    /// List the active number's groups.
    List(List),
    /// Show a group's settings and participants.
    Get {
        /// Group id.
        group_id: String,
        /// Comma-separated fields, instead of the default set.
        #[arg(long, value_name = "FIELD,...")]
        fields: Option<String>,
    },
    /// Change a group's subject, description or photo.
    Update(Update),
    /// Delete a group and remove everyone from it, including the business.
    Delete {
        /// Group id.
        group_id: String,
    },
    /// Show or reset a group's invite link.
    #[command(subcommand)]
    Invite(Invite),
    /// Remove participants. Joining only happens through the invite link.
    #[command(subcommand)]
    Members(Members),
    /// List, approve or reject requests to join.
    #[command(subcommand)]
    Requests(Requests),
    /// Pin or unpin a message in a group.
    #[command(subcommand)]
    Pin(Pin),
}

#[derive(Debug, Subcommand)]
pub enum Pin {
    /// Pin a message. Meta unpins the oldest beyond three.
    Create {
        /// Group id.
        group_id: String,
        /// The message to pin (wamid…).
        message_id: String,
        /// How long to keep it pinned, from 1 to 30 days.
        #[arg(long)]
        days: u32,
    },
    /// Unpin a message.
    Delete {
        /// Group id.
        group_id: String,
        /// The message to unpin (wamid…).
        message_id: String,
    },
}

impl Pin {
    async fn run(self, app: &App) -> Result<()> {
        match self {
            Self::Create {
                group_id,
                message_id,
                days,
            } => {
                validate::group_id(&group_id)?;
                validate::not_blank(&message_id, "message id")?;
                validate::group_pin_days(days)?;
                pin(app, &group_id, &message_id, Some(days)).await
            }
            Self::Delete {
                group_id,
                message_id,
            } => {
                validate::group_id(&group_id)?;
                validate::not_blank(&message_id, "message id")?;
                if !app.confirm_or_abort(&format!("unpin {message_id} in group {group_id}"))? {
                    return Ok(());
                }
                pin(app, &group_id, &message_id, None).await
            }
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum JoinApproval {
    /// People must be approved through a join request.
    ApprovalRequired,
    /// People who open the invite link join straight away.
    AutoApprove,
}

impl JoinApproval {
    const fn api(self) -> &'static str {
        match self {
            Self::ApprovalRequired => "approval_required",
            Self::AutoApprove => "auto_approve",
        }
    }
}

#[derive(Debug, Args)]
pub struct Create {
    /// Group subject.
    subject: String,
    /// Group description.
    #[arg(long)]
    description: Option<String>,
    /// Whether joining needs approval. Meta defaults to auto-approve.
    #[arg(long, value_enum)]
    join_approval: Option<JoinApproval>,
}

#[derive(Debug, Args)]
pub struct List {
    #[command(flatten)]
    page: Pagination,
}

#[derive(Debug, Args)]
pub struct Update {
    /// Group id.
    group_id: String,
    /// New group subject.
    #[arg(long)]
    subject: Option<String>,
    /// New group description.
    #[arg(long)]
    description: Option<String>,
    /// A square JPEG of at most 5 MB and at least 192 x 192.
    #[arg(long)]
    photo: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum Invite {
    /// Show the current invite link.
    Get {
        /// Group id.
        group_id: String,
    },
    /// Replace the invite link. Every earlier link stops working.
    Reset {
        /// Group id.
        group_id: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum Members {
    /// Remove participants, who can no longer rejoin through an invite link.
    Remove {
        /// Group id.
        group_id: String,
        /// E.164 number or WhatsApp user ID, up to 8.
        #[arg(required = true, value_name = "USER")]
        users: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum Requests {
    /// List pending join requests.
    List {
        /// Group id.
        group_id: String,
        #[command(flatten)]
        page: Pagination,
    },
    /// Approve join requests.
    Approve {
        /// Group id.
        group_id: String,
        /// Join request id; repeat it for several.
        #[arg(required = true, value_name = "REQUEST_ID")]
        request_ids: Vec<String>,
    },
    /// Reject join requests. The person can ask again.
    Reject {
        /// Group id.
        group_id: String,
        /// Join request id; repeat it for several.
        #[arg(required = true, value_name = "REQUEST_ID")]
        request_ids: Vec<String>,
    },
}

fn check_subject(subject: &str) -> Result<()> {
    validate::not_blank(subject, "group subject")?;
    validate::max_chars(subject.trim(), limits::MAX_GROUP_SUBJECT, "group subject")
}

fn check_description(description: Option<&str>) -> Result<()> {
    description.map_or(Ok(()), |d| {
        validate::max_chars(d, limits::MAX_GROUP_DESCRIPTION, "group description")
    })
}

fn check_photo(path: &std::path::Path) -> Result<()> {
    let path = validate::readable_file(path)?;
    let mime = paloma_graph::api::media::guess_mime(&path);
    if mime != "image/jpeg" {
        return Err(Error::validation("--photo must be a JPEG file"));
    }
    if validate::file_size(&path)? > limits::MAX_GROUP_PHOTO_BYTES {
        return Err(Error::validation("--photo must be at most 5 MB"));
    }
    Ok(())
}

fn check_request_ids(ids: &[String]) -> Result<()> {
    ids.iter()
        .try_for_each(|id| validate::not_blank(id, "join request id"))
}

/// Report what failed inside a 2xx response, after printing it, and exit 1.
fn report_partial(app: &App, data: &Value, failed_key: &str) -> Result<()> {
    let is_populated = |key: &str| {
        data.get(key)
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty())
    };
    if is_populated(failed_key) || is_populated("errors") {
        app.out.warn(format!(
            "Meta couldn't complete every item; see {failed_key} and errors in the response"
        ));
        return Err(Error::Silent(ExitCode::Generic));
    }
    Ok(())
}

/// `-q` prints the ids in `requested` that Meta didn't list under `failed_key`.
async fn send_partial(
    app: &App,
    req: &paloma_graph::Request,
    failed_key: &str,
    requested: &[String],
) -> Result<()> {
    let Some(output) = app.send_output(req).await? else {
        return Ok(());
    };
    let failed = output
        .data
        .get(failed_key)
        .map(Value::to_string)
        .unwrap_or_default();
    let done: Vec<&str> = requested
        .iter()
        .map(String::as_str)
        .filter(|id| !failed.contains(&format!("\"{id}\"")))
        .collect();
    let output = output.id((!done.is_empty()).then(|| done.join("\n")));
    app.out.emit(&output);
    report_partial(app, &output.data, failed_key)
}

async fn list(app: &App, phone: &str, a: List) -> Result<()> {
    let page = |after: Option<&str>| groups::list(phone, a.page.limit, after);
    if app.is_dry_run() {
        return app.print_dry_run(&[page(a.page.after.as_deref())]);
    }
    let mut output = if a.page.all {
        let mut found = Vec::new();
        let mut after = a.page.after.clone();
        for _ in 0..paging::MAX_PAGES {
            let current = app.fetch(&page(after.as_deref())).await?.json()?;
            found.extend(groups::page_groups(&current));
            after = paging::next_cursor(&current);
            if after.is_none() {
                break;
            }
        }
        if after.is_some() {
            return Err(Error::other(format!(
                "stopped after {} pages with more groups left; use a larger --limit",
                paging::MAX_PAGES
            )));
        }
        Output::new(json!({ "data": { "groups": found } }))
    } else {
        let response = app.fetch(&page(a.page.after.as_deref())).await?;
        let output = Output::response(&response)?;
        if let Some(cursor) = paging::next_cursor(&output.data) {
            app.out.note(format!(
                "more results exist: re-run with --all, or page with --after {cursor}"
            ));
        }
        output
    };
    let ids: Vec<String> = groups::page_groups(&output.data)
        .iter()
        .filter_map(|g| g.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect();
    output.primary_id = (!ids.is_empty()).then(|| ids.join("\n"));
    // With no groups, a live read returned an empty `data` array rather than `data.groups`.
    let rows = if output.data.get("data").is_some_and(Value::is_array) {
        "/data"
    } else {
        "/data/groups"
    };
    app.out.emit(&output.table(rows, LIST_COLUMNS));
    Ok(())
}

async fn pin(app: &App, group: &str, message: &str, days: Option<u32>) -> Result<()> {
    let phone = app.phone()?.to_owned();
    let ctx = app.ctx()?;
    let payload = messages::with_messaging_account(
        groups::pin_payload(group, message, days),
        ctx.messaging_account_id(),
    );
    app.send_and_emit(&messages::send(&phone, payload), messages::message_id)
        .await
}

impl GroupCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        match self {
            Self::Create(a) => a.run(app).await,
            Self::List(a) => {
                validate::page_size(a.page.limit, Some(limits::MAX_GROUPS_PAGE))?;
                let phone = app.phone()?;
                list(app, phone, a).await
            }
            Self::Get { group_id, fields } => {
                validate::group_id(&group_id)?;
                let fields = fields.as_deref().unwrap_or(groups::DETAIL_FIELDS);
                app.send_and_emit(&groups::get(&group_id, fields), field_id)
                    .await
            }
            Self::Update(a) => a.run(app).await,
            Self::Delete { group_id } => {
                validate::group_id(&group_id)?;
                if !app.confirm_or_abort(&format!(
                    "delete group {group_id} and remove all its participants"
                ))? {
                    return Ok(());
                }
                app.send_and_emit(&groups::delete(&group_id), crate::app::fixed_id(&group_id))
                    .await
            }
            Self::Invite(command) => command.run(app).await,
            Self::Members(command) => command.run(app).await,
            Self::Requests(command) => command.run(app).await,
            Self::Pin(command) => command.run(app).await,
        }
    }
}

impl Create {
    pub async fn run(self, app: &App) -> Result<()> {
        check_subject(&self.subject)?;
        check_description(self.description.as_deref())?;
        let phone = app.phone()?;
        let req = groups::create(
            phone,
            &self.subject,
            self.description.as_deref(),
            self.join_approval.map(JoinApproval::api),
        );
        let Some(output) = app.send_output(&req).await? else {
            return Ok(());
        };
        let id = output
            .data
            .get("request_id")
            .and_then(Value::as_str)
            .map(str::to_owned);
        app.out
            .note("the group id and invite link arrive in a group_lifecycle_update webhook");
        app.out.emit(&output.id(id));
        Ok(())
    }
}

impl Update {
    pub async fn run(self, app: &App) -> Result<()> {
        validate::group_id(&self.group_id)?;
        if self.subject.is_none() && self.description.is_none() && self.photo.is_none() {
            return Err(Error::validation(
                "pass --subject, --description or --photo",
            ));
        }
        if let Some(subject) = &self.subject {
            check_subject(subject)?;
        }
        check_description(self.description.as_deref())?;
        if let Some(photo) = &self.photo {
            check_photo(photo)?;
        }
        let req = match &self.photo {
            Some(photo) => groups::update_with_photo(
                &self.group_id,
                self.subject.as_deref(),
                self.description.as_deref(),
                photo,
            ),
            None => groups::update(
                &self.group_id,
                self.subject.as_deref(),
                self.description.as_deref(),
            ),
        };
        app.send_and_emit(&req, crate::app::fixed_id(&self.group_id))
            .await
    }
}

impl Invite {
    pub async fn run(self, app: &App) -> Result<()> {
        let link = |v: &Value| {
            v.get("invite_link")
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        match self {
            Invite::Get { group_id } => {
                validate::group_id(&group_id)?;
                app.send_and_emit(&groups::invite_link(&group_id), link)
                    .await
            }
            Invite::Reset { group_id } => {
                validate::group_id(&group_id)?;
                if !app.confirm_or_abort(&format!(
                    "reset the invite link of group {group_id}; earlier links stop working"
                ))? {
                    return Ok(());
                }
                app.send_and_emit(&groups::reset_invite_link(&group_id), link)
                    .await
            }
        }
    }
}

impl Requests {
    pub async fn run(self, app: &App) -> Result<()> {
        match self {
            Requests::List { group_id, page } => {
                validate::group_id(&group_id)?;
                page.check(None)?;
                super::listing::list(
                    app,
                    &page,
                    |after| groups::join_requests(&group_id, page.limit, after),
                    Rows {
                        columns: REQUEST_COLUMNS,
                        id: "join_request_id",
                    },
                )
                .await
            }
            Requests::Approve {
                group_id,
                request_ids,
            } => {
                validate::group_id(&group_id)?;
                check_request_ids(&request_ids)?;
                send_partial(
                    app,
                    &groups::approve_join_requests(&group_id, &request_ids),
                    "failed_join_requests",
                    &request_ids,
                )
                .await
            }
            Requests::Reject {
                group_id,
                request_ids,
            } => {
                validate::group_id(&group_id)?;
                check_request_ids(&request_ids)?;
                if !app.confirm_or_abort(&format!(
                    "reject {} join request(s) to group {group_id}",
                    request_ids.len()
                ))? {
                    return Ok(());
                }
                send_partial(
                    app,
                    &groups::reject_join_requests(&group_id, &request_ids),
                    "failed_join_requests",
                    &request_ids,
                )
                .await
            }
        }
    }
}

impl Members {
    pub async fn run(self, app: &App) -> Result<()> {
        let Self::Remove { group_id, users } = self;
        validate::group_id(&group_id)?;
        if users.len() > limits::MAX_GROUP_PARTICIPANTS_PER_REQUEST {
            return Err(Error::validation(format!(
                "at most {} participants per request",
                limits::MAX_GROUP_PARTICIPANTS_PER_REQUEST
            )));
        }
        users.iter().try_for_each(|u| validate::recipient(u))?;
        if !app.confirm_or_abort(&format!(
            "remove {} participant(s) from group {group_id}; they can't rejoin by invite link",
            users.len()
        ))? {
            return Ok(());
        }
        send_partial(
            app,
            &groups::remove_participants(&group_id, &users),
            "failed_participants",
            &users,
        )
        .await
    }
}
