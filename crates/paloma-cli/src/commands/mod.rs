//! One module per top-level resource. Commands stay thin: parse, validate locally,
//! build a request with `paloma_graph::api`, hand it to [`App`], emit the result.
//!
//! To add a command: add an `Args` struct and a variant to the group's enum, and match
//! it in the group's `run`. A verb that is not built yet returns [`planned`] so it stays
//! discoverable in `--help` and exits honestly.

use clap::Subcommand;
use paloma_core::{Error, Result, validate};
use paloma_graph::api::uploads;
use paloma_graph::retry;
use secrecy::SecretString;

use crate::app::App;

mod account;
mod auth;
mod automation;
mod block;
mod calling;
mod commerce;
mod config;
mod flow;
mod group;
mod link;
mod manifest;
mod media;
mod message;
mod number;
mod profile;
mod qr;
mod send;
mod template;
mod util;
mod webhook;

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Send a message (text, template, media, interactive, ...).
    #[command(subcommand)]
    Send(send::SendCommand),
    /// Act on messages (read receipts, typing indicator, delivery history).
    #[command(subcommand)]
    Message(message::MessageCommand),
    /// Upload, inspect, download and delete media.
    #[command(subcommand)]
    Media(media::MediaCommand),
    /// Manage message templates.
    #[command(subcommand)]
    Template(template::TemplateCommand),
    /// Manage business phone numbers.
    #[command(subcommand)]
    Number(number::NumberCommand),
    /// Read and update the business profile.
    #[command(subcommand)]
    Profile(profile::ProfileCommand),
    /// Read and update shopping cart and catalog settings.
    #[command(subcommand)]
    Commerce(commerce::CommerceCommand),
    /// Configure welcome messages, ice breakers and bot commands.
    #[command(subcommand)]
    Automation(automation::AutomationCommand),
    /// Manage the block list.
    #[command(subcommand)]
    Block(block::BlockCommand),
    /// Create and manage WhatsApp groups.
    #[command(subcommand)]
    Group(group::GroupCommand),
    /// Manage webhook callbacks and field subscriptions.
    #[command(subcommand)]
    Webhook(webhook::WebhookCommand),
    /// Configure voice calling.
    #[command(subcommand)]
    Calling(calling::CallingCommand),
    /// Manage WhatsApp Flows.
    #[command(subcommand)]
    Flow(flow::FlowCommand),
    /// Manage WABA settings, users and billing, and read its analytics.
    #[command(subcommand)]
    Account(account::AccountCommand),
    /// Manage message QR codes.
    #[command(subcommand)]
    Qr(qr::QrCommand),
    /// Build wa.me click-to-chat links (offline).
    #[command(subcommand)]
    Link(link::LinkCommand),
    /// Log in, inspect and log out of a profile.
    #[command(subcommand)]
    Auth(auth::AuthCommand),
    /// Read and edit the config file.
    #[command(subcommand)]
    Config(config::ConfigCommand),
    /// Interactive first-run setup.
    Configure(config::Configure),
    /// Export live configuration as YAML manifests.
    Export(manifest::Export),
    /// Apply YAML manifests (idempotent; only drift is written).
    Apply(manifest::Apply),
    /// Show drift between manifests and live configuration (exit 1 on drift).
    Diff(manifest::Diff),
    /// Generate shell completions.
    Completion(util::Completion),
    /// Print version information.
    Version,
}

impl Command {
    pub async fn run(self, app: &App) -> Result<()> {
        match self {
            Self::Send(c) => c.run(app).await,
            Self::Message(c) => c.run(app).await,
            Self::Media(c) => c.run(app).await,
            Self::Template(c) => c.run(app).await,
            Self::Number(c) => c.run(app).await,
            Self::Profile(c) => c.run(app).await,
            Self::Commerce(c) => c.run(app).await,
            Self::Block(c) => c.run(app).await,
            Self::Group(c) => c.run(app).await,
            Self::Automation(c) => c.run(app).await,
            Self::Webhook(c) => c.run(app).await,
            Self::Calling(c) => c.run(app).await,
            Self::Flow(c) => c.run(app).await,
            Self::Account(c) => c.run(app).await,
            Self::Qr(c) => c.run(app).await,
            Self::Link(c) => c.run(app),
            Self::Auth(c) => c.run(app).await,
            Self::Config(c) => c.run(app),
            Self::Configure(_) => config::Configure::run(app),
            Self::Export(c) => c.run(app).await,
            Self::Apply(c) => c.run(app).await,
            Self::Diff(c) => c.run(app).await,
            Self::Completion(c) => {
                c.run(app);
                Ok(())
            }
            Self::Version => {
                util::version(app);
                Ok(())
            }
        }
    }
}

/// A verb wired into the tree but not built yet.
pub fn planned(command: &str, phase: &'static str) -> Result<()> {
    Err(Error::NotImplemented {
        command: format!("paloma {command}"),
        phase,
    })
}

/// Where a secret value can come from, for commands that need one from the user.
#[derive(Debug)]
pub struct SecretSource {
    pub what: &'static str,
    pub flag: &'static str,
    pub stdin_flag: &'static str,
    pub env: &'static str,
    /// A missing access token is an auth failure (exit 3); any other missing secret is
    /// invalid input (exit 5).
    pub missing_is_auth: bool,
}

/// Read a secret from stdin, the flag, the environment or a prompt, in that order. The
/// environment isn't merged by clap here, because a flag value lands in argv where other
/// processes can read it, and that needs a warning that an env value doesn't.
pub fn read_secret(
    app: &App,
    source: &SecretSource,
    flag_value: Option<&str>,
    from_stdin: bool,
) -> Result<SecretString> {
    use std::io::{IsTerminal, Read};

    let env_value = || {
        std::env::var(source.env)
            .ok()
            .filter(|v| !v.trim().is_empty())
    };
    let value = if from_stdin {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| Error::io(format!("cannot read {} from stdin", source.what), e))?;
        buf
    } else if let Some(value) = flag_value {
        app.out.warn(format!(
            "{} is visible to other processes; prefer {} or {}",
            source.flag, source.env, source.stdin_flag
        ));
        value.to_owned()
    } else if let Some(value) = env_value() {
        value
    } else if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
        dialoguer::Password::new()
            .with_prompt(source.what)
            .interact()
            .map_err(|e| Error::other(format!("prompt failed: {e}")))?
    } else {
        let message = format!(
            "no {} given: pipe it with {}, or set {}",
            source.what, source.stdin_flag, source.env
        );
        return Err(if source.missing_is_auth {
            Error::auth(message)
        } else {
            Error::validation(message)
        });
    };
    let value = value.trim();
    if value.is_empty() {
        return Err(Error::validation(format!("{} is empty", source.what)));
    }
    Ok(SecretString::from(value))
}

/// The app id: explicit, or discovered from the WABA (a read, done even under
/// --dry-run so the printed request is the real one). A WABA can have several apps and the
/// first may not be the token's, so a discovered id is always reported.
pub async fn app_id(app: &App) -> Result<String> {
    use paloma_graph::api::webhook;

    let ctx = app.ctx()?;
    if let Some(id) = ctx.app_id.clone() {
        return Ok(id);
    }
    let Some(waba) = ctx.waba_id.clone() else {
        return Err(Error::auth(
            "no app id: pass --app-id, set PALOMA_APP_ID, or set a WABA id to look it up",
        ));
    };
    let subscribed = app.fetch(&webhook::subscribed_apps(&waba)).await?.json()?;
    let id = webhook::subscribed_app_id(&subscribed)
        .ok_or_else(paloma_graph::lookup::no_subscribed_app)?;
    let count = subscribed["data"].as_array().map_or(0, Vec::len);
    if count > 1 {
        app.out.warn(format!(
            "WABA {waba} has {count} subscribed apps; using the first, {id}. Pick one with --app-id"
        ));
    } else {
        app.out
            .note(format!("using app {id}, subscribed to WABA {waba}"));
    }
    Ok(id)
}

/// A file checked for the Resumable Upload API, so a bad one fails before any API call.
#[derive(Debug)]
pub struct HandleUpload {
    path: std::path::PathBuf,
    file_name: String,
    length: u64,
    modified: Option<std::time::SystemTime>,
    mime: String,
}

impl HandleUpload {
    /// `types` is what the command can use, from the types Meta's upload takes.
    pub fn new(file: &std::path::Path, mime: Option<String>, types: &[&str]) -> Result<Self> {
        let path = validate::readable_file(file)?;
        let mime = mime.unwrap_or_else(|| paloma_graph::api::media::guess_mime(&path));
        if !types.contains(&mime.as_str()) {
            return Err(Error::validation(format!(
                "this upload can't take {mime}; it takes {}",
                types.join(", ")
            )));
        }
        let unreadable = || {
            Error::validation(format!(
                "file not found or not readable: {}",
                path.display()
            ))
        };
        let metadata = std::fs::File::open(&path)
            .and_then(|file| file.metadata())
            .map_err(|_| unreadable())?;
        if metadata.len() == 0 {
            return Err(Error::validation(format!("{} is empty", path.display())));
        }
        let file_name = path
            .file_name()
            .map_or_else(|| "upload".to_owned(), |n| n.to_string_lossy().into_owned());
        Ok(Self {
            file_name,
            length: metadata.len(),
            modified: metadata.modified().ok(),
            mime,
            path,
        })
    }

    /// What a dry run prints. The session id only exists once the first call has run.
    pub fn requests(&self, app_id: &str) -> [paloma_graph::Request; 2] {
        [
            uploads::start(app_id, &self.file_name, self.length, &self.mime),
            uploads::transfer("upload:SESSION_ID", &self.path, 0),
        ]
    }

    /// The session declared this file's size, and a resume joins bytes from two reads, so a
    /// file that changed would produce a handle for something that was never on disk.
    fn ensure_unchanged(&self) -> Result<()> {
        let unchanged = std::fs::metadata(&self.path).is_ok_and(|metadata| {
            metadata.len() == self.length && metadata.modified().ok() == self.modified
        });
        if unchanged {
            Ok(())
        } else {
            Err(Error::validation(format!(
                "{} changed during the upload, so start it again",
                self.path.display()
            )))
        }
    }

    /// Open a session and send the file. Asking Meta for the offset is what makes a repeat
    /// safe here, so this resumes where a plain POST would never be repeated. An answer that
    /// says the call itself was wrong, such as a bad token, stops at once.
    pub async fn run(&self, app: &App, app_id: &str) -> Result<(String, paloma_graph::Response)> {
        let start = uploads::start(app_id, &self.file_name, self.length, &self.mime);
        let started = app.fetch(&start).await?.json()?;
        let session = uploads::session_id(&started)
            .ok_or_else(|| Error::other("the Graph API returned no upload session"))?;
        let mut offset = 0;
        let mut attempt = 1;
        let response = loop {
            self.ensure_unchanged()?;
            let transfer = uploads::transfer(session, &self.path, offset);
            match app.fetch(&transfer).await {
                Ok(response) => break response,
                Err(e) if attempt < retry::MAX_ATTEMPTS && interrupted(&e) => {
                    app.out.note(format!("{e}: checking how much arrived"));
                    tokio::time::sleep(retry::BASE_BACKOFF).await;
                    let status = app.fetch(&uploads::status(session)).await?.json()?;
                    offset = uploads::offset(&status)
                        .filter(|offset| *offset <= self.length)
                        .ok_or_else(|| {
                            Error::other("the Graph API returned an unusable upload offset")
                        })?;
                    attempt += 1;
                    app.out.note(format!(
                        "resuming at byte {offset} of {} (attempt {attempt} of {})",
                        self.length,
                        retry::MAX_ATTEMPTS
                    ));
                }
                Err(e) => {
                    // Meta's guide doesn't say what a call with nothing left to send returns.
                    if offset == self.length {
                        app.out.note(
                            "Meta has the whole file but returned no handle, so start the \
                             upload again",
                        );
                    }
                    return Err(e);
                }
            }
        };
        self.ensure_unchanged()?;
        let handle = uploads::handle(&response.json()?)
            .ok_or_else(|| Error::other("the Graph API returned no upload handle"))?
            .to_owned();
        Ok((handle, response))
    }
}

/// A transport failure or a server error, which says nothing about the request itself.
fn interrupted(error: &Error) -> bool {
    match error {
        Error::Other(_) => true,
        Error::Api { status, .. } => retry::is_server_error(*status),
        _ => false,
    }
}

/// A report boundary or date: UNIX seconds, or `YYYY-MM-DD` at midnight UTC.
pub fn timestamp(value: &str, flag: &str) -> Result<u64> {
    let invalid = || {
        Error::validation(format!(
            "{flag} must be nonnegative UNIX seconds or a valid YYYY-MM-DD date (UTC)"
        ))
    };
    if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) {
        return value.parse().map_err(|_| invalid());
    }
    if value.len() != 10 {
        return Err(invalid());
    }
    let parsed = humantime::parse_rfc3339(&format!("{value}T00:00:00Z")).map_err(|_| invalid())?;
    parsed
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|_| invalid())
}

/// Read a whole file for a `--file` style flag, with a validation error naming it.
pub fn read_file(path: &std::path::Path) -> Result<String> {
    paloma_core::validate::readable_file(path)?;
    std::fs::read_to_string(path)
        .map_err(|e| Error::validation(format!("could not read {}: {e}", path.display())))
}

/// Read and parse a JSON file.
pub fn read_json(path: &std::path::Path) -> Result<serde_json::Value> {
    let text = read_file(path)?;
    serde_json::from_str(&text)
        .map_err(|e| Error::validation(format!("{} is not valid JSON: {e}", path.display())))
}
