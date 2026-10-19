//! The HTTP transport: executes [`Request`]s against the Graph API.

use std::fmt;
use std::path::Path;
use std::sync::{Arc, Once};
use std::time::Duration;

use futures_util::StreamExt;
use paloma_core::{Credentials, Error, Result};
use reqwest::header::HeaderValue;
use reqwest::multipart::{Form, Part};
use secrecy::ExposeSecret;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tracing::{debug, trace};

use crate::error_map::map_error;
use crate::request::{
    Auth, Body, Method, PartValue, Request, Target, diagnostic_body, redact_json, redact_url,
};
use crate::retry::{self, Failure};

/// Called before each retry, so a wait never looks like a hang. The transport has no
/// access to the printer, which belongs to the CLI.
#[derive(Clone)]
pub struct RetryNotice(Arc<dyn Fn(&str) + Send + Sync>);

impl RetryNotice {
    pub fn new(notify: impl Fn(&str) + Send + Sync + 'static) -> Self {
        Self(Arc::new(notify))
    }
}

impl fmt::Debug for RetryNotice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RetryNotice")
    }
}

/// Transport settings.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Graph host, overridable for tests (`PALOMA_GRAPH_URL`).
    pub base_url: String,
    pub api_version: String,
    /// Whole-request timeout for API calls (downloads only bound the connect phase). It
    /// applies per try.
    pub timeout: Duration,
    pub user_agent: String,
    pub on_retry: Option<RetryNotice>,
}

impl ClientConfig {
    pub fn new(base_url: impl Into<String>, api_version: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            api_version: api_version.into(),
            timeout: Duration::from_secs(60),
            user_agent: concat!("paloma/", env!("CARGO_PKG_VERSION")).to_owned(),
            on_retry: None,
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
        if !(200..300).contains(&status) {
            // The status alone decides the exit code, so an unreadable body can't lose it.
            let text = resp.text().await.unwrap_or_default();
            trace!(status, body = %diagnostic_body(&text), "response");
            return Err(map_error(status, &text));
        }
        let text = resp.text().await.map_err(transport)?;
        trace!(status, body = %diagnostic_body(&text), "response");
        Ok(Response { status, text })
    }

    /// Stream the response body of a GET to `dest`, returning the bytes written.
    pub async fn download(&self, req: &Request, dest: &Path) -> Result<u64> {
        self.download_to(req, dest, false).await
    }

    /// Save a download only if the destination can be atomically created as a new file.
    pub async fn download_new(&self, req: &Request, dest: &Path) -> Result<u64> {
        self.download_to(req, dest, true).await
    }

    async fn download_to(&self, req: &Request, dest: &Path, create_new: bool) -> Result<u64> {
        let resp = self.send(req, Some(self.config.timeout)).await?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            let text = resp.text().await.unwrap_or_default();
            return Err(match req.target {
                Target::Url(_) if status == 429 => {
                    Error::RateLimited("download failed: rate limited (429)".into())
                }
                Target::Url(_) => Error::other(format!("download failed: HTTP {status}")),
                Target::Node(_) => map_error(status, &text),
            });
        }
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true);
        if create_new {
            options.create_new(true);
        } else {
            options.create(true).truncate(true);
        }
        let mut file = options
            .open(dest)
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

    /// Send the request, repeating it when [`retry::delay`] says that's safe. The last
    /// response is returned as it is, so callers map a final failure as usual.
    async fn send(&self, req: &Request, timeout: Option<Duration>) -> Result<reqwest::Response> {
        let mut attempt = 1;
        loop {
            let result = self.builder(req, timeout).await?.send().await;
            let (failure, reason) = match &result {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let retry_after = resp
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|value| value.to_str().ok())
                        .and_then(retry::parse_retry_after);
                    let failure = Failure::Status {
                        status,
                        retry_after,
                    };
                    (failure, format!("HTTP {status}"))
                }
                // Connecting is the one transport failure known to come before the request
                // was written. Anything later, a --timeout included, isn't repeated.
                Err(e) if e.is_connect() => (Failure::Unreachable, "cannot connect".to_owned()),
                Err(_) => return result.map_err(transport),
            };
            let Some(wait) = retry::delay(req.method, attempt, failure, retry::BASE_BACKOFF) else {
                return result.map_err(transport);
            };
            drop(result);
            attempt += 1;
            let message = format!(
                "{reason}: retrying in {}s (attempt {attempt} of {})",
                wait.as_secs(),
                retry::MAX_ATTEMPTS
            );
            debug!(%message, "retry");
            if let Some(notice) = &self.config.on_retry {
                (notice.0)(&message);
            }
            tokio::time::sleep(wait).await;
        }
    }

    async fn builder(
        &self,
        req: &Request,
        timeout: Option<Duration>,
    ) -> Result<reqwest::RequestBuilder> {
        let url = self.url(req);
        debug!(method = req.method.as_str(), url = %redact_url(&url), "request");
        let mut builder = match req.method {
            Method::Get => self.http.get(&url),
            Method::Post => self.http.post(&url),
            Method::Delete => self.http.delete(&url),
        };
        builder = match &req.auth {
            Auth::None => builder,
            Auth::User => builder.bearer_auth(self.creds.access_token()?.expose_secret()),
            Auth::App { app_id } => builder.bearer_auth(format!(
                "{app_id}|{}",
                self.creds.app_secret()?.expose_secret()
            )),
            Auth::OAuth => {
                let token = self.creds.access_token()?;
                let mut value = HeaderValue::from_str(&format!("OAuth {}", token.expose_secret()))
                    .map_err(|_| Error::auth("the access token isn't a valid header value"))?;
                value.set_sensitive(true);
                builder.header(reqwest::header::AUTHORIZATION, value)
            }
        };
        for (name, value) in &req.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
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
            Body::File { path, offset } => {
                let unreadable = |e| Error::io(format!("cannot read {}", path.display()), e);
                let mut file = tokio::fs::File::open(path).await.map_err(unreadable)?;
                let size = file.metadata().await.map_err(unreadable)?.len();
                file.seek(std::io::SeekFrom::Start(*offset))
                    .await
                    .map_err(unreadable)?;
                builder
                    .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                    .header(
                        reqwest::header::CONTENT_LENGTH,
                        size.saturating_sub(*offset),
                    )
                    .body(reqwest::Body::from(file))
            }
        };
        Ok(builder)
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
    if e.is_connect() {
        // Checked first: the connect phase has its own limit, which --timeout doesn't raise.
        Error::other(format!("cannot reach the Graph API: {e}"))
    } else if e.is_timeout() {
        Error::other("request timed out (raise it with --timeout)")
    } else {
        Error::other(format!("HTTP error: {e}"))
    }
}
