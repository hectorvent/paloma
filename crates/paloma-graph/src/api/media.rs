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

/// The MIME type for a local file, guessed from its extension.
pub fn guess_mime(path: &Path) -> String {
    mime_guess::from_path(path)
        .first_or_octet_stream()
        .essence_str()
        .to_owned()
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
