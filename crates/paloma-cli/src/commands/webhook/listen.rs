//! `webhook listen`: an ephemeral local receiver.
//!
//! stdout is the event stream, one JSON document per line whatever `-o` says, so it pipes
//! into `jq`. Each line is the delivery as Meta sent it, not a parsed model of it, so a field
//! or enum value Meta adds tomorrow still comes through. Everything else goes to stderr.

use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use clap::Args;
use paloma_core::{Error, Result, limits};
use paloma_graph::callback;
use secrecy::{ExposeSecret, SecretString};
use tokio::sync::Notify;

use super::VerifyTokenInput;
use crate::app::App;
use crate::output::Printer;

#[derive(Debug, Args)]
pub struct ListenArgs {
    /// IP address to bind. Anything but loopback exposes the listener to your network.
    #[arg(long, default_value = "127.0.0.1")]
    host: IpAddr,
    /// Port to bind; 0 picks a free one.
    #[arg(long, default_value_t = 8080)]
    port: u16,
    #[command(flatten)]
    verify_token: VerifyTokenInput,
}

struct Listener {
    verify_token: SecretString,
    app_secret: Option<SecretString>,
    out: Printer,
    stop: Notify,
    /// Why stdout failed, if that's what stopped the listener and it's worth reporting.
    write_error: Mutex<Option<io::Error>>,
}

pub async fn run(args: ListenArgs, app: &App) -> Result<()> {
    let addr = SocketAddr::new(args.host, args.port);
    if app.is_dry_run() {
        app.out.note(format!(
            "dry-run: a listener has nothing to send, so nothing was bound; would listen on http://{addr}"
        ));
        return Ok(());
    }
    let verify_token = args.verify_token.read(app)?;
    let app_secret = app.ctx()?.app_secret()?;
    if app_secret.is_none() {
        app.out.warn(
            "no app secret, so signatures are NOT checked and anyone who can reach this port \
             can post events; set PALOMA_APP_SECRET or an app_secret_ref",
        );
    }
    if !args.host.is_loopback() {
        app.out.warn(format!(
            "listening on {} without TLS: this is reachable from your network",
            args.host
        ));
    }
    let socket = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| Error::io(format!("cannot listen on {addr}"), e))?;
    let local = socket
        .local_addr()
        .map_err(|e| Error::io("cannot read the bound address", e))?;

    let state = Arc::new(Listener {
        verify_token,
        app_secret,
        out: app.out.clone(),
        stop: Notify::new(),
        write_error: Mutex::new(None),
    });
    let on_ctrl_c = Arc::clone(&state);
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            on_ctrl_c.stop.notify_one();
        }
    });
    // Let that task install its handler, so a Ctrl-C right after the line below still exits 0.
    tokio::task::yield_now().await;

    let router = Router::new()
        .fallback(handle)
        .layer(DefaultBodyLimit::max(limits::MAX_WEBHOOK_PAYLOAD))
        .with_state(Arc::clone(&state));
    app.out
        .info(format!("listening on http://{local} (Ctrl-C to stop)"));
    tokio::select! {
        served = axum::serve(socket, router).into_future() => {
            served.map_err(|e| Error::io("the listener stopped", e))?;
        }
        () = state.stop.notified() => {}
    }
    if let Some(e) = state.take_write_error() {
        return Err(Error::io("cannot write the event to stdout", e));
    }
    app.out.info("stopped");
    Ok(())
}

async fn handle(
    State(listener): State<Arc<Listener>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: std::result::Result<Bytes, BytesRejection>,
) -> Response {
    match method {
        Method::GET => listener.verify(&uri),
        Method::POST => match body {
            Ok(body) => listener.event(&uri, &headers, &body),
            Err(rejection) => {
                listener.out.warn(format!(
                    "rejected POST {}: {}",
                    uri.path(),
                    rejection.body_text()
                ));
                rejection.into_response()
            }
        },
        other => {
            listener
                .out
                .note(format!("{other} {}: answered 405", uri.path()));
            StatusCode::METHOD_NOT_ALLOWED.into_response()
        }
    }
}

impl Listener {
    /// Meta's rule: 200 and the challenge if the token matches, a 400-level status if not.
    fn verify(&self, uri: &Uri) -> Response {
        let query = Query::<HashMap<String, String>>::try_from_uri(uri)
            .map(|q| q.0)
            .unwrap_or_default();
        let param = |name: &str| query.get(name).map(String::as_str);
        let reject = |why: &str| {
            self.out.warn(format!(
                "rejected handshake on {}: {why}; answered 403",
                uri.path()
            ));
            StatusCode::FORBIDDEN.into_response()
        };
        if param("hub.mode") != Some("subscribe") {
            return reject("hub.mode isn't subscribe");
        }
        let token_ok = param("hub.verify_token")
            .is_some_and(|given| callback::tokens_match(given, self.verify_token.expose_secret()));
        if !token_ok {
            return reject("wrong or missing hub.verify_token");
        }
        let Some(challenge) = param("hub.challenge") else {
            return reject("no hub.challenge");
        };
        self.out
            .info(format!("handshake on {}: verified", uri.path()));
        (StatusCode::OK, challenge.to_owned()).into_response()
    }

    fn event(&self, uri: &Uri, headers: &HeaderMap, body: &[u8]) -> Response {
        if let Some(secret) = &self.app_secret {
            let signature = headers
                .get(callback::SIGNATURE_HEADER)
                .and_then(|v| v.to_str().ok());
            let problem = match signature {
                None => Some("no"),
                Some(header) => {
                    (!callback::signature_matches(secret.expose_secret(), body, header))
                        .then_some("a bad")
                }
            };
            if let Some(problem) = problem {
                self.out.warn(format!(
                    "rejected POST {}: {problem} {} signature; answered 403",
                    uri.path(),
                    callback::SIGNATURE_HEADER
                ));
                return StatusCode::FORBIDDEN.into_response();
            }
        }
        let Ok(line) = callback::compact_json(body) else {
            self.out.warn(format!(
                "rejected POST {}: the body isn't JSON; answered 400",
                uri.path()
            ));
            return StatusCode::BAD_REQUEST.into_response();
        };
        if let Err(e) = self.print(&line) {
            // Not a 200, or Meta would consider this event delivered.
            self.stop_after_failed_write(e);
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        StatusCode::OK.into_response()
    }

    /// `--quiet` narrows each event to its message and status ids, as it does for a send.
    fn print(&self, line: &str) -> io::Result<()> {
        if !self.out.is_quiet() {
            return self.out.stream_line(line);
        }
        let event = serde_json::from_str(line).unwrap_or_default();
        callback::event_ids(&event)
            .iter()
            .try_for_each(|id| self.out.stream_line(id))
    }

    /// A reader going away, as after `paloma webhook listen | head -1`, is a normal way to
    /// stop. Any other failure, such as a full disk, is an error the user needs to see.
    fn stop_after_failed_write(&self, e: io::Error) {
        if e.kind() != io::ErrorKind::BrokenPipe
            && let Ok(mut slot) = self.write_error.lock()
        {
            *slot = Some(e);
        }
        self.stop.notify_one();
    }

    fn take_write_error(&self) -> Option<io::Error> {
        self.write_error.lock().ok()?.take()
    }
}
