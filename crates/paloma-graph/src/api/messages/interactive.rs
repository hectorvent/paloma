//! Interactive messages: buttons, lists, Flows, carousels and the other `interactive`
//! types, each with its own body shape.

use super::{
    Map, MediaKind, MediaSource, Value, envelope, json, media_object, media_reference, non_blank,
    validate,
};
use paloma_core::{Error, Result, limits};

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

/// The number of cards a carousel can hold, checked before a request.
pub fn check_card_count(count: usize) -> Result<()> {
    if limits::CAROUSEL_CARDS.contains(&count) {
        return Ok(());
    }
    Err(Error::validation(format!(
        "a carousel holds {} to {} cards (got {count})",
        limits::CAROUSEL_CARDS.start(),
        limits::CAROUSEL_CARDS.end()
    )))
}

/// What a card's action has to match on every other card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CardButtons {
    Url,
    QuickReplies(usize),
}

/// Errors name the card by position and never echo its text.
pub fn check_media_cards(cards: &[Value]) -> Result<()> {
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
            if !crate::request::has_http_scheme(&url) || url != url.trim() {
                return Err(invalid("URL button's url must be an http(s) link"));
            }
            Ok(CardButtons::Url)
        }
        _ => Err(invalid(
            "action needs either one URL button or quick-reply buttons",
        )),
    }
}

#[cfg(test)]
mod interactive_kind_tests {
    use super::*;
    use crate::api::messages::interactive_catalog;

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
mod card_tests {
    use super::{check_card_count, check_media_cards, media_cards};
    use crate::api::messages::{MediaSource, media_reference};
    use paloma_core::Result;
    use serde_json::Value;

    fn checked(cards: Vec<Value>) -> Result<()> {
        check_media_cards(&media_cards(cards))
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
    fn a_carousel_holds_two_to_ten_cards() {
        assert!(check_card_count(2).is_ok());
        assert!(check_card_count(10).is_ok());
        assert!(check_card_count(1).is_err());
        assert!(check_card_count(11).is_err());
    }

    #[test]
    fn a_media_reference_is_an_id_or_an_http_link() {
        assert_eq!(
            media_reference(" 123 ", "f").unwrap(),
            MediaSource::Id("123".into())
        );
        assert_eq!(
            media_reference("https://example.com/a.png", "f").unwrap(),
            MediaSource::Link("https://example.com/a.png".into())
        );
        for bad in ["https://", "a b", "a:b", "x/y", " "] {
            assert!(media_reference(bad, "f").is_err(), "{bad}");
        }
    }
}
