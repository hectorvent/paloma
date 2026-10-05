//! The HTTP transport: executes [`Request`]s against the Graph API.

use std::path::Path;
use std::sync::{Arc, Once};
use std::time::Duration;

use futures_util::StreamExt;
use paloma_core::{Credentials, Error, Result};
use reqwest::multipart::{Form, Part};
use secrecy::ExposeSecret;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tracing::{debug, trace};

use crate::error_map::map_error;
use crate::request::{
    Auth, Body, Method, PartValue, Request, Target, diagnostic_body, redact_json, redact_url,
};

/// Transport settings.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Graph host, overridable for tests (`PALOMA_GRAPH_URL`).
    pub base_url: String,
    pub api_version: String,
    /// Whole-request timeout for API calls (downloads only bound the connect phase).
    pub timeout: Duration,
    pub user_agent: String,
}

impl ClientConfig {
    pub fn new(base_url: impl Into<String>, api_version: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            api_version: api_version.into(),
            timeout: Duration::from_secs(60),
            user_agent: concat!("paloma/", env!("CARGO_PKG_VERSION")).to_owned(),
        }
    }
}

/// A completed 2xx response.
#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    /// The body exactly as Graph sent it (what `-o json` passes through).
    pub text: String,
}

impl Response {
    /// The body as JSON; an empty body is `{}`.
    pub fn json(&self) -> Result<Value> {
        if self.text.trim().is_empty() {
            return Ok(Value::Object(serde_json::Map::new()));
        }
        serde_json::from_str(&self.text)
            .map_err(|e| Error::other(format!("could not parse Graph API response: {e}")))
    }

    pub fn parse<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_str(&self.text)
            .map_err(|e| Error::other(format!("could not parse Graph API response: {e}")))
    }
}

/// The Graph API client. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    config: Arc<ClientConfig>,
    creds: Arc<dyn Credentials>,
}

static CRYPTO: Once = Once::new();

impl Client {
    pub fn new(config: ClientConfig, creds: Arc<dyn Credentials>) -> Result<Self> {
        CRYPTO.call_once(|| {
            // Ignore the error: another component may already have installed a provider.
            let _ = rustls::crypto::ring::default_provider().install_default();
        });
        let http = reqwest::Client::builder()
            .user_agent(&config.user_agent)
            .connect_timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| Error::other(format!("cannot initialise HTTP client: {e}")))?;
        Ok(Self {
            http,
            config: Arc::new(config),
            creds,
        })
    }

    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    pub fn url(&self, req: &Request) -> String {
        req.url(&self.config.base_url, &self.config.api_version)
    }

    pub fn describe(&self, req: &Request) -> Value {
        req.describe(&self.config.base_url, &self.config.api_version)
    }

    /// Send the request; non-2xx responses become the mapped [`Error`].
    pub async fn execute(&self, req: &Request) -> Result<Response> {
        let resp = self.send(req, Some(self.config.timeout)).await?;
        let status = resp.status().as_u16();
        let text = resp.text().await.map_err(transport)?;
        trace!(status, body = %diagnostic_body(&text), "response");
        if (200..300).contains(&status) {
            Ok(Response { status, text })
        } else {
            Err(map_error(status, &text))
        }
    }

    /// Stream the response body of a GET to `dest`, returning the bytes written.
    pub async fn download(&self, req: &Request, dest: &Path) -> Result<u64> {
        let resp = self.send(req, None).await?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            let text = resp.text().await.unwrap_or_default();
            return Err(match req.target {
                Target::Url(_) => Error::other(format!("download failed: HTTP {status}")),
                Target::Node(_) => map_error(status, &text),
            });
        }
        let mut file = tokio::fs::File::create(dest)
            .await
            .map_err(|e| Error::io(format!("cannot create {}", dest.display()), e))?;
        let mut stream = resp.bytes_stream();
        let mut written = 0u64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(transport)?;
            file.write_all(&chunk)
                .await
                .map_err(|e| Error::io(format!("cannot write {}", dest.display()), e))?;
            written += chunk.len() as u64;
        }
        file.flush()
            .await
            .map_err(|e| Error::io(format!("cannot write {}", dest.display()), e))?;
        Ok(written)
    }

    async fn send(&self, req: &Request, timeout: Option<Duration>) -> Result<reqwest::Response> {
        let url = self.url(req);
        debug!(method = req.method.as_str(), url = %redact_url(&url), "request");
        let bearer = match &req.auth {
            Auth::User => self.creds.access_token()?.expose_secret().to_owned(),
            Auth::App { app_id } => {
                format!("{app_id}|{}", self.creds.app_secret()?.expose_secret())
            }
        };
        let mut builder = match req.method {
            Method::Get => self.http.get(&url),
            Method::Post => self.http.post(&url),
            Method::Delete => self.http.delete(&url),
        }
        .bearer_auth(bearer);
        if let Some(t) = timeout {
            builder = builder.timeout(t);
        }
        builder = match &req.body {
            Body::Empty => builder,
            Body::Json(v) => {
                trace!(body = %redact_json(v), "request body");
                builder.json(v)
            }
            Body::Multipart(parts) => builder.multipart(build_form(parts).await?),
        };
        builder.send().await.map_err(transport)
    }
}

async fn build_form(parts: &[crate::request::FormPart]) -> Result<Form> {
    let mut form = Form::new();
    for p in parts {
        let part = match &p.value {
            PartValue::Text(t) => Part::text(t.clone()),
            PartValue::File {
                path,
                file_name,
                mime,
            } => Part::file(path)
                .await
                .map_err(|e| Error::io(format!("cannot read {}", path.display()), e))?
                .file_name(file_name.clone())
                .mime_str(mime)
                .map_err(|e| Error::validation(format!("invalid content type '{mime}': {e}")))?,
        };
        form = form.part(p.name.clone(), part);
    }
    Ok(form)
}

#[allow(clippy::needless_pass_by_value)] // used as `map_err(transport)`
fn transport(e: reqwest::Error) -> Error {
    // reqwest attaches request URLs, which may contain verification tokens.
    let e = e.without_url();
    if e.is_timeout() {
        Error::other("request timed out (raise it with --timeout)")
    } else if e.is_connect() {
        Error::other(format!("cannot reach the Graph API: {e}"))
    } else {
        Error::other(format!("HTTP error: {e}"))
    }
}
