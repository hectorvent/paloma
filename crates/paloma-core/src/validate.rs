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

/// Analytics uses phone numbers, not phone-number ids or business-scoped user ids.
pub fn analytics_phone_number(value: &str) -> Result<()> {
    if !is_e164(value) {
        return Err(Error::validation(
            "analytics phone numbers must use E.164 format",
        ));
    }
    Ok(())
}

/// Analytics country filters use uppercase two-letter country codes.
pub fn country_code(value: &str) -> Result<()> {
    if value.len() != 2 || !value.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(Error::validation(
            "country must be a two-letter uppercase code, such as US",
        ));
    }
    Ok(())
}

/// Equal or reversed boundaries cannot describe an analytics interval.
pub fn analytics_range(start: u64, end: u64) -> Result<()> {
    if start >= end {
        return Err(Error::validation("--start must be earlier than --end"));
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
    template_ids_within(ids, MAX_TEMPLATE_DELETE_IDS)
}

/// The ids of a bulk request that takes at most `max`.
pub fn template_ids_within(ids: &[String], max: usize) -> Result<()> {
    if ids.len() > max {
        return Err(Error::validation(format!(
            "{} template ids given; max is {max} per request",
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

/// A template analytics `start` or `end` as Unix seconds: a UNIX timestamp or `YYYY-MM-DD`, and
/// only the date when the metrics use the account's timezone.
pub fn template_analytics_time(value: &str, what: &str, date_only: bool) -> Result<i64> {
    if date_only {
        return template_date_to_unix(value).ok_or_else(|| {
            Error::validation(format!("{what} must be YYYY-MM-DD with --waba-timezone"))
        });
    }
    let invalid = || Error::validation(format!("{what} must be YYYY-MM-DD or Unix seconds"));
    if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) {
        return value.parse().map_err(|_| invalid());
    }
    template_date_to_unix(value).ok_or_else(invalid)
}

/// Template analytics `--start` and `--end`. Unlike [`analytics_range`], they may be equal:
/// Meta moves both back to 00:00 of their day and reports whole days, so one day is a real
/// range. Its 90-day lookback is Meta's to enforce.
pub fn template_analytics_range(start: &str, end: &str, date_only: bool) -> Result<()> {
    let start = template_analytics_time(start, "--start", date_only)?;
    let end = template_analytics_time(end, "--end", date_only)?;
    if start > end {
        return Err(Error::validation("--start must not be after --end"));
    }
    Ok(())
}

fn template_date_to_unix(date: &str) -> Option<i64> {
    let [y1, y2, y3, y4, b'-', m1, m2, b'-', d1, d2] = *date.as_bytes() else {
        return None;
    };
    let number = |digits: &[u8]| {
        digits.iter().try_fold(0_i64, |total, digit| {
            digit
                .is_ascii_digit()
                .then(|| total * 10 + i64::from(digit - b'0'))
        })
    };
    let (year, month, day) = (
        number(&[y1, y2, y3, y4])?,
        number(&[m1, m2])?,
        number(&[d1, d2])?,
    );
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if !(1..=days_in_month).contains(&day) || year < 1970 {
        return None;
    }
    // Days from the civil calendar, with March as the first month so leap days fall last.
    let (shifted_year, shifted_month) = if month <= 2 {
        (year - 1, month + 9)
    } else {
        (year, month - 3)
    };
    let era = shifted_year / 400;
    let year_of_era = shifted_year - era * 400;
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some((era * 146_097 + day_of_era - 719_468) * 86_400)
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
    let special = if thirty_days { ", or 30d" } else { "" };
    Err(Error::validation(format!(
        "--ttl for {category} must be {}s to {}s{special}",
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

/// Days a group message stays pinned.
pub fn group_pin_days(days: u32) -> Result<()> {
    if (crate::limits::MIN_PIN_DAYS..=crate::limits::MAX_PIN_DAYS).contains(&days) {
        return Ok(());
    }
    Err(Error::validation(format!(
        "--days must be between {} and {}",
        crate::limits::MIN_PIN_DAYS,
        crate::limits::MAX_PIN_DAYS
    )))
}

/// An `HHMM-HHMM` opening period, 0000 to 2359, that opens before it closes. Returns the
/// two times as written.
pub fn opening_period(value: &str, flag: &str) -> Result<(String, String)> {
    let invalid = || Error::validation(format!("{flag} needs times as HHMM-HHMM, 0000 to 2359"));
    let (open, close) = value.split_once('-').ok_or_else(invalid)?;
    for time in [open, close] {
        let valid = time.len() == 4
            && time.bytes().all(|b| b.is_ascii_digit())
            && time[..2] < *"24"
            && time[2..] < *"60";
        if !valid {
            return Err(invalid());
        }
    }
    if open >= close {
        return Err(Error::validation(format!(
            "{flag} must open before it closes"
        )));
    }
    Ok((open.to_owned(), close.to_owned()))
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

/// Graph object identifiers are decimal; reject path syntax before context resolution.
pub fn numeric_id(id: &str, what: &str) -> Result<()> {
    if id.is_empty() || !id.bytes().all(|c| c.is_ascii_digit()) {
        return Err(Error::validation(format!(
            "{what} must contain only digits"
        )));
    }
    Ok(())
}

/// Meta defines timezone identifiers; IANA names are not accepted by this endpoint.
pub fn timezone_id(id: &str) -> Result<()> {
    numeric_id(id, "timezone id")
}

/// QR codes require a useful prefilled message within Meta's limit.
pub fn qr_message(message: &str) -> Result<()> {
    not_blank(message, "QR message")?;
    max_chars(message, crate::limits::MAX_QR_MESSAGE, "QR message")
}

/// Check syntax only; availability, eligibility and transfers remain Meta decisions.
pub fn business_username(value: &str) -> Result<()> {
    use crate::limits::{MAX_BUSINESS_USERNAME, MIN_BUSINESS_USERNAME};
    let lower = value.to_ascii_lowercase();
    let domain = [
        ".com", ".org", ".net", ".int", ".edu", ".gov", ".mil", ".arpa", ".html", ".htm", ".txt",
        ".xml",
    ];
    if !(MIN_BUSINESS_USERNAME..=MAX_BUSINESS_USERNAME).contains(&value.len())
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_')
        || !value.bytes().any(|b| b.is_ascii_alphabetic())
        || lower.starts_with('.')
        || lower.ends_with('.')
        || lower.contains("..")
        || lower.starts_with("www.")
        || domain.iter().any(|suffix| lower.ends_with(suffix))
    {
        return Err(Error::validation(
            "username must be 3–35 ASCII letters, digits, periods or underscores, include a letter, and follow Meta's domain/period restrictions",
        ));
    }
    Ok(())
}

/// A country calling code, as `number add` sends it: digits only, no `+` or leading zero.
pub fn calling_code(value: &str) -> Result<()> {
    if !(1..=crate::limits::MAX_CALLING_CODE_DIGITS).contains(&value.len())
        || !value.bytes().all(|b| b.is_ascii_digit())
        || value.starts_with('0')
    {
        return Err(Error::validation(format!(
            "country calling code '{value}' must be 1–3 digits without a plus sign or leading zero"
        )));
    }
    Ok(())
}

/// A group id ends up in a request path, so it's limited to the URL-safe base64 Meta uses.
pub fn group_id(value: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'='))
    {
        return Err(Error::validation(
            "group id must contain only letters, digits, '-', '_' or '='",
        ));
    }
    Ok(())
}

/// A Graph API version such as `v26.0`.
pub fn graph_version(value: &str) -> Result<()> {
    let valid = value
        .strip_prefix('v')
        .and_then(|rest| rest.split_once('.'))
        .is_some_and(|(major, minor)| {
            [major, minor]
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        });
    if !valid {
        return Err(Error::validation(format!(
            "Graph version '{value}' must look like v26.0"
        )));
    }
    Ok(())
}

/// Verification codes must never be included in a validation diagnostic.
pub fn verification_code(value: &str) -> Result<()> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::validation(
            "verification code must contain only digits",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_ids_are_safe_in_a_path() {
        assert!(group_id("Y2FwaV9ncm91cDoxNzA1NTU1MDEzOToxMjAzNjM0MDQ2OTQyMzM4MjAZD").is_ok());
        for id in ["", "../x", "a/b", "a b", "a?b"] {
            assert!(group_id(id).is_err(), "{id}");
        }
    }

    #[test]
    fn graph_versions_look_like_v26_0() {
        assert!(graph_version("v26.0").is_ok());
        for version in ["26.0", "v26", "v26.", "v.0", "vX.0", "v26.0/x", ""] {
            assert!(graph_version(version).is_err(), "{version}");
        }
    }

    #[test]
    fn calling_codes_are_short_digit_strings() {
        for code in ["1", "52", "598"] {
            assert!(calling_code(code).is_ok());
        }
        for code in ["", "+1", "0", "01", "1234", "1a"] {
            assert!(calling_code(code).is_err());
        }
    }

    #[test]
    fn business_usernames_and_verification_codes_follow_their_own_rules() {
        for username in ["Abc", "acme_store", "Acme.Shop"] {
            assert!(business_username(username).is_ok());
        }
        for username in [
            "ab",
            "1234",
            ".acme",
            "acme.",
            "acme..shop",
            "www.acme",
            "acme.com",
            "école",
        ] {
            assert!(business_username(username).is_err());
        }
        assert!(business_username(&"a".repeat(crate::limits::MAX_BUSINESS_USERNAME)).is_ok());
        assert!(business_username(&"a".repeat(crate::limits::MAX_BUSINESS_USERNAME + 1)).is_err());
        for code in ["0", "12345", "01234567"] {
            assert!(verification_code(code).is_ok());
        }
        for code in ["", "ABC", "12 34"] {
            assert!(verification_code(code).is_err());
        }
    }

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
    fn group_pins_last_one_to_thirty_days() {
        assert!(group_pin_days(1).is_ok());
        assert!(group_pin_days(30).is_ok());
        assert!(group_pin_days(0).is_err());
        assert!(group_pin_days(31).is_err());
    }

    #[test]
    fn opening_periods_are_hhmm_and_open_before_closing() {
        assert_eq!(
            opening_period("0900-1700", "--open").unwrap(),
            ("0900".into(), "1700".into())
        );
        for bad in ["9-17", "2400-2410", "0960-1000", "1700-0900", "0900-0900", "0900"] {
            assert!(opening_period(bad, "--open").is_err(), "{bad}");
        }
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
    #[test]
    fn template_analytics_times_are_timestamps_or_dates() {
        assert!(template_analytics_time("1718064000", "--start", false).is_ok());
        assert!(template_analytics_time("2026-11-02", "--start", false).is_ok());
        assert!(template_analytics_time("2026-11-02", "--start", true).is_ok());
        assert!(template_analytics_time("1718064000", "--start", true).is_err());
        assert!(template_analytics_time("yesterday", "--start", false).is_err());
        assert!(template_analytics_time("", "--start", false).is_err());
        assert!(template_analytics_time("2026-02-30", "--start", false).is_err());
        assert!(template_analytics_time("2026-13-01", "--start", true).is_err());
        assert_eq!(
            template_analytics_time("1970-01-02", "--start", true).unwrap(),
            86_400
        );
    }

    #[test]
    fn template_analytics_range_stops_only_when_it_runs_backwards() {
        assert!(template_analytics_range("2026-10-01", "2026-10-08", false).is_ok());
        assert!(template_analytics_range("2026-10-01", "2026-10-01", true).is_ok());
        assert!(template_analytics_range("1718064000", "1718122745", false).is_ok());
        assert!(template_analytics_range("2026-10-08", "2026-10-01", true).is_err());
        assert!(template_analytics_range("1718122745", "1718064000", false).is_err());
        assert!(template_analytics_range("2026-01-02", "1718064000", false).is_err());
    }
}
