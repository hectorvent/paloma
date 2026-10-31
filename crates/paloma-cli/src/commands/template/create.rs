//! Creating and editing templates: a JSON file, or typed flags that Paloma turns into
//! components with the examples Meta requires for review.

#[allow(clippy::wildcard_imports)] // part of `template`: shares its imports and helpers
use super::*;

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum HeaderMedia {
    Image,
    Video,
    Gif,
    Document,
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("kind").required(true).args(["auth"])))]
pub struct Preview {
    /// Preview authentication templates, the only kind Meta previews.
    #[arg(id = "auth", long = "auth")]
    pub(super) _auth: bool,
    /// Keep only these language codes, comma-separated.
    #[arg(long, value_delimiter = ',', value_name = "CODE,...")]
    pub(super) language: Vec<String>,
    /// Include the security recommendation in the body.
    #[arg(long)]
    pub(super) security_recommendation: bool,
    /// Include the code expiration footer, with this many minutes (1 to 90).
    #[arg(long, value_name = "MINUTES")]
    pub(super) code_expiration: Option<u32>,
}

pub(super) async fn create(app: &App, a: Create) -> Result<()> {
    let body = match (&a.file, &a.name) {
        (Some(file), _) => read_json(file)?,
        (None, Some(name)) => templates::inline_template(&inline_template(name, &a)?),
        (None, None) => unreachable!("clap enforces --file or --name"),
    };
    let waba = app.ctx()?.require_waba_id()?.to_owned();
    app.send_and_emit(&templates::create(&waba, body), field_id)
        .await
}

pub(super) async fn update(app: &App, a: Update) -> Result<()> {
    let body = read_json(&a.file)?;
    let waba = app.ctx()?.require_waba_id()?.to_owned();
    let id = if app.dry_run() {
        "<template-id>".to_owned()
    } else {
        let t =
            paloma_graph::lookup::find_one(app.client()?, &waba, &a.name, a.language.as_deref())
                .await?;
        field_id(&t).ok_or_else(|| Error::other("template has no id"))?
    };
    if !app.confirm_or_abort(&format!(
        "change template '{}' with {}; an approved template goes back to review",
        a.name,
        a.file.display()
    ))? {
        return Ok(());
    }
    app.send_and_emit(&templates::edit(&id, body), |_| Some(id.clone()))
        .await
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("source").required(true).args(["file", "name"])))]
pub struct Create {
    /// Full template definition (JSON).
    #[arg(long, conflicts_with_all = [
        "name", "category", "language", "body", "example", "header_text", "header_example",
        "header_media", "header_handle", "footer", "buttons", "url_examples",
        "parameter_format", "allow_category_change", "ttl",
    ])]
    pub(super) file: Option<PathBuf>,
    /// Template name (inline mode).
    #[arg(long, requires_all = ["category", "language", "body"])]
    pub(super) name: Option<String>,
    /// MARKETING, UTILITY or AUTHENTICATION.
    #[arg(long)]
    pub(super) category: Option<String>,
    /// Language code, e.g. en_US.
    #[arg(long)]
    pub(super) language: Option<String>,
    /// BODY text, with {{1}}-style variables.
    #[arg(long)]
    pub(super) body: Option<String>,
    /// Comma-separated example values for the body variables, in order; write a comma in a
    /// value as `\,`. With --parameter-format named, each is name=value.
    #[arg(long)]
    pub(super) example: Vec<String>,
    /// How variables are written: positional ({{1}}, the default) or named ({{first_name}}).
    #[arg(long, value_enum)]
    pub(super) parameter_format: Option<ParameterFormat>,
    /// Text header (up to 60 characters, at most one variable).
    #[arg(long, conflicts_with = "header_media")]
    pub(super) header_text: Option<String>,
    /// Example value for the text header's variable. With --parameter-format named, name=value.
    #[arg(long, requires = "header_text")]
    pub(super) header_example: Option<String>,
    /// Media header type; needs --header-handle.
    #[arg(long, value_enum, requires = "header_handle")]
    pub(super) header_media: Option<HeaderMedia>,
    /// Upload handle of the sample media, from `paloma media upload --handle`.
    #[arg(long, requires = "header_media")]
    pub(super) header_handle: Option<String>,
    /// Footer text (up to 60 characters).
    #[arg(long)]
    pub(super) footer: Option<String>,
    /// A button, in display order; repeatable. One of quick_reply:TEXT, url:TEXT=URL,
    /// phone_number:TEXT=NUMBER or copy_code:EXAMPLE.
    #[arg(long = "button", value_name = "TYPE:VALUE")]
    pub(super) buttons: Vec<String>,
    /// Example value for a URL button's variable; repeat in the order of those buttons.
    #[arg(long = "url-example", requires = "buttons")]
    pub(super) url_examples: Vec<String>,
    /// Let Meta reassign the template's category. Meta has done this by default since
    /// April 2025.
    #[arg(long)]
    pub(super) allow_category_change: bool,
    /// How long Meta keeps trying to deliver a message from this template, such as 10m or
    /// 12h. The range depends on --category; 30d means Meta's 30-day default.
    #[arg(long, value_name = "DURATION", value_parser = humantime::parse_duration)]
    pub(super) ttl: Option<Duration>,
}

#[derive(Debug, Args)]
pub struct Update {
    /// Template name.
    pub(super) name: String,
    /// Language code (required when the template exists in several).
    #[arg(long)]
    pub(super) language: Option<String>,
    /// JSON body with the fields to change (e.g. components, category).
    #[arg(short = 'f', long)]
    pub(super) file: PathBuf,
}

pub(super) async fn preview(app: &App, a: &Preview) -> Result<()> {
    if let Some(minutes) = a.code_expiration
        && !limits::TEMPLATE_PREVIEW_CODE_EXPIRATION.contains(&minutes)
    {
        return Err(Error::validation(format!(
            "--code-expiration must be {} to {}",
            limits::TEMPLATE_PREVIEW_CODE_EXPIRATION.start(),
            limits::TEMPLATE_PREVIEW_CODE_EXPIRATION.end()
        )));
    }
    for language in &a.language {
        validate::not_blank(language, "--language entry")?;
    }
    let waba = app.ctx()?.require_waba_id()?.to_owned();
    let page = |after: Option<&str>| {
        templates::auth_previews(&waba, a.security_recommendation, a.code_expiration, after)
    };
    if app.dry_run() {
        return app.print_dry_run(&[page(None)]);
    }
    let mut merged = paging::collect_all(app.client()?, None, page).await?;
    if !a.language.is_empty()
        && let Some(previews) = merged.get_mut("data").and_then(Value::as_array_mut)
    {
        previews.retain(|preview| {
            preview
                .get("language")
                .and_then(Value::as_str)
                .is_some_and(|language| a.language.iter().any(|wanted| wanted == language))
        });
        for wanted in &a.language {
            let found = previews
                .iter()
                .any(|preview| preview.get("language").and_then(Value::as_str) == Some(wanted));
            if !found {
                app.out.warn(format!("no preview for language {wanted}"));
            }
        }
    }
    app.out
        .emit(&Output::new(merged).table("/data", PREVIEW_COLUMNS));
    Ok(())
}

pub(super) async fn upsert(app: &App, file: &std::path::Path) -> Result<()> {
    let body = read_json(file)?;
    if body.get("language").is_some() {
        return Err(Error::validation(
            "upsert takes `languages`, an array, instead of `language`",
        ));
    }
    let Some(name) = body.get("name").and_then(Value::as_str) else {
        return Err(Error::validation("the file needs a `name`"));
    };
    validate::template_name(name)?;
    if body.get("category").and_then(Value::as_str) != Some("AUTHENTICATION") {
        return Err(Error::validation(
            "upsert only takes authentication templates: the file needs \
             `\"category\": \"AUTHENTICATION\"`",
        ));
    }
    let Some(languages) = body
        .get("languages")
        .and_then(Value::as_array)
        .filter(|languages| !languages.is_empty())
    else {
        return Err(Error::validation("the file needs a `languages` array"));
    };
    for language in languages {
        let Some(language) = language.as_str() else {
            return Err(Error::validation(
                "`languages` must contain language-code strings",
            ));
        };
        validate::not_blank(language, "a language code in `languages`")?;
    }
    if let Some(property) = upsert_unsupported_property(&body) {
        return Err(Error::validation(format!(
            "upsert doesn't support the `{property}` property"
        )));
    }
    let waba = app.ctx()?.require_waba_id()?.to_owned();
    let count = languages.len();
    if !app.confirm_or_abort(&format!(
        "create or replace authentication template '{name}' in {count} language(s)"
    ))? {
        return Ok(());
    }
    if let Some(resp) = app.send(&templates::upsert(&waba, body)).await? {
        let out = Output::response(&resp)?.table("/data", UPSERT_COLUMNS);
        let id = out.data.pointer("/data/0").and_then(field_id);
        app.out.emit(&out.id(id));
    }
    Ok(())
}

/// The first property, at any depth, that Meta's bulk management page rules out for upsert.
pub(super) fn upsert_unsupported_property(value: &Value) -> Option<&'static str> {
    match value {
        Value::Object(fields) => ["text", "autofill_text"]
            .into_iter()
            .find(|property| fields.contains_key(*property))
            .or_else(|| fields.values().find_map(upsert_unsupported_property)),
        Value::Array(items) => items.iter().find_map(upsert_unsupported_property),
        _ => None,
    }
}

pub(super) fn split_examples(given: &[String]) -> Vec<String> {
    given
        .iter()
        .flat_map(|v| crate::commands::util::split_rows(v))
        .collect()
}

/// Validate the inline flags and turn them into a template, before anything needs an id.
pub(super) fn inline_template(name: &str, a: &Create) -> Result<InlineTemplate> {
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
    let ttl_seconds = a.ttl.map(|ttl| ttl_seconds(category, ttl)).transpose()?;
    if let Some(ttl) = ttl_seconds {
        validate::template_ttl(category, ttl)?;
    }
    Ok(InlineTemplate {
        name: name.to_owned(),
        category: category.to_owned(),
        language: a.language.clone().unwrap_or_default(),
        parameter_format: a.parameter_format.as_ref().map(enum_name),
        header: inline_header(a, format)?,
        body: body.to_owned(),
        body_example: examples(body, &split_examples(&a.example), format, "--example")?,
        footer: a.footer.clone(),
        buttons: parse_buttons(&a.buttons, &a.url_examples)?,
        allow_category_change: a.allow_category_change,
        ttl_seconds,
    })
}

pub(super) fn inline_header(a: &Create, format: ParameterFormat) -> Result<Option<Header>> {
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
                format: enum_name(media),
                handle: handle.clone(),
            }))
        }
        _ => Ok(None),
    }
}

pub(super) fn distinct_variables(text: &str) -> Vec<&str> {
    let mut variables = validate::template_variables(text);
    variables.sort_unstable();
    variables.dedup();
    variables
}

/// Check `given` covers the variables in `text`, as Meta requires an example for each, and
/// shape them for the parameter format.
pub(super) fn examples(
    text: &str,
    given: &[String],
    format: ParameterFormat,
    flag: &str,
) -> Result<Examples> {
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

pub(super) fn named_examples(variables: &[&str], given: &[String], flag: &str) -> Result<Examples> {
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
pub(super) fn parse_buttons(specs: &[String], url_examples: &[String]) -> Result<Vec<Button>> {
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

pub(super) fn parse_button<'a>(
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

/// `message_send_ttl_seconds` for a `--ttl`. Authentication and utility templates spell
/// Meta's 30-day default as -1; marketing takes 30 days in seconds like any other value.
fn ttl_seconds(category: &str, ttl: Duration) -> Result<i64> {
    const THIRTY_DAYS: u64 = 30 * 24 * 60 * 60;
    if ttl.subsec_nanos() != 0 {
        return Err(Error::validation("--ttl must be whole seconds"));
    }
    if ttl.as_secs() == THIRTY_DAYS && !category.eq_ignore_ascii_case("MARKETING") {
        return Ok(limits::TEMPLATE_TTL_30_DAYS);
    }
    i64::try_from(ttl.as_secs()).map_err(|_| Error::validation("--ttl is too long"))
}
