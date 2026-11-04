//! `${...}` placeholder expansion in the string values of a manifest document.
//!
//! Accepted forms inside `${...}`:
//! - `${NAME}` / `${env:NAME}`: an environment variable
//! - `${keyring:ACCOUNT}`: a secret from the OS keyring
//! - `${REF:-default}`: fall back to `default` when the reference is unset or blank
//!
//! `$$` is a literal `$`. A reference that resolves to nothing and has no default is a
//! validation error naming the field. Keys, structure and non-string scalars are left
//! alone. Resolution is supplied by the caller, keeping this module pure.

use paloma_core::{Error, Result};
use serde_json::{Map, Value};

/// Where a placeholder points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    Env,
    Keyring,
}

/// Expand every placeholder in `doc`.
pub fn interpolate(doc: &Value, lookup: &dyn Fn(Scheme, &str) -> Option<String>) -> Result<Value> {
    walk(doc, "", lookup)
}

fn walk(v: &Value, path: &str, lookup: &dyn Fn(Scheme, &str) -> Option<String>) -> Result<Value> {
    Ok(match v {
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, child) in m {
                let p = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                out.insert(k.clone(), walk(child, &p, lookup)?);
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(
            a.iter()
                .enumerate()
                .map(|(i, el)| walk(el, &format!("{path}[{i}]"), lookup))
                .collect::<Result<_>>()?,
        ),
        Value::String(s) => Value::String(expand(s, path, lookup)?),
        other => other.clone(),
    })
}

/// Expand every `${...}` and `$$` in one string.
pub(crate) fn expand(
    value: &str,
    path: &str,
    lookup: &dyn Fn(Scheme, &str) -> Option<String>,
) -> Result<String> {
    if !value.contains('$') {
        return Ok(value.to_owned());
    }
    let mut out = String::with_capacity(value.len() + 16);
    let mut rest = value;
    while let Some(i) = rest.find('$') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if let Some(after) = tail.strip_prefix("$$") {
            out.push('$');
            rest = after;
        } else if let Some(body) = tail.strip_prefix("${") {
            let close = body.find('}').ok_or_else(|| {
                Error::validation(format!(
                    "manifest field '{path}' has an unterminated placeholder: {tail}"
                ))
            })?;
            out.push_str(&resolve(&body[..close], path, lookup)?);
            rest = &body[close + 1..];
        } else {
            out.push('$');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    Ok(out)
}

fn resolve(
    expr: &str,
    path: &str,
    lookup: &dyn Fn(Scheme, &str) -> Option<String>,
) -> Result<String> {
    let (reference, default) = match expr.split_once(":-") {
        Some((r, d)) => (r, Some(d)),
        None => (expr, None),
    };
    let reference = reference.trim();
    let (scheme, name) = if let Some(n) = reference.strip_prefix("keyring:") {
        (Scheme::Keyring, n.trim())
    } else {
        (
            Scheme::Env,
            reference.strip_prefix("env:").unwrap_or(reference).trim(),
        )
    };
    if name.is_empty() {
        return Err(Error::validation(format!(
            "manifest field '{path}' has an empty placeholder reference: ${{{expr}}}"
        )));
    }
    match (
        lookup(scheme, name).filter(|v| !v.trim().is_empty()),
        default,
    ) {
        (Some(v), _) => Ok(v),
        (None, Some(d)) => Ok(d.to_owned()),
        (None, None) => {
            let source = match scheme {
                Scheme::Keyring => format!("keyring secret '{name}'"),
                Scheme::Env => format!("environment variable '{name}'"),
            };
            Err(Error::validation(format!(
                "manifest field '{path}' references unset ${{{reference}}}: set the {source} or provide a ${{...:-default}}"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn lookup(scheme: Scheme, name: &str) -> Option<String> {
        match (scheme, name) {
            (Scheme::Env, "HOST") => Some("example.com".into()),
            (Scheme::Env, "BLANK") => Some("  ".into()),
            (Scheme::Keyring, "paloma/verify") => Some("s3cret".into()),
            _ => None,
        }
    }

    #[test]
    fn expands_env_keyring_defaults_and_escapes() {
        let doc = json!({
            "spec": {
                "callbackUrl": "https://${HOST}/hook",
                "explicit": "${env:HOST}",
                "verifyToken": "${keyring:paloma/verify}",
                "fallback": "${MISSING:-dflt}",
                "blank": "${BLANK:-used}",
                "literal": "$${HOST} costs $5",
                "list": ["${HOST}", 3],
                "n": 7
            }
        });
        let out = interpolate(&doc, &lookup).unwrap();
        assert_eq!(out["spec"]["callbackUrl"], "https://example.com/hook");
        assert_eq!(out["spec"]["explicit"], "example.com");
        assert_eq!(out["spec"]["verifyToken"], "s3cret");
        assert_eq!(out["spec"]["fallback"], "dflt");
        assert_eq!(out["spec"]["blank"], "used");
        assert_eq!(out["spec"]["literal"], "${HOST} costs $5");
        assert_eq!(out["spec"]["list"], json!(["example.com", 3]));
        assert_eq!(out["spec"]["n"], 7);
    }

    #[test]
    fn unset_reference_names_the_field() {
        let err = interpolate(&json!({"spec": {"a": ["${NOPE}"]}}), &lookup).unwrap_err();
        assert_eq!(err.exit_code().code(), 5);
        assert!(err.to_string().contains("spec.a[0]"), "{err}");
        assert!(
            err.to_string().contains("environment variable 'NOPE'"),
            "{err}"
        );
    }

    #[test]
    fn malformed_placeholders_fail() {
        assert!(expand("${HOST", "f", &lookup).is_err());
        assert!(expand("${}", "f", &lookup).is_err());
        assert!(expand("${keyring:}", "f", &lookup).is_err());
    }
}
