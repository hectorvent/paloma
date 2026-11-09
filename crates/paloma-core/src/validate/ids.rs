//! Identifiers and codes that end up in a Graph path or query: node ids, versions,
//! countries, PINs and verification codes.

use super::{DATA_LOCALIZATION_REGIONS, Error, PIN_DIGITS, Result, max_chars, not_blank};

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

/// Analytics country filters use uppercase two-letter country codes.
pub fn country_code(value: &str) -> Result<()> {
    if value.len() != 2 || !value.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(Error::validation(
            "country must be a two-letter uppercase code, such as US",
        ));
    }
    Ok(())
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

/// A Graph API version such as `v27.0`.
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
            "Graph version '{value}' must look like v27.0"
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
