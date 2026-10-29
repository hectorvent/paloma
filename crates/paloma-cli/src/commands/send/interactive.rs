//! Interactive messages. Each type has its own limits, all checked before any request,
//! because Meta rejects the whole message for one bad row or card.

#[allow(clippy::wildcard_imports)] // part of `send`: shares its imports and helpers
use super::*;

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
        value_name = "SKU,SKU",
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
    /// How long the button lasts, in minutes (1 to 43200). Meta defaults to 7 days.
    #[arg(long, value_name = "MINUTES")]
    pub(super) ttl_minutes: Option<u32>,
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

pub(super) fn check_card_count(count: usize) -> Result<()> {
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
    check_card_count(cards.len())?;
    let cards = messages::media_cards(cards);
    check_media_cards(&cards)?;
    Ok(cards)
}

/// What a card's action has to match on every other card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CardButtons {
    Url,
    QuickReplies(usize),
}

/// Errors name the card by position and never echo its text.
pub(super) fn check_media_cards(cards: &[Value]) -> Result<()> {
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
pub(super) fn check_card_header(card: &Value, position: usize) -> Result<()> {
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

pub(super) fn check_card_action(card: &Value, position: usize) -> Result<CardButtons> {
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
            let source = media_reference(value, flag)?;
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
