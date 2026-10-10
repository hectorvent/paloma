//! A Graph API call described as data.

use std::fmt::Write as _;
use std::path::PathBuf;

use serde_json::{Map, Value, json};

/// HTTP verb.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Delete,
}

impl Method {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Delete => "DELETE",
        }
    }

    /// Whether the call changes state on Meta's side.
    pub const fn is_write(self) -> bool {
        !matches!(self, Self::Get)
    }
}

/// Which bearer token authorizes the call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Auth {
    /// The profile's access token.
    User,
    /// An app access token `<app_id>|<app_secret>` (app-level webhook subscriptions).
    App { app_id: String },
}

/// Where the request goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A Graph node path relative to `/{version}/`, e.g. `123/messages`.
    Node(String),
    /// A full URL handed back by Graph (media/asset downloads on a CDN host).
    Url(String),
}

/// One multipart form field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormPart {
    pub name: String,
    pub value: PartValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartValue {
    Text(String),
    File {
        path: PathBuf,
        file_name: String,
        mime: String,
    },
}

impl FormPart {
    pub fn text(name: &str, value: impl Into<String>) -> Self {
        Self {
            name: name.to_owned(),
            value: PartValue::Text(value.into()),
        }
    }

    pub fn file(
        name: &str,
        path: impl Into<PathBuf>,
        file_name: impl Into<String>,
        mime: impl Into<String>,
    ) -> Self {
        Self {
            name: name.to_owned(),
            value: PartValue::File {
                path: path.into(),
                file_name: file_name.into(),
                mime: mime.into(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    Empty,
    Json(Value),
    Multipart(Vec<FormPart>),
}

/// A fully-specified Graph API call.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub method: Method,
    pub target: Target,
    pub query: Vec<(String, String)>,
    pub body: Body,
    pub auth: Auth,
}

impl Request {
    fn new(method: Method, node: impl Into<String>) -> Self {
        Self {
            method,
            target: Target::Node(node.into()),
            query: Vec::new(),
            body: Body::Empty,
            auth: Auth::User,
        }
    }

    pub fn get(node: impl Into<String>) -> Self {
        Self::new(Method::Get, node)
    }

    pub fn post(node: impl Into<String>) -> Self {
        Self::new(Method::Post, node)
    }

    pub fn delete(node: impl Into<String>) -> Self {
        Self::new(Method::Delete, node)
    }

    /// A GET on an absolute URL (CDN download), authorized with the user token.
    pub fn get_url(url: impl Into<String>) -> Self {
        Self {
            target: Target::Url(url.into()),
            ..Self::new(Method::Get, String::new())
        }
    }

    #[must_use]
    #[allow(clippy::needless_pass_by_value)] // builder ergonomics: `.query("limit", 100)`
    pub fn query(mut self, key: &str, value: impl ToString) -> Self {
        self.query.push((key.to_owned(), value.to_string()));
        self
    }

    /// Add a query parameter only when a value is present and non-blank.
    #[must_use]
    #[allow(clippy::needless_pass_by_value)]
    pub fn query_opt(self, key: &str, value: Option<impl ToString>) -> Self {
        match value
            .as_ref()
            .map(ToString::to_string)
            .filter(|v| !v.trim().is_empty())
        {
            Some(v) => self.query(key, v),
            None => self,
        }
    }

    #[must_use]
    pub fn json(mut self, body: Value) -> Self {
        self.body = Body::Json(body);
        self
    }

    #[must_use]
    pub fn multipart(mut self, parts: Vec<FormPart>) -> Self {
        self.body = Body::Multipart(parts);
        self
    }

    #[must_use]
    pub fn app_auth(mut self, app_id: impl Into<String>) -> Self {
        self.auth = Auth::App {
            app_id: app_id.into(),
        };
        self
    }

    /// The URL this request resolves to, including the query string.
    pub fn url(&self, base: &str, version: &str) -> String {
        let mut url = match &self.target {
            Target::Node(node) => format!("{}/{}/{}", base.trim_end_matches('/'), version, node),
            Target::Url(u) => u.clone(),
        };
        if !self.query.is_empty() {
            let qs: Vec<String> = self
                .query
                .iter()
                .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
                .collect();
            url.push(if url.contains('?') { '&' } else { '?' });
            url.push_str(&qs.join("&"));
        }
        url
    }

    /// The `--dry-run` rendering: method, URL and body, with multipart files shown by
    /// name and size rather than content. Never includes credentials.
    pub fn describe(&self, base: &str, version: &str) -> Value {
        let mut out = Map::new();
        out.insert("method".into(), self.method.as_str().into());
        out.insert("url".into(), redact_url(&self.url(base, version)).into());
        if let Auth::App { app_id } = &self.auth {
            out.insert(
                "auth".into(),
                format!("app token ({app_id}|<app-secret>)").into(),
            );
        }
        match &self.body {
            Body::Empty => {}
            Body::Json(v) => {
                out.insert("body".into(), redact_json(v));
            }
            Body::Multipart(parts) => {
                let mut form = Map::new();
                for p in parts {
                    let v = match &p.value {
                        PartValue::Text(t) => {
                            if sensitive_key(&p.name) {
                                Value::String("<redacted>".into())
                            } else {
                                redact_json(&Value::String(t.clone()))
                            }
                        }
                        PartValue::File {
                            path,
                            file_name,
                            mime,
                        } => {
                            let size = std::fs::metadata(path).map(|m| m.len()).ok();
                            json!({ "file": path.display().to_string(), "filename": file_name, "content_type": mime, "bytes": size })
                        }
                    };
                    form.insert(p.name.clone(), v);
                }
                out.insert("multipart".into(), Value::Object(form));
            }
        }
        Value::Object(out)
    }

    /// The JSON body, if any (what `-o raw` prints for a dry-run).
    pub fn json_body(&self) -> Option<&Value> {
        match &self.body {
            Body::Json(v) => Some(v),
            _ => None,
        }
    }
}

/// Diagnostic rendering is separate from transport data: secrets must still reach
/// Graph unchanged, but never appear in dry-runs or tracing.
pub fn redact_json(value: &Value) -> Value {
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, value)| {
                    let value = if sensitive_key(key) {
                        Value::String("<redacted>".into())
                    } else {
                        redact_json(value)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redact_json).collect()),
        Value::String(text) if has_http_scheme(text) => Value::String(redact_url(text)),
        _ => value.clone(),
    }
}

/// Whether text starts with an http or https scheme. Schemes are case-insensitive, and a
/// link missed here would keep any credentials in its query string.
pub fn has_http_scheme(text: &str) -> bool {
    ["http://", "https://"].iter().any(|scheme| {
        text.get(..scheme.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(scheme))
    })
}

fn sensitive_key(key: &str) -> bool {
    let normalized: String = key
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect();
    normalized == "pin"
        || normalized == "authorization"
        || normalized.ends_with("token")
        || normalized.ends_with("secret")
        || normalized.ends_with("password")
        || normalized == "appsecretproof"
}

/// Mask sensitive query parameters, including those in nested callback URLs.
/// Invalid URLs are omitted rather than risking credentials in parser diagnostics.
pub fn redact_url(text: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(text) else {
        return "<redacted URL>".into();
    };
    let mut changed = false;
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(key, value)| {
            let safe = if sensitive_key(&key) {
                "<redacted>".to_owned()
            } else if has_http_scheme(&value) {
                redact_url(&value)
            } else {
                value.to_string()
            };
            changed |= safe != value;
            (key.into_owned(), safe)
        })
        .collect();
    if changed {
        url.query_pairs_mut().clear().extend_pairs(pairs);
    }
    if !url.username().is_empty() || url.password().is_some() {
        let _ = url.set_username("redacted");
        let _ = url.set_password(None);
        changed = true;
    }
    if url.fragment().is_some() {
        url.set_fragment(None);
        changed = true;
    }
    if changed {
        url.to_string()
    } else {
        text.to_owned()
    }
}

/// Non-JSON responses are omitted from diagnostics because they have no reliable
/// field boundaries for identifying credentials.
pub fn diagnostic_body(text: &str) -> String {
    serde_json::from_str::<Value>(text).map_or_else(
        |_| "<non-JSON body omitted>".into(),
        |value| redact_json(&value).to_string(),
    )
}

/// Percent-encode a query component (RFC 3986 unreserved characters pass through).
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn links_are_redacted_whatever_the_scheme_case() {
        let body = serde_json::json!({
            "link": "HTTPS://x.test/a.png?access_token=SECRET1",
            "other": "Http://x.test/?callback=hTTps%3A%2F%2Fy.test%2F%3Ftoken%3DSECRET2",
            "text": "httpfoo://not-a-link?access_token=KEPT"
        });
        let redacted = super::redact_json(&body).to_string();
        assert!(!redacted.contains("SECRET1"), "{redacted}");
        assert!(!redacted.contains("SECRET2"), "{redacted}");
        assert!(redacted.contains("KEPT"), "{redacted}");
    }

    use super::*;

    #[test]
    fn url_includes_version_and_encoded_query() {
        let r = Request::get("123/message_templates")
            .query("name", "hello world")
            .query_opt("after", None::<&str>);
        assert_eq!(
            r.url("https://graph.facebook.com", "v26.0"),
            "https://graph.facebook.com/v26.0/123/message_templates?name=hello%20world"
        );
    }

    #[test]
    fn describe_hides_app_secret_and_shows_body() {
        let r = Request::post("1/subscriptions")
            .app_auth("42")
            .json(json!({"a": 1}));
        let d = r.describe("https://g", "v1");
        assert_eq!(d["method"], "POST");
        assert_eq!(d["body"]["a"], 1);
        assert_eq!(d["auth"], "app token (42|<app-secret>)");
    }
}

#[cfg(test)]
mod redaction_tests {
    use super::*;

    #[test]
    fn descriptions_mask_nested_secrets_without_mutating_requests() {
        let body =
            json!({"pin":"825194", "nested":[{"verifyToken":"VERIFY_SENTINEL", "name":"visible"}]});
        let request = Request::post("111/register").json(body.clone());
        let description = request.describe("https://graph.facebook.com", "v26.0");
        assert_eq!(description["body"]["pin"], "<redacted>");
        assert_eq!(
            description["body"]["nested"][0]["verifyToken"],
            "<redacted>"
        );
        assert_eq!(description["body"]["nested"][0]["name"], "visible");
        assert_eq!(request.json_body(), Some(&body));
    }

    #[test]
    fn urls_mask_encoded_keys_nested_callbacks_and_userinfo() {
        let url = "https://user:PASS_SENTINEL@graph.example/next?access%5Ftoken=TOKEN_SENTINEL&after=C1&callback_url=https%3A%2F%2Fexample.test%2F%3Fverify_token%3DVERIFY_SENTINEL";
        let safe = redact_url(url);
        for secret in ["PASS_SENTINEL", "TOKEN_SENTINEL", "VERIFY_SENTINEL"] {
            assert!(!safe.contains(secret));
        }
        assert!(safe.contains("after=C1"));
        assert_eq!(
            redact_url("https://g/next?name=hello%20world"),
            "https://g/next?name=hello%20world"
        );
    }

    #[test]
    fn multipart_and_response_diagnostics_mask_secrets() {
        let request = Request::post("111/media").multipart(vec![
            FormPart::text("verify_token", "VERIFY_SENTINEL"),
            FormPart::text("messaging_product", "whatsapp"),
        ]);
        let rendered = request.describe("https://g", "v1");
        assert_eq!(rendered["multipart"]["verify_token"], "<redacted>");
        assert_eq!(rendered["multipart"]["messaging_product"], "whatsapp");
        let body = r#"{"paging":{"next":"https://g/next?access_token=TOKEN_SENTINEL"}}"#;
        assert!(!diagnostic_body(body).contains("TOKEN_SENTINEL"));
        assert_eq!(
            diagnostic_body("not json TOKEN_SENTINEL"),
            "<non-JSON body omitted>"
        );
    }
}
