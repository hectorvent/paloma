//! Interactive messages: buttons, lists, Flows, carousels and the other `interactive`
//! types, each with its own body shape.

#[allow(clippy::wildcard_imports)] // part of `messages`: shares the envelope and helpers
use super::*;

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

pub(super) fn interactive(
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
pub(super) fn body_only(
    to: &str,
    kind: &str,
    body: &str,
    action: Value,
    reply_to: Option<&str>,
) -> Value {
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
