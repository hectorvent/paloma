//! Sample webhook deliveries for `paloma webhook simulate`.
//!
//! The shapes follow Meta's webhook reference pages. An endpoint is most likely to break on
//! what's newest: users with no phone number in the payload, the system message for a
//! changed BSUID, and the new pricing values. Each delivery gets its own message id, because
//! Meta retries webhooks and tells endpoints to deduplicate, so a repeated id could be
//! dropped and look like a passing test.

use serde_json::{Value, json};

/// The names `--event` takes.
pub const SAMPLE_EVENTS: [&str; 9] = [
    "text",
    "text-username",
    "status-delivered",
    "status-failed",
    "status-paid-exempt",
    "status-marketing-lite-dynamic",
    "user-changed-number",
    "user-changed-user-id",
    "template-status",
];

const WA_ID: &str = "16505551234";
const NEW_WA_ID: &str = "12195555358";
const BSUID: &str = "US.13491208655302741918";
const PREVIOUS_BSUID: &str = "US.10230984751029384756";
const PARENT_BSUID: &str = "US.ENT.11815799212886844830";
const PREVIOUS_PARENT_BSUID: &str = "US.ENT.918273645500112";

/// The parts of a sample that differ per delivery or per account.
#[derive(Debug, Clone)]
pub struct SampleContext {
    pub waba_id: String,
    pub phone_number_id: String,
    /// Unix seconds.
    pub timestamp: u64,
    /// Makes the message id unique.
    pub nonce: String,
}

/// Meta's example ids, for samples when no WABA or number is configured.
pub const EXAMPLE_WABA_ID: &str = "102290129340398";
pub const EXAMPLE_PHONE_NUMBER_ID: &str = "106540352242922";

impl SampleContext {
    fn message_id(&self) -> String {
        format!("wamid.PALOMASIMULATED{}", self.nonce)
    }

    fn timestamp(&self) -> String {
        self.timestamp.to_string()
    }

    /// The `messages` field envelope.
    fn messages_delivery(&self, value: Value) -> Value {
        let mut merged = json!({
            "messaging_product": "whatsapp",
            "metadata": {
                "display_phone_number": "15550783881",
                "phone_number_id": self.phone_number_id,
            },
        });
        if let (Some(merged), Value::Object(value)) = (merged.as_object_mut(), value) {
            merged.extend(value);
        }
        self.delivery("messages", &merged)
    }

    fn delivery(&self, field: &str, value: &Value) -> Value {
        json!({
            "object": "whatsapp_business_account",
            "entry": [{
                "id": self.waba_id,
                "changes": [{ "value": value, "field": field }],
            }],
        })
    }

    fn status(&self, status: &str, extra: &Value) -> Value {
        let mut item = json!({
            "id": self.message_id(),
            "status": status,
            "timestamp": self.timestamp(),
            "recipient_id": WA_ID,
        });
        if let (Some(item), Some(extra)) = (item.as_object_mut(), extra.as_object()) {
            item.extend(extra.clone());
        }
        item
    }

    /// The `contacts` a sent, delivered or read status carries, and its recipient BSUID.
    fn delivered(&self, pricing: &Value) -> Value {
        self.messages_delivery(json!({
            "contacts": [{
                "profile": { "name": "Sheena Nelson" },
                "wa_id": WA_ID,
                "user_id": BSUID,
            }],
            "statuses": [self.status("delivered", &json!({
                "recipient_user_id": BSUID,
                "pricing": pricing,
            }))],
        }))
    }
}

/// The delivery for a name in [`SAMPLE_EVENTS`].
#[allow(clippy::too_many_lines)] // one sample per arm, kept beside the others so they stay alike
pub fn payload(event: &str, cx: &SampleContext) -> Option<Value> {
    let text = json!({ "body": "Does it come in another color?" });
    Some(match event {
        // A user with no username: phone number and BSUID side by side.
        "text" => cx.messages_delivery(json!({
            "contacts": [{
                "profile": { "name": "Sheena Nelson" },
                "wa_id": WA_ID,
                "user_id": BSUID,
            }],
            "messages": [{
                "from": WA_ID,
                "from_user_id": BSUID,
                "id": cx.message_id(),
                "timestamp": cx.timestamp(),
                "type": "text",
                "text": text,
            }],
        })),
        // A username user whose number can't be shared: `wa_id` and `from` are absent.
        "text-username" => cx.messages_delivery(json!({
            "contacts": [{
                "profile": { "name": "Sheena Nelson", "username": "realsheenanelson" },
                "user_id": BSUID,
                "parent_user_id": PARENT_BSUID,
            }],
            "messages": [{
                "from_user_id": BSUID,
                "from_parent_user_id": PARENT_BSUID,
                "id": cx.message_id(),
                "timestamp": cx.timestamp(),
                "type": "text",
                "text": text,
            }],
        })),
        "status-delivered" => cx.delivered(&json!({
            "billable": true,
            "pricing_model": "PMP",
            "type": "regular",
            "category": "marketing",
        })),
        // Failed statuses have no `contacts`, and no BSUID when sent to a phone number.
        "status-failed" => cx.messages_delivery(json!({
            "statuses": [cx.status("failed", &json!({
                "errors": [{
                    "code": 131_049,
                    "title": "This message was not delivered to maintain healthy ecosystem engagement.",
                    "message": "This message was not delivered to maintain healthy ecosystem engagement.",
                    "error_data": {
                        "details": "In order to maintain a healthy ecosystem engagement, the message failed to be delivered.",
                    },
                    "href": "/documentation/business-messaging/whatsapp/support/error-codes",
                }],
            }))],
        })),
        // A free service message from an eligible government or non-profit.
        "status-paid-exempt" => cx.delivered(&json!({
            "billable": false,
            "pricing_model": "PMP",
            "category": "service",
            "type": "free_customer_service",
            "subtype": "paid_exempt",
        })),
        // A marketing message sent with a max price.
        "status-marketing-lite-dynamic" => cx.delivered(&json!({
            "billable": true,
            "pricing_model": "PMP",
            "category": "marketing_lite_dynamic",
            "type": "regular",
            "cost": { "amount": 0.035, "currency": "USD" },
        })),
        // System messages have no `contacts`.
        "user-changed-number" => cx.messages_delivery(json!({
            "messages": [{
                "from": WA_ID,
                "id": cx.message_id(),
                "timestamp": cx.timestamp(),
                "system": {
                    "body": format!("User Sheena Nelson changed from {WA_ID} to {NEW_WA_ID}"),
                    "wa_id": NEW_WA_ID,
                    "type": "user_changed_number",
                    "user_id": BSUID,
                    "parent_user_id": PARENT_BSUID,
                    "previous_user_id": PREVIOUS_BSUID,
                    "previous_parent_user_id": PREVIOUS_PARENT_BSUID,
                },
                "type": "system",
            }],
        })),
        // No phone number can be shared, so there's no `from` or `wa_id`.
        "user-changed-user-id" => cx.messages_delivery(json!({
            "messages": [{
                "id": cx.message_id(),
                "timestamp": cx.timestamp(),
                "system": {
                    "body": format!("User changed from {PREVIOUS_BSUID} to {BSUID}"),
                    "type": "user_changed_user_id",
                    "user_id": BSUID,
                    "parent_user_id": PARENT_BSUID,
                    "previous_user_id": PREVIOUS_BSUID,
                    "previous_parent_user_id": PREVIOUS_PARENT_BSUID,
                },
                "type": "system",
            }],
        })),
        "template-status" => cx.delivery(
            "message_template_status_update",
            &json!({
                "event": "APPROVED",
                "message_template_id": 1_689_556_908_129_832_u64,
                "message_template_name": "order_confirmation",
                "message_template_language": "en-US",
                "reason": "NONE",
                "message_template_category": "UTILITY",
            }),
        ),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cx() -> SampleContext {
        SampleContext {
            waba_id: "W".into(),
            phone_number_id: "P".into(),
            timestamp: 1_750_000_000,
            nonce: "abc".into(),
        }
    }

    fn value(event: &str) -> Value {
        let delivery = payload(event, &cx()).unwrap();
        assert_eq!(delivery["object"], "whatsapp_business_account");
        assert_eq!(delivery["entry"][0]["id"], "W");
        delivery["entry"][0]["changes"][0]["value"].clone()
    }

    #[test]
    fn every_listed_event_has_a_payload() {
        for event in SAMPLE_EVENTS {
            assert!(payload(event, &cx()).is_some(), "{event}");
        }
        assert!(payload("nope", &cx()).is_none());
    }

    #[test]
    fn a_username_user_has_a_bsuid_and_no_phone_number() {
        let v = value("text-username");
        let (contact, message) = (&v["contacts"][0], &v["messages"][0]);
        assert_eq!(contact["user_id"], BSUID);
        assert_eq!(contact["profile"]["username"], "realsheenanelson");
        assert_eq!(message["from_user_id"], BSUID);
        assert!(contact.get("wa_id").is_none());
        assert!(message.get("from").is_none());
        assert_eq!(v["metadata"]["phone_number_id"], "P");
    }

    #[test]
    fn a_bsuid_change_names_both_ids_and_no_phone_number() {
        let v = value("user-changed-user-id");
        let message = &v["messages"][0];
        assert!(v.get("contacts").is_none());
        assert!(message.get("from").is_none());
        assert_eq!(message["type"], "system");
        let system = &message["system"];
        assert_eq!(system["type"], "user_changed_user_id");
        assert_eq!(system["user_id"], BSUID);
        assert_eq!(system["previous_user_id"], PREVIOUS_BSUID);
        assert!(system.get("wa_id").is_none());
    }

    #[test]
    fn pricing_samples_carry_the_new_values() {
        let exempt = value("status-paid-exempt");
        let pricing = &exempt["statuses"][0]["pricing"];
        assert_eq!(pricing["subtype"], "paid_exempt");
        assert_eq!(pricing["type"], "free_customer_service");
        assert_eq!(pricing["billable"], false);
        let dynamic = value("status-marketing-lite-dynamic");
        let pricing = &dynamic["statuses"][0]["pricing"];
        assert_eq!(pricing["category"], "marketing_lite_dynamic");
        assert_eq!(pricing["cost"]["currency"], "USD");
    }

    #[test]
    fn a_failed_status_has_no_contacts() {
        let v = value("status-failed");
        assert!(v.get("contacts").is_none());
        assert_eq!(v["statuses"][0]["errors"][0]["code"], 131_049);
        assert!(v["statuses"][0].get("recipient_user_id").is_none());
    }

    #[test]
    fn message_ids_differ_between_deliveries() {
        let other = SampleContext {
            nonce: "def".into(),
            ..cx()
        };
        let id = |c: &SampleContext| {
            payload("text", c).unwrap()["entry"][0]["changes"][0]["value"]["messages"][0]["id"]
                .clone()
        };
        assert_ne!(id(&cx()), id(&other));
    }

    #[test]
    fn template_status_is_not_a_messages_delivery() {
        let delivery = payload("template-status", &cx()).unwrap();
        assert_eq!(
            delivery["entry"][0]["changes"][0]["field"],
            "message_template_status_update"
        );
    }
}
