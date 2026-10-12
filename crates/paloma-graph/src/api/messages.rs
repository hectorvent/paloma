//! `POST /{phone-number-id}/messages`: one physical endpoint behind every message type.
//!
//! The builders return the full JSON envelope so `--dry-run` shows exactly what would be
//! sent. Optional fields are omitted rather than sent as `null`.

use serde::Serialize;
use serde_json::{Map, Value, json};

use super::MESSAGING_PRODUCT;
use crate::Request;

/// Send any message payload.
pub fn send(phone_number_id: &str, payload: Value) -> Request {
    Request::post(format!("{phone_number_id}/messages")).json(payload)
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

/// The shared envelope: `{messaging_product, recipient_type, to, type, context?, <type>: content}`.
fn envelope(to: &str, kind: &str, reply_to: Option<&str>, content: impl Serialize) -> Value {
    let mut m = Map::new();
    m.insert("messaging_product".into(), MESSAGING_PRODUCT.into());
    m.insert("recipient_type".into(), "individual".into());
    m.insert("to".into(), to.trim().into());
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
            let media = match source {
                MediaSource::Id(id) => json!({ "id": id }),
                MediaSource::Link(link) => json!({ "link": link }),
            };
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

/// The header/body/footer text shared by every interactive message.
#[derive(Debug, Clone, Default)]
pub struct InteractiveText {
    pub header: Option<String>,
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
    if let Some(h) = non_blank(text.header.as_deref()) {
        m.insert("header".into(), json!({ "type": "text", "text": h }));
    }
    m.insert("body".into(), json!({ "text": text.body }));
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
    fn text_envelope_shape_and_order() {
        let v = text("15551234567", "hi", false, Some("wamid.X"));
        assert_eq!(
            serde_json::to_string(&v).unwrap(),
            r#"{"messaging_product":"whatsapp","recipient_type":"individual","to":"15551234567","type":"text","context":{"message_id":"wamid.X"},"text":{"preview_url":false,"body":"hi"}}"#
        );
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
            header: Some(" ".into()),
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
    fn message_id_reads_first_message() {
        assert_eq!(
            message_id(&json!({"messages": [{"id": "wamid.1"}]})).as_deref(),
            Some("wamid.1")
        );
        assert_eq!(message_id(&json!({})), None);
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
