//! Media sends: a local file is checked against Meta's type and size limits, then uploaded,
//! before the message goes out; an id or link is sent as-is.

#[allow(clippy::wildcard_imports)] // part of `send`: shares its imports and helpers
use super::*;

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("source").required(true).args(["file", "id", "link"])))]
pub struct Media {
    /// Recipient (E.164 number or business-scoped user ID).
    pub(super) to: String,
    /// Treat the recipient as a group ID (see `paloma group`). Meta takes group sends for
    /// text, media and templates only.
    #[arg(long)]
    pub(super) group: bool,
    /// Upload this local file first, then send it.
    #[arg(short = 'f', long)]
    pub(super) file: Option<PathBuf>,
    /// An already-uploaded media id.
    #[arg(long)]
    pub(super) id: Option<String>,
    /// A public HTTPS URL.
    #[arg(long)]
    pub(super) link: Option<String>,
    /// Caption (image, video and document only).
    #[arg(long)]
    pub(super) caption: Option<String>,
    /// Displayed file name (document only).
    #[arg(long)]
    pub(super) filename: Option<String>,
    #[command(flatten)]
    pub(super) reply: Reply,
    #[command(flatten)]
    pub(super) policy: Policy,
}

#[derive(Debug, Args)]
pub struct Audio {
    #[command(flatten)]
    pub(super) media: Media,
    /// Send as a voice message; use a mono OGG file encoded with OPUS.
    #[arg(long)]
    pub(super) voice: bool,
}

pub(super) async fn send_media(app: &App, kind: MediaKind, a: Media, voice: bool) -> Result<()> {
    check_to(&a.to, a.group)?;
    if a.caption.is_some() && !kind.supports_caption() {
        return Err(Error::usage(format!(
            "--caption is not supported for {} messages",
            kind.as_str()
        )));
    }
    if a.filename.is_some() && !kind.supports_filename() {
        return Err(Error::usage(format!(
            "--filename is only supported for document messages, not {}",
            kind.as_str()
        )));
    }
    validate::caption(a.caption.as_deref())?;
    let options = MediaOptions {
        caption: a.caption.as_deref(),
        filename: a.filename.as_deref(),
        voice,
        reply_to: a.reply.id()?,
    };
    let file = a.file.as_deref().map(validate::readable_file).transpose()?;
    let mime = file.as_deref().map(media_api::guess_mime);
    if let (Some(path), Some(mime)) = (&file, &mime) {
        let limit = match kind {
            MediaKind::Image => &limits::IMAGE_MEDIA,
            MediaKind::Document => &limits::DOCUMENT_MEDIA,
            MediaKind::Audio => &limits::AUDIO_MEDIA,
            MediaKind::Video => &limits::VIDEO_MEDIA,
            MediaKind::Sticker => &limits::STICKER_MEDIA,
        };
        if let Some(warning) = validate::media(limit, mime, validate::file_size(path)?)? {
            app.out.warn(warning);
        }
    }
    if !a.group {
        a.policy.enforce(&a.to)?;
    }

    let phone = app.ctx()?.require_phone_number_id()?.to_owned();
    let source = match (&file, a.id, a.link) {
        (Some(path), _, _) => {
            let mime = mime.as_deref().expect("set whenever there is a file");
            let upload = media_api::upload(&phone, path, mime);
            if app.dry_run() {
                let payload = messages::media(
                    &a.to,
                    kind,
                    &MediaSource::Id("<uploaded-at-send-time>".into()),
                    &options,
                );
                let payload = messages::with_messaging_account(
                    addressed(payload, a.group),
                    app.ctx()?.messaging_account_id.as_deref(),
                );
                return app.print_dry_run(&[upload, messages::send(&phone, payload)]);
            }
            let uploaded: media_api::Uploaded = app.fetch(&upload).await?.parse()?;
            app.out.note(format!(
                "uploaded {} as media {}",
                path.display(),
                uploaded.id
            ));
            MediaSource::Id(uploaded.id)
        }
        (None, Some(id), _) => MediaSource::Id(id),
        (None, None, Some(link)) => MediaSource::Link(link),
        (None, None, None) => unreachable!("clap enforces one media source"),
    };
    let payload = messages::media(&a.to, kind, &source, &options);
    dispatch(app, addressed(payload, a.group)).await
}
