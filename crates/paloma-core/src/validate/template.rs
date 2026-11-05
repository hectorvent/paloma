//! Template names, ids, text, variables, TTLs and analytics windows.

use super::{
    Error, MAX_TEMPLATE_DELETE_IDS, MAX_TEMPLATE_NAME, Result, TEMPLATE_TTL_30_DAYS,
    TEMPLATE_TTL_AUTHENTICATION, TEMPLATE_TTL_MARKETING, TEMPLATE_TTL_UTILITY, not_blank,
};

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

pub(super) fn template_date_to_unix(date: &str) -> Option<i64> {
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
pub fn template_ttl(category: crate::TemplateCategory, ttl: i64) -> Result<()> {
    use crate::TemplateCategory as C;
    let (range, thirty_days) = match category {
        C::Authentication => (TEMPLATE_TTL_AUTHENTICATION, true),
        C::Utility => (TEMPLATE_TTL_UTILITY, true),
        C::Marketing => (TEMPLATE_TTL_MARKETING, false),
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
