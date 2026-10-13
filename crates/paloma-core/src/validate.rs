//! Local pre-flight checks, run before resolving credentials or building a request, so a
//! bad invocation fails with exit 5 without spending an API call.

use std::path::{Path, PathBuf};

use crate::limits::{DATA_LOCALIZATION_REGIONS, MAX_CAPTION, MAX_TEXT_BODY, PIN_DIGITS};
use crate::{Error, Result};

/// An E.164 number (optional `+`, 7–15 digits, no leading zero). Surrounding whitespace
/// is tolerated.
pub fn recipient(to: &str) -> Result<()> {
    let trimmed = to.trim();
    if trimmed.is_empty() {
        return Err(Error::validation("recipient <to> is required"));
    }
    let digits = trimmed.strip_prefix('+').unwrap_or(trimmed);
    let valid = (7..=15).contains(&digits.len())
        && digits.bytes().all(|b| b.is_ascii_digit())
        && !digits.starts_with('0');
    if valid {
        Ok(())
    } else {
        Err(Error::validation(format!(
            "recipient '{to}' is not a valid E.164 number"
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

/// Named template parameters use lowercase ASCII letters and underscores.
pub fn template_parameter_name(name: &str) -> Result<()> {
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
        return Err(Error::validation(
            "template parameter names must contain only lowercase letters and underscores",
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
