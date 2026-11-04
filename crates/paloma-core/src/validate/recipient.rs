//! Who a message or setting is for: E.164 numbers, business-scoped user IDs, usernames
//! and calling codes.

#[allow(clippy::wildcard_imports)] // part of `validate`: shares its imports and helpers
use super::*;

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
pub(super) fn is_e164(value: &str) -> bool {
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

/// Analytics uses phone numbers, not phone-number ids or business-scoped user ids.
pub fn analytics_phone_number(value: &str) -> Result<()> {
    if !is_e164(value) {
        return Err(Error::validation(
            "analytics phone numbers must use E.164 format",
        ));
    }
    Ok(())
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
