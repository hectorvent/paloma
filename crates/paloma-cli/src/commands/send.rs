use std::io::Read;
use std::path::{Path, PathBuf};

use clap::{ArgGroup, Args, Subcommand, ValueEnum};
use paloma_core::compliance::{NoTracking, SendPolicy, enforce_session_window};
use paloma_core::limits::{self, MAX_LIST_ROWS, MAX_REPLY_BUTTONS};
use paloma_core::{Error, Result, validate};
use paloma_graph::api::media as media_api;
use paloma_graph::api::messages::{
    self, Button, InteractiveHeader, InteractiveText, MediaKind, MediaOptions, MediaSource, Row,
    Section,
};
use paloma_graph::request::has_http_scheme;
use serde_json::Value;

use super::{read_file, read_json};
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
    /// Send a contact card from flags, or contact cards from a JSON file.
    Contact(Contact),
    /// Send an interactive message (buttons, list, call-to-action URL, carousel and more).
    #[command(subcommand)]
    Interactive(Interactive),
    /// React to a message with an emoji.
    Reaction(Reaction),
    /// Send an arbitrary message object as-is.
    Raw(Raw),
    /// Send a WhatsApp Flow, opened from a button on the message.
    Flow(FlowMessage),
    /// Ask the user for permission to call them. Needs calling enabled on the number.
    CallPermission(CallPermission),
    /// Send one product from your catalog.
    Product(ProductMessage),
    /// Send up to 30 products from your catalog, in sections.
    ProductList(ProductList),
    /// Send a button that opens your catalog.
    Catalog(Catalog),
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
    /// Recipient (E.164 number or business-scoped user ID).
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
    /// Recipient (E.164 number or business-scoped user ID).
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
    /// Recipient (E.164 number or business-scoped user ID).
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
    /// Recipient (E.164 number or business-scoped user ID).
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
#[command(group(ArgGroup::new("contact_source").required(true).args(["file", "name"])))]
pub struct Contact {
    /// Recipient (E.164 number or business-scoped user ID).
    to: String,
    /// JSON file holding the Cloud API `contacts` array (or `{"contacts": [...]}`).
    #[arg(long, conflicts_with_all = ["first_name", "last_name", "phones", "wa_id", "emails"])]
    file: Option<PathBuf>,
    /// The contact's full name, shown on the message.
    #[arg(long)]
    name: Option<String>,
    /// The contact's first name.
    #[arg(long, requires = "name")]
    first_name: Option<String>,
    /// The contact's last name.
    #[arg(long, requires = "name")]
    last_name: Option<String>,
    /// A phone number for the contact; repeatable.
    #[arg(long = "phone", value_name = "NUMBER", requires = "name")]
    phones: Vec<String>,
    /// WhatsApp id of the first --phone. It adds the Message and Save contact buttons;
    /// without it the message shows an Invite to WhatsApp button.
    #[arg(long, requires = "phones")]
    wa_id: Option<String>,
    /// An email address for the contact; repeatable.
    #[arg(long = "email", value_name = "ADDRESS", requires = "name")]
    emails: Vec<String>,
    #[command(flatten)]
    reply: Reply,
    #[command(flatten)]
    policy: Policy,
}

#[derive(Debug, Args)]
pub struct Reaction {
    /// Recipient (E.164 number or business-scoped user ID).
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

/// Where a Flow starts: `data_exchange` asks your endpoint for the first screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FlowAction {
    Navigate,
    #[value(name = "data_exchange")]
    DataExchange,
}

impl FlowAction {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Navigate => "navigate",
            Self::DataExchange => "data_exchange",
        }
    }
}

/// Which version of a Flow to send.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FlowMode {
    Draft,
    Published,
}

impl FlowMode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Published => "published",
        }
    }
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("flow_ref").required(true).args(["flow_id", "flow_name"])))]
pub struct FlowMessage {
    #[command(flatten)]
    common: InteractiveCommon,
    /// The Flow's id.
    #[arg(long)]
    flow_id: Option<String>,
    /// The Flow's name.
    #[arg(long)]
    flow_name: Option<String>,
    /// Text on the button that opens the Flow. Meta advises 30 characters or less, no emoji.
    #[arg(long)]
    cta: String,
    /// Your own id for this send, returned when the Flow completes. Meta defaults it to
    /// "unused".
    #[arg(long, value_name = "ID")]
    flow_token: Option<String>,
    /// How the Flow starts. Meta defaults to navigate.
    #[arg(long, value_enum)]
    action: Option<FlowAction>,
    /// Send the draft or the published version. Meta defaults to published.
    #[arg(long, value_enum)]
    mode: Option<FlowMode>,
    /// Id of the first screen (navigate only). Meta defaults to FIRST_ENTRY_SCREEN.
    #[arg(long, value_name = "ID")]
    screen: Option<String>,
    /// Input for the first screen, as a non-empty JSON object (navigate only).
    #[arg(long, value_name = "JSON", conflicts_with = "data_file")]
    data: Option<String>,
    /// Read the first screen's input from a JSON file: a non-empty object (navigate only).
    #[arg(long, value_name = "PATH")]
    data_file: Option<PathBuf>,
    /// Image for the header: a media id or an http(s) link.
    #[arg(
        long,
        value_name = "ID|URL",
        group = "header_media",
        conflicts_with = "header"
    )]
    header_image: Option<String>,
    /// Video for the header: a media id or an http(s) link.
    #[arg(
        long,
        value_name = "ID|URL",
        group = "header_media",
        conflicts_with = "header"
    )]
    header_video: Option<String>,
    /// Document for the header: a media id or an http(s) link.
    #[arg(
        long,
        value_name = "ID|URL",
        group = "header_media",
        conflicts_with = "header"
    )]
    header_document: Option<String>,
}

#[derive(Debug, Args)]
pub struct CallPermission {
    /// Recipient (E.164 number or business-scoped user ID).
    to: String,
    /// Why you want to call. Optional, but Meta recommends giving the user context.
    #[arg(long)]
    body: Option<String>,
    #[command(flatten)]
    reply: Reply,
    #[command(flatten)]
    policy: Policy,
}

#[derive(Debug, Args)]
pub struct ProductMessage {
    /// Recipient (E.164 number or business-scoped user ID).
    to: String,
    /// The catalog holding the product.
    #[arg(long)]
    catalog_id: String,
    /// The product's retailer id (its SKU; Content ID in Commerce Manager).
    #[arg(long, value_name = "SKU")]
    product: String,
    /// Body text.
    #[arg(long)]
    body: Option<String>,
    /// Footer text.
    #[arg(long)]
    footer: Option<String>,
    #[command(flatten)]
    reply: Reply,
    #[command(flatten)]
    policy: Policy,
}

#[derive(Debug, Args)]
pub struct ProductList {
    /// Recipient (E.164 number or business-scoped user ID).
    to: String,
    /// The catalog holding the products.
    #[arg(long)]
    catalog_id: String,
    /// Header text.
    #[arg(long)]
    header: String,
    /// Body text.
    #[arg(long)]
    body: String,
    /// Footer text.
    #[arg(long)]
    footer: Option<String>,
    /// A section as `Title:sku1,sku2`; repeatable. Write a comma in a SKU as `\,`.
    #[arg(long = "section", value_name = "SECTION", required = true)]
    sections: Vec<String>,
    #[command(flatten)]
    reply: Reply,
    #[command(flatten)]
    policy: Policy,
}

#[derive(Debug, Args)]
pub struct Catalog {
    /// Recipient (E.164 number or business-scoped user ID).
    to: String,
    /// Body text.
    #[arg(long)]
    body: String,
    /// Footer text.
    #[arg(long)]
    footer: Option<String>,
    /// SKU of the product whose image heads the message. Meta defaults to the first item
    /// in the catalog.
    #[arg(long, value_name = "SKU")]
    thumbnail: Option<String>,
    #[command(flatten)]
    reply: Reply,
    #[command(flatten)]
    policy: Policy,
}

#[derive(Debug, Subcommand)]
pub enum Interactive {
    /// Up to three reply buttons.
    Button(InteractiveButton),
    /// A tappable list of sectioned rows.
    List(InteractiveList),
    /// A button that opens a URL.
    CtaUrl(InteractiveCta),
    /// A button that asks the user to share their location.
    LocationRequest(InteractiveBody),
    /// A form that asks for a shipping address (India only).
    Address(InteractiveAddress),
    /// 2 to 10 scrollable cards: media from a file, or products from a catalog.
    Carousel(InteractiveCarousel),
    /// A button that calls your business. Needs calling enabled on the number.
    Call(InteractiveCall),
    /// A button that asks the user to share their phone number.
    RequestContact(InteractiveBody),
}

#[derive(Debug, Args)]
pub struct InteractiveCommon {
    /// Recipient (E.164 number or business-scoped user ID).
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
            header: self.header.clone().map(InteractiveHeader::Text),
            body: self.body.clone(),
            footer: self.footer.clone(),
        }
    }
}

/// The flags of a kind where Meta allows no header or footer.
#[derive(Debug, Args)]
pub struct InteractiveBody {
    /// Recipient (E.164 number or business-scoped user ID).
    to: String,
    /// Body text.
    #[arg(long)]
    body: String,
    #[command(flatten)]
    reply: Reply,
    #[command(flatten)]
    policy: Policy,
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("address_source").required(true).args(["country", "parameters_file"])))]
pub struct InteractiveAddress {
    #[command(flatten)]
    common: InteractiveCommon,
    /// The address form's country, as an ISO code. Meta supports IN.
    #[arg(long, value_name = "CODE")]
    country: Option<String>,
    /// JSON file with the action parameters: `country`, plus optional `values`,
    /// `saved_addresses` and `validation_errors`.
    #[arg(long, value_name = "PATH")]
    parameters_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("carousel_cards").required(true).args(["card_file", "products"])))]
pub struct InteractiveCarousel {
    #[command(flatten)]
    base: InteractiveBody,
    /// JSON file with the media cards: an array, or {"cards": [...]}.
    #[arg(long, value_name = "PATH")]
    card_file: Option<PathBuf>,
    /// Product retailer ids (SKUs), one card each, separated by commas.
    #[arg(
        long,
        value_name = "SKU,SKU",
        value_delimiter = ',',
        requires = "catalog_id"
    )]
    products: Vec<String>,
    /// The catalog holding every product.
    #[arg(long, requires = "products")]
    catalog_id: Option<String>,
}

#[derive(Debug, Args)]
pub struct InteractiveCall {
    #[command(flatten)]
    base: InteractiveBody,
    /// Button label. Meta defaults to "Call Now".
    #[arg(long)]
    display: Option<String>,
    /// How long the button lasts, in minutes (1 to 43200). Meta defaults to 7 days.
    #[arg(long, value_name = "MINUTES")]
    ttl_minutes: Option<u32>,
    /// A string returned in the `calls` webhook as `cta_payload`, for tracking.
    #[arg(long)]
    payload: Option<String>,
}

#[derive(Debug, Args)]
pub struct InteractiveButton {
    #[command(flatten)]
    common: InteractiveCommon,
    /// A reply button as `id:Title` (or just `Title`); repeat up to three times.
    #[arg(long = "button", value_name = "ID:TITLE", required = true)]
    buttons: Vec<String>,
    /// Image for the header: a media id or an http(s) link.
    #[arg(
        long,
        value_name = "ID|URL",
        group = "header_media",
        conflicts_with = "header"
    )]
    header_image: Option<String>,
    /// Video for the header: a media id or an http(s) link.
    #[arg(
        long,
        value_name = "ID|URL",
        group = "header_media",
        conflicts_with = "header"
    )]
    header_video: Option<String>,
    /// Document for the header: a media id or an http(s) link.
    #[arg(
        long,
        value_name = "ID|URL",
        group = "header_media",
        conflicts_with = "header"
    )]
    header_document: Option<String>,
}

#[derive(Debug, Args)]
pub struct InteractiveList {
    #[command(flatten)]
    common: InteractiveCommon,
    /// Label of the button that opens the list.
    #[arg(long)]
    button: String,
    /// A section as `Title:id:Label::Description,id2:Label2`; repeatable. The description
    /// is optional; write a comma in a row's text as `\,`.
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
    /// Image for the header: an http(s) link.
    #[arg(
        long,
        value_name = "URL",
        group = "header_media",
        conflicts_with = "header"
    )]
    header_image: Option<String>,
    /// Video for the header: an http(s) link.
    #[arg(
        long,
        value_name = "URL",
        group = "header_media",
        conflicts_with = "header"
    )]
    header_video: Option<String>,
    /// Document for the header: an http(s) link.
    #[arg(
        long,
        value_name = "URL",
        group = "header_media",
        conflicts_with = "header"
    )]
    header_document: Option<String>,
}

/// The contacts array from a `--file`, passed through as authored.
fn contacts_file(path: &Path) -> Result<Value> {
    let doc = read_json(path)?;
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
    let count = contacts.as_array().map_or(0, Vec::len);
    if count == 0 {
        return Err(Error::validation("contacts file has no contacts"));
    }
    if count > limits::MAX_CONTACTS {
        return Err(Error::validation(format!(
            "contacts file has {count} contacts; max is {}",
            limits::MAX_CONTACTS
        )));
    }
    Ok(contacts)
}

/// The contacts array for the typed flags. clap has already required `--name` here.
fn typed_contact(a: &Contact) -> Result<Value> {
    let formatted_name = a.name.as_deref().unwrap_or_default();
    validate::not_blank(formatted_name, "--name")?;
    for (value, flag) in [
        (&a.first_name, "--first-name"),
        (&a.last_name, "--last-name"),
        (&a.wa_id, "--wa-id"),
    ] {
        if let Some(value) = value {
            validate::not_blank(value, flag)?;
        }
    }
    for phone in &a.phones {
        validate::not_blank(phone, "--phone")?;
    }
    for email in &a.emails {
        validate::not_blank(email, "--email")?;
    }
    Ok(messages::contact_card(&messages::ContactCard {
        formatted_name,
        first_name: a.first_name.as_deref(),
        last_name: a.last_name.as_deref(),
        phones: &a.phones,
        wa_id: a.wa_id.as_deref(),
        emails: &a.emails,
    }))
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
                let contacts = match &a.file {
                    Some(path) => contacts_file(path)?,
                    None => typed_contact(&a)?,
                };
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
            Self::Flow(a) => send_flow(app, &a).await,
            Self::CallPermission(a) => {
                validate::recipient(&a.to)?;
                if let Some(body) = &a.body {
                    check_body(body)?;
                }
                let reply_to = a.reply.id()?;
                a.policy.enforce(&a.to)?;
                let payload = messages::call_permission_request(&a.to, a.body.as_deref(), reply_to);
                dispatch(app, payload).await
            }
            Self::Product(a) => {
                validate::recipient(&a.to)?;
                validate::not_blank(&a.catalog_id, "--catalog-id")?;
                validate::not_blank(&a.product, "--product")?;
                if let Some(body) = &a.body {
                    check_body(body)?;
                }
                check_footer(a.footer.as_deref())?;
                let reply_to = a.reply.id()?;
                a.policy.enforce(&a.to)?;
                let payload = messages::interactive_product(
                    &a.to,
                    &a.catalog_id,
                    &a.product,
                    a.body.as_deref(),
                    a.footer.as_deref(),
                    reply_to,
                );
                dispatch(app, payload).await
            }
            Self::ProductList(a) => {
                validate::recipient(&a.to)?;
                validate::not_blank(&a.catalog_id, "--catalog-id")?;
                validate::not_blank(&a.header, "--header")?;
                validate::max_chars(&a.header, limits::MAX_INTERACTIVE_HEADER, "--header")?;
                check_body(&a.body)?;
                check_footer(a.footer.as_deref())?;
                let sections = parse_product_sections(&a.sections)?;
                let reply_to = a.reply.id()?;
                a.policy.enforce(&a.to)?;
                let text = InteractiveText {
                    header: Some(InteractiveHeader::Text(a.header.clone())),
                    body: a.body.clone(),
                    footer: a.footer.clone(),
                };
                let payload = messages::interactive_product_list(
                    &a.to,
                    &text,
                    &a.catalog_id,
                    &sections,
                    reply_to,
                );
                dispatch(app, payload).await
            }
            Self::Catalog(a) => {
                validate::recipient(&a.to)?;
                check_body(&a.body)?;
                check_footer(a.footer.as_deref())?;
                if let Some(thumbnail) = &a.thumbnail {
                    validate::not_blank(thumbnail, "--thumbnail")?;
                }
                let reply_to = a.reply.id()?;
                a.policy.enforce(&a.to)?;
                let payload = messages::interactive_catalog(
                    &a.to,
                    &a.body,
                    a.footer.as_deref(),
                    a.thumbnail.as_deref(),
                    reply_to,
                );
                dispatch(app, payload).await
            }
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
    /// The recipient, body, reply and policy flags every kind has.
    fn parts(&self) -> (&str, &str, &Reply, Policy) {
        match self {
            Self::Button(InteractiveButton { common: c, .. })
            | Self::List(InteractiveList { common: c, .. })
            | Self::CtaUrl(InteractiveCta { common: c, .. })
            | Self::Address(InteractiveAddress { common: c, .. }) => {
                (&c.to, &c.body, &c.reply, c.policy)
            }
            Self::LocationRequest(b)
            | Self::RequestContact(b)
            | Self::Carousel(InteractiveCarousel { base: b, .. })
            | Self::Call(InteractiveCall { base: b, .. }) => (&b.to, &b.body, &b.reply, b.policy),
        }
    }

    async fn run(self, app: &App) -> Result<()> {
        let (to, body, reply, policy) = self.parts();
        validate::recipient(to)?;
        validate::not_blank(body, "--body")?;
        let reply_to = reply.id()?;
        let payload = match &self {
            Self::Button(a) => {
                let mut text = a.common.text();
                let header = interactive_media_header(
                    a.header_image.as_deref(),
                    a.header_video.as_deref(),
                    a.header_document.as_deref(),
                )?;
                if header.is_some() {
                    text.header = header;
                }
                messages::interactive_buttons(to, &text, &parse_buttons(&a.buttons)?, reply_to)
            }
            Self::List(a) => {
                validate::not_blank(&a.button, "--button")?;
                let sections = parse_sections(&a.sections)?;
                messages::interactive_list(to, &a.common.text(), &a.button, &sections, reply_to)
            }
            Self::CtaUrl(a) => {
                let mut text = a.common.text();
                validate::not_blank(&a.display, "--display")?;
                validate::not_blank(&a.url, "--url")?;
                let header = interactive_media_header(
                    a.header_image.as_deref(),
                    a.header_video.as_deref(),
                    a.header_document.as_deref(),
                )?;
                if let Some(InteractiveHeader::Media(_, MediaSource::Id(_))) = header {
                    return Err(Error::validation(
                        "a cta-url media header must be an http(s) link, not a media id",
                    ));
                }
                if header.is_some() {
                    text.header = header;
                }
                messages::interactive_cta_url(to, &text, &a.display, &a.url, reply_to)
            }
            Self::LocationRequest(_) => {
                check_body(body)?;
                messages::interactive_location_request(to, body, reply_to)
            }
            Self::Address(a) => {
                check_common(&a.common)?;
                let parameters = match (&a.country, &a.parameters_file) {
                    (Some(country), _) => {
                        validate::not_blank(country, "--country")?;
                        messages::address_parameters(country.trim())
                    }
                    (None, Some(path)) => address_parameters_file(path)?,
                    (None, None) => unreachable!("clap enforces one address source"),
                };
                messages::interactive_address(to, &a.common.text(), &parameters, reply_to)
            }
            Self::Carousel(a) => {
                check_body(body)?;
                let cards = match (&a.card_file, &a.catalog_id) {
                    (Some(path), _) => media_cards_file(path)?,
                    (None, Some(catalog_id)) => {
                        validate::not_blank(catalog_id, "--catalog-id")?;
                        check_card_count(a.products.len())?;
                        let products: Vec<String> =
                            a.products.iter().map(|p| p.trim().to_owned()).collect();
                        for product in &products {
                            validate::not_blank(product, "--products")?;
                        }
                        messages::product_cards(catalog_id, &products)
                    }
                    (None, None) => unreachable!("clap enforces one card source"),
                };
                messages::interactive_carousel(to, body, &cards, reply_to)
            }
            Self::Call(a) => {
                check_body(body)?;
                if let Some(display) = &a.display {
                    validate::not_blank(display, "--display")?;
                    validate::max_chars(display, limits::MAX_CALL_BUTTON_TEXT, "--display")?;
                }
                if let Some(ttl) = a.ttl_minutes
                    && !limits::CALL_BUTTON_TTL_MINUTES.contains(&ttl)
                {
                    return Err(Error::validation(format!(
                        "--ttl-minutes must be between {} and {}",
                        limits::CALL_BUTTON_TTL_MINUTES.start(),
                        limits::CALL_BUTTON_TTL_MINUTES.end()
                    )));
                }
                if let Some(payload) = &a.payload {
                    validate::not_blank(payload, "--payload")?;
                    validate::max_chars(payload, limits::MAX_CALL_BUTTON_PAYLOAD, "--payload")?;
                }
                let button = messages::CallButton {
                    display_text: a.display.as_deref(),
                    ttl_minutes: a.ttl_minutes,
                    payload: a.payload.as_deref(),
                };
                messages::interactive_call(to, body, &button, reply_to)
            }
            Self::RequestContact(_) => {
                check_body(body)?;
                messages::interactive_request_contact(to, body, reply_to)
            }
        };
        policy.enforce(to)?;
        dispatch(app, payload).await
    }
}

fn check_body(body: &str) -> Result<()> {
    validate::not_blank(body, "--body")?;
    validate::max_chars(body, limits::MAX_INTERACTIVE_BODY, "--body")
}

fn check_footer(footer: Option<&str>) -> Result<()> {
    footer.map_or(Ok(()), |footer| {
        validate::max_chars(footer, limits::MAX_INTERACTIVE_FOOTER, "--footer")
    })
}

fn check_common(c: &InteractiveCommon) -> Result<()> {
    check_body(&c.body)?;
    if let Some(header) = &c.header {
        validate::max_chars(header, limits::MAX_INTERACTIVE_HEADER, "--header")?;
    }
    check_footer(c.footer.as_deref())
}

async fn send_flow(app: &App, a: &FlowMessage) -> Result<()> {
    let c = &a.common;
    validate::recipient(&c.to)?;
    check_common(c)?;
    let reply_to = c.reply.id()?;
    let flow = match (&a.flow_id, &a.flow_name) {
        (Some(id), _) => {
            validate::not_blank(id, "--flow-id")?;
            messages::FlowRef::Id(id)
        }
        (None, Some(name)) => {
            validate::not_blank(name, "--flow-name")?;
            messages::FlowRef::Name(name)
        }
        (None, None) => unreachable!("clap enforces one flow reference"),
    };
    validate::not_blank(&a.cta, "--cta")?;
    for (value, flag) in [(&a.flow_token, "--flow-token"), (&a.screen, "--screen")] {
        if let Some(value) = value {
            validate::not_blank(value, flag)?;
        }
    }
    let data = flow_data(a)?;
    if a.action == Some(FlowAction::DataExchange) && (a.screen.is_some() || data.is_some()) {
        return Err(Error::validation(
            "--screen and --data only apply when --action is navigate",
        ));
    }
    let mut text = c.text();
    if let Some(header) = interactive_media_header(
        a.header_image.as_deref(),
        a.header_video.as_deref(),
        a.header_document.as_deref(),
    )? {
        text.header = Some(header);
    }
    if a.cta.chars().count() > limits::ADVISED_FLOW_CTA {
        app.out.warn(format!(
            "--cta is over the {} characters Meta advises",
            limits::ADVISED_FLOW_CTA
        ));
    }
    c.policy.enforce(&c.to)?;
    let params = messages::FlowParams {
        flow,
        cta: &a.cta,
        token: a.flow_token.as_deref(),
        action: a.action.map(FlowAction::as_str),
        mode: a.mode.map(FlowMode::as_str),
        screen: a.screen.as_deref(),
        data: data.as_ref(),
    };
    dispatch(
        app,
        messages::interactive_flow(&c.to, &text, &params, reply_to),
    )
    .await
}

/// The first screen's input. Meta requires a non-empty object when there is one. Errors
/// never echo the input, which is often personal data.
fn flow_data(a: &FlowMessage) -> Result<Option<Value>> {
    let (data, what) = match (&a.data, &a.data_file) {
        (Some(text), _) => (
            serde_json::from_str(text)
                .map_err(|e| Error::validation(format!("--data is not valid JSON: {e}")))?,
            "--data",
        ),
        (None, Some(path)) => (read_json(path)?, "--data-file"),
        (None, None) => return Ok(None),
    };
    match data {
        Value::Object(fields) if !fields.is_empty() => Ok(Some(Value::Object(fields))),
        _ => Err(Error::validation(format!(
            "{what} must hold a non-empty JSON object"
        ))),
    }
}

/// `Title:sku1,sku2`. Meta counts products across every section.
fn parse_product_sections(specs: &[String]) -> Result<Vec<messages::ProductSection>> {
    if specs.len() > limits::MAX_PRODUCT_LIST_SECTIONS {
        return Err(Error::validation(format!(
            "a product list holds at most {} sections (got {})",
            limits::MAX_PRODUCT_LIST_SECTIONS,
            specs.len()
        )));
    }
    let sections = specs
        .iter()
        .map(|spec| {
            let (title, products) = spec.split_once(':').ok_or_else(|| {
                Error::validation(format!("--section '{spec}' must look like Title:sku1,sku2"))
            })?;
            validate::not_blank(title, "section title")?;
            validate::max_chars(
                title.trim(),
                limits::MAX_PRODUCT_LIST_SECTION_TITLE,
                "section title",
            )?;
            let products: Vec<String> = split_rows(products)
                .iter()
                .map(|product| product.trim().to_owned())
                .filter(|product| !product.is_empty())
                .collect();
            if products.is_empty() {
                return Err(Error::validation(format!(
                    "--section '{title}' has no products"
                )));
            }
            Ok(messages::ProductSection {
                title: title.trim().to_owned(),
                products,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let total: usize = sections.iter().map(|section| section.products.len()).sum();
    if total > limits::MAX_PRODUCT_LIST_PRODUCTS {
        return Err(Error::validation(format!(
            "a product list holds at most {} products in total (got {total})",
            limits::MAX_PRODUCT_LIST_PRODUCTS
        )));
    }
    Ok(sections)
}

/// An address message's parameters, passed through as authored. Meta rejects a message
/// without a `country`.
fn address_parameters_file(path: &Path) -> Result<Value> {
    let parameters = read_json(path)?;
    if !parameters.is_object() {
        return Err(Error::validation(
            "address parameters file must hold a JSON object",
        ));
    }
    if parameters
        .get("country")
        .and_then(Value::as_str)
        .is_none_or(|country| country.trim().is_empty())
    {
        return Err(Error::validation(
            "address parameters file needs a non-empty 'country' string",
        ));
    }
    for (key, valid) in [
        ("values", parameters.get("values").map(Value::is_object)),
        (
            "saved_addresses",
            parameters.get("saved_addresses").map(Value::is_array),
        ),
        (
            "validation_errors",
            parameters.get("validation_errors").map(Value::is_object),
        ),
    ] {
        if valid == Some(false) {
            let shape = if key == "saved_addresses" {
                "an array"
            } else {
                "an object"
            };
            return Err(Error::validation(format!(
                "address parameters '{key}' must be {shape}"
            )));
        }
    }
    Ok(parameters)
}

fn check_card_count(count: usize) -> Result<()> {
    if limits::CAROUSEL_CARDS.contains(&count) {
        return Ok(());
    }
    Err(Error::validation(format!(
        "a carousel holds {} to {} cards (got {count})",
        limits::CAROUSEL_CARDS.start(),
        limits::CAROUSEL_CARDS.end()
    )))
}

/// The media cards from a `--card-file`, as authored apart from what
/// [`messages::media_cards`] fills in.
fn media_cards_file(path: &Path) -> Result<Vec<Value>> {
    let invalid = || Error::validation("card file must hold a JSON array or {\"cards\": [...]}");
    let cards = match read_json(path)? {
        Value::Array(cards) => cards,
        Value::Object(mut fields) => match fields.remove("cards") {
            Some(Value::Array(cards)) => cards,
            _ => return Err(invalid()),
        },
        _ => return Err(invalid()),
    };
    check_card_count(cards.len())?;
    let cards = messages::media_cards(cards);
    check_media_cards(&cards)?;
    Ok(cards)
}

/// What a card's action has to match on every other card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CardButtons {
    Url,
    QuickReplies(usize),
}

/// Errors name the card by position and never echo its text.
fn check_media_cards(cards: &[Value]) -> Result<()> {
    let mut indexes = Vec::with_capacity(cards.len());
    let mut first_buttons = None;
    for (position, card) in cards.iter().enumerate() {
        let invalid = |problem: &str| Error::validation(format!("cards[{position}] {problem}"));
        if !card.is_object() {
            return Err(invalid("must be an object"));
        }
        if card.get("type").and_then(Value::as_str) == Some("product") {
            return Err(invalid(
                "is a product card; use --products and --catalog-id instead",
            ));
        }
        // Meta's syntax and examples use this one value on every media card.
        if card.get("type").is_some_and(|kind| kind != "cta_url") {
            return Err(invalid("type must be cta_url, or left out"));
        }
        let index = card
            .get("card_index")
            .and_then(Value::as_u64)
            .filter(|index| *index < *limits::CAROUSEL_CARDS.end() as u64)
            .ok_or_else(|| {
                invalid(&format!(
                    "card_index must be a whole number from 0 to {}",
                    limits::CAROUSEL_CARDS.end() - 1
                ))
            })?;
        if indexes.contains(&index) {
            return Err(invalid("repeats a card_index"));
        }
        indexes.push(index);
        check_card_header(card, position)?;
        if let Some(body) = card.get("body") {
            let body = body
                .get("text")
                .and_then(Value::as_str)
                .filter(|text| !text.trim().is_empty())
                .ok_or_else(|| invalid("body needs a non-empty 'text' string"))?;
            validate::max_chars(
                body,
                limits::MAX_CAROUSEL_CARD_BODY,
                &format!("cards[{position}] body"),
            )?;
            if body.matches('\n').count() > limits::MAX_CAROUSEL_CARD_BODY_LINE_BREAKS {
                return Err(invalid(&format!(
                    "body has more than {} line breaks",
                    limits::MAX_CAROUSEL_CARD_BODY_LINE_BREAKS
                )));
            }
        }
        let buttons = check_card_action(card, position)?;
        if *first_buttons.get_or_insert(buttons) != buttons {
            return Err(invalid(
                "has different buttons from cards[0]; every card needs the same type and number",
            ));
        }
    }
    Ok(())
}

/// Meta's page only takes a card's media as a link, and only an image or a video.
fn check_card_header(card: &Value, position: usize) -> Result<()> {
    let invalid = |problem: &str| Error::validation(format!("cards[{position}] {problem}"));
    let kind = card
        .pointer("/header/type")
        .and_then(Value::as_str)
        .filter(|kind| matches!(*kind, "image" | "video"))
        .ok_or_else(|| invalid("needs a header of type image or video"))?;
    let link = card
        .pointer(&format!("/header/{kind}/link"))
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("header needs an http(s) link, not a media id"))?;
    if link != link.trim() {
        return Err(invalid("header link has spaces around it"));
    }
    match media_reference(link, &format!("cards[{position}] header link"))? {
        MediaSource::Link(_) => Ok(()),
        MediaSource::Id(_) => Err(invalid("header needs an http(s) link, not a media id")),
    }
}

fn check_card_action(card: &Value, position: usize) -> Result<CardButtons> {
    let invalid = |problem: &str| Error::validation(format!("cards[{position}] {problem}"));
    let text = |pointer: &str, problem: &str| {
        card.pointer(pointer)
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .map(str::to_owned)
            .ok_or_else(|| invalid(problem))
    };
    let label = format!("cards[{position}] button label");
    match (
        card.pointer("/action/buttons"),
        card.pointer("/action/parameters"),
    ) {
        (Some(buttons), None) => {
            let buttons = buttons
                .as_array()
                .filter(|buttons| !buttons.is_empty())
                .ok_or_else(|| invalid("action.buttons must be a non-empty array"))?;
            for button in buttons {
                if button.get("type").and_then(Value::as_str) != Some("quick_reply") {
                    return Err(invalid("action.buttons entries must be quick_reply"));
                }
                let reply = |field: &str| {
                    button
                        .pointer(&format!("/quick_reply/{field}"))
                        .and_then(Value::as_str)
                        .filter(|text| !text.trim().is_empty())
                        .ok_or_else(|| invalid("quick_reply needs a non-empty id and title"))
                };
                validate::max_chars(
                    reply("id")?,
                    limits::MAX_CAROUSEL_REPLY_ID,
                    &format!("cards[{position}] button id"),
                )?;
                validate::max_chars(reply("title")?, limits::MAX_CAROUSEL_BUTTON_LABEL, &label)?;
            }
            Ok(CardButtons::QuickReplies(buttons.len()))
        }
        (None, Some(_)) => {
            if card.pointer("/action/name").and_then(Value::as_str) != Some("cta_url") {
                return Err(invalid("a URL button's action.name must be cta_url"));
            }
            let display = text(
                "/action/parameters/display_text",
                "URL button needs a non-empty display_text",
            )?;
            validate::max_chars(&display, limits::MAX_CAROUSEL_BUTTON_LABEL, &label)?;
            let url = text("/action/parameters/url", "URL button needs a non-empty url")?;
            if !paloma_graph::request::has_http_scheme(&url) || url != url.trim() {
                return Err(invalid("URL button's url must be an http(s) link"));
            }
            Ok(CardButtons::Url)
        }
        _ => Err(invalid(
            "action needs either one URL button or quick-reply buttons",
        )),
    }
}

fn interactive_media_header(
    image: Option<&str>,
    video: Option<&str>,
    document: Option<&str>,
) -> Result<Option<InteractiveHeader>> {
    let media = [
        (image, MediaKind::Image, "--header-image"),
        (video, MediaKind::Video, "--header-video"),
        (document, MediaKind::Document, "--header-document"),
    ];
    for (value, kind, flag) in media {
        if let Some(value) = value {
            let source = media_reference(value, flag)?;
            return Ok(Some(InteractiveHeader::Media(kind, source)));
        }
    }
    Ok(None)
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

/// `id:Label`, or `id:Label::Description`. A single `:` can't start the description, since
/// a label has always been free to contain one. So a label can't hold `::` or start with `:`.
fn parse_row(row: &str, section: &str) -> Result<Row> {
    let (id, rest) = row.split_once(':').ok_or_else(|| {
        Error::validation(format!(
            "row '{row}' in --section '{section}' must look like id:Label"
        ))
    })?;
    let (label, description) = match rest.split_once("::") {
        Some((label, description)) => (label, Some(description.trim())),
        None => (rest, None),
    };
    let (id, label) = (id.trim(), label.trim());
    validate::not_blank(id, "row id")?;
    validate::not_blank(label, "row title")?;
    if label.starts_with(':') {
        return Err(Error::validation(format!(
            "row '{row}' in --section '{section}' has no title; use id:Label::Description"
        )));
    }
    validate::max_chars(id, limits::MAX_LIST_ROW_ID, "row id")?;
    validate::max_chars(label, limits::MAX_LIST_ROW_TITLE, "row title")?;
    if let Some(description) = description {
        validate::not_blank(description, "row description")?;
        validate::max_chars(
            description,
            limits::MAX_LIST_ROW_DESCRIPTION,
            "row description",
        )?;
    }
    Ok(Row {
        id: id.to_owned(),
        title: label.to_owned(),
        description: description.map(str::to_owned),
    })
}

/// `Title:id:Label::Description,id2:Label2`.
fn parse_sections(specs: &[String]) -> Result<Vec<Section>> {
    let sections = specs
        .iter()
        .map(|spec| {
            let (title, rows) = spec.split_once(':').ok_or_else(|| {
                Error::validation(format!(
                    "--section '{spec}' must look like Title:id:Label,id2:Label2"
                ))
            })?;
            let rows = split_rows(rows)
                .iter()
                .filter(|r| !r.trim().is_empty())
                .map(|r| parse_row(r, title))
                .collect::<Result<Vec<_>>>()?;
            if rows.is_empty() {
                return Err(Error::validation(format!(
                    "--section '{title}' has no rows"
                )));
            }
            validate::not_blank(title, "section title")?;
            validate::max_chars(
                title.trim(),
                limits::MAX_LIST_SECTION_TITLE,
                "section title",
            )?;
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
    let mime = file.as_deref().map(media_api::guess_mime);
    if let (Some(path), Some(mime)) = (&file, &mime) {
        let limit = match kind {
            MediaKind::Image => &limits::IMAGE_MEDIA,
            MediaKind::Document => &limits::DOCUMENT_MEDIA,
            MediaKind::Audio => &limits::AUDIO_MEDIA,
            MediaKind::Video => &limits::VIDEO_MEDIA,
            MediaKind::Sticker => &limits::STICKER_MEDIA,
        };
        if let Some(warning) = validate::media(limit, mime, validate::file_size(path)?)? {
            app.out.warn(warning);
        }
    }
    a.policy.enforce(&a.to)?;

    let phone = app.ctx()?.require_phone_number_id()?.to_owned();
    let source = match (&file, a.id, a.link) {
        (Some(path), _, _) => {
            let mime = mime.as_deref().expect("set whenever there is a file");
            let upload = media_api::upload(&phone, path, mime);
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

    #[test]
    fn a_row_keeps_its_colons_and_takes_an_optional_description() {
        let s = parse_sections(&["Times:a:Today: 10:30,b:Soon::Next day: by 5".into()]).unwrap();
        assert_eq!(s[0].rows[0].title, "Today: 10:30");
        assert_eq!(s[0].rows[0].description, None);
        assert_eq!(s[0].rows[1].title, "Soon");
        assert_eq!(s[0].rows[1].description.as_deref(), Some("Next day: by 5"));
    }

    #[test]
    fn product_sections_parse_and_cap_their_totals() {
        let s = parse_product_sections(&["Top:a,b".into(), r"Sale:c\,d:1".into()]).unwrap();
        assert_eq!(s[0].products, ["a", "b"]);
        assert_eq!(s[1].products, ["c,d:1"]);
        assert!(parse_product_sections(&["Empty:".into()]).is_err());
        assert!(parse_product_sections(&["NoProducts".into()]).is_err());
        let thirty = ["s"; 30].join(",");
        assert!(parse_product_sections(&[format!("A:{thirty}")]).is_ok());
        assert!(parse_product_sections(&[format!("A:{thirty},s")]).is_err());
        let eleven = vec!["A:s".to_owned(); 11];
        assert!(parse_product_sections(&eleven).is_err());
    }

    fn card(action: &Value) -> Value {
        serde_json::json!({
            "header": { "type": "image", "image": { "link": "https://x.test/a.png" } },
            "action": action,
        })
    }

    fn url_action() -> Value {
        serde_json::json!({
            "name": "cta_url",
            "parameters": { "display_text": "Buy", "url": "https://x.test" }
        })
    }

    fn replies(count: usize) -> Value {
        let buttons: Vec<Value> = (0..count)
            .map(|i| {
                serde_json::json!({
                    "type": "quick_reply",
                    "quick_reply": { "id": format!("b{i}"), "title": "Go" }
                })
            })
            .collect();
        serde_json::json!({ "buttons": buttons })
    }

    fn checked(cards: Vec<Value>) -> Result<()> {
        check_media_cards(&messages::media_cards(cards))
    }

    #[test]
    fn media_cards_must_have_matching_buttons() {
        assert!(checked(vec![card(&url_action()), card(&url_action())]).is_ok());
        assert!(checked(vec![card(&replies(2)), card(&replies(2))]).is_ok());
        assert!(checked(vec![card(&replies(2)), card(&replies(1))]).is_err());
        assert!(checked(vec![card(&url_action()), card(&replies(1))]).is_err());
    }

    #[test]
    fn a_media_card_needs_a_linked_image_or_video() {
        let mut by_id = card(&url_action());
        by_id["header"] = serde_json::json!({ "type": "image", "image": { "id": "9" } });
        assert!(checked(vec![by_id, card(&url_action())]).is_err());
        let mut document = card(&url_action());
        document["header"] =
            serde_json::json!({ "type": "document", "document": { "link": "https://x.test" } });
        assert!(checked(vec![document, card(&url_action())]).is_err());
    }

    #[test]
    fn a_media_card_body_has_a_length_and_line_limit() {
        let with_body = |text: &str| {
            let mut c = card(&url_action());
            c["body"] = serde_json::json!({ "text": text });
            c
        };
        assert!(checked(vec![with_body("a\nb\nc"), card(&url_action())]).is_ok());
        assert!(checked(vec![with_body("a\nb\nc\nd"), card(&url_action())]).is_err());
        assert!(checked(vec![with_body(&"x".repeat(160)), card(&url_action())]).is_ok());
        assert!(checked(vec![with_body(&"x".repeat(161)), card(&url_action())]).is_err());
    }

    #[test]
    fn card_indexes_stay_in_range_and_unique() {
        let mut first = card(&url_action());
        first["card_index"] = 1.into();
        assert!(checked(vec![first.clone(), card(&url_action())]).is_err());
        first["card_index"] = 10.into();
        assert!(checked(vec![first, card(&url_action())]).is_err());
    }

    #[test]
    fn an_escaped_comma_stays_inside_the_row() {
        let s = parse_sections(&[r"S:a:A::One\, two,b:B".into()]).unwrap();
        assert_eq!(s[0].rows.len(), 2);
        assert_eq!(s[0].rows[0].description.as_deref(), Some("One, two"));
    }
}
