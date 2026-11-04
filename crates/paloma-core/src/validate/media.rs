//! Local files and their media types, checked against Meta's per-type limits.

#[allow(clippy::wildcard_imports)] // part of `validate`: shares its imports and helpers
use super::*;

pub fn caption(caption: Option<&str>) -> Result<()> {
    if let Some(c) = caption {
        let len = c.chars().count();
        if len > MAX_CAPTION {
            return Err(Error::validation(format!(
                "caption is {len} chars; max is {MAX_CAPTION}"
            )));
        }
    }
    Ok(())
}

/// The path must be a regular, readable file.
pub fn readable_file(path: &Path) -> Result<PathBuf> {
    match std::fs::metadata(path) {
        Ok(m) if m.is_file() => Ok(path.to_path_buf()),
        _ => Err(Error::validation(format!(
            "file not found or not readable: {}",
            path.display()
        ))),
    }
}

/// The size of a file that [`readable_file`] accepted.
pub fn file_size(path: &Path) -> Result<u64> {
    std::fs::metadata(path).map(|m| m.len()).map_err(|_| {
        Error::validation(format!(
            "file not found or not readable: {}",
            path.display()
        ))
    })
}

/// The type without parameters: `audio/ogg; codecs=opus` is still `audio/ogg`.
pub(super) fn mime_essence(mime: &str) -> String {
    mime.split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

/// Check a file against one media type. `Ok(Some(..))` is a warning to print: Meta may take
/// the file, but doesn't support its type.
pub fn media(limit: &MediaLimit, mime: &str, bytes: u64) -> Result<Option<String>> {
    let essence = mime_essence(mime);
    let listed = limit.types.contains(&essence.as_str());
    if !listed && !limit.allows_unlisted {
        return Err(Error::validation(format!(
            "{} files must be {}; this one is {mime}",
            limit.kind,
            limit.types.join(", ")
        )));
    }
    if bytes > limit.max_bytes {
        return Err(Error::validation(format!(
            "{} file is {bytes} bytes; max is {}",
            limit.kind,
            size_label(limit.max_bytes)
        )));
    }
    Ok((!listed).then(|| {
        format!(
            "{mime} isn't a {} type Meta supports, so it may not display correctly",
            limit.kind
        )
    }))
}

/// Check a file whose use isn't known yet, so its type picks the limit. Audio, image and
/// video types Meta doesn't list are refused. Anything else could still go out as a
/// document, which is the one type Meta leaves open.
pub fn media_upload(mime: &str, bytes: u64) -> Result<Option<String>> {
    let essence = mime_essence(mime);
    if essence.is_empty() {
        return Err(Error::validation("--type must not be blank"));
    }
    if let Some(limit) = MEDIA_LIMITS
        .iter()
        .find(|limit| limit.types.contains(&essence.as_str()))
    {
        return media(limit, mime, bytes);
    }
    let family = essence.split('/').next().unwrap_or_default();
    if let Some(limit) = MEDIA_LIMITS
        .iter()
        .find(|limit| limit.kind == family && !limit.allows_unlisted)
    {
        return media(limit, mime, bytes);
    }
    media(&DOCUMENT_MEDIA, mime, bytes)
}

pub(super) fn size_label(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{} MB", bytes / (1024 * 1024))
    } else {
        format!("{} KB", bytes / 1024)
    }
}
