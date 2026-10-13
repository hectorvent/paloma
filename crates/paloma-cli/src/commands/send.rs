use std::io::Read;
use std::path::PathBuf;

use clap::{ArgGroup, Args, Subcommand};
use paloma_core::compliance::{NoTracking, SendPolicy, enforce_session_window};
use paloma_core::limits::{self, MAX_LIST_ROWS, MAX_REPLY_BUTTONS};
use paloma_core::{Error, Result, validate};
use paloma_graph::api::media as media_api;
use paloma_graph::api::messages::{
    self, Button, InteractiveText, MediaKind, MediaOptions, MediaSource, Row, Section,
};
use paloma_graph::request::has_http_scheme;
use serde_json::Value;

use super::{planned, read_file, read_json};
use crate::app::App;
use crate::output::Output;

#[derive(Debug, Subcommand)]
pub enum SendCommand {
    /// Send a text message.
    Text(Text),
    /// Send an approved template.
    Template(Template),
    /// Send an image.
    Image(Media),
    /// Send a document.
    Document(Media),
    /// Send an audio clip.
    Audio(Audio),
    /// Send a video.
    Video(Media),
    /// Send a sticker.
    Sticker(Media),
    /// Send a location pin.
    Location(Location),
    /// Send contact cards from a JSON file.
    Contact(Contact),
    /// Send an interactive message (buttons, list, call-to-action URL).
    #[command(subcommand)]
    Interactive(Interactive),
    /// React to a message with an emoji.
    Reaction(Reaction),
    /// Send an arbitrary message object as-is.
    Raw(Raw),
    /// Send a Flow into a chat (planned v1.0).
    Flow,
}

/// The flag on every send type that Meta lets quote an earlier message.
#[derive(Debug, Args)]
pub struct Reply {
    /// Quote-reply to this message id (wamid…).
    #[arg(long)]
    reply_to: Option<String>,
}

impl Reply {
    /// A blank id is a mistake, such as an unset shell variable, and would otherwise send
    /// a message that doesn't quote anything.
    fn id(&self) -> Result<Option<&str>> {
        if let Some(id) = &self.reply_to {
            validate::not_blank(id, "--reply-to")?;
        }
        Ok(self.reply_to.as_deref())
    }
}

/// The 24-hour window guardrail flags.
#[derive(Debug, Clone, Copy, Args)]
pub struct Policy {
    /// Send even if the 24h customer-service window may be closed.
    #[arg(long)]
    force: bool,
    /// Refuse free-form sends unless the window is known to be open.
    #[arg(long, conflicts_with = "force")]
    strict: bool,
}

impl Policy {
    fn enforce(self, to: &str) -> Result<()> {
        enforce_session_window(
            &NoTracking,
            to,
            false,
            SendPolicy {
                force: self.force,
                strict: self.strict,
            },
        )
    }
}

#[derive(Debug, Args)]
pub struct Text {
    /// Recipient (E.164 number).
    to: String,
    /// Message body.
    #[arg(required_unless_present = "body_file", conflicts_with = "body_file")]
    body: Option<String>,
    /// Read the body from a file.
    #[arg(long)]
    body_file: Option<PathBuf>,
    /// Render a link preview for the first URL in the body.
    #[arg(long)]
    preview_url: bool,
    #[command(flatten)]
    reply: Reply,
    #[command(flatten)]
    policy: Policy,
}

#[derive(Debug, Args)]
pub struct Template {
    /// Recipient.
    to: String,
    /// Template name.
    name: String,
    /// Template language code, e.g. en_US.
    #[arg(long)]
    lang: String,
    /// Body variable values, in order ({{1}}, {{2}}, …); repeatable.
    #[arg(long = "body-var", value_name = "VALUE", allow_hyphen_values = true)]
    body_vars: Vec<String>,
    /// Named body text parameter; repeat for each name.
    #[arg(long = "var", value_name = "NAME=VALUE", conflicts_with = "body_vars")]
    named_vars: Vec<String>,
    /// Text value for a positional header parameter.
    #[arg(
        long,
        value_name = "TEXT",
        conflicts_with_all = ["named_vars", "header_media"],
        allow_hyphen_values = true
    )]
    header_text: Option<String>,
    /// Image for the header: a media id or an http(s) link.
    #[arg(long, value_name = "ID|URL", group = "header_media")]
    header_image: Option<String>,
    /// Video for the header: a media id or an http(s) link.
    #[arg(long, value_name = "ID|URL", group = "header_media")]
    header_video: Option<String>,
    /// Document for the header: a media id or an http(s) link.
    #[arg(long, value_name = "ID|URL", group = "header_media")]
    header_document: Option<String>,
    /// A button's parameter; repeatable. TYPE is url, quick_reply, copy_code, flow or catalog.
    #[arg(long = "button", value_name = "INDEX:TYPE=VALUE")]
    buttons: Vec<String>,
    /// JSON send components: an array or {"components": [...]}.
    #[arg(
        long,
        value_name = "PATH",
        conflicts_with_all = ["body_vars", "named_vars", "header_text", "header_media", "buttons"]
    )]
    components_file: Option<PathBuf>,
    #[command(flatten)]
    reply: Reply,
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("source").required(true).args(["file", "id", "link"])))]
pub struct Media {
    /// Recipient.
    to: String,
    /// Upload this local file first, then send it.
    #[arg(long)]
    file: Option<PathBuf>,
    /// An already-uploaded media id.
    #[arg(long)]
    id: Option<String>,
    /// A public HTTPS URL.
    #[arg(long)]
    link: Option<String>,
    /// Caption (image, video and document only).
    #[arg(long)]
    caption: Option<String>,
    /// Displayed file name (document only).
    #[arg(long)]
    filename: Option<String>,
    #[command(flatten)]
    reply: Reply,
    #[command(flatten)]
    policy: Policy,
}

#[derive(Debug, Args)]
pub struct Audio {
    #[command(flatten)]
    media: Media,
    /// Send as a voice message; use a mono OGG file encoded with OPUS.
    #[arg(long)]
    voice: bool,
}

#[derive(Debug, Args)]
pub struct Location {
    /// Recipient.
    to: String,
    #[arg(long, allow_hyphen_values = true)]
    lat: f64,
    #[arg(long, allow_hyphen_values = true)]
    long: f64,
    /// Place name.
    #[arg(long)]
    name: Option<String>,
    /// Street address.
    #[arg(long)]
    address: Option<String>,
    #[command(flatten)]
    reply: Reply,
    #[command(flatten)]
    policy: Policy,
}

#[derive(Debug, Args)]
pub struct Contact {
    /// Recipient.
    to: String,
    /// JSON file holding the Cloud API `contacts` array (or `{"contacts": [...]}`).
    #[arg(long)]
    file: PathBuf,
    #[command(flatten)]
    reply: Reply,
    #[command(flatten)]
    policy: Policy,
}

#[derive(Debug, Args)]
pub struct Reaction {
    /// Recipient.
    to: String,
    /// The message to react to (wamid…).
    #[arg(long)]
    message_id: String,
    /// The emoji; an empty string removes the reaction.
    #[arg(long)]
    emoji: String,
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("input").required(true).args(["file", "stdin"])))]
pub struct Raw {
    /// JSON file with a complete message object.
    #[arg(long)]
    file: Option<PathBuf>,
    /// Read the message object from stdin.
    #[arg(long)]
    stdin: bool,
}

#[derive(Debug, Subcommand)]
pub enum Interactive {
    /// Up to three reply buttons.
    Button(InteractiveButton),
    /// A tappable list of sectioned rows.
    List(InteractiveList),
    /// A button that opens a URL.
    CtaUrl(InteractiveCta),
}

#[derive(Debug, Args)]
pub struct InteractiveCommon {
    /// Recipient.
    to: String,
    /// Body text.
    #[arg(long)]
    body: String,
    /// Header text.
    #[arg(long)]
    header: Option<String>,
    /// Footer text.
    #[arg(long)]
    footer: Option<String>,
    #[command(flatten)]
    reply: Reply,
    #[command(flatten)]
    policy: Policy,
}

impl InteractiveCommon {
    fn text(&self) -> InteractiveText {
        InteractiveText {
            header: self.header.clone(),
            body: self.body.clone(),
            footer: self.footer.clone(),
        }
    }
}

#[derive(Debug, Args)]
pub struct InteractiveButton {
    #[command(flatten)]
    common: InteractiveCommon,
    /// A reply button as `id:Title` (or just `Title`); repeat up to three times.
    #[arg(long = "button", value_name = "ID:TITLE", required = true)]
    buttons: Vec<String>,
}

#[derive(Debug, Args)]
pub struct InteractiveList {
    #[command(flatten)]
    common: InteractiveCommon,
    /// Label of the button that opens the list.
    #[arg(long)]
    button: String,
    /// A section as `Title:id:Label,id2:Label2`; repeatable.
    #[arg(long = "section", value_name = "SECTION", required = true)]
    sections: Vec<String>,
}

#[derive(Debug, Args)]
pub struct InteractiveCta {
    #[command(flatten)]
    common: InteractiveCommon,
    /// Button label.
    #[arg(long)]
    display: String,
    /// URL the button opens.
    #[arg(long)]
    url: String,
}

impl SendCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        match self {
            Self::Text(a) => {
                validate::recipient(&a.to)?;
                let body = match &a.body_file {
                    Some(path) => read_file(path)?,
                    None => a.body.clone().unwrap_or_default(),
                };
                validate::text_body(&body)?;
                let reply_to = a.reply.id()?;
                a.policy.enforce(&a.to)?;
                dispatch(app, messages::text(&a.to, &body, a.preview_url, reply_to)).await
            }
            Self::Template(a) => {
                validate::recipient(&a.to)?;
                validate::not_blank(&a.name, "template name")?;
                validate::not_blank(&a.lang, "--lang")?;
                let reply_to = a.reply.id()?;
                let components = if let Some(path) = &a.components_file {
                    template_components_file(path)?
                } else {
                    let mut named = Vec::new();
                    for entry in &a.named_vars {
                        let (name, value) = entry
                            .split_once('=')
                            .ok_or_else(|| Error::validation("--var must use NAME=VALUE"))?;
                        validate::template_parameter_name(name)?;
                        validate::not_blank(value, "--var value")?;
                        if named.iter().any(|(existing, _)| existing == name) {
                            return Err(Error::validation("--var parameter names must be unique"));
                        }
                        named.push((name.to_owned(), value.to_owned()));
                    }
                    for value in &a.body_vars {
                        validate::not_blank(value, "--body-var")?;
                    }
                    let header = template_header(&a)?;
                    let buttons = parse_template_buttons(&a.buttons)?;
                    messages::template_components(&messages::TemplateParams {
                        body_vars: &a.body_vars,
                        named_vars: &named,
                        header,
                        buttons: &buttons,
                    })
                };
                if reply_to.is_some() {
                    app.out
                        .warn("WhatsApp doesn't show the quoted message on a template");
                }
                dispatch(
                    app,
                    messages::template_with_components(
                        &a.to, &a.name, &a.lang, components, reply_to,
                    ),
                )
                .await
            }
            Self::Image(a) => send_media(app, MediaKind::Image, a, false).await,
            Self::Document(a) => send_media(app, MediaKind::Document, a, false).await,
            Self::Audio(a) => send_media(app, MediaKind::Audio, a.media, a.voice).await,
            Self::Video(a) => send_media(app, MediaKind::Video, a, false).await,
            Self::Sticker(a) => send_media(app, MediaKind::Sticker, a, false).await,
            Self::Location(a) => {
                validate::recipient(&a.to)?;
                if !(-90.0..=90.0).contains(&a.lat) || !(-180.0..=180.0).contains(&a.long) {
                    return Err(Error::validation(
                        "--lat must be within ±90 and --long within ±180",
                    ));
                }
                let reply_to = a.reply.id()?;
                a.policy.enforce(&a.to)?;
                let payload = messages::location(
                    &a.to,
                    a.lat,
                    a.long,
                    a.name.as_deref(),
                    a.address.as_deref(),
                    reply_to,
                );
                dispatch(app, payload).await
            }
            Self::Contact(a) => {
                validate::recipient(&a.to)?;
                let doc = read_json(&a.file)?;
                let contacts = match doc {
                    Value::Array(_) => doc,
                    Value::Object(ref m) if m.get("contacts").is_some_and(Value::is_array) => {
                        m["contacts"].clone()
                    }
                    _ => {
                        return Err(Error::validation(
                            "contacts file must hold a JSON array or {\"contacts\": [...]}",
                        ));
                    }
                };
                if contacts.as_array().is_none_or(Vec::is_empty) {
                    return Err(Error::validation("contacts file has no contacts"));
                }
                let reply_to = a.reply.id()?;
                a.policy.enforce(&a.to)?;
                dispatch(app, messages::contacts(&a.to, contacts, reply_to)).await
            }
            Self::Interactive(i) => i.run(app).await,
            Self::Reaction(a) => {
                validate::recipient(&a.to)?;
                validate::not_blank(&a.message_id, "--message-id")?;
                dispatch(app, messages::reaction(&a.to, &a.message_id, &a.emoji)).await
            }
            Self::Raw(a) => {
                let text = match &a.file {
                    Some(path) => read_file(path)?,
                    None => {
                        let mut buf = String::new();
                        std::io::stdin()
                            .read_to_string(&mut buf)
                            .map_err(|e| Error::io("cannot read stdin", e))?;
                        buf
                    }
                };
                let payload: Value = serde_json::from_str(&text)
                    .map_err(|e| Error::validation(format!("message is not valid JSON: {e}")))?;
                dispatch(app, payload).await
            }
            Self::Flow => planned("send flow", "v1.0"),
        }
    }
}

fn template_header(a: &Template) -> Result<Option<messages::TemplateHeader>> {
    if let Some(text) = &a.header_text {
        validate::not_blank(text, "--header-text")?;
        return Ok(Some(messages::TemplateHeader::Text(text.clone())));
    }
    let media = [
        (&a.header_image, MediaKind::Image, "--header-image"),
        (&a.header_video, MediaKind::Video, "--header-video"),
        (&a.header_document, MediaKind::Document, "--header-document"),
    ];
    for (value, kind, flag) in media {
        if let Some(value) = value {
            let source = media_reference(value, flag)?;
            return Ok(Some(messages::TemplateHeader::Media(kind, source)));
        }
    }
    Ok(None)
}

/// A media id or an http(s) link. An error never echoes the value, since a link can carry
/// credentials in its query string.
fn media_reference(value: &str, flag: &str) -> Result<MediaSource> {
    validate::not_blank(value, flag)?;
    let value = value.trim();
    let invalid = || Error::validation(format!("{flag} must be a media id or an http(s) link"));
    if value.chars().any(char::is_whitespace) {
        return Err(invalid());
    }
    if has_http_scheme(value) {
        let host = value.split_once("://").map_or("", |(_, rest)| rest);
        if host.is_empty() || host.starts_with('/') {
            return Err(invalid());
        }
        return Ok(MediaSource::Link(value.to_owned()));
    }
    if value.contains([':', '/']) {
        return Err(invalid());
    }
    Ok(MediaSource::Id(value.to_owned()))
}

/// `INDEX:TYPE=VALUE`, split at the first `:` and `=`, so a value may contain both. Errors
/// name the flag by position and never echo the text, which a typo can fill with a token.
fn parse_template_buttons(specs: &[String]) -> Result<Vec<messages::TemplateButton>> {
    use messages::{TemplateButton, TemplateButtonKind as Kind};

    let mut buttons: Vec<TemplateButton> = Vec::new();
    for (position, spec) in (1..).zip(specs) {
        let invalid =
            |problem: String| Error::validation(format!("--button #{position} {problem}"));
        let parsed = spec.split_once(':').and_then(|(index, rest)| {
            rest.split_once('=')
                .map(|(kind, value)| (index, kind, value))
        });
        let Some((index, kind, value)) = parsed else {
            return Err(invalid("must use INDEX:TYPE=VALUE".into()));
        };
        let index = index
            .parse::<u8>()
            .ok()
            .filter(|index| *index < limits::MAX_TEMPLATE_BUTTONS)
            .ok_or_else(|| {
                invalid(format!(
                    "index must be 0 to {}",
                    limits::MAX_TEMPLATE_BUTTONS - 1
                ))
            })?;
        let kind = match kind {
            "url" => Kind::Url,
            "quick_reply" => Kind::QuickReply,
            "copy_code" => Kind::CopyCode,
            "flow" => Kind::Flow,
            "catalog" => Kind::Catalog,
            _ => {
                return Err(invalid(
                    "type must be url, quick_reply, copy_code, flow or catalog".into(),
                ));
            }
        };
        if value.trim().is_empty() {
            return Err(invalid("value is empty".into()));
        }
        if kind == Kind::CopyCode {
            if value.chars().count() > limits::MAX_COUPON_CODE {
                return Err(invalid(format!(
                    "coupon code is over {} characters",
                    limits::MAX_COUPON_CODE
                )));
            }
            if buttons.iter().any(|button| button.kind == Kind::CopyCode) {
                return Err(invalid(
                    "is a second copy_code; a template can have one".into(),
                ));
            }
        }
        if buttons.iter().any(|button| button.index == index) {
            return Err(invalid(format!("index {index} is already used")));
        }
        buttons.push(TemplateButton {
            index,
            kind,
            value: value.to_owned(),
        });
    }
    Ok(buttons)
}

/// Accept the component envelope, not a full message that could override the target.
fn template_components_file(path: &std::path::Path) -> Result<Vec<Value>> {
    let document = read_json(path)?;
    let components = match document {
        Value::Array(items) => items,
        Value::Object(mut fields) if fields.len() == 1 => match fields.remove("components") {
            Some(Value::Array(items)) => items,
            _ => {
                return Err(Error::validation(
                    "components file must hold a JSON array or {\"components\": [...]}",
                ));
            }
        },
        _ => {
            return Err(Error::validation(
                "components file must hold a JSON array or {\"components\": [...]}",
            ));
        }
    };
    for (index, component) in components.iter().enumerate() {
        if !component.is_object()
            || component
                .get("type")
                .and_then(Value::as_str)
                .is_none_or(|s| s.trim().is_empty())
        {
            return Err(Error::validation(format!(
                "components[{index}] needs a non-empty 'type' string"
            )));
        }
        if let Some(parameters) = component.get("parameters") {
            let Some(parameters) = parameters.as_array() else {
                return Err(Error::validation(format!(
                    "components[{index}].parameters must be an array"
                )));
            };
            if parameters.iter().any(|parameter| {
                !parameter.is_object()
                    || parameter
                        .get("type")
                        .and_then(Value::as_str)
                        .is_none_or(|s| s.trim().is_empty())
            }) {
                return Err(Error::validation(format!(
                    "components[{index}].parameters entries need a non-empty 'type' string"
                )));
            }
        }
    }
    Ok(components)
}

impl Interactive {
    fn common(&self) -> &InteractiveCommon {
        match self {
            Self::Button(a) => &a.common,
            Self::List(a) => &a.common,
            Self::CtaUrl(a) => &a.common,
        }
    }

    async fn run(self, app: &App) -> Result<()> {
        let c = self.common();
        validate::recipient(&c.to)?;
        validate::not_blank(&c.body, "--body")?;
        let reply_to = c.reply.id()?;
        let (to, text) = (c.to.clone(), c.text());
        let payload = match &self {
            Self::Button(a) => {
                messages::interactive_buttons(&to, &text, &parse_buttons(&a.buttons)?, reply_to)
            }
            Self::List(a) => {
                validate::not_blank(&a.button, "--button")?;
                let sections = parse_sections(&a.sections)?;
                messages::interactive_list(&to, &text, &a.button, &sections, reply_to)
            }
            Self::CtaUrl(a) => {
                validate::not_blank(&a.display, "--display")?;
                validate::not_blank(&a.url, "--url")?;
                messages::interactive_cta_url(&to, &text, &a.display, &a.url, reply_to)
            }
        };
        self.common().policy.enforce(&to)?;
        dispatch(app, payload).await
    }
}

/// `id:Title`, or a bare title used as its own id.
fn parse_buttons(specs: &[String]) -> Result<Vec<Button>> {
    if specs.is_empty() || specs.len() > MAX_REPLY_BUTTONS {
        return Err(Error::validation(format!(
            "pass 1 to {MAX_REPLY_BUTTONS} --button values"
        )));
    }
    specs
        .iter()
        .map(|s| {
            let (id, title) = s.split_once(':').unwrap_or((s, s));
            validate::not_blank(id, "button id")?;
            validate::not_blank(title, "button title")?;
            Ok(Button {
                id: id.trim().to_owned(),
                title: title.trim().to_owned(),
            })
        })
        .collect()
}

/// Split at commas, reading `\,` as a literal comma.
pub(super) fn split_rows(rows: &str) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut chars = rows.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&',') => {
                chars.next();
                parts.last_mut().expect("starts non-empty").push(',');
            }
            ',' => parts.push(String::new()),
            _ => parts.last_mut().expect("starts non-empty").push(c),
        }
    }
    parts
}

/// `Title:id:Label,id2:Label2`.
fn parse_sections(specs: &[String]) -> Result<Vec<Section>> {
    let sections = specs
        .iter()
        .map(|spec| {
            let (title, rows) = spec.split_once(':').ok_or_else(|| {
                Error::validation(format!(
                    "--section '{spec}' must look like Title:id:Label,id2:Label2"
                ))
            })?;
            let rows = rows
                .split(',')
                .filter(|r| !r.trim().is_empty())
                .map(|r| {
                    let (id, label) = r.split_once(':').ok_or_else(|| {
                        Error::validation(format!(
                            "row '{r}' in --section '{title}' must look like id:Label"
                        ))
                    })?;
                    Ok(Row {
                        id: id.trim().to_owned(),
                        title: label.trim().to_owned(),
                        description: None,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            if rows.is_empty() {
                return Err(Error::validation(format!(
                    "--section '{title}' has no rows"
                )));
            }
            Ok(Section {
                title: title.trim().to_owned(),
                rows,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let total: usize = sections.iter().map(|s| s.rows.len()).sum();
    if total > MAX_LIST_ROWS {
        return Err(Error::validation(format!(
            "a list holds at most {MAX_LIST_ROWS} rows in total (got {total})"
        )));
    }
    Ok(sections)
}

async fn send_media(app: &App, kind: MediaKind, a: Media, voice: bool) -> Result<()> {
    validate::recipient(&a.to)?;
    if a.caption.is_some() && !kind.supports_caption() {
        return Err(Error::usage(format!(
            "--caption is not supported for {} messages",
            kind.as_str()
        )));
    }
    if a.filename.is_some() && !kind.supports_filename() {
        return Err(Error::usage(format!(
            "--filename is only supported for document messages, not {}",
            kind.as_str()
        )));
    }
    validate::caption(a.caption.as_deref())?;
    let options = MediaOptions {
        caption: a.caption.as_deref(),
        filename: a.filename.as_deref(),
        voice,
        reply_to: a.reply.id()?,
    };
    let file = a.file.as_deref().map(validate::readable_file).transpose()?;
    a.policy.enforce(&a.to)?;

    let phone = app.ctx()?.require_phone_number_id()?.to_owned();
    let source = match (&file, a.id, a.link) {
        (Some(path), _, _) => {
            let upload = media_api::upload(&phone, path, &media_api::guess_mime(path));
            if app.dry_run() {
                let payload = messages::media(
                    &a.to,
                    kind,
                    &MediaSource::Id("<uploaded-at-send-time>".into()),
                    &options,
                );
                let payload = messages::with_messaging_account(
                    payload,
                    app.ctx()?.messaging_account_id.as_deref(),
                );
                return app.print_dry_run(&[upload, messages::send(&phone, payload)]);
            }
            let uploaded: media_api::Uploaded = app.fetch(&upload).await?.parse()?;
            app.out.note(format!(
                "uploaded {} as media {}",
                path.display(),
                uploaded.id
            ));
            MediaSource::Id(uploaded.id)
        }
        (None, Some(id), _) => MediaSource::Id(id),
        (None, None, Some(link)) => MediaSource::Link(link),
        (None, None, None) => unreachable!("clap enforces one media source"),
    };
    dispatch(app, messages::media(&a.to, kind, &source, &options)).await
}

/// POST the message and print the response with its wamid as the primary id.
async fn dispatch(app: &App, payload: Value) -> Result<()> {
    let ctx = app.ctx()?;
    let phone = ctx.require_phone_number_id()?.to_owned();
    let payload = messages::with_messaging_account(payload, ctx.messaging_account_id.as_deref());
    if let Some(resp) = app.send(&messages::send(&phone, payload)).await? {
        let out = Output::response(&resp)?;
        let id = messages::message_id(&out.data);
        app.out.emit(&out.id(id));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_parse_id_and_title() {
        let b = parse_buttons(&["yes:Yes please".into(), "No".into()]).unwrap();
        assert_eq!(
            b[0],
            Button {
                id: "yes".into(),
                title: "Yes please".into()
            }
        );
        assert_eq!(
            b[1],
            Button {
                id: "No".into(),
                title: "No".into()
            }
        );
        assert!(parse_buttons(&vec!["a".into(); 4]).is_err());
    }

    #[test]
    fn sections_parse_and_cap_rows() {
        let s = parse_sections(&["Fruit:a:Apple,b:Banana".into()]).unwrap();
        assert_eq!(s[0].title, "Fruit");
        assert_eq!(s[0].rows[1].title, "Banana");
        assert!(parse_sections(&["Empty:".into()]).is_err());
        let many: String = (0..11)
            .map(|i| format!("{i}:R{i}"))
            .collect::<Vec<_>>()
            .join(",");
        assert!(parse_sections(&[format!("Big:{many}")]).is_err());
    }
}
