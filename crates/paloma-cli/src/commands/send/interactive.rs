//! Interactive messages. Each type has its own limits, all checked before any request,
//! because Meta rejects the whole message for one bad row or card.

use super::{
    App, ArgGroup, Args, Button, Error, InteractiveHeader, InteractiveText, MAX_LIST_ROWS,
    MAX_REPLY_BUTTONS, MediaKind, MediaSource, Path, PathBuf, Policy, Reply, Result, Row, Section,
    Subcommand, Value, check_body, check_footer, dispatch, limits, messages, read_json, split_rows,
    validate,
};

#[derive(Debug, Subcommand)]
pub enum Interactive {
    /// Send up to three reply buttons.
    Button(InteractiveButton),
    /// Send a tappable list of sectioned rows.
    List(InteractiveList),
    /// Send a button that opens a URL.
    CtaUrl(InteractiveCta),
    /// Send a button that asks the user to share their location.
    LocationRequest(InteractiveBody),
    /// Send a form that asks for a shipping address (India only).
    Address(InteractiveAddress),
    /// Send 2 to 10 scrollable cards: media from a file, or products from a catalog.
    Carousel(InteractiveCarousel),
    /// Send a button that calls your business; needs calling enabled on the number.
    Call(InteractiveCall),
    /// Send a button that asks the user to share their phone number.
    RequestContact(InteractiveBody),
}

#[derive(Debug, Args)]
pub struct InteractiveCommon {
    /// Recipient (E.164 number or business-scoped user ID).
    pub(super) to: String,
    /// Body text.
    #[arg(long)]
    pub(super) body: String,
    /// Header text.
    #[arg(long)]
    pub(super) header: Option<String>,
    /// Footer text.
    #[arg(long)]
    pub(super) footer: Option<String>,
    #[command(flatten)]
    pub(super) reply: Reply,
    #[command(flatten)]
    pub(super) policy: Policy,
}

impl InteractiveCommon {
    pub(super) fn text(&self) -> InteractiveText {
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
    pub(super) to: String,
    /// Body text.
    #[arg(long)]
    pub(super) body: String,
    #[command(flatten)]
    pub(super) reply: Reply,
    #[command(flatten)]
    pub(super) policy: Policy,
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("address_source").required(true).args(["country", "parameters_file"])))]
pub struct InteractiveAddress {
    #[command(flatten)]
    pub(super) common: InteractiveCommon,
    /// The address form's country, as an ISO code. Meta supports IN.
    #[arg(long, value_name = "CODE")]
    pub(super) country: Option<String>,
    /// JSON file with the action parameters: `country`, plus optional `values`,
    /// `saved_addresses` and `validation_errors`.
    #[arg(long, value_name = "PATH")]
    pub(super) parameters_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("carousel_cards").required(true).args(["card_file", "products"])))]
pub struct InteractiveCarousel {
    #[command(flatten)]
    pub(super) base: InteractiveBody,
    /// JSON file with the media cards: an array, or {"cards": [...]}.
    #[arg(long, value_name = "PATH")]
    pub(super) card_file: Option<PathBuf>,
    /// Product retailer ids (SKUs), one card each, separated by commas.
    #[arg(
        long,
        value_name = "SKU,...",
        value_delimiter = ',',
        requires = "catalog_id"
    )]
    pub(super) products: Vec<String>,
    /// The catalog holding every product.
    #[arg(long, requires = "products")]
    pub(super) catalog_id: Option<String>,
}

#[derive(Debug, Args)]
pub struct InteractiveCall {
    #[command(flatten)]
    pub(super) base: InteractiveBody,
    /// Button label. Meta defaults to "Call Now".
    #[arg(long)]
    pub(super) display: Option<String>,
    /// How long the button lasts, in whole minutes up to 30d, such as 90m or 2d. Meta
    /// defaults to 7 days.
    #[arg(long, value_name = "DURATION", value_parser = humantime::parse_duration)]
    pub(super) ttl: Option<std::time::Duration>,
    /// A string returned in the `calls` webhook as `cta_payload`, for tracking.
    #[arg(long)]
    pub(super) payload: Option<String>,
}

#[derive(Debug, Args)]
pub struct InteractiveButton {
    #[command(flatten)]
    pub(super) common: InteractiveCommon,
    /// A reply button as `id:Title` (or just `Title`); repeat up to three times.
    #[arg(long = "button", value_name = "ID:TITLE", required = true)]
    pub(super) buttons: Vec<String>,
    /// Image for the header: a media id or an http(s) link.
    #[arg(
        long,
        value_name = "ID|URL",
        group = "header_media",
        conflicts_with = "header"
    )]
    pub(super) header_image: Option<String>,
    /// Video for the header: a media id or an http(s) link.
    #[arg(
        long,
        value_name = "ID|URL",
        group = "header_media",
        conflicts_with = "header"
    )]
    pub(super) header_video: Option<String>,
    /// Document for the header: a media id or an http(s) link.
    #[arg(
        long,
        value_name = "ID|URL",
        group = "header_media",
        conflicts_with = "header"
    )]
    pub(super) header_document: Option<String>,
}

#[derive(Debug, Args)]
pub struct InteractiveList {
    #[command(flatten)]
    pub(super) common: InteractiveCommon,
    /// Label of the button that opens the list.
    #[arg(long)]
    pub(super) button: String,
    /// A section as `Title:id:Label::Description,id2:Label2`; repeatable. The description
    /// is optional; write a comma in a row's text as `\,`.
    #[arg(long = "section", value_name = "SECTION", required = true)]
    pub(super) sections: Vec<String>,
}

#[derive(Debug, Args)]
pub struct InteractiveCta {
    #[command(flatten)]
    pub(super) common: InteractiveCommon,
    /// Button label.
    #[arg(long)]
    pub(super) display: String,
    /// URL the button opens.
    #[arg(long)]
    pub(super) url: String,
    /// Image for the header: an http(s) link.
    #[arg(
        long,
        value_name = "URL",
        group = "header_media",
        conflicts_with = "header"
    )]
    pub(super) header_image: Option<String>,
    /// Video for the header: an http(s) link.
    #[arg(
        long,
        value_name = "URL",
        group = "header_media",
        conflicts_with = "header"
    )]
    pub(super) header_video: Option<String>,
    /// Document for the header: an http(s) link.
    #[arg(
        long,
        value_name = "URL",
        group = "header_media",
        conflicts_with = "header"
    )]
    pub(super) header_document: Option<String>,
}

impl Interactive {
    /// The recipient, body, reply and policy flags every kind has.
    pub(super) fn parts(&self) -> (&str, &str, &Reply, Policy) {
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

    pub(super) async fn run(self, app: &App) -> Result<()> {
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
                        messages::check_card_count(a.products.len())?;
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
            Self::Call(a) => call_payload(a, to, body, reply_to)?,
            Self::RequestContact(_) => {
                check_body(body)?;
                messages::interactive_request_contact(to, body, reply_to)
            }
        };
        policy.enforce(to)?;
        dispatch(app, payload).await
    }
}

pub(super) fn check_common(c: &InteractiveCommon) -> Result<()> {
    check_body(&c.body)?;
    if let Some(header) = &c.header {
        validate::max_chars(header, limits::MAX_INTERACTIVE_HEADER, "--header")?;
    }
    check_footer(c.footer.as_deref())
}

/// An address message's parameters, passed through as authored. Meta rejects a message
/// without a `country`.
pub(super) fn address_parameters_file(path: &Path) -> Result<Value> {
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

/// The media cards from a `--card-file`, as authored apart from what
/// [`messages::media_cards`] fills in.
pub(super) fn media_cards_file(path: &Path) -> Result<Vec<Value>> {
    let invalid = || Error::validation("card file must hold a JSON array or {\"cards\": [...]}");
    let cards = match read_json(path)? {
        Value::Array(cards) => cards,
        Value::Object(mut fields) => match fields.remove("cards") {
            Some(Value::Array(cards)) => cards,
            _ => return Err(invalid()),
        },
        _ => return Err(invalid()),
    };
    messages::check_card_count(cards.len())?;
    let cards = messages::media_cards(cards);
    messages::check_media_cards(&cards)?;
    Ok(cards)
}

pub(super) fn interactive_media_header(
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
            let source = messages::media_reference(value, flag)?;
            return Ok(Some(InteractiveHeader::Media(kind, source)));
        }
    }
    Ok(None)
}

/// `id:Title`, or a bare title used as its own id.
pub(super) fn parse_buttons(specs: &[String]) -> Result<Vec<Button>> {
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

/// `id:Label`, or `id:Label::Description`. A single `:` can't start the description, since
/// a label has always been free to contain one. So a label can't hold `::` or start with `:`.
pub(super) fn parse_row(row: &str, section: &str) -> Result<Row> {
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
pub(super) fn parse_sections(specs: &[String]) -> Result<Vec<Section>> {
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

/// The call button, with its optional label, lifetime and tracking payload checked.
fn call_payload(
    a: &InteractiveCall,
    to: &str,
    body: &str,
    reply_to: Option<&str>,
) -> Result<Value> {
    check_body(body)?;
    if let Some(display) = &a.display {
        validate::not_blank(display, "--display")?;
        validate::max_chars(display, limits::MAX_CALL_BUTTON_TEXT, "--display")?;
    }
    let ttl_minutes = a
        .ttl
        .map(|ttl| {
            let minutes = u32::try_from(ttl.as_secs() / 60).ok().filter(|m| {
                ttl.as_secs() % 60 == 0
                    && ttl.subsec_nanos() == 0
                    && limits::CALL_BUTTON_TTL_MINUTES.contains(m)
            });
            minutes.ok_or_else(|| {
                Error::validation(format!(
                    "--ttl must be whole minutes, from {}m to {}m",
                    limits::CALL_BUTTON_TTL_MINUTES.start(),
                    limits::CALL_BUTTON_TTL_MINUTES.end()
                ))
            })
        })
        .transpose()?;
    if let Some(payload) = &a.payload {
        validate::not_blank(payload, "--payload")?;
        validate::max_chars(payload, limits::MAX_CALL_BUTTON_PAYLOAD, "--payload")?;
    }
    let button = messages::CallButton {
        display_text: a.display.as_deref(),
        ttl_minutes,
        payload: a.payload.as_deref(),
    };
    Ok(messages::interactive_call(to, body, &button, reply_to))
}
