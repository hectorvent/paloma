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
        out.insert("url".into(), self.url(base, version).into());
        if let Auth::App { app_id } = &self.auth {
            out.insert(
                "auth".into(),
                format!("app token ({app_id}|<app-secret>)").into(),
            );
        }
        match &self.body {
            Body::Empty => {}
            Body::Json(v) => {
                out.insert("body".into(), v.clone());
            }
            Body::Multipart(parts) => {
                let mut form = Map::new();
                for p in parts {
                    let v = match &p.value {
                        PartValue::Text(t) => Value::String(t.clone()),
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
