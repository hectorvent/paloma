//! Calling settings (`/{phone-number-id}/settings`). Every write is a partial update of
//! the `calling` object, except `call_hours`, which Meta replaces whole.

use serde_json::{Map, Value, json};

use super::MESSAGING_PRODUCT;
use crate::Request;

pub fn get(phone_number_id: &str, include_sip_credentials: bool) -> Request {
    let req = Request::get(format!("{phone_number_id}/settings")).query("fields", "calling");
    if include_sip_credentials {
        req.query("include_sip_credentials", "true")
    } else {
        req
    }
}

/// A phone number goes in `user_wa_id` and a business-scoped user ID in `recipient`.
pub fn permissions(phone_number_id: &str, user: &str) -> Request {
    let user = user.trim();
    let param = if paloma_core::validate::bsuid(user).is_some() {
        "recipient"
    } else {
        "user_wa_id"
    };
    Request::get(format!("{phone_number_id}/call_permissions"))
        .query(param, user.trim_start_matches('+'))
}

/// Partially update the `calling` settings object with a caller-built object (a
/// manifest spec or a `--file`).
#[allow(clippy::needless_pass_by_value)] // takes ownership like every other body builder
pub fn set(phone_number_id: &str, calling: Value) -> Request {
    Request::post(format!("{phone_number_id}/settings"))
        .json(json!({ "messaging_product": MESSAGING_PRODUCT, "calling": calling }))
}

/// What turning calling off confirms, from the CLI or a manifest alike.
pub const DISABLE_CONFIRM: &str = "turn calling off, so people can't call the number";

/// What turning SIP off confirms.
pub const SIP_OFF_CONFIRM: &str = "turn SIP off, so calls stop reaching your SIP servers";
/// What turning voicemail off confirms.
pub const VOICEMAIL_OFF_CONFIRM: &str = "turn voicemail off";
/// What emptying the additional codecs confirms.
pub const CODECS_CLEAR_CONFIRM: &str = "remove every additional audio codec";

/// Whether a `calling` object turns calling off, which [`DISABLE_CONFIRM`] guards.
pub fn turns_off(calling: &Value) -> bool {
    calling.get("status").and_then(Value::as_str) == Some(status(false))
}

/// The confirmation a whole `calling` object needs (a manifest spec or `calling set
/// --file`): the same ones `calling disable`, `sip disable`, `voicemail disable` and
/// `codecs clear` ask, the first that applies.
pub fn confirm_for(calling: &Value) -> Option<&'static str> {
    let off =
        |pointer: &str| calling.pointer(pointer).and_then(Value::as_str) == Some(status(false));
    if turns_off(calling) {
        Some(DISABLE_CONFIRM)
    } else if off("/sip/status") {
        Some(SIP_OFF_CONFIRM)
    } else if off("/voicemail/status") {
        Some(VOICEMAIL_OFF_CONFIRM)
    } else if calling
        .pointer("/audio/additional_codecs")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
    {
        Some(CODECS_CLEAR_CONFIRM)
    } else {
        None
    }
}

/// Turn calling on or off.
pub fn set_status(phone_number_id: &str, enabled: bool) -> Request {
    set(phone_number_id, json!({ "status": status(enabled) }))
}

fn status(enabled: bool) -> &'static str {
    if enabled { "ENABLED" } else { "DISABLED" }
}

/// A closed set of Meta's calling values, with the wire name each one sends.
macro_rules! wire_enum {
    ($(#[$doc:meta])* $name:ident { $($variant:ident => $wire:literal),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $wire),+
                }
            }
        }
    };
}

wire_enum!(
    /// On or off, for calling itself and for callback permission.
    Toggle { Enabled => "ENABLED", Disabled => "DISABLED" }
);
wire_enum!(
    /// Whether WhatsApp shows the call button.
    CallIcon { Default => "DEFAULT", DisableAll => "DISABLE_ALL" }
);
wire_enum!(
    /// SRTP key exchange for SIP calls. The guide's values: the settings reference says
    /// `DTLS-SRTP` and `SDES-SRTP`, but it also gets the casing of `status` wrong.
    Srtp { Dtls => "DTLS", Sdes => "SDES" }
);
wire_enum!(
    /// Additional audio codecs.
    Codec { Pcma => "PCMA", Pcmu => "PCMU" }
);
wire_enum!(
    /// When voicemail answers.
    VoicemailTrigger { Reject => "REJECT", Timeout => "TIMEOUT" }
);

/// Top-level calling settings; `None` leaves a setting as it is.
#[derive(Debug, Default)]
pub struct Settings {
    pub status: Option<Toggle>,
    pub call_icon_visibility: Option<CallIcon>,
    pub callback_permission_status: Option<Toggle>,
    /// `Some(empty)` shows the call button in every country again.
    pub icon_countries: Option<Vec<String>>,
    pub srtp_key_exchange_protocol: Option<Srtp>,
}

impl Settings {
    /// Whether these settings turn calling off, which [`DISABLE_CONFIRM`] guards.
    pub fn turns_off(&self) -> bool {
        self.status == Some(Toggle::Disabled)
    }

    pub fn is_empty(&self) -> bool {
        self.body().as_object().is_none_or(Map::is_empty)
    }

    fn body(&self) -> Value {
        let mut m = Map::new();
        let mut put = |key: &str, value: Option<&str>| {
            if let Some(v) = value {
                m.insert(key.into(), v.into());
            }
        };
        put("status", self.status.map(Toggle::as_str));
        put(
            "call_icon_visibility",
            self.call_icon_visibility.map(CallIcon::as_str),
        );
        put(
            "callback_permission_status",
            self.callback_permission_status.map(Toggle::as_str),
        );
        put(
            "srtp_key_exchange_protocol",
            self.srtp_key_exchange_protocol.map(Srtp::as_str),
        );
        if let Some(countries) = &self.icon_countries {
            m.insert(
                "call_icons".into(),
                json!({ "restrict_to_user_countries": countries }),
            );
        }
        Value::Object(m)
    }
}

pub fn set_settings(phone_number_id: &str, settings: &Settings) -> Request {
    set(phone_number_id, settings.body())
}

/// One SIP server Meta signals calls to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SipServer {
    pub hostname: String,
    pub port: u16,
}

/// Turn SIP on with these servers. `tgrp` and `context` become each server's request-URI
/// user parameters.
pub fn set_sip(
    phone_number_id: &str,
    servers: &[SipServer],
    tgrp: Option<&str>,
    context: Option<&str>,
    enable_calling: bool,
) -> Request {
    let mut params = Map::new();
    for (key, value) in [("tgrp", tgrp), ("context", context)] {
        if let Some(v) = value.filter(|v| !v.is_empty()) {
            params.insert(key.into(), v.into());
        }
    }
    let servers: Vec<Value> = servers
        .iter()
        .map(|s| {
            let mut server = json!({ "hostname": s.hostname, "port": s.port });
            if !params.is_empty() {
                server["request_uri_user_params"] = Value::Object(params.clone());
            }
            server
        })
        .collect();
    let mut calling = Map::new();
    if enable_calling {
        calling.insert("status".into(), status(true).into());
    }
    calling.insert(
        "sip".into(),
        json!({ "status": status(true), "servers": servers }),
    );
    set(phone_number_id, Value::Object(calling))
}

pub fn disable_sip(phone_number_id: &str) -> Request {
    set(
        phone_number_id,
        json!({ "sip": { "status": status(false) } }),
    )
}

/// Replace the additional audio codecs; an empty list clears them.
pub fn set_codecs(phone_number_id: &str, codecs: &[Codec]) -> Request {
    let codecs: Vec<&str> = codecs.iter().map(|c| c.as_str()).collect();
    set(
        phone_number_id,
        json!({ "audio": { "additional_codecs": codecs } }),
    )
}

/// Turn voicemail on. `timeout_seconds` goes with a `TIMEOUT` trigger.
pub fn set_voicemail(
    phone_number_id: &str,
    triggers: &[VoicemailTrigger],
    announcement_media_id: u64,
    timeout_seconds: Option<u32>,
) -> Request {
    let triggers: Vec<&str> = triggers.iter().map(|t| t.as_str()).collect();
    let mut audio = json!({ "announcement_media_id": announcement_media_id });
    if let Some(seconds) = timeout_seconds {
        audio["timeout_seconds"] = json!(seconds);
    }
    set(
        phone_number_id,
        json!({ "voicemail": {
            "status": status(true),
            "triggers": triggers,
            "audio": { "default": audio },
        } }),
    )
}

pub fn disable_voicemail(phone_number_id: &str) -> Request {
    set(
        phone_number_id,
        json!({ "voicemail": { "status": status(false) } }),
    )
}

/// One weekly opening period; times are `HHMM`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct OpeningHours {
    pub day_of_week: &'static str,
    pub open_time: String,
    pub close_time: String,
}

/// One holiday override; `date` is `YYYY-MM-DD` and times are `HHMM`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holiday {
    pub date: String,
    pub start_time: String,
    pub end_time: String,
}

/// Replace the whole call-hours schedule and turn it on. Holidays left out are deleted.
pub fn set_hours(
    phone_number_id: &str,
    timezone_id: &str,
    weekly: &[OpeningHours],
    holidays: &[Holiday],
) -> Request {
    let weekly: Vec<Value> = weekly
        .iter()
        .map(|h| {
            json!({ "day_of_week": h.day_of_week, "open_time": h.open_time, "close_time": h.close_time })
        })
        .collect();
    let holidays: Vec<Value> = holidays
        .iter()
        .map(|h| json!({ "date": h.date, "start_time": h.start_time, "end_time": h.end_time }))
        .collect();
    set(
        phone_number_id,
        json!({ "call_hours": {
            "status": status(true),
            "timezone_id": timezone_id,
            "weekly_operating_hours": weekly,
            "holiday_schedule": holidays,
        } }),
    )
}

/// Turn call hours off. Meta needs the saved schedule resent with the change, or it deletes
/// it, so `current` is the live `call_hours` object.
pub fn disable_hours(phone_number_id: &str, mut current: Value) -> Request {
    current["status"] = json!(status(false));
    set(phone_number_id, json!({ "call_hours": current }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calling(req: &Request) -> Value {
        req.json_body().expect("a JSON body")["calling"].clone()
    }

    #[test]
    fn settings_send_only_what_is_set() {
        let settings = Settings {
            status: Some(Toggle::Enabled),
            icon_countries: Some(vec!["US".into()]),
            ..Settings::default()
        };
        assert_eq!(
            calling(&set_settings("1", &settings)),
            json!({ "status": "ENABLED", "call_icons": { "restrict_to_user_countries": ["US"] } })
        );
        assert!(Settings::default().is_empty());
        let everywhere = Settings {
            icon_countries: Some(vec![]),
            ..Settings::default()
        };
        assert!(!everywhere.is_empty());
    }

    #[test]
    fn sip_params_go_on_every_server() {
        let servers = [
            SipServer {
                hostname: "a".into(),
                port: 5061,
            },
            SipServer {
                hostname: "b".into(),
                port: 5060,
            },
        ];
        let body = calling(&set_sip("1", &servers, Some("t"), Some(""), true));
        assert_eq!(body["status"], "ENABLED");
        assert_eq!(
            body["sip"]["servers"][1]["request_uri_user_params"],
            json!({ "tgrp": "t" })
        );
        let bare = calling(&set_sip("1", &servers[..1], None, None, false));
        assert_eq!(
            bare,
            json!({ "sip": { "status": "ENABLED", "servers": [{ "hostname": "a", "port": 5061 }] } })
        );
    }

    #[test]
    fn hours_disable_keeps_the_schedule() {
        let current = json!({ "status": "ENABLED", "timezone_id": "UTC", "holiday_schedule": [] });
        assert_eq!(
            calling(&disable_hours("1", current)),
            json!({ "call_hours": { "status": "DISABLED", "timezone_id": "UTC", "holiday_schedule": [] } })
        );
    }
}
