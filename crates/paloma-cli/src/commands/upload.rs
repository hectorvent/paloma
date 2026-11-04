//! Uploads through Meta's Resumable Upload API, which needs an app id and can stop
//! halfway, so a file is checked up front and a transfer resumes from Meta's offset.

use paloma_core::{Error, Result, validate};
use paloma_graph::api::uploads;
use paloma_graph::retry;

use crate::app::App;

/// The app id, saying on stderr which app was picked when it had to be looked up (a read,
/// done even under --dry-run so the printed request is the real one).
pub async fn app_id(app: &App) -> Result<String> {
    use paloma_graph::lookup::{AppChoice, find_app};

    let ctx = app.ctx()?;
    let choice = find_app(app.client()?, ctx.app_id(), ctx.waba_id()).await?;
    let waba = ctx.waba_id().unwrap_or_default();
    match &choice {
        AppChoice::Given(_) => {}
        AppChoice::Discovered { id, subscribed } if *subscribed > 1 => app.out.warn(format!(
            "WABA {waba} has {subscribed} subscribed apps; using the first, {id}. Pick one with --app-id"
        )),
        AppChoice::Discovered { id, .. } => app
            .out
            .note(format!("using app {id}, subscribed to WABA {waba}")),
    }
    Ok(choice.id().to_owned())
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
        Error::Transport { .. } => true,
        Error::Api { status, .. } => retry::is_server_error(*status),
        _ => false,
    }
}
