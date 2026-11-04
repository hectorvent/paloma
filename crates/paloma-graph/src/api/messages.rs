//! Message envelopes are shared by Cloud API and Marketing Messages API sends.
//!
//! The builders return the full JSON envelope so `--dry-run` shows exactly what would be
//! sent. Optional fields are omitted rather than sent as `null`.
//!
//! Every builder goes through `envelope`, which is where a recipient becomes `to` or
//! `recipient`. A new message type gets business-scoped user IDs by using it.

use paloma_core::validate;

use super::MESSAGING_PRODUCT;
use crate::Request;
use serde::Serialize;
use serde_json::{Map, Value, json};
mod commerce;
mod interactive;
mod media;
mod template;
pub use commerce::*;
pub use interactive::*;
pub use media::*;
pub use template::*;

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

/// Address a built message to a group: its `to` holds the group id.
pub fn for_group(mut payload: Value) -> Value {
    payload["recipient_type"] = "group".into();
    payload
}

pub fn text(to: &str, body: &str, preview_url: bool, reply_to: Option<&str>) -> Value {
    envelope(
        to,
        "text",
        reply_to,
        json!({ "preview_url": preview_url, "body": body }),
    )
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
