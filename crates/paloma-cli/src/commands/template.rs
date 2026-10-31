//! Template lifecycle operations validate locally before resolving an account.

use std::path::PathBuf;

use super::util::{enum_name, read_json};
use crate::app::{App, field_id};
use crate::cli::Format;
use crate::output::Output;
use clap::{ArgGroup, Args, Subcommand, ValueEnum};
use paloma_core::{Error, ExitCode, Result, limits, validate};
use paloma_graph::api::templates::{
    self, AnalyticsQuery, Button, Examples, GroupUpdate, Header, InlineTemplate, LibraryFilter,
    LibraryTemplate, ListFilter, Migration,
};
use paloma_graph::paging;
use serde_json::{Value, json};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
mod analytics;
mod create;
mod group;
mod library;
use super::listing::{Pagination, Rows, list};
use analytics::{Analytics, Compare, analytics, compare};
use create::{Create, Preview, Update, preview, upsert};
use group::{GroupCommand, group};
use library::{LibraryCommand, library};

const LIST_COLUMNS: &[&str] = &["name", "language", "category", "status", "id"];

const QUALITY_COLUMNS: &[&str] = &[
    "name",
    "language",
    "status",
    "/quality_score/score",
    "/health_status/can_send_message",
    "id",
];
const LIBRARY_COLUMNS: &[&str] = &["name", "language", "category", "topic", "usecase", "id"];
const COMPARE_COLUMNS: &[&str] = &[
    "metric",
    "order_by_relative_metric",
    "number_values",
    "string_values",
];
const PREVIEW_COLUMNS: &[&str] = &["language", "body", "footer", "/buttons/0/text"];
const UPSERT_COLUMNS: &[&str] = &["id", "language", "status"];
const ANALYTICS_COLUMNS: &[&str] = &["template_id", "start", "end", "sent", "delivered", "read"];
const GROUP_ANALYTICS_COLUMNS: &[&str] = &[
    "template_group_id",
    "start",
    "end",
    "sent",
    "delivered",
    "read",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ParameterFormat {
    Named,
    Positional,
}

#[derive(Debug, Subcommand)]
pub enum TemplateCommand {
    /// List templates.
    List(List),
    /// Show a template's full definition, or only its review status or quality.
    Get {
        /// Template name.
        name: String,
        /// Language code, when the template exists in several.
        #[arg(long, conflicts_with = "status")]
        language: Option<String>,
        /// Show only the status, quality score and health status.
        #[arg(long, conflicts_with = "status")]
        quality: bool,
        /// Show only the review status of the first template with this name; `-q` prints it.
        #[arg(long)]
        status: bool,
    },
    /// Create a template from a JSON file or inline flags.
    #[command(visible_alias = "submit")]
    Create(Create),
    /// Wait until a template is approved (exit 0) or rejected (exit 1).
    Watch(Watch),
    /// Change an existing template from a JSON file.
    Update(Update),
    /// Delete a template by name (every language, or one with --language), or by id.
    Delete(Delete),
    /// Show the WABA's template namespace.
    Namespace,
    /// Archive templates by id. Meta deletes them after 28 days unless they are unarchived.
    Archive(Ids),
    /// Restore archived templates by id, within 28 days of archiving them.
    Unarchive(Ids),
    /// Unpause a template that was paused during template pacing.
    Unpause {
        /// Template id.
        id: String,
    },
    /// Browse Meta's Template Library and create templates from it.
    Library {
        #[command(subcommand)]
        command: LibraryCommand,
    },
    /// Compare two templates' sends and block rates.
    Compare(Compare),
    /// Recreate templates from another WABA of the same business on this one.
    Migrate(Migrate),
    /// Preview authentication template text in each language.
    Preview(Preview),
    /// Create or update an authentication template in several languages from a JSON file.
    Upsert {
        /// The template definition, with `languages` instead of `language`.
        #[arg(long)]
        file: PathBuf,
    },
    /// Manage template groups, which track a set of templates' metrics together.
    Group {
        #[command(subcommand)]
        command: GroupCommand,
    },
    /// Show daily metrics for templates or template groups. Needs template analytics
    /// enabled on the WABA: `paloma account set --insights`.
    Analytics(Analytics),
}

#[derive(Debug, Args)]
pub struct Ids {
    /// Template id; repeat it or separate ids with commas (up to 100).
    #[arg(
        long = "id",
        required = true,
        value_delimiter = ',',
        value_name = "ID,..."
    )]
    ids: Vec<String>,
}

#[derive(Debug, Args)]
pub struct Migrate {
    /// The WABA to copy templates from. The active WABA receives them.
    #[arg(long)]
    source_waba_id: String,
    /// Migrate only this template id; repeat it or separate ids with commas (up to 500).
    #[arg(long = "id", value_delimiter = ',', value_name = "ID,...")]
    ids: Vec<String>,
    /// Which set of 500 templates to migrate, from 0.
    #[arg(long)]
    page: Option<u32>,
    /// Batch size, up to 500. Lower it if the request times out.
    #[arg(long)]
    count: Option<u32>,
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
    /// Filter by name. Meta may also return other names that contain it.
    #[arg(long)]
    name: Option<String>,
    /// Comma-separated fields to return, e.g. name,quality_score. They become the table's
    /// columns.
    #[arg(long, value_delimiter = ',', value_name = "FIELD,...")]
    fields: Vec<String>,
    #[command(flatten)]
    page: Pagination,
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("target").required(true).multiple(true).args(["name", "ids"])))]
pub struct Delete {
    /// Template name. On its own, deletes every language of it.
    name: Option<String>,
    /// Delete only this language of the name.
    #[arg(long, requires = "name", conflicts_with = "ids")]
    language: Option<String>,
    /// Delete by template id; repeat it or separate ids with commas (up to 100). With a
    /// name, one id deletes only that template. If any id is invalid, Meta deletes none.
    #[arg(long = "id", value_delimiter = ',', value_name = "ID,...")]
    ids: Vec<String>,
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

impl TemplateCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        match self {
            Self::List(a) => {
                validate::page_size(a.page.limit, None)?;
                if let Some(name) = &a.name {
                    validate::not_blank(name, "--name")?;
                }
                let fields: Vec<String> = a.fields.iter().map(|f| f.trim().to_owned()).collect();
                for field in &fields {
                    validate::not_blank(field, "--fields entry")?;
                }
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                let columns = list_columns(&fields);
                let filter = ListFilter {
                    name: a.name,
                    status: a.status,
                    category: a.category,
                    language: a.language,
                    fields: (!fields.is_empty()).then(|| fields.join(",")),
                    limit: a.page.limit,
                };
                list(
                    app,
                    &a.page,
                    |after| templates::list(&waba, &filter, after),
                    Rows { columns, id: "id" },
                )
                .await
            }
            Self::Get {
                name, status: true, ..
            } => {
                validate::not_blank(&name, "template name")?;
                let view = status(app, &name).await?;
                let st = view
                    .get("status")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                app.out.emit(&Output::new(view).id(st));
                Ok(())
            }
            Self::Get {
                name,
                language,
                quality,
                status: false,
            } => {
                validate::not_blank(&name, "template name")?;
                get(app, &name, language.as_deref(), quality).await
            }
            Self::Create(a) => create::create(app, a).await,
            Self::Watch(a) => watch(app, &a).await,
            Self::Update(a) => create::update(app, a).await,
            Self::Delete(a) => delete(app, a).await,
            Self::Namespace => {
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                app.send_and_emit(&templates::namespace(&waba), |data| {
                    data.get("message_template_namespace")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .await
            }
            Self::Archive(a) => archive(app, &a).await,
            Self::Unarchive(a) => {
                validate::template_ids_within(&a.ids, limits::MAX_TEMPLATE_ARCHIVE_IDS)?;
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                send_bulk(app, &templates::unarchive(&waba, &a.ids), &a.ids).await
            }
            Self::Unpause { id } => {
                validate::template_id(&id)?;
                app.send_and_emit(&templates::unpause(&id), |_| Some(id.clone()))
                    .await
            }
            Self::Library { command } => library(app, command).await,
            Self::Compare(a) => compare(app, &a).await,
            Self::Migrate(a) => migrate(app, &a).await,
            Self::Preview(a) => preview(app, &a).await,
            Self::Upsert { file } => upsert(app, &file).await,
            Self::Group { command } => group(app, command).await,
            Self::Analytics(a) => analytics(app, &a).await,
        }
    }
}

async fn archive(app: &App, a: &Ids) -> Result<()> {
    validate::template_ids_within(&a.ids, limits::MAX_TEMPLATE_ARCHIVE_IDS)?;
    let waba = app.ctx()?.require_waba_id()?.to_owned();
    let what = format!(
        "archive {} template(s) by id ({}), which Meta deletes after 28 days unless unarchived",
        a.ids.len(),
        a.ids.join(", ")
    );
    if !app.confirm_or_abort(&what)? {
        return Ok(());
    }
    send_bulk(app, &templates::archive(&waba, &a.ids), &a.ids).await
}

/// Send a bulk write that answers 200 even when some templates fail. Those are still a
/// failure for a script, so the response is printed and the exit code says so.
async fn send_bulk(app: &App, req: &paloma_graph::Request, ids: &[String]) -> Result<()> {
    let Some(resp) = app.send(req).await? else {
        return Ok(());
    };
    let out = Output::response(&resp)?.id((!ids.is_empty()).then(|| ids.join("\n")));
    let failed = templates::failed_count(&out.data);
    app.out.emit(&out);
    if failed > 0 {
        app.out
            .warn(format!("{failed} template(s) failed; see failed_templates"));
        return Err(Error::Silent(ExitCode::Generic));
    }
    Ok(())
}

async fn migrate(app: &App, a: &Migrate) -> Result<()> {
    validate::numeric_id(&a.source_waba_id, "--source-waba-id")?;
    validate::template_ids_within(&a.ids, limits::MAX_TEMPLATE_MIGRATE_IDS)?;
    if a.count
        .is_some_and(|c| c > limits::MAX_TEMPLATE_MIGRATE_COUNT)
    {
        return Err(Error::validation(format!(
            "--count max is {}",
            limits::MAX_TEMPLATE_MIGRATE_COUNT
        )));
    }
    let waba = app.ctx()?.require_waba_id()?.to_owned();
    let what = format!(
        "migrate templates from WABA {} to WABA {waba}",
        a.source_waba_id
    );
    if !app.confirm_or_abort(&what)? {
        return Ok(());
    }
    let migration = Migration {
        source_waba_id: &a.source_waba_id,
        page_number: a.page,
        count: a.count,
        template_ids: &a.ids,
    };
    send_bulk(app, &templates::migrate(&waba, &migration), &a.ids).await
}

/// Template ids where Meta types them as integers.
fn integer_ids(ids: &[String]) -> Result<Vec<u64>> {
    ids.iter()
        .map(|id| {
            validate::template_id(id)?;
            id.parse()
                .map_err(|_| Error::validation(format!("template id {id} is too long")))
        })
        .collect()
}

/// The table columns for a listing: what `--fields` asked for, or the defaults. A process
/// runs one command, so leaking this one short list is what makes it `'static` for
/// [`list`], whose signature other commands share.
fn list_columns(fields: &[String]) -> &'static [&'static str] {
    if fields.is_empty() {
        return LIST_COLUMNS;
    }
    fields
        .iter()
        .map(|field| &*field.clone().leak())
        .collect::<Vec<&'static str>>()
        .leak()
}

/// The review status of the first template named exactly `name`.
async fn status(app: &App, name: &str) -> Result<Value> {
    let waba = app.ctx()?.require_waba_id()?.to_owned();
    let missing = || Error::other(format!("no template named '{name}' on this WABA"));
    let page = paloma_graph::lookup::find_page(app.client()?, &waba, name, None)
        .await?
        .ok_or_else(missing)?;
    let matches = templates::exact_matches(&page, name, None);
    let t = matches.first().ok_or_else(missing)?;
    Ok(json!({
        "name": t.get("name"),
        "language": t.get("language"),
        "category": t.get("category"),
        "status": t.get("status"),
        "rejected_reason": t.get("rejected_reason"),
    }))
}

/// Show the exact matches from the first page that has any. Later pages aren't fetched, so
/// without a language other translations of the name may be there; say so on stderr.
async fn get(app: &App, name: &str, language: Option<&str>, quality: bool) -> Result<()> {
    let waba = app.ctx()?.require_waba_id()?.to_owned();
    let fields = quality.then_some(templates::QUALITY_FIELDS);
    let lookup =
        |after: Option<&str>| templates::lookup_fields_page(&waba, name, language, fields, after);
    if app.dry_run() {
        return app.print_dry_run(&[lookup(None)]);
    }
    let not_found = || {
        let scope = language.map(|l| format!(" ({l})")).unwrap_or_default();
        Error::other(format!("no template named '{name}'{scope} on this WABA"))
    };
    let page = paging::find_page(app.client()?, lookup, |page| {
        !templates::exact_matches(page, name, language).is_empty()
    })
    .await?
    .ok_or_else(not_found)?;
    if language.is_none() && paging::next_cursor(&page).is_some() {
        app.out.note(
            "more results exist: other languages of this name may be on later pages; \
             pass --language to pick one",
        );
    }
    let matches = templates::exact_matches(&page, name, language);
    let id = matches.first().and_then(|t| field_id(t));
    let out = Output::new(json!({ "data": matches })).id(id);
    app.out.emit(&if quality {
        out.table("/data", QUALITY_COLUMNS)
    } else {
        out
    });
    Ok(())
}

/// Delete by bulk ids, by one id, by one language or by name, naming the target in the
/// confirmation prompt.
async fn delete(app: &App, a: Delete) -> Result<()> {
    if let Some(name) = &a.name {
        validate::not_blank(name, "template name")?;
    }
    validate::template_ids(&a.ids)?;
    let id = match (&a.name, a.ids.as_slice()) {
        (Some(_), [_, _, ..]) => {
            return Err(Error::validation(
                "with a name, pass one --id; without one, --id deletes every id given",
            ));
        }
        (Some(_), [id]) => Some(id.clone()),
        _ => None,
    };
    let waba = app.ctx()?.require_waba_id()?.to_owned();

    let Some(name) = a.name else {
        let what = format!(
            "delete {} template(s) by id ({})",
            a.ids.len(),
            a.ids.join(", ")
        );
        if !app.confirm_or_abort(&what)? {
            return Ok(());
        }
        let ids = a.ids.join("\n");
        return app
            .send_and_emit(&templates::delete_many(&waba, &a.ids), |_| {
                Some(ids.clone())
            })
            .await;
    };

    let scope = match (&id, &a.language) {
        (Some(id), _) => format!("id {id}"),
        (None, Some(language)) => language.clone(),
        (None, None) => "all languages".to_owned(),
    };
    if !app.confirm_or_abort(&format!("delete template '{name}' ({scope})"))? {
        return Ok(());
    }
    let hsm_id = match (&id, &a.language, app.dry_run()) {
        (Some(id), _, _) => Some(id.clone()),
        (None, None, _) => None,
        (None, Some(_), true) => Some("<template-id>".to_owned()),
        (None, Some(l), false) => {
            // Without an id this would delete every language, not the one confirmed.
            let found =
                paloma_graph::lookup::find_one(app.client()?, &waba, &name, Some(l)).await?;
            Some(field_id(&found).ok_or_else(|| Error::other("the template has no id"))?)
        }
    };
    let primary = id.unwrap_or_else(|| name.clone());
    app.send_and_emit(&templates::delete(&waba, &name, hsm_id.as_deref()), |_| {
        Some(primary.clone())
    })
    .await
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
