use std::path::PathBuf;
use std::time::{Duration, Instant};

use clap::{ArgGroup, Args, Subcommand};
use paloma_core::{Error, Result, validate};
use paloma_graph::api::templates::{self, ListFilter};
use paloma_graph::paging;
use serde_json::{Value, json};

use super::{planned, read_json};
use crate::app::{App, field_id};
use crate::output::Output;

const LIST_COLUMNS: &[&str] = &["name", "language", "category", "status", "id"];

#[derive(Debug, Subcommand)]
pub enum TemplateCommand {
    /// List templates.
    List(List),
    /// Show a template's review status.
    Status {
        /// Template name.
        name: String,
    },
    /// Show a template's full definition.
    Get {
        /// Template name.
        name: String,
        /// Language code, when the template exists in several.
        #[arg(long)]
        language: Option<String>,
    },
    /// Create a template from a JSON file or inline flags.
    #[command(visible_alias = "submit")]
    Create(Create),
    /// Wait until a template is approved (exit 0) or rejected (exit 1).
    Watch(Watch),
    /// Edit an existing template from a JSON file.
    Edit(Edit),
    /// Delete a template (all languages, or one with --language).
    Delete {
        /// Template name.
        name: String,
        /// Delete only this language.
        #[arg(long)]
        language: Option<String>,
    },
    /// Show the WABA's template namespace (planned v1.0).
    Namespace,
}

#[derive(Debug, Args)]
pub struct List {
    /// Filter by status (APPROVED, PENDING, REJECTED, ...).
    #[arg(long)]
    status: Option<String>,
    /// Filter by category (MARKETING, UTILITY, AUTHENTICATION).
    #[arg(long)]
    category: Option<String>,
    /// Filter by language code.
    #[arg(long)]
    language: Option<String>,
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

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("source").required(true).args(["file", "name"])))]
pub struct Create {
    /// Full template definition (JSON).
    #[arg(long, conflicts_with_all = ["name", "category", "language", "body", "example"])]
    file: Option<PathBuf>,
    /// Template name (inline mode).
    #[arg(long, requires_all = ["category", "language", "body"])]
    name: Option<String>,
    /// MARKETING, UTILITY or AUTHENTICATION.
    #[arg(long)]
    category: Option<String>,
    /// Language code, e.g. en_US.
    #[arg(long)]
    language: Option<String>,
    /// BODY text, with {{1}}-style variables.
    #[arg(long)]
    body: Option<String>,
    /// Comma-separated example values for the body variables.
    #[arg(long, value_delimiter = ',')]
    example: Vec<String>,
}

#[derive(Debug, Args)]
pub struct Watch {
    /// Template name.
    name: String,
    /// Time between polls.
    #[arg(long, default_value = "10s", value_parser = humantime::parse_duration)]
    interval: Duration,
    /// Give up after this long.
    #[arg(long, default_value = "10m", value_parser = humantime::parse_duration)]
    max_wait: Duration,
}

#[derive(Debug, Args)]
pub struct Edit {
    /// Template name.
    name: String,
    /// Language code (required when the template exists in several).
    #[arg(long)]
    language: Option<String>,
    /// JSON body with the fields to change (e.g. components, category).
    #[arg(long)]
    file: PathBuf,
}

impl TemplateCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        match self {
            Self::List(a) => {
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                let filter = ListFilter {
                    status: a.status,
                    category: a.category,
                    language: a.language,
                    limit: a.limit,
                    ..ListFilter::default()
                };
                list(
                    app,
                    a.all,
                    a.after,
                    |after| templates::list(&waba, &filter, after),
                    LIST_COLUMNS,
                )
                .await
            }
            Self::Status { name } => {
                validate::not_blank(&name, "template name")?;
                let view = status(app, &name).await?;
                let st = view
                    .get("status")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                app.out.emit(&Output::new(view).id(st));
                Ok(())
            }
            Self::Get { name, language } => {
                validate::not_blank(&name, "template name")?;
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                app.send_and_emit(&templates::lookup(&waba, &name, language.as_deref()), |v| {
                    v.pointer("/data/0/id")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .await
            }
            Self::Create(a) => {
                let body = match (&a.file, &a.name) {
                    (Some(file), _) => read_json(file)?,
                    (None, Some(name)) => templates::inline_body(
                        name,
                        a.category.as_deref().unwrap_or_default(),
                        a.language.as_deref().unwrap_or_default(),
                        a.body.as_deref().unwrap_or_default(),
                        &a.example,
                    ),
                    (None, None) => unreachable!("clap enforces --file or --name"),
                };
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                app.send_and_emit(&templates::create(&waba, body), field_id)
                    .await
            }
            Self::Watch(a) => watch(app, &a).await,
            Self::Edit(a) => {
                let body = read_json(&a.file)?;
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                let id = if app.dry_run() {
                    "<template-id>".to_owned()
                } else {
                    let t =
                        templates::find_one(app.client()?, &waba, &a.name, a.language.as_deref())
                            .await?;
                    field_id(&t).ok_or_else(|| Error::other("template has no id"))?
                };
                app.send_and_emit(&templates::edit(&id, body), |_| Some(id.clone()))
                    .await
            }
            Self::Delete { name, language } => {
                validate::not_blank(&name, "template name")?;
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                let scope = language.as_deref().unwrap_or("all languages");
                if !app.confirm_or_abort(&format!("delete template '{name}' ({scope})"))? {
                    return Ok(());
                }
                let hsm_id = match (&language, app.dry_run()) {
                    (None, _) => None,
                    (Some(_), true) => Some("<template-id>".to_owned()),
                    (Some(l), false) => {
                        field_id(&templates::find_one(app.client()?, &waba, &name, Some(l)).await?)
                    }
                };
                app.send_and_emit(&templates::delete(&waba, &name, hsm_id.as_deref()), |_| {
                    Some(name.clone())
                })
                .await
            }
            Self::Namespace => planned("template namespace", "v1.0"),
        }
    }
}

/// A listing: one page (warning when more exist) or every page with `--all`.
pub async fn list(
    app: &App,
    all: bool,
    after: Option<String>,
    page: impl Fn(Option<&str>) -> paloma_graph::Request,
    columns: &'static [&'static str],
) -> Result<()> {
    if app.dry_run() {
        return app.print_dry_run(&[page(after.as_deref())]);
    }
    if all {
        let merged = paging::collect_all(app.client()?, after, page).await?;
        app.out.emit(&Output::new(merged).table("/data", columns));
    } else {
        let resp = app.fetch(&page(after.as_deref())).await?;
        let out = Output::response(&resp)?.table("/data", columns);
        if let Some(next) = paging::next_cursor(&out.data) {
            app.out.note(format!(
                "more results exist: re-run with --all, or page with --after {next}"
            ));
        }
        app.out.emit(&out);
    }
    Ok(())
}

/// The review status of the first template named exactly `name`.
async fn status(app: &App, name: &str) -> Result<Value> {
    let waba = app.ctx()?.require_waba_id()?.to_owned();
    let resp = app
        .fetch(&templates::lookup(&waba, name, None))
        .await?
        .json()?;
    let t = templates::exact_matches(&resp, name, None)
        .into_iter()
        .next()
        .ok_or_else(|| Error::other(format!("no template named '{name}' on this WABA")))?;
    Ok(json!({
        "name": t.get("name"),
        "language": t.get("language"),
        "category": t.get("category"),
        "status": t.get("status"),
        "rejected_reason": t.get("rejected_reason"),
    }))
}

async fn watch(app: &App, a: &Watch) -> Result<()> {
    validate::not_blank(&a.name, "template name")?;
    let deadline = Instant::now() + a.max_wait;
    let spinner = progress(&a.name);
    loop {
        let view = status(app, &a.name).await?;
        let st = view
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("UNKNOWN")
            .to_owned();
        let done = |msg: Option<String>| -> Result<()> {
            spinner.finish_and_clear();
            app.out
                .emit(&Output::new(view.clone()).id(Some(st.clone())));
            msg.map_or(Ok(()), |m| Err(Error::other(m)))
        };
        match st.as_str() {
            "APPROVED" => return done(None),
            "REJECTED" => return done(Some(format!("template '{}' was REJECTED", a.name))),
            _ if Instant::now() >= deadline => {
                return done(Some(format!(
                    "timed out after {}; last status: {st}",
                    humantime::format_duration(a.max_wait)
                )));
            }
            _ => {
                if spinner.is_hidden() {
                    app.out.note(format!(
                        "status {st}; waiting {}...",
                        humantime::format_duration(a.interval)
                    ));
                } else {
                    spinner.set_message(format!("{}: {st}", a.name));
                }
                tokio::time::sleep(a.interval).await;
            }
        }
    }
}

/// A stderr spinner when attached to a terminal; hidden (and replaced by plain notes)
/// otherwise, so CI logs stay readable.
fn progress(name: &str) -> indicatif::ProgressBar {
    let bar =
        indicatif::ProgressBar::with_draw_target(None, indicatif::ProgressDrawTarget::stderr());
    bar.set_style(
        indicatif::ProgressStyle::with_template("{spinner} {msg} ({elapsed})")
            .expect("valid template"),
    );
    bar.set_message(format!("{name}: waiting for review"));
    bar.enable_steady_tick(Duration::from_millis(120));
    bar
}
