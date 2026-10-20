//! Calling a user's webhook endpoint the way Meta does, and checking what Meta signs.
//!
//! This isn't the Graph API: the host is the user's, there's no bearer token, and the URL of
//! a handshake carries the verify token. So it has its own small client that never follows
//! a redirect, which would hand the token to another host, and never reports a URL or a
//! response body, either of which can hold that token.

use std::fmt::Write as _;
use std::time::Duration;

use paloma_core::{Error, Result};
pub use reqwest::Url;
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};
use serde::de::IgnoredAny;
use serde_json::Value;

use crate::request::redact_url;

/// Meta's signature header on every webhook POST.
pub const SIGNATURE_HEADER: &str = "X-Hub-Signature-256";

/// The header value for `body`: HMAC-SHA256 with the app secret as the key.
pub fn sign(app_secret: &str, body: &[u8]) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, app_secret.as_bytes());
    format!("sha256={}", hex(hmac::sign(&key, body).as_ref()))
}

/// Whether a header value is the right signature for `body`, in constant time.
pub fn signature_matches(app_secret: &str, body: &[u8], header: &str) -> bool {
    let Some(given) = header.strip_prefix("sha256=").and_then(from_hex) else {
        return false;
    };
    let key = hmac::Key::new(hmac::HMAC_SHA256, app_secret.as_bytes());
    hmac::verify(&key, body, &given).is_ok()
}

/// A well-formed signature no app secret produced, for testing that an endpoint rejects it.
pub fn wrong_signature(body: &[u8]) -> Result<String> {
    Ok(sign(&hex(&random_bytes::<32>()?), body))
}

/// Compare a verify token without leaking how much of it matched.
pub fn tokens_match(given: &str, expected: &str) -> bool {
    given.len() == expected.len()
        && given
            .bytes()
            .zip(expected.bytes())
            .fold(0, |diff, (a, b)| diff | (a ^ b))
            == 0
}

/// A challenge like Meta's: random digits, so an endpoint can't pass by returning a constant.
pub fn challenge() -> Result<String> {
    let number = u64::from_be_bytes(random_bytes::<8>()?);
    Ok(format!("{:010}", number % 10_000_000_000))
}

/// A short random hex string.
pub fn nonce() -> Result<String> {
    Ok(hex(&random_bytes::<8>()?))
}

fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0; N];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| Error::other("the system random source failed"))?;
    Ok(bytes)
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    let digits: Vec<u32> = text
        .chars()
        .map(|c| c.to_digit(16))
        .collect::<Option<_>>()?;
    if !digits.len().is_multiple_of(2) {
        return None;
    }
    digits
        .chunks_exact(2)
        .map(|pair| u8::try_from(pair[0] * 16 + pair[1]).ok())
        .collect()
}

/// A JSON document on one line, byte for byte otherwise. Going through `serde_json::Value`
/// would rewrite numbers it can't hold exactly, so this only drops the whitespace between
/// tokens, and everything Meta adds later passes through untouched.
pub fn compact_json(body: &[u8]) -> std::result::Result<String, serde_json::Error> {
    // Skipping a value doesn't check the UTF-8 inside its strings, so that comes first.
    let text = std::str::from_utf8(body).map_err(serde::de::Error::custom)?;
    serde_json::from_str::<IgnoredAny>(text)?;
    let mut out = Vec::with_capacity(body.len());
    let (mut in_string, mut escaped) = (false, false);
    for &byte in body {
        if in_string {
            out.push(byte);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
        } else if !matches!(byte, b' ' | b'\t' | b'\n' | b'\r') {
            out.push(byte);
            in_string = byte == b'"';
        }
    }
    // Valid UTF-8 went in, and only ASCII whitespace came out, so this changes nothing.
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// The message and status ids in a `messages` delivery, for `--quiet`.
pub fn event_ids(event: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    for change in array(event, "entry")
        .iter()
        .flat_map(|entry| array(entry, "changes"))
    {
        let Some(value) = change.get("value") else {
            continue;
        };
        for item in array(value, "messages")
            .iter()
            .chain(array(value, "statuses"))
        {
            ids.extend(item.get("id").and_then(Value::as_str).map(str::to_owned));
        }
    }
    ids
}

fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// A callback URL checked before anything is spent on it.
pub fn parse_url(raw: &str) -> Result<Url> {
    let url = Url::parse(raw)
        .map_err(|e| Error::validation(format!("not a valid URL: {} ({e})", redact_url(raw))))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(Error::validation(format!(
            "not an http or https URL: {}",
            redact_url(raw)
        )));
    }
    Ok(url)
}

/// What an endpoint answered to the verification handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Handshake {
    pub status: u16,
    pub echoed_challenge: bool,
}

impl Handshake {
    /// Meta's rule: status 200 and the challenge value, nothing else.
    pub fn passed(self) -> bool {
        self.status == 200 && self.echoed_challenge
    }
}

/// An HTTP client for the user's endpoint.
#[derive(Debug, Clone)]
pub struct Probe {
    http: reqwest::Client,
}

impl Probe {
    pub fn new(timeout: Duration) -> Result<Self> {
        crate::client::install_crypto_provider();
        let http = reqwest::Client::builder()
            .user_agent(concat!("paloma/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .timeout(timeout)
            .build()
            .map_err(|e| Error::other(format!("cannot initialise HTTP client: {e}")))?;
        Ok(Self { http })
    }

    /// Meta's `GET <url>?hub.mode=subscribe&hub.challenge=..&hub.verify_token=..`.
    pub async fn handshake(
        &self,
        url: &Url,
        verify_token: &str,
        challenge: &str,
    ) -> Result<Handshake> {
        let mut target = url.clone();
        target
            .query_pairs_mut()
            .append_pair("hub.mode", "subscribe")
            .append_pair("hub.challenge", challenge)
            .append_pair("hub.verify_token", verify_token);
        let mut response = self.http.get(target).send().await.map_err(transport)?;
        let status = response.status().as_u16();
        // One byte past the challenge settles it, however much the endpoint has to say.
        let limit = challenge.len() + 1;
        let mut body = Vec::with_capacity(limit);
        while body.len() < limit {
            let Some(chunk) = response.chunk().await.map_err(transport)? else {
                break;
            };
            let take = chunk.len().min(limit - body.len());
            body.extend_from_slice(&chunk[..take]);
        }
        Ok(Handshake {
            status,
            echoed_challenge: body == challenge.as_bytes(),
        })
    }

    /// Meta's webhook POST: exactly `body`, with the signature header if there is one.
    pub async fn deliver(&self, url: &Url, body: Vec<u8>, signature: Option<&str>) -> Result<u16> {
        let mut request = self
            .http
            .post(url.clone())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body);
        if let Some(signature) = signature {
            request = request.header(SIGNATURE_HEADER, signature);
        }
        let response = request.send().await.map_err(transport)?;
        Ok(response.status().as_u16())
    }
}

fn transport(e: reqwest::Error) -> Error {
    // reqwest attaches the URL, and a handshake's URL has the verify token in it.
    let e = e.without_url();
    if e.is_timeout() {
        return Error::other("the endpoint timed out (raise it with --timeout)");
    }
    let mut message = format!("cannot reach the endpoint: {e}");
    let mut source = std::error::Error::source(&e);
    while let Some(cause) = source {
        let _ = write!(message, ": {cause}");
        source = cause.source();
    }
    Error::other(message)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn signs_like_openssl_and_verifies() {
        // printf '%s' '{"a":1}' | openssl dgst -sha256 -hmac 'app-secret'
        let expected = "sha256=a2ac6607a24977f9757e6d78319ae1660ad42ffeee0ee3f0dff30a92bcf85448";
        assert_eq!(sign("app-secret", br#"{"a":1}"#), expected);
        assert!(signature_matches("app-secret", br#"{"a":1}"#, expected));
        assert!(signature_matches(
            "app-secret",
            br#"{"a":1}"#,
            &expected.to_uppercase().replace("SHA256", "sha256"),
        ));
    }

    #[test]
    fn a_bad_signature_never_matches() {
        let good = sign("s", b"body");
        assert!(!signature_matches("other", b"body", &good));
        assert!(!signature_matches("s", b"tampered", &good));
        assert!(!signature_matches(
            "s",
            b"body",
            good.trim_start_matches("sha256=")
        ));
        assert!(!signature_matches("s", b"body", "sha256=zz"));
        assert!(!signature_matches("s", b"body", "sha256=abc"));
        assert!(!signature_matches("s", b"body", ""));
        assert!(!signature_matches(
            "s",
            b"body",
            &wrong_signature(b"body").unwrap()
        ));
    }

    #[test]
    fn tokens_must_match_exactly() {
        assert!(tokens_match("abc", "abc"));
        assert!(!tokens_match("abd", "abc"));
        assert!(!tokens_match("ab", "abc"));
    }

    #[test]
    fn challenges_are_ten_random_digits() {
        let (a, b) = (challenge().unwrap(), challenge().unwrap());
        assert_eq!(a.len(), 10);
        assert!(a.bytes().all(|c| c.is_ascii_digit()));
        assert_ne!(a, b);
    }

    #[test]
    fn compact_json_only_removes_whitespace_between_tokens() {
        let body = b"{\n  \"big\": 123456789012345678901234567890,\n  \"f\" : 1.10,\n  \"s\": \"a b\\\" \\n c\",\n  \"e\": 1e2 }\n";
        assert_eq!(
            compact_json(body).unwrap(),
            r#"{"big":123456789012345678901234567890,"f":1.10,"s":"a b\" \n c","e":1e2}"#
        );
        assert!(compact_json(b"not json").is_err());
        assert!(compact_json(b"").is_err());
        assert!(compact_json(b"{\"a\":\"\xff\"}").is_err());
        assert_eq!(
            compact_json("{ \"a\" : \"ñ é\" }".as_bytes()).unwrap(),
            "{\"a\":\"ñ é\"}"
        );
    }

    #[test]
    fn event_ids_reads_messages_and_statuses() {
        let event = json!({"entry": [
            {"changes": [{"value": {"messages": [{"id": "m1"}], "statuses": [{"id": "s1"}]}}]},
            {"changes": [{"value": {"event": "APPROVED"}}, {"field": "x"}]},
        ]});
        assert_eq!(event_ids(&event), ["m1", "s1"]);
        assert!(event_ids(&json!({"other": 1})).is_empty());
    }

    #[test]
    fn only_http_urls_with_a_host_pass() {
        assert!(parse_url("https://example.test/hook?a=1").is_ok());
        assert!(parse_url("http://127.0.0.1:9/hook").is_ok());
        for bad in [
            "example.test/hook",
            "ftp://example.test",
            "mailto:a@b.test",
            "",
        ] {
            assert!(parse_url(bad).is_err(), "{bad}");
        }
    }
}
