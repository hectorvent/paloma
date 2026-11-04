//! Input helpers every group shares, so a `--file`, a date or a secret is read and
//! rejected the same way everywhere, and before any API call.

use std::io::Read;

use clap::ValueEnum;
use paloma_core::{Error, Result};
use secrecy::SecretString;

use crate::app::{App, interactive};

/// The CLI name of a `ValueEnum` value, which is also Meta's wire value for the enums that
/// mirror a Graph field.
pub fn enum_name(value: &impl ValueEnum) -> String {
    value
        .to_possible_value()
        .expect("no skipped variants")
        .get_name()
        .to_owned()
}

/// The value of a `--x`/`--no-x` pair: `Some(true)`, `Some(false)`, or `None` when neither was
/// given. clap's `overrides_with` keeps only the last of the two.
pub fn toggle(on: bool, off: bool) -> Option<bool> {
    match (on, off) {
        (true, _) => Some(true),
        (_, true) => Some(false),
        _ => None,
    }
}

/// The ids a bulk write acted on, one per line, from the response's `ok` list. Each entry is
/// an id string, or an object whose `field` holds one.
pub fn acted_on(response: &serde_json::Value, ok: &str, field: &str) -> Option<String> {
    let ids: Vec<&str> = response
        .pointer(ok)
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| {
            v.as_str()
                .or_else(|| v.get(field).and_then(serde_json::Value::as_str))
        })
        .collect();
    (!ids.is_empty()).then(|| ids.join("\n"))
}

/// The values `--category` takes, from [`TemplateCategory::ALL`]; any case is accepted.
pub fn template_category()
-> impl clap::builder::TypedValueParser<Value = paloma_core::TemplateCategory> {
    use clap::builder::{PossibleValuesParser, TypedValueParser};
    PossibleValuesParser::new(
        paloma_core::TemplateCategory::ALL.map(paloma_core::TemplateCategory::as_str),
    )
    .map(|name| name.parse().expect("only listed names reach here"))
}

/// All of stdin, for `--*-stdin` flags and piped payloads.
pub fn read_stdin(what: &str) -> Result<String> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| Error::io(format!("cannot read {what} from stdin"), e))?;
    Ok(buf)
}

/// Where a secret value can come from, for commands that need one from the user.
#[derive(Debug)]
pub struct SecretSource {
    pub what: &'static str,
    /// The command-line flag, for the one secret that has one (`--token`); it warns.
    pub flag: Option<&'static str>,
    pub stdin_flag: &'static str,
    pub env: &'static str,
    /// A missing access token is an auth failure (exit 3); any other missing secret is
    /// invalid input (exit 5).
    pub missing_is_auth: bool,
}

/// Read a secret from stdin, the flag, the environment or a prompt, in that order. The
/// environment isn't merged by clap here, because a flag value lands in argv where other
/// processes can read it, and that needs a warning that an env value doesn't.
pub fn read_secret(
    app: &App,
    source: &SecretSource,
    flag_value: Option<&str>,
    from_stdin: bool,
) -> Result<SecretString> {
    let env_value = || {
        std::env::var(source.env)
            .ok()
            .filter(|v| !v.trim().is_empty())
    };
    let value = if from_stdin {
        read_stdin(source.what)?
    } else if let (Some(flag), Some(value)) = (source.flag, flag_value) {
        app.out.warn(format!(
            "{flag} is visible to other processes; prefer {} or {}",
            source.env, source.stdin_flag
        ));
        value.to_owned()
    } else if let Some(value) = env_value() {
        value
    } else if interactive() {
        dialoguer::Password::new()
            .with_prompt(source.what)
            .interact()
            .map_err(|e| Error::other(format!("prompt failed: {e}")))?
    } else {
        let message = format!(
            "no {} given: pipe it with {}, or set {}",
            source.what, source.stdin_flag, source.env
        );
        return Err(if source.missing_is_auth {
            Error::auth(message)
        } else {
            Error::validation(message)
        });
    };
    let value = value.trim();
    if value.is_empty() {
        return Err(Error::validation(format!("{} is empty", source.what)));
    }
    Ok(SecretString::from(value))
}

/// A report boundary or date: UNIX seconds, or `YYYY-MM-DD` at midnight UTC.
pub fn timestamp(value: &str, flag: &str) -> Result<u64> {
    let invalid = || {
        Error::validation(format!(
            "{flag} must be nonnegative UNIX seconds or a valid YYYY-MM-DD date (UTC)"
        ))
    };
    if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) {
        return value.parse().map_err(|_| invalid());
    }
    if value.len() != 10 {
        return Err(invalid());
    }
    let parsed = humantime::parse_rfc3339(&format!("{value}T00:00:00Z")).map_err(|_| invalid())?;
    parsed
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|_| invalid())
}

/// Read a whole file for a `--file` style flag, with a validation error naming it.
pub fn read_file(path: &std::path::Path) -> Result<String> {
    paloma_core::validate::readable_file(path)?;
    std::fs::read_to_string(path)
        .map_err(|e| Error::validation(format!("could not read {}: {e}", path.display())))
}

/// Read and parse a JSON file.
pub fn read_json(path: &std::path::Path) -> Result<serde_json::Value> {
    let text = read_file(path)?;
    parse_json(path, &text)
}

/// Read a JSON file and check it parses, keeping the text exactly as written for requests
/// that send it on as a string (a Flow's JSON).
pub fn read_json_text(path: &std::path::Path) -> Result<String> {
    let text = read_file(path)?;
    parse_json(path, &text)?;
    Ok(text)
}

fn parse_json(path: &std::path::Path, text: &str) -> Result<serde_json::Value> {
    serde_json::from_str(text)
        .map_err(|e| Error::validation(format!("{} is not valid JSON: {e}", path.display())))
}

/// Split at commas, reading `\,` as a literal comma.
pub fn split_rows(rows: &str) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut chars = rows.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&',') => {
                chars.next();
                parts.last_mut().expect("starts non-empty").push(',');
            }
            ',' => parts.push(String::new()),
            _ => parts.last_mut().expect("starts non-empty").push(c),
        }
    }
    parts
}
