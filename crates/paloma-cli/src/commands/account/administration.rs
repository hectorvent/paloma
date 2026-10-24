//! Assigned-user reads require business context; welcome sequences are WABA-scoped
//! ad assets, with explicit identifiers and confirmation on changes or deletion.
use crate::output::Output;
use crate::{
    app::{App, no_id},
    commands::read_json,
};
use clap::{Args, Subcommand};
use paloma_core::{Error, Result, validate};
use paloma_graph::{api::account, paging};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[derive(Debug, Args)]
pub struct Users {
    #[arg(long, required_unless_present = "system", conflicts_with = "system")]
    business_id: Option<String>,
    /// List WABA system users instead of assigned users.
    #[arg(long)]
    system: bool,
    #[arg(long)]
    all: bool,
    #[arg(long)]
    after: Option<String>,
    #[arg(long)]
    limit: Option<u32>,
}
impl Users {
    pub async fn run(self, app: &App) -> Result<()> {
        if let Some(id) = &self.business_id {
            validate::numeric_id(id, "business id")?;
        }
        validate::page_size(
            self.limit,
            Some(paloma_core::limits::MAX_ASSIGNED_USERS_PAGE),
        )?;
        let waba = app.ctx()?.require_waba_id()?.to_owned();
        let page = |after: Option<&str>| {
            account::users(
                &waba,
                self.system,
                self.business_id.as_deref(),
                self.limit,
                after,
            )
        };
        if app.dry_run() {
            return app.print_dry_run(&[page(self.after.as_deref())]);
        }
        let mut output = if self.all {
            Output::new(paging::collect_all(app.client()?, self.after.clone(), page).await?)
        } else {
            let response = app.fetch(&page(self.after.as_deref())).await?;
            let output = Output::response(&response)?;
            if let Some(cursor) = paging::next_cursor(&output.data) {
                app.out.note(format!(
                    "more results exist: re-run with --all, or page with --after {cursor}"
                ));
            }
            output
        };
        let ids = output
            .data
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|v| v.get("id").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n");
        output.primary_id = (!ids.is_empty()).then_some(ids);
        app.out
            .emit(&output.table("/data", &["id", "name", "tasks"]));
        Ok(())
    }
}
#[derive(Debug, Subcommand)]
pub enum WelcomeSequence {
    /// List sequences, optionally selecting one ID.
    List {
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Create a sequence from its JSON content.
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        file: PathBuf,
    },
    /// Update a sequence's name and/or JSON content.
    Set {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Delete a sequence (Meta refuses sequences used in active ads).
    Delete { id: String },
}
fn content(path: &Path) -> Result<Value> {
    let body = read_json(path)?;
    let object = body
        .as_object()
        .filter(|m| !m.is_empty())
        .ok_or_else(|| Error::validation("welcome sequence must be a nonempty JSON object"))?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "text" | "autofill_message" | "ice_breakers"))
    {
        return Err(Error::validation(
            "sequence accepts text, autofill_message and ice_breakers",
        ));
    }
    if let Some(value) = body.get("text") {
        validate::not_blank(
            value
                .as_str()
                .ok_or_else(|| Error::validation("sequence text must be a string"))?,
            "sequence text",
        )?;
    }
    if let Some(value) = body.get("autofill_message") {
        let text = value
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::validation("autofill_message requires string content"))?;
        validate::not_blank(text, "autofill message")?;
    }
    if let Some(value) = body.get("ice_breakers") {
        let list = value
            .as_array()
            .ok_or_else(|| Error::validation("ice_breakers must be an array"))?;
        for item in list {
            let title = item
                .get("title")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::validation("each ice breaker requires string title"))?;
            validate::not_blank(title, "ice breaker title")?;
        }
    }
    Ok(body)
}
/// Meta documents a bare array here; `data` is accepted too, since other edges wrap lists.
async fn list_sequences(app: &App, req: &paloma_graph::Request) -> Result<()> {
    let Some(response) = app.send(req).await? else {
        return Ok(());
    };
    let mut output = Output::response(&response)?;
    let rows = if output.data.is_array() { "" } else { "/data" };
    let ids = output
        .data
        .pointer(rows)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| v.get("sequence_id").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    output.primary_id = (!ids.is_empty()).then_some(ids);
    app.out
        .emit(&output.table(rows, &["sequence_id", "name", "is_used_in_ad"]));
    Ok(())
}

impl WelcomeSequence {
    pub async fn run(self, app: &App) -> Result<()> {
        let (read, delete, id, body, limit) = match self {
            Self::List { id, limit } => {
                if let Some(id) = &id {
                    validate::numeric_id(id, "sequence id")?;
                }
                validate::page_size(limit, None)?;
                (true, false, id, Value::Null, limit)
            }
            Self::Delete { id } => {
                validate::numeric_id(&id, "sequence id")?;
                (false, true, Some(id), Value::Null, None)
            }
            Self::Create { name, file } => {
                validate::not_blank(&name, "sequence name")?;
                let sequence = content(&file)?;
                (
                    false,
                    false,
                    None,
                    json!({"name":name,"welcome_message_sequence":sequence}),
                    None,
                )
            }
            Self::Set { id, name, file } => {
                validate::numeric_id(&id, "sequence id")?;
                if name.is_none() && file.is_none() {
                    return Err(Error::validation("pass --name or --file"));
                }
                let mut body = json!({});
                if let Some(name) = name {
                    validate::not_blank(&name, "sequence name")?;
                    body["name"] = json!(name);
                }
                if let Some(file) = file {
                    body["welcome_message_sequence"] = content(&file)?;
                }
                (false, false, Some(id), body, None)
            }
        };
        let waba = app.ctx()?.require_waba_id()?.to_owned();
        if !read
            && id.is_some()
            && !app.confirm_or_abort(
                "change or delete a welcome sequence used by click-to-WhatsApp ads",
            )?
        {
            return Ok(());
        }
        let req = account::welcome_sequence(&waba, read, delete, id.as_deref(), body, limit);
        if read {
            return list_sequences(app, &req).await;
        }
        app.send_and_emit(&req, |v| {
            v.get("sequence_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| no_id(v))
        })
        .await
    }
}
