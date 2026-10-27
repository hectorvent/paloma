use std::path::PathBuf;

use clap::{Args, Subcommand};
use paloma_core::{Error, Result, limits, validate};
use paloma_graph::Request;
use paloma_graph::api::media;
use serde_json::json;

use super::{HandleUpload, app_id};
use crate::app::{App, field_id};
use crate::output::Output;

#[derive(Debug, Subcommand)]
pub enum MediaCommand {
    /// Upload a file; prints the media id, or a handle with --handle.
    Upload(Upload),
    /// Show a media object's metadata (including its short-lived URL).
    Get {
        /// Media id.
        media_id: String,
    },
    /// Download a media object to a file.
    Download(Download),
    /// Delete an uploaded media object.
    Delete {
        /// Media id.
        media_id: String,
    },
}

#[derive(Debug, Args)]
pub struct Upload {
    /// File to upload.
    file: PathBuf,
    /// MIME type [default: guessed from the extension].
    #[arg(long = "type", value_name = "MIME")]
    mime: Option<String>,
    /// Get a resumable upload handle, for template headers, instead of a media id.
    #[arg(long)]
    handle: bool,
    /// Upload a calling voicemail greeting (OGG/Opus, under 60 seconds).
    #[arg(long, conflicts_with = "handle")]
    voicemail_announcement: bool,
}

#[derive(Debug, Args)]
pub struct Download {
    /// Media id.
    media_id: String,
    /// Destination file (`-o` is the global --output flag, hence `-O`).
    #[arg(short = 'O', long)]
    out: PathBuf,
}

impl MediaCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        match self {
            Self::Upload(a) if a.handle => {
                let upload = HandleUpload::new(&a.file, a.mime, &limits::RESUMABLE_UPLOAD_TYPES)?;
                let app_id = app_id(app).await?;
                if app.dry_run() {
                    return app.print_dry_run(&upload.requests(&app_id));
                }
                let (handle, response) = upload.run(app, &app_id).await?;
                app.out.emit(&Output::response(&response)?.id(Some(handle)));
                Ok(())
            }
            Self::Upload(a) => {
                let path = validate::readable_file(&a.file)?;
                let mime = a.mime.unwrap_or_else(|| media::guess_mime(&path));
                let size = validate::file_size(&path)?;
                if let Some(warning) = validate::media_upload(&mime, size)? {
                    app.out.warn(warning);
                }
                let essence = mime.split(';').next().unwrap_or_default().trim();
                if a.voicemail_announcement && !essence.eq_ignore_ascii_case("audio/ogg") {
                    return Err(Error::validation(format!(
                        "a voicemail announcement must be audio/ogg with the Opus codec, not {mime}"
                    )));
                }
                let phone = app.ctx()?.require_phone_number_id()?.to_owned();
                let request = if a.voicemail_announcement {
                    media::upload_voicemail_announcement(&phone, &path, &mime)
                } else {
                    media::upload(&phone, &path, &mime)
                };
                app.send_and_emit(&request, field_id).await
            }
            Self::Get { media_id } => app.send_and_emit(&media::get(&media_id), field_id).await,
            Self::Delete { media_id } => {
                let phone = app.ctx()?.phone_number_id.clone();
                if !app.confirm_or_abort(&format!("delete media {media_id}"))? {
                    return Ok(());
                }
                app.send_and_emit(&media::delete(&media_id, phone.as_deref()), |_| {
                    Some(media_id.clone())
                })
                .await
            }
            Self::Download(a) => {
                let lookup = media::get(&a.media_id);
                if app.dry_run() {
                    return app.print_dry_run(&[
                        lookup,
                        Request::get_placeholder("<url from the media lookup>"),
                    ]);
                }
                let info: media::MediaInfo = app.fetch(&lookup).await?.parse()?;
                let url = info.url.filter(|u| !u.is_empty()).ok_or_else(|| {
                    Error::other(format!("media {} has no download URL", a.media_id))
                })?;
                let bytes = app
                    .client()?
                    .download(&Request::get_url(url), &a.out)
                    .await?;
                let path = a.out.display().to_string();
                app.out.note(format!("wrote {bytes} bytes to {path}"));
                app.out.emit(
                    &Output::new(json!({ "media_id": a.media_id, "path": path, "bytes": bytes }))
                        .id(Some(path)),
                );
                Ok(())
            }
        }
    }
}
