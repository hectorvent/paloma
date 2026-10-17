//! Message envelopes are shared by Cloud API and Marketing Messages API sends.
//!
//! The builders return the full JSON envelope so `--dry-run` shows exactly what would be
//! sent. Optional fields are omitted rather than sent as `null`.
//!
//! Every builder goes through [`envelope`], which is where a recipient becomes `to` or
//! `recipient`. A new message type gets business-scoped user IDs by using it.

use paloma_core::validate;
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::MESSAGING_PRODUCT;
use crate::Request;

/// Send any message payload.
pub fn send(phone_number_id: &str, payload: Value) -> Request {
    Request::post(format!("{phone_number_id}/messages")).json(payload)
}

/// Marketing templates use a separate edge; utility and authentication stay on Cloud API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateEndpoint {
    Cloud,
    Marketing,
}

impl TemplateEndpoint {
    pub const fn edge(self) -> &'static str {
        match self {
            Self::Cloud => "messages",
            Self::Marketing => "marketing_messages",
        }
    }

    pub fn for_category(category: &str) -> Option<Self> {
        match category {
            "MARKETING" => Some(Self::Marketing),
            "UTILITY" | "AUTHENTICATION" => Some(Self::Cloud),
            _ => None,
        }
    }
}

pub fn send_template(phone: &str, payload: Value, endpoint: TemplateEndpoint) -> Request {
    Request::post(format!("{phone}/{}", endpoint.edge())).json(payload)
}

/// An offline plan cannot know the category. Never execute this placeholder request.
pub fn template_routing_preview(phone: &str, payload: Value) -> Request {
    Request::post(format!(
        "{phone}/<messages-or-marketing_messages-from-category>"
    ))
    .json(payload)
}

/// Name the account to bill. Only messages are billed, and those carry a `type`; status
/// updates such as read receipts don't. A value already in the payload wins, so a
/// `send raw` payload says what it means. That includes the deprecated alias, which Meta
/// ignores once `messaging_account_id` is present.
pub fn with_messaging_account(mut payload: Value, account: Option<&str>) -> Value {
    if let (Some(account), Some(fields)) = (account, payload.as_object_mut())
        && fields.contains_key("type")
        && !fields.contains_key("paid_messaging_account_id")
    {
        fields
            .entry("messaging_account_id")
            .or_insert_with(|| account.into());
    }
    payload
}

/// The `wamid…` of the sent message.
pub fn message_id(response: &Value) -> Option<String> {
    response
        .pointer("/messages/0/id")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// The shared envelope: `{messaging_product, recipient_type, to | recipient, type, context?,
/// <type>: content}`. A business-scoped user ID goes in `recipient` and a phone number in
/// `to`. One argument can't be both, so the payload never carries the two together, a case
/// where Meta would use `to`.
fn envelope(to: &str, kind: &str, reply_to: Option<&str>, content: impl Serialize) -> Value {
    let mut m = Map::new();
    m.insert("messaging_product".into(), MESSAGING_PRODUCT.into());
    m.insert("recipient_type".into(), "individual".into());
    let to = to.trim();
    let field = if validate::bsuid(to).is_some() {
        "recipient"
    } else {
        "to"
    };
    m.insert(field.into(), to.into());
    m.insert("type".into(), kind.into());
    if let Some(id) = non_blank(reply_to) {
        m.insert("context".into(), json!({ "message_id": id }));
    }
    m.insert(
        kind.into(),
        serde_json::to_value(content).expect("message content serializes"),
    );
    Value::Object(m)
}

pub fn text(to: &str, body: &str, preview_url: bool, reply_to: Option<&str>) -> Value {
    envelope(
        to,
        "text",
        reply_to,
        json!({ "preview_url": preview_url, "body": body }),
    )
}

/// A template send; `body_vars` fill `{{1}}`, `{{2}}`, … of the BODY component in order.
pub fn template(to: &str, name: &str, language: &str, body_vars: &[String]) -> Value {
    let params = TemplateParams {
        body_vars,
        ..TemplateParams::default()
    };
    template_with_components(to, name, language, template_components(&params), None)
}

/// A template's header parameter at send time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateHeader {
    Text(String),
    Media(MediaKind, MediaSource),
}

/// What a template button takes at send time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateButtonKind {
    /// The variable part of a URL, including a one-time password.
    Url,
    QuickReply,
    CopyCode,
    Flow,
    Catalog,
}

/// One template button's send-time parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateButton {
    pub index: u8,
    pub kind: TemplateButtonKind,
    pub value: String,
}

impl TemplateButton {
    /// Meta's examples write `index` as a string for some kinds and an integer for others,
    /// so each kind follows its own documented shape.
    fn to_component(&self) -> Value {
        let (sub_type, index, parameter) = match self.kind {
            TemplateButtonKind::Url => (
                "url",
                self.index.to_string().into(),
                json!({ "type": "text", "text": self.value }),
            ),
            TemplateButtonKind::QuickReply => (
                "quick_reply",
                self.index.to_string().into(),
                json!({ "type": "payload", "payload": self.value }),
            ),
            TemplateButtonKind::CopyCode => (
                "copy_code",
                Value::from(self.index),
                json!({ "type": "coupon_code", "coupon_code": self.value }),
            ),
            TemplateButtonKind::Flow => (
                "flow",
                self.index.to_string().into(),
                json!({ "type": "action", "action": { "flow_token": self.value } }),
            ),
            TemplateButtonKind::Catalog => (
                "CATALOG",
                Value::from(self.index),
                json!({
                    "type": "action",
                    "action": { "thumbnail_product_retailer_id": self.value }
                }),
            ),
        };
        json!({ "type": "button", "sub_type": sub_type, "index": index, "parameters": [parameter] })
    }
}

/// The send-time parameters with typed flags. Anything else goes through
/// [`template_with_components`] as full component objects.
#[derive(Debug, Default)]
pub struct TemplateParams<'a> {
    pub body_vars: &'a [String],
    pub named_vars: &'a [(String, String)],
    pub header: Option<TemplateHeader>,
    pub buttons: &'a [TemplateButton],
}

pub fn template_components(params: &TemplateParams<'_>) -> Vec<Value> {
    let mut components = Vec::new();
    match &params.header {
        Some(TemplateHeader::Text(text)) => components
            .push(json!({ "type": "header", "parameters": [{ "type": "text", "text": text }] })),
        Some(TemplateHeader::Media(kind, source)) => {
            let media = media_object(source);
            let kind = kind.as_str();
            components
                .push(json!({ "type": "header", "parameters": [{ "type": kind, kind: media }] }));
        }
        None => {}
    }
    let parameters: Vec<Value> =
        params
            .body_vars
            .iter()
            .map(|text| json!({ "type": "text", "text": text }))
            .chain(params.named_vars.iter().map(
                |(name, text)| json!({ "type": "text", "parameter_name": name, "text": text }),
            ))
            .collect();
    if !parameters.is_empty() {
        components.push(json!({ "type": "body", "parameters": parameters }));
    }
    components.extend(params.buttons.iter().map(TemplateButton::to_component));
    components
}

/// Preserve caller-supplied send components, omitting the field when none are needed.
pub fn template_with_components(
    to: &str,
    name: &str,
    language: &str,
    components: Vec<Value>,
    reply_to: Option<&str>,
) -> Value {
    let mut content = json!({ "name": name, "language": { "code": language } });
    if !components.is_empty() {
        content["components"] = components.into();
    }
    envelope(to, "template", reply_to, content)
}

/// Media message types that share the `{id | link, caption?, filename?}` shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Document,
    Audio,
    Video,
    Sticker,
}

impl MediaKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Document => "document",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::Sticker => "sticker",
        }
    }

    pub const fn supports_caption(self) -> bool {
        matches!(self, Self::Image | Self::Video | Self::Document)
    }

    pub const fn supports_filename(self) -> bool {
        matches!(self, Self::Document)
    }

    pub const fn supports_voice(self) -> bool {
        matches!(self, Self::Audio)
    }
}

/// Where the media comes from: an uploaded media id or a public HTTPS link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaSource {
    Id(String),
    Link(String),
}

fn media_object(source: &MediaSource) -> Value {
    match source {
        MediaSource::Id(id) => json!({ "id": id }),
        MediaSource::Link(link) => json!({ "link": link }),
    }
}

/// The optional parts of a media message. A kind that can't use one drops it.
#[derive(Debug, Clone, Copy, Default)]
pub struct MediaOptions<'a> {
    pub caption: Option<&'a str>,
    pub filename: Option<&'a str>,
    pub voice: bool,
    pub reply_to: Option<&'a str>,
}

pub fn media(to: &str, kind: MediaKind, source: &MediaSource, options: &MediaOptions<'_>) -> Value {
    #[derive(Serialize)]
    struct Media<'a> {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        link: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        caption: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        filename: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        voice: Option<bool>,
    }
    let (id, link) = match source {
        MediaSource::Id(id) => (Some(id.as_str()), None),
        MediaSource::Link(l) => (None, Some(l.as_str())),
    };
    let m = Media {
        id,
        link,
        caption: non_blank(options.caption).filter(|_| kind.supports_caption()),
        filename: non_blank(options.filename).filter(|_| kind.supports_filename()),
        voice: (options.voice && kind.supports_voice()).then_some(true),
    };
    envelope(to, kind.as_str(), options.reply_to, m)
}

pub fn location(
    to: &str,
    latitude: f64,
    longitude: f64,
    name: Option<&str>,
    address: Option<&str>,
    reply_to: Option<&str>,
) -> Value {
    #[derive(Serialize)]
    struct Location<'a> {
        latitude: f64,
        longitude: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        address: Option<&'a str>,
    }
    envelope(
        to,
        "location",
        reply_to,
        Location {
            latitude,
            longitude,
            name: non_blank(name),
            address: non_blank(address),
        },
    )
}

/// `contacts` is the Cloud API contacts array, passed through as authored.
pub fn contacts(to: &str, contacts: Value, reply_to: Option<&str>) -> Value {
    envelope(to, "contacts", reply_to, contacts)
}

/// One contact from typed fields. Meta requires only `formatted_name`, the name shown on
/// the message.
#[derive(Debug, Clone, Copy, Default)]
pub struct ContactCard<'a> {
    pub formatted_name: &'a str,
    pub first_name: Option<&'a str>,
    pub last_name: Option<&'a str>,
    pub phones: &'a [String],
    /// Goes on the first phone. Without it, the message shows an Invite to WhatsApp button
    /// rather than the Message and Save contact buttons.
    pub wa_id: Option<&'a str>,
    pub emails: &'a [String],
}

/// The Cloud API contacts array holding one card, for [`contacts`]. Empty parts are
/// omitted.
pub fn contact_card(card: &ContactCard) -> Value {
    let mut name = Map::new();
    name.insert("formatted_name".into(), card.formatted_name.into());
    if let Some(first) = non_blank(card.first_name) {
        name.insert("first_name".into(), first.into());
    }
    if let Some(last) = non_blank(card.last_name) {
        name.insert("last_name".into(), last.into());
    }
    let mut contact = Map::new();
    contact.insert("name".into(), name.into());
    if !card.phones.is_empty() {
        let phones = card.phones.iter().enumerate().map(|(i, phone)| {
            match non_blank(card.wa_id).filter(|_| i == 0) {
                Some(wa_id) => json!({ "phone": phone, "wa_id": wa_id }),
                None => json!({ "phone": phone }),
            }
        });
        contact.insert("phones".into(), phones.collect());
    }
    if !card.emails.is_empty() {
        let emails = card.emails.iter().map(|email| json!({ "email": email }));
        contact.insert("emails".into(), emails.collect());
    }
    json!([contact])
}

/// React to a message; an empty emoji removes the reaction. Meta doesn't allow a reaction
/// to be a contextual reply, so this one takes no `reply_to`.
pub fn reaction(to: &str, message_id: &str, emoji: &str) -> Value {
    envelope(
        to,
        "reaction",
        None,
        json!({ "message_id": message_id, "emoji": emoji }),
    )
}

/// An interactive message's header. A list message can only use `Text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractiveHeader {
    Text(String),
    Media(MediaKind, MediaSource),
}

/// The header, body and footer shared by every interactive message. Only the kinds where
/// Meta makes the body optional may leave it blank, which omits it.
#[derive(Debug, Clone, Default)]
pub struct InteractiveText {
    pub header: Option<InteractiveHeader>,
    pub body: String,
    pub footer: Option<String>,
}

/// A reply button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    pub id: String,
    pub title: String,
}

/// A list row; `description` is optional.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
}

/// A titled section of rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub title: String,
    pub rows: Vec<Row>,
}

fn interactive(
    to: &str,
    kind: &str,
    text: &InteractiveText,
    action: Value,
    reply_to: Option<&str>,
) -> Value {
    let mut m = Map::new();
    m.insert("type".into(), kind.into());
    match &text.header {
        Some(InteractiveHeader::Text(h)) if !h.trim().is_empty() => {
            m.insert("header".into(), json!({ "type": "text", "text": h }));
        }
        Some(InteractiveHeader::Media(kind, source)) => {
            let kind = kind.as_str();
            m.insert(
                "header".into(),
                json!({ "type": kind, kind: media_object(source) }),
            );
        }
        _ => {}
    }
    if non_blank(Some(&text.body)).is_some() {
        m.insert("body".into(), json!({ "text": text.body }));
    }
    if let Some(f) = non_blank(text.footer.as_deref()) {
        m.insert("footer".into(), json!({ "text": f }));
    }
    m.insert("action".into(), action);
    envelope(to, "interactive", reply_to, Value::Object(m))
}

pub fn interactive_buttons(
    to: &str,
    text: &InteractiveText,
    buttons: &[Button],
    reply_to: Option<&str>,
) -> Value {
    let buttons: Vec<Value> = buttons
        .iter()
        .map(|b| json!({ "type": "reply", "reply": { "id": b.id, "title": b.title } }))
        .collect();
    interactive(to, "button", text, json!({ "buttons": buttons }), reply_to)
}

pub fn interactive_list(
    to: &str,
    text: &InteractiveText,
    button: &str,
    sections: &[Section],
    reply_to: Option<&str>,
) -> Value {
    let sections: Vec<Value> = sections
        .iter()
        .map(|s| {
            let rows: Vec<Value> = s
                .rows
                .iter()
                .map(|r| {
                    let mut row = json!({ "id": r.id, "title": r.title });
                    if let Some(d) = non_blank(r.description.as_deref()) {
                        row["description"] = d.into();
                    }
                    row
                })
                .collect();
            json!({ "title": s.title, "rows": rows })
        })
        .collect();
    interactive(
        to,
        "list",
        text,
        json!({ "button": button, "sections": sections }),
        reply_to,
    )
}

pub fn interactive_cta_url(
    to: &str,
    text: &InteractiveText,
    display_text: &str,
    url: &str,
    reply_to: Option<&str>,
) -> Value {
    interactive(
        to,
        "cta_url",
        text,
        json!({ "name": "cta_url", "parameters": { "display_text": display_text, "url": url } }),
        reply_to,
    )
}

/// Which Flow to open: Meta takes one or the other, never both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowRef<'a> {
    Id(&'a str),
    Name(&'a str),
}

/// A Flow message's action parameters. Anything unset is left to Meta's default.
#[derive(Debug, Clone, Copy)]
pub struct FlowParams<'a> {
    pub flow: FlowRef<'a>,
    pub cta: &'a str,
    pub token: Option<&'a str>,
    pub action: Option<&'a str>,
    pub mode: Option<&'a str>,
    pub screen: Option<&'a str>,
    pub data: Option<&'a Value>,
}

/// Meta's Flows page has `data` as a JSON object serialized to a string, though its OpenAPI
/// schema calls it an object. The page wins, so a caller passes the object and it is
/// serialized here.
pub fn interactive_flow(
    to: &str,
    text: &InteractiveText,
    params: &FlowParams<'_>,
    reply_to: Option<&str>,
) -> Value {
    let mut parameters = Map::new();
    parameters.insert("flow_message_version".into(), "3".into());
    match params.flow {
        FlowRef::Id(id) => parameters.insert("flow_id".into(), id.into()),
        FlowRef::Name(name) => parameters.insert("flow_name".into(), name.into()),
    };
    parameters.insert("flow_cta".into(), params.cta.into());
    let optional = [
        ("flow_token", params.token),
        ("flow_action", params.action),
        ("mode", params.mode),
    ];
    for (key, value) in optional {
        if let Some(value) = value {
            parameters.insert(key.into(), value.into());
        }
    }
    let mut payload = Map::new();
    if let Some(screen) = params.screen {
        payload.insert("screen".into(), screen.into());
    }
    if let Some(data) = params.data {
        payload.insert("data".into(), data.to_string().into());
    }
    if !payload.is_empty() {
        parameters.insert("flow_action_payload".into(), payload.into());
    }
    interactive(
        to,
        "flow",
        text,
        json!({ "name": "flow", "parameters": parameters }),
        reply_to,
    )
}

/// A kind that carries nothing but body text and a fixed action.
fn body_only(to: &str, kind: &str, body: &str, action: Value, reply_to: Option<&str>) -> Value {
    let text = InteractiveText {
        body: body.to_owned(),
        ..InteractiveText::default()
    };
    interactive(to, kind, &text, action, reply_to)
}

pub fn interactive_location_request(to: &str, body: &str, reply_to: Option<&str>) -> Value {
    body_only(
        to,
        "location_request_message",
        body,
        json!({ "name": "send_location" }),
        reply_to,
    )
}

pub fn interactive_request_contact(to: &str, body: &str, reply_to: Option<&str>) -> Value {
    body_only(
        to,
        "request_contact_info",
        body,
        json!({ "name": "request_contact_info" }),
        reply_to,
    )
}

/// `parameters` holds a `country`, and optionally `values`, `saved_addresses` and
/// `validation_errors`.
pub fn interactive_address(
    to: &str,
    text: &InteractiveText,
    parameters: &Value,
    reply_to: Option<&str>,
) -> Value {
    interactive(
        to,
        "address_message",
        text,
        json!({ "name": "address_message", "parameters": parameters }),
        reply_to,
    )
}

/// The action parameters of an address message that only names its country.
pub fn address_parameters(country: &str) -> Value {
    json!({ "country": country })
}

/// What a call button can set. Meta has a default for each, and `parameters` is left out
/// when none is given.
#[derive(Debug, Clone, Copy, Default)]
pub struct CallButton<'a> {
    pub display_text: Option<&'a str>,
    pub ttl_minutes: Option<u32>,
    pub payload: Option<&'a str>,
}

pub fn interactive_call(
    to: &str,
    body: &str,
    button: &CallButton<'_>,
    reply_to: Option<&str>,
) -> Value {
    let mut parameters = Map::new();
    if let Some(text) = button.display_text {
        parameters.insert("display_text".into(), text.into());
    }
    if let Some(ttl) = button.ttl_minutes {
        parameters.insert("ttl_minutes".into(), ttl.into());
    }
    if let Some(payload) = button.payload {
        parameters.insert("payload".into(), payload.into());
    }
    let mut action = json!({ "name": "voice_call" });
    if !parameters.is_empty() {
        action["parameters"] = parameters.into();
    }
    body_only(to, "voice_call", body, action, reply_to)
}

/// The body is optional, and Meta doesn't allow a header or footer here.
pub fn call_permission_request(to: &str, body: Option<&str>, reply_to: Option<&str>) -> Value {
    body_only(
        to,
        "call_permission_request",
        body.unwrap_or_default(),
        json!({ "name": "call_permission_request" }),
        reply_to,
    )
}

/// Media cards as authored, with what Meta requires filled in where the author left it
/// out: `card_index` from the card's position, and `type`, which Meta's page sets to
/// `cta_url` even on a card with quick-reply buttons.
pub fn media_cards(mut cards: Vec<Value>) -> Vec<Value> {
    for (index, card) in cards.iter_mut().enumerate() {
        if let Some(fields) = card.as_object_mut() {
            fields.entry("card_index").or_insert_with(|| index.into());
            fields.entry("type").or_insert_with(|| "cta_url".into());
        }
    }
    cards
}

/// One card per product. Meta requires every card to use the same catalog, so there's one
/// catalog id.
pub fn product_cards(catalog_id: &str, products: &[String]) -> Vec<Value> {
    products
        .iter()
        .enumerate()
        .map(|(index, product)| {
            json!({
                "card_index": index,
                "type": "product",
                "action": { "product_retailer_id": product, "catalog_id": catalog_id }
            })
        })
        .collect()
}

/// The same message type carries [`media_cards`] and [`product_cards`].
pub fn interactive_carousel(
    to: &str,
    body: &str,
    cards: &[Value],
    reply_to: Option<&str>,
) -> Value {
    body_only(to, "carousel", body, json!({ "cards": cards }), reply_to)
}

/// One catalog product. Meta doesn't allow a header on this kind, and the body is optional.
pub fn interactive_product(
    to: &str,
    catalog_id: &str,
    product: &str,
    body: Option<&str>,
    footer: Option<&str>,
    reply_to: Option<&str>,
) -> Value {
    let text = InteractiveText {
        header: None,
        body: body.unwrap_or_default().to_owned(),
        footer: footer.map(str::to_owned),
    };
    interactive(
        to,
        "product",
        &text,
        json!({ "catalog_id": catalog_id, "product_retailer_id": product }),
        reply_to,
    )
}

/// A titled section of a multi-product message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductSection {
    pub title: String,
    pub products: Vec<String>,
}

pub fn interactive_product_list(
    to: &str,
    text: &InteractiveText,
    catalog_id: &str,
    sections: &[ProductSection],
    reply_to: Option<&str>,
) -> Value {
    let sections: Vec<Value> = sections
        .iter()
        .map(|section| {
            let items: Vec<Value> = section
                .products
                .iter()
                .map(|product| json!({ "product_retailer_id": product }))
                .collect();
            json!({ "title": section.title, "product_items": items })
        })
        .collect();
    interactive(
        to,
        "product_list",
        text,
        json!({ "catalog_id": catalog_id, "sections": sections }),
        reply_to,
    )
}

/// `thumbnail` is the product whose image heads the message. Without it, `parameters` is
/// left out and Meta uses the first item in the catalog.
pub fn interactive_catalog(
    to: &str,
    body: &str,
    footer: Option<&str>,
    thumbnail: Option<&str>,
    reply_to: Option<&str>,
) -> Value {
    let text = InteractiveText {
        header: None,
        body: body.to_owned(),
        footer: footer.map(str::to_owned),
    };
    let mut action = json!({ "name": "catalog_message" });
    if let Some(thumbnail) = thumbnail {
        action["parameters"] = json!({ "thumbnail_product_retailer_id": thumbnail });
    }
    interactive(to, "catalog_message", &text, action, reply_to)
}

/// Mark an inbound message as read.
pub fn mark_read(message_id: &str) -> Value {
    json!({ "messaging_product": MESSAGING_PRODUCT, "status": "read", "message_id": message_id })
}

/// Mark an inbound message as read and show a typing indicator (lasts up to 25 s or
/// until the next message).
pub fn typing(message_id: &str) -> Value {
    json!({
        "messaging_product": MESSAGING_PRODUCT,
        "status": "read",
        "message_id": message_id,
        "typing_indicator": { "type": "text" }
    })
}

fn non_blank(s: Option<&str>) -> Option<&str> {
    s.filter(|v| !v.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contact_card_sends_only_what_was_given() {
        let bare = ContactCard {
            formatted_name: "Ana",
            ..ContactCard::default()
        };
        assert_eq!(
            contact_card(&bare),
            json!([{ "name": { "formatted_name": "Ana" } }])
        );
        let phones = ["+1650".to_owned(), "+1917".to_owned()];
        let emails = ["a@b.test".to_owned()];
        let full = ContactCard {
            formatted_name: "Ana Diaz",
            first_name: Some("Ana"),
            last_name: Some("Diaz"),
            phones: &phones,
            wa_id: Some("1650"),
            emails: &emails,
        };
        assert_eq!(
            contact_card(&full),
            json!([{
                "name": { "formatted_name": "Ana Diaz", "first_name": "Ana", "last_name": "Diaz" },
                "phones": [{ "phone": "+1650", "wa_id": "1650" }, { "phone": "+1917" }],
                "emails": [{ "email": "a@b.test" }],
            }])
        );
    }

    #[test]
    fn text_envelope_shape_and_order() {
        let v = text("15551234567", "hi", false, Some("wamid.X"));
        assert_eq!(
            serde_json::to_string(&v).unwrap(),
            r#"{"messaging_product":"whatsapp","recipient_type":"individual","to":"15551234567","type":"text","context":{"message_id":"wamid.X"},"text":{"preview_url":false,"body":"hi"}}"#
        );
    }

    #[test]
    fn a_bsuid_goes_in_recipient_and_a_number_in_to() {
        for bsuid in ["US.13491208655302741918", "US.ENT.11815799212886844830"] {
            let v = text(&format!(" {bsuid} "), "hi", false, None);
            assert_eq!(
                serde_json::to_string(&v).unwrap(),
                format!(
                    r#"{{"messaging_product":"whatsapp","recipient_type":"individual","recipient":"{bsuid}","type":"text","text":{{"preview_url":false,"body":"hi"}}}}"#
                )
            );
        }
        let v = text("+15551234567", "hi", false, None);
        assert_eq!(v["to"], "+15551234567");
        assert!(v.get("recipient").is_none());
    }

    #[test]
    fn template_without_vars_omits_components() {
        let v = template("1", "hello_world", "en_US", &[]);
        assert!(v["template"].get("components").is_none());
        let v = template("1", "order", "en", &["A".into(), "B".into()]);
        assert_eq!(v["template"]["components"][0]["parameters"][1]["text"], "B");
    }

    #[test]
    fn media_drops_unsupported_caption_and_filename() {
        let v = media(
            "1",
            MediaKind::Audio,
            &MediaSource::Link("https://x/a.ogg".into()),
            &MediaOptions {
                caption: Some("c"),
                filename: Some("f"),
                ..MediaOptions::default()
            },
        );
        assert_eq!(v["audio"], json!({ "link": "https://x/a.ogg" }));
        let v = media(
            "1",
            MediaKind::Document,
            &MediaSource::Id("9".into()),
            &MediaOptions {
                caption: Some("c"),
                filename: Some("f.pdf"),
                ..MediaOptions::default()
            },
        );
        assert_eq!(
            v["document"],
            json!({ "id": "9", "caption": "c", "filename": "f.pdf" })
        );
    }

    #[test]
    fn voice_is_set_only_on_audio_and_only_when_asked() {
        let source = MediaSource::Id("9".into());
        let voice = MediaOptions {
            voice: true,
            ..MediaOptions::default()
        };
        let v = media("1", MediaKind::Audio, &source, &voice);
        assert_eq!(v["audio"], json!({ "id": "9", "voice": true }));
        let v = media("1", MediaKind::Audio, &source, &MediaOptions::default());
        assert_eq!(v["audio"], json!({ "id": "9" }));
        let v = media("1", MediaKind::Video, &source, &voice);
        assert_eq!(v["video"], json!({ "id": "9" }));
    }

    #[test]
    fn interactive_omits_blank_header_footer() {
        let t = InteractiveText {
            header: Some(InteractiveHeader::Text(" ".into())),
            body: "Pick".into(),
            footer: None,
        };
        let v = interactive_buttons(
            "1",
            &t,
            &[Button {
                id: "y".into(),
                title: "Yes".into(),
            }],
            None,
        );
        assert!(v["interactive"].get("header").is_none());
        assert_eq!(
            v["interactive"]["action"]["buttons"][0]["reply"]["title"],
            "Yes"
        );
    }

    #[test]
    fn interactive_media_header_uses_the_kind_as_its_key() {
        let t = InteractiveText {
            header: Some(InteractiveHeader::Media(
                MediaKind::Document,
                MediaSource::Link("https://x.test/a.pdf".into()),
            )),
            body: "Pick".into(),
            footer: None,
        };
        let v = interactive_cta_url("1", &t, "Open", "https://x.test", None);
        assert_eq!(
            v["interactive"]["header"],
            json!({ "type": "document", "document": { "link": "https://x.test/a.pdf" } })
        );
    }

    #[test]
    fn message_id_reads_first_message() {
        assert_eq!(
            message_id(&json!({"messages": [{"id": "wamid.1"}]})).as_deref(),
            Some("wamid.1")
        );
        assert_eq!(message_id(&json!({})), None);
    }
}

#[cfg(test)]
mod interactive_kind_tests {
    use super::*;

    #[test]
    fn a_flow_sends_only_what_was_given_and_serializes_its_data() {
        let text = InteractiveText {
            body: "Book".into(),
            ..InteractiveText::default()
        };
        let bare = FlowParams {
            flow: FlowRef::Name("booking"),
            cta: "Open",
            token: None,
            action: None,
            mode: None,
            screen: None,
            data: None,
        };
        assert_eq!(
            interactive_flow("1", &text, &bare, None)["interactive"]["action"],
            json!({
                "name": "flow",
                "parameters": {
                    "flow_message_version": "3",
                    "flow_name": "booking",
                    "flow_cta": "Open",
                }
            })
        );
        let data = json!({ "plan": "gold" });
        let full = FlowParams {
            flow: FlowRef::Id("42"),
            token: Some("t-1"),
            action: Some("navigate"),
            mode: Some("draft"),
            screen: Some("START"),
            data: Some(&data),
            ..bare
        };
        assert_eq!(
            interactive_flow("1", &text, &full, None)["interactive"]["action"]["parameters"],
            json!({
                "flow_message_version": "3",
                "flow_id": "42",
                "flow_cta": "Open",
                "flow_token": "t-1",
                "flow_action": "navigate",
                "mode": "draft",
                "flow_action_payload": { "screen": "START", "data": "{\"plan\":\"gold\"}" },
            })
        );
    }

    #[test]
    fn a_call_button_leaves_out_empty_parameters() {
        let v = interactive_call("1", "Call", &CallButton::default(), None);
        assert_eq!(v["interactive"]["action"], json!({ "name": "voice_call" }));
    }

    #[test]
    fn a_call_permission_request_can_omit_its_body() {
        let v = call_permission_request("1", None, None);
        assert_eq!(
            v["interactive"],
            json!({
                "type": "call_permission_request",
                "action": { "name": "call_permission_request" },
            })
        );
    }

    #[test]
    fn media_cards_fill_only_what_the_author_left_out() {
        let cards = media_cards(vec![json!({}), json!({ "card_index": 7, "type": "x" })]);
        assert_eq!(cards[0], json!({ "card_index": 0, "type": "cta_url" }));
        assert_eq!(cards[1], json!({ "card_index": 7, "type": "x" }));
    }

    #[test]
    fn catalog_leaves_out_parameters_without_a_thumbnail() {
        let v = interactive_catalog("1", "Browse", None, None, None);
        assert_eq!(
            v["interactive"]["action"],
            json!({ "name": "catalog_message" })
        );
    }
}

#[cfg(test)]
mod template_component_tests {
    use super::*;

    #[test]
    fn header_only_template_omits_body_and_empty_components_are_omitted() {
        let components = template_components(&TemplateParams {
            header: Some(TemplateHeader::Text("Order update".into())),
            ..TemplateParams::default()
        });
        let payload = template_with_components("15551234567", "order", "en_US", components, None);
        assert_eq!(
            payload["template"]["components"],
            json!([
                {"type":"header", "parameters":[{"type":"text", "text":"Order update"}]}
            ])
        );
        let payload = template_with_components("15551234567", "hello", "en_US", vec![], None);
        assert!(payload["template"].get("components").is_none());
    }
}
