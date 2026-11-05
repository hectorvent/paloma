//! Media messages: an uploaded id or a link, with the options each type allows.

use super::{Serialize, Value, envelope, json, non_blank, validate};
use paloma_core::{Error, Result};

/// Media message types that share the `{id | link, caption?, filename?}` shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Document,
    Audio,
    Video,
    Sticker,
}

impl MediaKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Document => "document",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::Sticker => "sticker",
        }
    }

    pub const fn supports_caption(self) -> bool {
        matches!(self, Self::Image | Self::Video | Self::Document)
    }

    pub const fn supports_filename(self) -> bool {
        matches!(self, Self::Document)
    }

    pub const fn supports_voice(self) -> bool {
        matches!(self, Self::Audio)
    }
}

/// Where the media comes from: an uploaded media id or a public HTTPS link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaSource {
    Id(String),
    Link(String),
}

pub(super) fn media_object(source: &MediaSource) -> Value {
    match source {
        MediaSource::Id(id) => json!({ "id": id }),
        MediaSource::Link(link) => json!({ "link": link }),
    }
}

/// The optional parts of a media message. A kind that can't use one drops it.
#[derive(Debug, Clone, Copy, Default)]
pub struct MediaOptions<'a> {
    pub caption: Option<&'a str>,
    pub filename: Option<&'a str>,
    pub voice: bool,
    pub reply_to: Option<&'a str>,
}

pub fn media(to: &str, kind: MediaKind, source: &MediaSource, options: &MediaOptions<'_>) -> Value {
    #[derive(Serialize)]
    struct Media<'a> {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        link: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        caption: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        filename: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        voice: Option<bool>,
    }
    let (id, link) = match source {
        MediaSource::Id(id) => (Some(id.as_str()), None),
        MediaSource::Link(l) => (None, Some(l.as_str())),
    };
    let m = Media {
        id,
        link,
        caption: non_blank(options.caption).filter(|_| kind.supports_caption()),
        filename: non_blank(options.filename).filter(|_| kind.supports_filename()),
        voice: (options.voice && kind.supports_voice()).then_some(true),
    };
    envelope(to, kind.as_str(), options.reply_to, m)
}

/// A media id or an http(s) link. An error never echoes the value, since a link can carry
/// credentials in its query string.
pub fn media_reference(value: &str, flag: &str) -> Result<MediaSource> {
    validate::not_blank(value, flag)?;
    let value = value.trim();
    let invalid = || Error::validation(format!("{flag} must be a media id or an http(s) link"));
    if value.chars().any(char::is_whitespace) {
        return Err(invalid());
    }
    if crate::request::has_http_scheme(value) {
        let host = value.split_once("://").map_or("", |(_, rest)| rest);
        if host.is_empty() || host.starts_with('/') {
            return Err(invalid());
        }
        return Ok(MediaSource::Link(value.to_owned()));
    }
    if value.contains([':', '/']) {
        return Err(invalid());
    }
    Ok(MediaSource::Id(value.to_owned()))
}
