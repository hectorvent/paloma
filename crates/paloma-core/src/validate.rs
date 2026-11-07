//! Local pre-flight checks, run before resolving credentials or building a request, so a
//! bad invocation fails with exit 5 without spending an API call.

use std::path::{Path, PathBuf};

use crate::limits::{
    DATA_LOCALIZATION_REGIONS, DOCUMENT_MEDIA, MAX_BSUID_ID, MAX_CAPTION, MAX_TEMPLATE_DELETE_IDS,
    MAX_TEMPLATE_NAME, MAX_TEXT_BODY, MEDIA_LIMITS, MediaLimit, PIN_DIGITS, TEMPLATE_TTL_30_DAYS,
    TEMPLATE_TTL_AUTHENTICATION, TEMPLATE_TTL_MARKETING, TEMPLATE_TTL_UTILITY,
};
use crate::{Error, Result};
mod ids;
mod media;
mod recipient;
mod template;
pub use ids::*;
pub use media::*;
pub use recipient::*;
pub use template::*;

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

/// Reject a blank value for a flag that clap already marks required but that the user
/// passed as an empty string.
pub fn not_blank(value: &str, what: &str) -> Result<()> {
    if value.trim().is_empty() {
        Err(Error::validation(format!("{what} must not be blank")))
    } else {
        Ok(())
    }
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
    fn user_takes_a_number_or_any_bsuid() {
        for ok in [
            "15551234567",
            "+15551234567",
            "US.13491208655302741918",
            "US.ENT.11815799212886844830",
        ] {
            assert!(user(ok).is_ok(), "{ok}");
        }
        for bad in ["", "  ", "abc", "us.123"] {
            assert_eq!(user(bad).unwrap_err().exit_code().code(), 5, "{bad}");
        }
    }

    #[test]
    fn group_participant_takes_a_scoped_bsuid_but_no_parent() {
        assert!(group_participant("US.13491208655302741918").is_ok());
        assert!(group_participant("15551234567").is_ok());
        let err = group_participant("US.ENT.11815799212886844830").unwrap_err();
        assert_eq!(err.exit_code().code(), 5);
        assert!(err.to_string().contains("parent"), "{err}");
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
        use crate::TemplateCategory::{Authentication, Marketing, Utility};
        assert!(template_ttl(Utility, 30).is_ok());
        assert!(template_ttl(Utility, 43_200).is_ok());
        assert!(template_ttl(Utility, -1).is_ok());
        assert!(template_ttl(Utility, 43_201).is_err());
        assert!(template_ttl(Authentication, 900).is_ok());
        assert!(template_ttl(Authentication, 901).is_err());
        assert!(template_ttl(Marketing, 43_200).is_ok());
        assert!(template_ttl(Marketing, 120).is_err());
        assert!(template_ttl(Marketing, -1).is_err());
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
        for bad in [
            "9-17",
            "2400-2410",
            "0960-1000",
            "1700-0900",
            "0900-0900",
            "0900",
        ] {
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
