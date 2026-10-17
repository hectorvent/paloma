use std::path::PathBuf;
use std::time::{Duration, Instant};

use clap::{ArgGroup, Args, Subcommand, ValueEnum};
use paloma_core::{Error, Result, limits, validate};
use paloma_graph::api::templates::{self, Button, Examples, Header, InlineTemplate, ListFilter};
use paloma_graph::paging;
use serde_json::{Value, json};

use super::read_json;
use crate::app::{App, field_id, no_id};
use crate::output::Output;

const LIST_COLUMNS: &[&str] = &["name", "language", "category", "status", "id"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ParameterFormat {
    Named,
    Positional,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "SCREAMING_SNAKE_CASE")]
enum HeaderMedia {
    Image,
    Video,
    Gif,
    Document,
}

fn enum_str(v: &impl ValueEnum) -> String {
    v.to_possible_value()
        .expect("no skipped variants")
        .get_name()
        .to_owned()
}

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
    /// Delete a template (all languages, one with --language or --id), or several with --ids.
    Delete(Delete),
    /// Show the WABA's template namespace.
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
    /// Filter by name. Meta may also return other names that contain it.
    #[arg(long)]
    name: Option<String>,
    /// Comma-separated fields to return, e.g. name,quality_score. They become the table's
    /// columns.
    #[arg(long, value_delimiter = ',', value_name = "FIELD,...")]
    fields: Vec<String>,
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
    #[arg(long, conflicts_with_all = [
        "name", "category", "language", "body", "example", "header_text", "header_example",
        "header_media", "header_handle", "footer", "buttons", "url_examples",
        "parameter_format", "allow_category_change", "ttl_seconds",
    ])]
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
    /// Comma-separated example values for the body variables, in order; write a comma in a
    /// value as `\,`. With --parameter-format named, each is name=value.
    #[arg(long)]
    example: Vec<String>,
    /// How variables are written: positional ({{1}}, the default) or named ({{first_name}}).
    #[arg(long, value_enum)]
    parameter_format: Option<ParameterFormat>,
    /// Text header (up to 60 characters, at most one variable).
    #[arg(long, conflicts_with = "header_media")]
    header_text: Option<String>,
    /// Example value for the text header's variable. With --parameter-format named, name=value.
    #[arg(long, requires = "header_text")]
    header_example: Option<String>,
    /// Media header type; needs --header-handle.
    #[arg(long, value_enum, requires = "header_handle")]
    header_media: Option<HeaderMedia>,
    /// Upload handle of the sample media, from `paloma media upload --handle`.
    #[arg(long, requires = "header_media")]
    header_handle: Option<String>,
    /// Footer text (up to 60 characters).
    #[arg(long)]
    footer: Option<String>,
    /// A button, in display order; repeatable. One of quick_reply:TEXT, url:TEXT=URL,
    /// phone_number:TEXT=NUMBER or copy_code:EXAMPLE.
    #[arg(long = "button", value_name = "TYPE:VALUE")]
    buttons: Vec<String>,
    /// Example value for a URL button's variable; repeat in the order of those buttons.
    #[arg(long = "url-example", requires = "buttons")]
    url_examples: Vec<String>,
    /// Let Meta reassign the template's category. Meta has done this by default since
    /// April 2025.
    #[arg(long)]
    allow_category_change: bool,
    /// Time-to-live for messages using this template (message_send_ttl_seconds). The range
    /// depends on --category.
    #[arg(long, allow_negative_numbers = true, value_name = "SECONDS")]
    ttl_seconds: Option<i64>,
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("target").required(true).args(["name", "ids"])))]
pub struct Delete {
    /// Template name. On its own, deletes every language of it.
    name: Option<String>,
    /// Delete only this language.
    #[arg(long, requires = "name", conflicts_with = "id")]
    language: Option<String>,
    /// Delete only the template with this id. Meta requires the name as well.
    #[arg(long, requires = "name")]
    id: Option<String>,
    /// Delete these templates by id, comma-separated (up to 100), instead of by name. If any
    /// id is invalid, Meta deletes none of them.
    #[arg(long, value_delimiter = ',', value_name = "ID,...", conflicts_with_all = ["id", "language"])]
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
                validate::page_size(a.limit, None)?;
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
                    limit: a.limit,
                };
                list(
                    app,
                    a.all,
                    a.after,
                    |after| templates::list(&waba, &filter, after),
                    columns,
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
                get(app, &name, language.as_deref()).await
            }
            Self::Create(a) => {
                let body = match (&a.file, &a.name) {
                    (Some(file), _) => read_json(file)?,
                    (None, Some(name)) => templates::inline_template(&inline_template(name, &a)?),
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
            Self::Delete(a) => delete(app, a).await,
            Self::Namespace => {
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                app.send_and_emit(&templates::namespace(&waba), |v| {
                    v.get("message_template_namespace")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .await
            }
        }
    }
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
    let missing = || Error::other(format!("no template named '{name}' on this WABA"));
    let page = templates::find_page(app.client()?, &waba, name, None)
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
async fn get(app: &App, name: &str, language: Option<&str>) -> Result<()> {
    let waba = app.ctx()?.require_waba_id()?.to_owned();
    if app.dry_run() {
        return app.print_dry_run(&[templates::lookup(&waba, name, language)]);
    }
    let not_found = || {
        let scope = language.map(|l| format!(" ({l})")).unwrap_or_default();
        Error::other(format!("no template named '{name}'{scope} on this WABA"))
    };
    let page = templates::find_page(app.client()?, &waba, name, language)
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
    app.out
        .emit(&Output::new(json!({ "data": matches })).id(id));
    Ok(())
}

/// Delete by bulk ids, by one id, by one language or by name, naming the target in the
/// confirmation prompt.
async fn delete(app: &App, a: Delete) -> Result<()> {
    if let Some(name) = &a.name {
        validate::not_blank(name, "template name")?;
    }
    if let Some(id) = &a.id {
        validate::template_id(id)?;
    }
    validate::template_ids(&a.ids)?;
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
        return app
            .send_and_emit(&templates::delete_many(&waba, &a.ids), no_id)
            .await;
    };

    let scope = match (&a.id, &a.language) {
        (Some(id), _) => format!("id {id}"),
        (None, Some(language)) => language.clone(),
        (None, None) => "all languages".to_owned(),
    };
    if !app.confirm_or_abort(&format!("delete template '{name}' ({scope})"))? {
        return Ok(());
    }
    let hsm_id = match (&a.id, &a.language, app.dry_run()) {
        (Some(id), _, _) => Some(id.clone()),
        (None, None, _) => None,
        (None, Some(_), true) => Some("<template-id>".to_owned()),
        (None, Some(l), false) => {
            // Without an id this would delete every language, not the one confirmed.
            let found = templates::find_one(app.client()?, &waba, &name, Some(l)).await?;
            Some(field_id(&found).ok_or_else(|| Error::other("the template has no id"))?)
        }
    };
    let primary = a.id.unwrap_or_else(|| name.clone());
    app.send_and_emit(&templates::delete(&waba, &name, hsm_id.as_deref()), |_| {
        Some(primary.clone())
    })
    .await
}

fn split_examples(given: &[String]) -> Vec<String> {
    given
        .iter()
        .flat_map(|v| super::send::split_rows(v))
        .collect()
}

/// Validate the inline flags and turn them into a template, before anything needs an id.
fn inline_template(name: &str, a: &Create) -> Result<InlineTemplate> {
    let category = a.category.as_deref().unwrap_or_default();
    let body = a.body.as_deref().unwrap_or_default();
    let format = a.parameter_format.unwrap_or(ParameterFormat::Positional);

    validate::template_name(name)?;
    validate::template_text(body, limits::MAX_TEMPLATE_BODY, "--body")?;
    if category.eq_ignore_ascii_case("AUTHENTICATION")
        && (a.header_text.is_some()
            || a.header_media.is_some()
            || a.footer.is_some()
            || !a.buttons.is_empty())
    {
        return Err(Error::validation(
            "authentication templates have their own footer and one-time password button; \
             create them with --file",
        ));
    }
    if let Some(footer) = &a.footer {
        validate::template_text(footer, limits::MAX_TEMPLATE_FOOTER, "--footer")?;
    }
    if let Some(ttl) = a.ttl_seconds {
        validate::template_ttl(category, ttl)?;
    }
    Ok(InlineTemplate {
        name: name.to_owned(),
        category: category.to_owned(),
        language: a.language.clone().unwrap_or_default(),
        parameter_format: a.parameter_format.as_ref().map(enum_str),
        header: inline_header(a, format)?,
        body: body.to_owned(),
        body_example: examples(body, &split_examples(&a.example), format, "--example")?,
        footer: a.footer.clone(),
        buttons: parse_buttons(&a.buttons, &a.url_examples)?,
        allow_category_change: a.allow_category_change,
        ttl_seconds: a.ttl_seconds,
    })
}

fn inline_header(a: &Create, format: ParameterFormat) -> Result<Option<Header>> {
    if let Some(text) = &a.header_text {
        validate::template_text(text, limits::MAX_TEMPLATE_HEADER_TEXT, "--header-text")?;
        if distinct_variables(text).len() > limits::MAX_TEMPLATE_HEADER_PARAMS {
            return Err(Error::validation(format!(
                "--header-text can have at most {} variable",
                limits::MAX_TEMPLATE_HEADER_PARAMS
            )));
        }
        let given: Vec<String> = a.header_example.iter().cloned().collect();
        let example = examples(text, &given, format, "--header-example")?;
        return Ok(Some(Header::Text {
            text: text.clone(),
            example,
        }));
    }
    match (&a.header_media, &a.header_handle) {
        (Some(media), Some(handle)) => {
            validate::not_blank(handle, "--header-handle")?;
            Ok(Some(Header::Media {
                format: enum_str(media),
                handle: handle.clone(),
            }))
        }
        _ => Ok(None),
    }
}

fn distinct_variables(text: &str) -> Vec<&str> {
    let mut variables = validate::template_variables(text);
    variables.sort_unstable();
    variables.dedup();
    variables
}

/// Check `given` covers the variables in `text`, as Meta requires an example for each, and
/// shape them for the parameter format.
fn examples(text: &str, given: &[String], format: ParameterFormat, flag: &str) -> Result<Examples> {
    let variables = distinct_variables(text);
    if format == ParameterFormat::Named {
        return named_examples(&variables, given, flag);
    }
    let mut numbers = Vec::with_capacity(variables.len());
    for variable in &variables {
        let Ok(number) = variable.parse::<usize>() else {
            return Err(Error::validation(format!(
                "variable {{{{{variable}}}}} isn't positional; pass --parameter-format named"
            )));
        };
        numbers.push(number);
    }
    numbers.sort_unstable();
    if numbers.iter().copied().ne(1..=numbers.len()) {
        return Err(Error::validation(
            "positional variables must be numbered from {{1}} without gaps",
        ));
    }
    if given.len() != numbers.len() {
        return Err(Error::validation(format!(
            "{flag} has {} value(s) for {} variable(s)",
            given.len(),
            numbers.len()
        )));
    }
    for value in given {
        validate::not_blank(value, flag)?;
    }
    Ok(Examples::Positional(given.to_vec()))
}

fn named_examples(variables: &[&str], given: &[String], flag: &str) -> Result<Examples> {
    let mut pairs = Vec::with_capacity(given.len());
    for example in given {
        let Some((name, value)) = example.split_once('=') else {
            return Err(Error::validation(format!(
                "{flag} must use name=value with --parameter-format named"
            )));
        };
        validate::template_parameter_name(name)?;
        validate::not_blank(value, flag)?;
        if !variables.contains(&name) {
            return Err(Error::validation(format!(
                "{flag} names '{name}', which isn't a variable in the text"
            )));
        }
        pairs.push((name.to_owned(), value.to_owned()));
    }
    for variable in variables {
        validate::template_parameter_name(variable)?;
        if !pairs.iter().any(|(name, _)| name == variable) {
            return Err(Error::validation(format!(
                "{flag} has no example for {{{{{variable}}}}}"
            )));
        }
    }
    Ok(Examples::Named(pairs))
}

/// Parse `--button` values in display order and apply Meta's per-type and grouping limits.
fn parse_buttons(specs: &[String], url_examples: &[String]) -> Result<Vec<Button>> {
    if specs.len() > usize::from(limits::MAX_TEMPLATE_BUTTONS) {
        return Err(Error::validation(format!(
            "{} buttons given; max is {}",
            specs.len(),
            limits::MAX_TEMPLATE_BUTTONS
        )));
    }
    let mut url_examples = url_examples.iter();
    let mut buttons = Vec::with_capacity(specs.len());
    for (position, spec) in (1..).zip(specs) {
        let button = parse_button(spec, &mut url_examples)
            .map_err(|problem| Error::validation(format!("--button #{position} {problem}")))?;
        buttons.push(button);
    }
    if url_examples.next().is_some() {
        return Err(Error::validation(
            "more --url-example values than URL buttons with a variable",
        ));
    }

    let count = |matches: fn(&Button) -> bool| buttons.iter().filter(|b| matches(b)).count();
    for (found, max, kind) in [
        (
            count(|b| matches!(b, Button::Url { .. })),
            limits::MAX_TEMPLATE_URL_BUTTONS,
            "URL",
        ),
        (
            count(|b| matches!(b, Button::PhoneNumber { .. })),
            limits::MAX_TEMPLATE_PHONE_BUTTONS,
            "phone number",
        ),
        (
            count(|b| matches!(b, Button::CopyCode { .. })),
            limits::MAX_TEMPLATE_COPY_CODE_BUTTONS,
            "copy code",
        ),
    ] {
        if found > max {
            return Err(Error::validation(format!(
                "{found} {kind} buttons given; a template can have {max}"
            )));
        }
    }

    let is_quick_reply = |b: &Button| matches!(b, Button::QuickReply { .. });
    let groups = buttons
        .chunk_by(|a, b| is_quick_reply(a) == is_quick_reply(b))
        .filter(|group| is_quick_reply(&group[0]))
        .count();
    if groups > 1 {
        return Err(Error::validation(
            "quick_reply buttons must sit together, before or after the other buttons",
        ));
    }
    Ok(buttons)
}

fn parse_button<'a>(
    spec: &str,
    url_examples: &mut impl Iterator<Item = &'a String>,
) -> std::result::Result<Button, String> {
    const USAGE: &str =
        "must be quick_reply:TEXT, url:TEXT=URL, phone_number:TEXT=NUMBER or copy_code:EXAMPLE";
    let (kind, value) = spec.split_once(':').ok_or(USAGE)?;
    let label = |text: &str| -> std::result::Result<String, String> {
        validate::template_text(text, limits::MAX_TEMPLATE_BUTTON_TEXT, "label")
            .map_err(|e| e.to_string())?;
        Ok(text.to_owned())
    };
    match kind {
        "quick_reply" => Ok(Button::QuickReply {
            text: label(value)?,
        }),
        "copy_code" => {
            validate::template_text(value, limits::MAX_COUPON_CODE, "example")
                .map_err(|e| e.to_string())?;
            Ok(Button::CopyCode {
                example: value.to_owned(),
            })
        }
        "url" => {
            let (text, url) = value.split_once('=').ok_or(USAGE)?;
            validate::template_text(url, limits::MAX_TEMPLATE_BUTTON_URL, "URL")
                .map_err(|e| e.to_string())?;
            let variables = validate::template_variables(url);
            if variables.len() > 1 || (variables.len() == 1 && !url.ends_with("}}")) {
                return Err("URL can have one variable, at its end".to_owned());
            }
            let example = if variables.is_empty() {
                None
            } else {
                let example = url_examples
                    .next()
                    .ok_or("URL has a variable; pass its value with --url-example")?;
                validate::not_blank(example, "--url-example").map_err(|e| e.to_string())?;
                Some(example.clone())
            };
            Ok(Button::Url {
                text: label(text)?,
                url: url.to_owned(),
                example,
            })
        }
        "phone_number" => {
            let (text, phone_number) = value.split_once('=').ok_or(USAGE)?;
            validate::template_text(phone_number, limits::MAX_TEMPLATE_BUTTON_PHONE, "number")
                .map_err(|e| e.to_string())?;
            Ok(Button::PhoneNumber {
                text: label(text)?,
                phone_number: phone_number.to_owned(),
            })
        }
        _ => Err(USAGE.to_owned()),
    }
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
