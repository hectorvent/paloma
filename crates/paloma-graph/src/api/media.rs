//! Media upload, lookup and deletion.

use std::path::Path;

use serde::Deserialize;

use super::MESSAGING_PRODUCT;
use crate::{FormPart, Request};

/// `POST /{phone-number-id}/media` (multipart).
pub fn upload(phone_number_id: &str, path: &Path, mime: &str) -> Request {
    let file_name = path
        .file_name()
        .map_or_else(|| "upload".to_owned(), |n| n.to_string_lossy().into_owned());
    Request::post(format!("{phone_number_id}/media")).multipart(vec![
        FormPart::text("messaging_product", MESSAGING_PRODUCT),
        FormPart::text("type", mime),
        FormPart::file("file", path, file_name, mime),
    ])
}

/// The MIME type for a local file, guessed from its extension. `mime_guess` calls `.m4a`
/// `audio/m4a` and doesn't know `.amr`, and Meta supports both under other names.
pub fn guess_mime(path: &Path) -> String {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("m4a") => "audio/mp4".to_owned(),
        Some("amr") => "audio/amr".to_owned(),
        _ => mime_guess::from_path(path)
            .first_or_octet_stream()
            .essence_str()
            .to_owned(),
    }
}

/// `GET /{media-id}`: metadata including a short-lived download URL.
pub fn get(media_id: &str) -> Request {
    Request::get(media_id)
}

/// `DELETE /{media-id}`, scoped to the phone number that owns it.
pub fn delete(media_id: &str, phone_number_id: Option<&str>) -> Request {
    Request::delete(media_id).query_opt("phone_number_id", phone_number_id)
}

#[derive(Debug, Clone, Deserialize)]
pub struct MediaInfo {
    pub url: Option<String>,
    pub mime_type: Option<String>,
    pub file_size: Option<serde_json::Value>,
    pub id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Uploaded {
    pub id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use paloma_core::limits::MEDIA_LIMITS;

    /// Every extension in Meta's supported-media tables has to guess to a type those tables
    /// list, or a valid file would fail the pre-flight check.
    #[test]
    fn extensions_meta_lists_guess_to_types_it_lists() {
        for extension in [
            "aac", "amr", "mp3", "m4a", "ogg", "txt", "xls", "xlsx", "doc", "docx", "ppt", "pptx",
            "pdf", "jpeg", "jpg", "png", "webp", "3gp", "mp4", "M4A",
        ] {
            let mime = guess_mime(Path::new(&format!("file.{extension}")));
            assert!(
                MEDIA_LIMITS
                    .iter()
                    .any(|l| l.types.contains(&mime.as_str())),
                "{extension} guessed as {mime}"
            );
        }
    }
}
