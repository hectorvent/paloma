//! Local pre-flight checks, run before resolving credentials or building a request, so a
//! bad invocation fails with exit 5 without spending an API call.

use std::path::{Path, PathBuf};

use crate::limits::{
    DATA_LOCALIZATION_REGIONS, DOCUMENT_MEDIA, MAX_BSUID_ID, MAX_CAPTION, MAX_TEMPLATE_DELETE_IDS,
    MAX_TEMPLATE_NAME, MAX_TEXT_BODY, MEDIA_LIMITS, MediaLimit, PIN_DIGITS, TEMPLATE_TTL_30_DAYS,
    TEMPLATE_TTL_AUTHENTICATION, TEMPLATE_TTL_MARKETING, TEMPLATE_TTL_UTILITY,
};
use crate::{Error, Result};

/// The two kinds of business-scoped user ID (BSUID).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bsuid {
    /// `US.13491208655302741918`, scoped to one business portfolio.
    Scoped,
    /// `US.ENT.11815799212886844830`, shared by the portfolios of a parent BSUID account.
    Parent,
}

/// Classify a BSUID: a two-letter country code, a period, then up to 128 alphanumerics. A
/// phone number holds only digits and a leading `+`, so one argument can accept both.
/// Meta fails a request whose BSUID was altered, so a lowercase country is refused rather
/// than corrected. Surrounding whitespace is tolerated.
pub fn bsuid(value: &str) -> Option<Bsuid> {
    let (country, rest) = value.trim().split_once('.')?;
    if country.len() != 2 || !country.bytes().all(|b| b.is_ascii_uppercase()) {
        return None;
    }
    let (kind, id) = match rest.strip_prefix("ENT.") {
        Some(id) => (Bsuid::Parent, id),
        None => (Bsuid::Scoped, rest),
    };
    let valid =
        (1..=MAX_BSUID_ID).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric());
    valid.then_some(kind)
}

/// Optional `+`, 7–15 digits, no leading zero.
fn is_e164(value: &str) -> bool {
    let trimmed = value.trim();
    let digits = trimmed.strip_prefix('+').unwrap_or(trimmed);
    (7..=15).contains(&digits.len())
        && digits.bytes().all(|b| b.is_ascii_digit())
        && !digits.starts_with('0')
}

/// An E.164 number or a BSUID, parent BSUIDs included. Surrounding whitespace is tolerated.
pub fn recipient(to: &str) -> Result<()> {
    if to.trim().is_empty() {
        return Err(Error::validation("recipient <to> is required"));
    }
    if is_e164(to) || bsuid(to).is_some() {
        Ok(())
    } else {
        Err(Error::validation(format!(
            "recipient '{to}' is not a valid E.164 number or business-scoped user id \
             (such as US.13491208655302741918)"
        )))
    }
}

/// A user to block or unblock: like [`recipient`], except that Meta fails a parent BSUID
/// on this endpoint.
pub fn block_user(user: &str) -> Result<()> {
    if bsuid(user) == Some(Bsuid::Parent) {
        return Err(Error::validation(format!(
            "'{user}' is a parent business-scoped user id, which can't be blocked or unblocked; \
             use the user's phone number or their business-scoped user id"
        )));
    }
    if is_e164(user) || bsuid(user).is_some() {
        Ok(())
    } else {
        Err(Error::validation(format!(
            "user '{user}' is not a valid E.164 number or business-scoped user id \
             (such as US.13491208655302741918)"
        )))
    }
}

pub fn text_body(body: &str) -> Result<()> {
    if body.is_empty() {
        return Err(Error::validation("message body is empty"));
    }
    let len = body.chars().count();
    if len > MAX_TEXT_BODY {
        return Err(Error::validation(format!(
            "text body is {len} chars; max is {MAX_TEXT_BODY}"
        )));
    }
    Ok(())
}

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

/// Reject a blank value for a flag that clap already marks required but that the user
/// passed as an empty string.
pub fn not_blank(value: &str, what: &str) -> Result<()> {
    if value.trim().is_empty() {
        Err(Error::validation(format!("{what} must not be blank")))
    } else {
        Ok(())
    }
}

/// QR codes are opaque alphanumeric identifiers, not Graph numeric node ids.
pub fn qr_code(code: &str) -> Result<()> {
    if code.len() != crate::limits::QR_CODE_LENGTH
        || !code.bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return Err(Error::validation(format!(
            "QR code must contain {} ASCII letters or digits",
            crate::limits::QR_CODE_LENGTH
        )));
    }
    Ok(())
}

/// An opaque Graph node id must remain a single path segment, never a URL or query.
pub fn graph_node_id(value: &str, what: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return Err(Error::validation(format!(
            "{what} must contain only ASCII letters, digits, underscores or hyphens"
        )));
    }
    Ok(())
}

/// Currency support is account-dependent; validate the ISO code shape locally.
pub fn currency_code(value: &str) -> Result<()> {
    if value.len() != 3 || !value.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(Error::validation(
            "currency must be a three-letter uppercase code, such as INR",
        ));
    }
    Ok(())
}

/// A length check in characters, not bytes. The error never echoes the value.
pub fn max_chars(value: &str, max: usize, what: &str) -> Result<()> {
    let len = value.chars().count();
    if len > max {
        return Err(Error::validation(format!(
            "{what} is {len} chars; max is {max}"
        )));
    }
    Ok(())
}

/// Named template parameters use lowercase ASCII letters and underscores.
pub fn template_parameter_name(name: &str) -> Result<()> {
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
        return Err(Error::validation(
            "template parameter names must contain only lowercase letters and underscores",
        ));
    }
    Ok(())
}

/// A template id: Graph ids are digits, and bulk delete sends them unquoted.
pub fn template_id(id: &str) -> Result<()> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::validation(format!(
            "template id '{id}' must contain only digits"
        )));
    }
    Ok(())
}

/// The ids of a bulk delete. Meta fails the whole request over one bad id.
pub fn template_ids(ids: &[String]) -> Result<()> {
    if ids.len() > MAX_TEMPLATE_DELETE_IDS {
        return Err(Error::validation(format!(
            "{} template ids given; max is {MAX_TEMPLATE_DELETE_IDS} per request",
            ids.len()
        )));
    }
    let mut seen = std::collections::HashSet::new();
    if let Some(id) = ids.iter().find(|id| !seen.insert(*id)) {
        return Err(Error::validation(format!(
            "template id {id} is given twice"
        )));
    }
    ids.iter().try_for_each(|id| template_id(id))
}

/// Template names are lowercase letters, digits and underscores.
pub fn template_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err(Error::validation(
            "template names must contain only lowercase letters, digits and underscores",
        ));
    }
    template_text(name, MAX_TEMPLATE_NAME, "template name")
}

/// A piece of template text: not blank and within its limit, counted in characters.
pub fn template_text(value: &str, max: usize, what: &str) -> Result<()> {
    not_blank(value, what)?;
    let len = value.chars().count();
    if len > max {
        return Err(Error::validation(format!(
            "{what} is {len} chars; max is {max}"
        )));
    }
    Ok(())
}

/// The `{{...}}` placeholders in a template string, in order of appearance.
#[must_use]
pub fn template_variables(text: &str) -> Vec<&str> {
    let mut variables = Vec::new();
    let mut rest = text;
    while let Some((_, after_open)) = rest.split_once("{{") {
        let Some((variable, after_close)) = after_open.split_once("}}") else {
            break;
        };
        variables.push(variable.trim());
        rest = after_close;
    }
    variables
}

/// A TTL within the range Meta documents for the category. Other categories have no
/// documented range, so Meta decides.
pub fn template_ttl(category: &str, ttl: i64) -> Result<()> {
    let (range, thirty_days) = match category.to_ascii_uppercase().as_str() {
        "AUTHENTICATION" => (TEMPLATE_TTL_AUTHENTICATION, true),
        "UTILITY" => (TEMPLATE_TTL_UTILITY, true),
        "MARKETING" => (TEMPLATE_TTL_MARKETING, false),
        _ => return Ok(()),
    };
    if range.contains(&ttl) || (thirty_days && ttl == TEMPLATE_TTL_30_DAYS) {
        return Ok(());
    }
    let special = if thirty_days {
        format!(", or {TEMPLATE_TTL_30_DAYS} for 30 days")
    } else {
        String::new()
    };
    Err(Error::validation(format!(
        "--ttl-seconds for {category} must be {} to {}{special}",
        range.start(),
        range.end()
    )))
}

pub fn pin(pin: &str) -> Result<()> {
    if pin.len() == PIN_DIGITS && pin.bytes().all(|b| b.is_ascii_digit()) {
        Ok(())
    } else {
        Err(Error::validation(format!(
            "PIN must be exactly {PIN_DIGITS} digits"
        )))
    }
}

/// A `--limit` page size: at least one, and at most `max` where the edge documents one.
pub fn page_size(limit: Option<u32>, max: Option<u32>) -> Result<()> {
    match (limit, max) {
        (Some(n), Some(max)) if n == 0 || n > max => Err(Error::validation(format!(
            "--limit must be between 1 and {max}"
        ))),
        (Some(0), None) => Err(Error::validation("--limit must be at least 1")),
        _ => Ok(()),
    }
}

/// A country that local storage supports, as an uppercase ISO 3166 code.
pub fn data_localization_region(region: &str) -> Result<()> {
    if DATA_LOCALIZATION_REGIONS.contains(&region) {
        Ok(())
    } else {
        Err(Error::validation(format!(
            "'{region}' is not a supported data localization region; use one of {}",
            DATA_LOCALIZATION_REGIONS.join(", ")
        )))
    }
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
fn mime_essence(mime: &str) -> String {
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

fn size_label(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{} MB", bytes / (1024 * 1024))
    } else {
        format!("{} KB", bytes / 1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipient_accepts_e164() {
        for ok in ["15551234567", "+15551234567", " 15551234567 ", "1234567"] {
            assert!(recipient(ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn recipient_rejects_garbage() {
        for bad in [
            "",
            "  ",
            "0123456789",
            "123456",
            "1234567890123456",
            "+1-555-123",
            "abc",
        ] {
            let err = recipient(bad).unwrap_err();
            assert_eq!(err.exit_code().code(), 5, "{bad}");
        }
    }

    #[test]
    fn bsuid_shapes_are_classified_and_never_overlap_with_a_number() {
        for scoped in ["US.13491208655302741918", " BR.aB3 ", "US.ENT"] {
            assert_eq!(bsuid(scoped), Some(Bsuid::Scoped), "{scoped}");
            assert!(recipient(scoped).is_ok(), "{scoped}");
        }
        let parent = "US.ENT.11815799212886844830";
        assert_eq!(bsuid(parent), Some(Bsuid::Parent));
        assert!(recipient(parent).is_ok());
        for number in ["15551234567", "+15551234567", "1234567"] {
            assert_eq!(bsuid(number), None, "{number}");
        }
    }

    #[test]
    fn a_malformed_bsuid_is_refused() {
        let longest = format!("US.{}", "1".repeat(MAX_BSUID_ID));
        assert_eq!(bsuid(&longest), Some(Bsuid::Scoped));
        let parent = format!("US.ENT.{}", "1".repeat(MAX_BSUID_ID));
        assert_eq!(bsuid(&parent), Some(Bsuid::Parent));
        for bad in [
            "US.",
            "US.ENT.",
            "us.123",
            "USA.123",
            "U.123",
            "1S.123",
            ".123",
            "US123",
            "US.12.34",
            "US.ENT.12.34",
            "US.12-34",
            "US.12 34",
            "US.ñ",
            &format!("{longest}1"),
            &format!("{parent}1"),
        ] {
            assert_eq!(bsuid(bad), None, "{bad}");
            let err = recipient(bad).unwrap_err();
            assert_eq!(err.exit_code().code(), 5, "{bad}");
        }
    }

    #[test]
    fn block_user_takes_a_number_or_a_bsuid_but_no_parent() {
        for ok in ["15551234567", "+15551234567", "US.13491208655302741918"] {
            assert!(block_user(ok).is_ok(), "{ok}");
        }
        let err = block_user("US.ENT.11815799212886844830").unwrap_err();
        assert_eq!(err.exit_code().code(), 5);
        assert!(err.to_string().contains("parent"), "{err}");
        for bad in ["", "  ", "abc", "us.123"] {
            assert_eq!(block_user(bad).unwrap_err().exit_code().code(), 5, "{bad}");
        }
    }

    #[test]
    fn text_body_counts_characters_not_bytes() {
        let emoji = "😀".repeat(MAX_TEXT_BODY);
        assert!(text_body(&emoji).is_ok());
        assert!(text_body(&format!("{emoji}x")).is_err());
        assert!(text_body("").is_err());
    }

    #[test]
    fn caption_limit() {
        assert!(caption(None).is_ok());
        assert!(caption(Some(&"a".repeat(MAX_CAPTION))).is_ok());
        assert!(caption(Some(&"a".repeat(MAX_CAPTION + 1))).is_err());
    }

    #[test]
    fn template_variables_are_found_in_order() {
        assert_eq!(
            template_variables("Hi {{1}}, {{ 2 }} and {{name}}{{broken"),
            ["1", "2", "name"]
        );
        assert!(template_variables("no variables").is_empty());
    }

    #[test]
    fn template_ttl_ranges_follow_the_category() {
        assert!(template_ttl("UTILITY", 30).is_ok());
        assert!(template_ttl("utility", 43_200).is_ok());
        assert!(template_ttl("UTILITY", -1).is_ok());
        assert!(template_ttl("UTILITY", 43_201).is_err());
        assert!(template_ttl("AUTHENTICATION", 900).is_ok());
        assert!(template_ttl("AUTHENTICATION", 901).is_err());
        assert!(template_ttl("MARKETING", 43_200).is_ok());
        assert!(template_ttl("MARKETING", 120).is_err());
        assert!(template_ttl("MARKETING", -1).is_err());
    }

    #[test]
    fn template_names_are_lowercase_alphanumeric() {
        assert!(template_name("order_ready_2").is_ok());
        assert!(template_name("Order").is_err());
        assert!(template_name("order ready").is_err());
        assert!(template_name(&"a".repeat(513)).is_err());
    }

    #[test]
    fn max_chars_counts_characters_and_never_echoes_the_value() {
        assert!(max_chars(&"😀".repeat(3), 3, "x").is_ok());
        let err = max_chars("secret", 3, "field").unwrap_err();
        assert_eq!(err.exit_code().code(), 5);
        assert!(!err.to_string().contains("secret"), "{err}");
    }

    #[test]
    fn media_applies_the_limit_at_its_exact_boundary() {
        use crate::limits::{IMAGE_MEDIA, STICKER_MEDIA};
        let max = IMAGE_MEDIA.max_bytes;
        assert_eq!(media(&IMAGE_MEDIA, "image/png", max).unwrap(), None);
        assert!(media(&IMAGE_MEDIA, "image/png", max + 1).is_err());
        assert!(media(&IMAGE_MEDIA, "image/webp", 1).is_err());
        // The animated limit, since a static sticker can't be told apart here.
        assert!(media(&STICKER_MEDIA, "image/webp", 500 * 1024).is_ok());
        assert!(media(&STICKER_MEDIA, "image/webp", 500 * 1024 + 1).is_err());
    }

    #[test]
    fn media_ignores_type_parameters_and_case() {
        use crate::limits::AUDIO_MEDIA;
        assert_eq!(
            media(&AUDIO_MEDIA, "audio/ogg; codecs=opus", 1).unwrap(),
            None
        );
        assert_eq!(media(&AUDIO_MEDIA, "AUDIO/OGG", 1).unwrap(), None);
    }

    #[test]
    fn an_unlisted_document_warns_but_still_has_the_size_limit() {
        let warning = media(&DOCUMENT_MEDIA, "application/zip", 1).unwrap();
        assert!(warning.unwrap().contains("application/zip"));
        assert!(
            media(
                &DOCUMENT_MEDIA,
                "application/zip",
                DOCUMENT_MEDIA.max_bytes + 1
            )
            .is_err()
        );
    }

    #[test]
    fn media_upload_picks_the_limit_from_the_type() {
        assert_eq!(media_upload("video/mp4", 1).unwrap(), None);
        assert!(media_upload("video/mp4", 16 * 1024 * 1024 + 1).is_err());
        assert!(media_upload("image/webp", 500 * 1024 + 1).is_err());
        for refused in ["image/gif", "video/quicktime", "audio/wav", " "] {
            let err = media_upload(refused, 1).unwrap_err();
            assert_eq!(err.exit_code().code(), 5, "{refused}");
        }
        assert!(media_upload("application/zip", 1).unwrap().is_some());
    }

    #[test]
    fn pin_is_six_digits() {
        assert!(pin("123456").is_ok());
        assert!(pin("12345").is_err());
        assert!(pin("12345a").is_err());
    }

    #[test]
    fn page_size_bounds() {
        assert!(page_size(None, Some(100)).is_ok());
        assert!(page_size(Some(100), Some(100)).is_ok());
        assert!(page_size(Some(101), Some(100)).is_err());
        assert!(page_size(Some(0), Some(100)).is_err());
        assert!(page_size(Some(5000), None).is_ok());
        assert!(page_size(Some(0), None).is_err());
    }

    #[test]
    fn data_localization_region_is_a_closed_set() {
        for ok in DATA_LOCALIZATION_REGIONS {
            assert!(data_localization_region(ok).is_ok(), "{ok}");
        }
        for bad in ["", "US", "de", "DEU"] {
            let err = data_localization_region(bad).unwrap_err();
            assert_eq!(err.exit_code().code(), 5, "{bad}");
        }
    }
}
