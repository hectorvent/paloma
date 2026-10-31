//! Flows stay drafts until published, and Meta reports Flow JSON errors in the response
//! body, so those are printed and the command exits 5.

mod tools;

use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use paloma_core::{Error, Result, validate};
use paloma_graph::Request;
use paloma_graph::api::flows::{self, DEFAULT_GET_FIELDS, FLOW_JSON_NAME, PREVIEW_FIELD};
use serde_json::{Map, Value, json};

use super::listing::{Pagination, Rows, list};
use super::util::{enum_name, read_file};
use crate::app::{App, field_id};
use crate::output::Output;

const LIST_COLUMNS: &[&str] = &["id", "name", "status", "categories"];

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "SCREAMING_SNAKE_CASE")]
enum Category {
    SignUp,
    SignIn,
    AppointmentBooking,
    LeadGeneration,
    ContactUs,
    CustomerSupport,
    Survey,
    Other,
}

fn categories(cats: &[Category]) -> Vec<Value> {
    cats.iter().map(|c| enum_name(c).into()).collect()
}

#[derive(Debug, Subcommand)]
pub enum FlowCommand {
    /// List flows.
    List(ListArgs),
    /// Show a flow.
    Get(Get),
    /// Create a flow.
    Create(Create),
    /// Update a flow's metadata and/or Flow JSON.
    Update(Update),
    /// Publish a flow (irreversible).
    Publish {
        /// Flow id.
        flow_id: String,
    },
    /// Deprecate a published flow.
    Deprecate {
        /// Flow id.
        flow_id: String,
    },
    /// Delete a draft flow.
    Delete {
        /// Flow id.
        flow_id: String,
    },
    /// Copy Flows from another WABA owned by the same business.
    Migrate(tools::Migrate),
    /// Manage the Flow endpoint public encryption key.
    #[command(subcommand)]
    Key(tools::KeyCommand),
    /// List a flow's assets, or download its Flow JSON with --out.
    Assets(Assets),
}

#[derive(Debug, Args)]
pub struct ListArgs {
    #[command(flatten)]
    page: Pagination,
    /// Comma-separated fields to return.
    #[arg(long, value_name = "FIELD,...")]
    fields: Option<String>,
}

#[derive(Debug, Args)]
pub struct Get {
    /// Flow id.
    flow_id: String,
    /// Include a preview URL.
    #[arg(long)]
    preview: bool,
    /// Comma-separated fields (replaces the defaults).
    #[arg(long, value_name = "FIELD,...")]
    fields: Option<String>,
}

#[derive(Debug, Args)]
pub struct Create {
    #[arg(long)]
    name: String,
    /// Flow category; repeat or comma-separate.
    #[arg(
        long = "category",
        value_enum,
        value_delimiter = ',',
        value_name = "CATEGORY,...",
        required = true,
        ignore_case = true
    )]
    categories: Vec<Category>,
    /// Flow JSON file for the layout.
    #[arg(long, conflicts_with = "clone_from")]
    flow_json: Option<PathBuf>,
    /// Copy the layout of an existing flow.
    #[arg(long)]
    clone_from: Option<String>,
    /// Data-exchange endpoint URL.
    #[arg(long)]
    endpoint_uri: Option<String>,
    /// Publish immediately.
    #[arg(long)]
    publish: bool,
}

#[derive(Debug, Args)]
pub struct Update {
    /// Flow id.
    flow_id: String,
    #[arg(long)]
    name: Option<String>,
    #[arg(
        long = "category",
        value_enum,
        value_delimiter = ',',
        value_name = "CATEGORY,...",
        ignore_case = true
    )]
    categories: Vec<Category>,
    #[arg(long)]
    endpoint_uri: Option<String>,
    /// Upload a new Flow JSON.
    #[arg(long)]
    flow_json: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct Assets {
    /// Flow id.
    flow_id: String,
    /// Download the Flow JSON to this file (or directory, as flow.json).
    #[arg(short = 'O', long)]
    out: Option<PathBuf>,
}

impl FlowCommand {
    #[allow(clippy::too_many_lines)] // one short arm per verb; each arm calls into its module
    pub async fn run(self, app: &App) -> Result<()> {
        match self {
            Self::Migrate(a) => a.run(app).await,
            Self::Key(a) => a.run(app).await,
            Self::List(a) => {
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                let fields = a.fields.clone();
                list(
                    app,
                    &a.page,
                    |after| flows::list(&waba, fields.as_deref(), a.page.limit, after),
                    Rows {
                        columns: LIST_COLUMNS,
                        id: "id",
                    },
                )
                .await
            }
            Self::Get(a) => {
                let mut fields = a.fields.unwrap_or_else(|| DEFAULT_GET_FIELDS.to_owned());
                if a.preview {
                    fields.push(',');
                    fields.push_str(PREVIEW_FIELD);
                }
                app.send_and_emit(&flows::get(&a.flow_id, &fields), field_id)
                    .await
            }
            Self::Create(a) => {
                let mut body = Map::new();
                body.insert("name".into(), a.name.into());
                body.insert("categories".into(), categories(&a.categories).into());
                if let Some(path) = &a.flow_json {
                    body.insert("flow_json".into(), read_flow_json(path)?.into());
                }
                if let Some(id) = a.clone_from {
                    body.insert("clone_flow_id".into(), id.into());
                }
                if let Some(uri) = a.endpoint_uri {
                    body.insert("endpoint_uri".into(), uri.into());
                }
                if a.publish {
                    body.insert("publish".into(), true.into());
                }
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                write(app, &[flows::create(&waba, Value::Object(body))]).await
            }
            Self::Update(a) => {
                let mut meta = Map::new();
                if let Some(n) = a.name {
                    meta.insert("name".into(), n.into());
                }
                if !a.categories.is_empty() {
                    meta.insert("categories".into(), categories(&a.categories).into());
                }
                if let Some(uri) = a.endpoint_uri {
                    meta.insert("endpoint_uri".into(), uri.into());
                }
                if meta.is_empty() && a.flow_json.is_none() {
                    return Err(Error::validation(
                        "nothing to update: pass --name, --category, --endpoint-uri or --flow-json",
                    ));
                }
                let mut reqs = Vec::new();
                if !meta.is_empty() {
                    reqs.push(flows::update(&a.flow_id, meta));
                }
                if let Some(path) = &a.flow_json {
                    read_flow_json(path)?;
                    reqs.push(flows::upload_json(&a.flow_id, path));
                }
                write(app, &reqs).await
            }
            Self::Publish { flow_id } => {
                if !app.confirm_or_abort(&format!("publish flow {flow_id} (cannot be undone)"))? {
                    return Ok(());
                }
                write(app, &[flows::publish(&flow_id)]).await
            }
            Self::Deprecate { flow_id } => {
                if !app.confirm_or_abort(&format!("deprecate flow {flow_id}"))? {
                    return Ok(());
                }
                write(app, &[flows::deprecate(&flow_id)]).await
            }
            Self::Delete { flow_id } => {
                if !app.confirm_or_abort(&format!("delete flow {flow_id}"))? {
                    return Ok(());
                }
                write(app, &[flows::delete(&flow_id)]).await
            }
            Self::Assets(a) => {
                let req = flows::assets(&a.flow_id);
                let Some(out) = a.out else {
                    return app.send_and_emit(&req, crate::app::no_id).await;
                };
                if app.dry_run() {
                    return app.print_dry_run(&[
                        req,
                        Request::get_url("<download_url of the FLOW_JSON asset>"),
                    ]);
                }
                let assets = app.fetch(&req).await?.json()?;
                let url = flows::flow_json_download_url(&assets).ok_or_else(|| {
                    Error::other(format!(
                        "flow {} has no {FLOW_JSON_NAME} asset to download",
                        a.flow_id
                    ))
                })?;
                let target = if out.is_dir() {
                    out.join(FLOW_JSON_NAME)
                } else {
                    out
                };
                let bytes = app
                    .client()?
                    .download(&Request::get_url(url), &target)
                    .await?;
                let path = target.display().to_string();
                app.out.note(format!("wrote {bytes} bytes to {path}"));
                app.out.emit(
                    &Output::new(json!({ "flow_id": a.flow_id, "path": path })).id(Some(path)),
                );
                Ok(())
            }
        }
    }
}

/// Send one or more write requests. Graph accepts a Flow JSON with validation errors
/// (the write happens), so the response still prints, but listed errors exit 5 rather
/// than pretending success.
async fn write(app: &App, reqs: &[Request]) -> Result<()> {
    if app.dry_run() {
        return app.print_dry_run(reqs);
    }
    let mut bodies = Vec::new();
    let mut raw = None;
    for req in reqs {
        let resp = app.fetch(req).await?;
        bodies.push(resp.json()?);
        raw = Some(resp.text);
    }
    let out = match bodies.len() {
        1 => Output {
            raw,
            ..Output::new(bodies.remove(0))
        },
        _ => Output::new(json!({ "metadata": bodies[0], "flow_json": bodies[1] })),
    };
    let id = field_id(&out.data);
    app.out.emit(&out.clone().id(id));

    let errors = flows::validation_errors(&out.data);
    if errors.is_empty() {
        return Ok(());
    }
    let n = errors.len();
    app.out.warn(format!(
        "{n} validation error{} (the flow cannot be published until they are fixed):",
        if n == 1 { "" } else { "s" }
    ));
    for e in &errors {
        app.out.info(format!("  {e}"));
    }
    Err(Error::validation(format!(
        "Flow JSON has {n} validation error{}",
        if n == 1 { "" } else { "s" }
    )))
}

fn read_flow_json(path: &std::path::Path) -> Result<String> {
    validate::readable_file(path)
        .map_err(|_| Error::validation(format!("no such Flow JSON file: {}", path.display())))?;
    let text = read_file(path)?;
    serde_json::from_str::<Value>(&text)
        .map_err(|e| Error::validation(format!("{} is not valid JSON: {e}", path.display())))?;
    Ok(text)
}
