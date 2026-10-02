//! Drift between a desired spec and current state. Objects compare field by field
//! (desired must be a subset of current); arrays and scalars compare by full equality,
//! since Meta treats config arrays as full replacements.

use serde_json::Value;

pub fn in_sync(desired: &Value, current: Option<&Value>) -> bool {
    match desired {
        Value::Object(d) => match current {
            Some(Value::Object(c)) => d.iter().all(|(k, v)| in_sync(v, c.get(k))),
            _ => d.is_empty(),
        },
        other => current == Some(other),
    }
}

/// The fields that would change, as `path: current -> desired`.
pub fn changes(desired: &Value, current: Option<&Value>) -> Vec<String> {
    let mut out = Vec::new();
    collect("", desired, current, &mut out);
    out
}

fn collect(path: &str, desired: &Value, current: Option<&Value>, out: &mut Vec<String>) {
    if let Value::Object(d) = desired {
        for (k, v) in d {
            let child = if path.is_empty() {
                k.clone()
            } else {
                format!("{path}.{k}")
            };
            collect(&child, v, current.and_then(|c| c.get(k)), out);
        }
        return;
    }
    if current != Some(desired) {
        let cur = current.map_or_else(|| "(unset)".to_owned(), Value::to_string);
        out.push(format!("{path}: {cur} -> {desired}"));
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn subset_objects_are_in_sync_arrays_must_match() {
        let current = json!({"status": "ENABLED", "extra": 1, "codecs": ["PCMA", "PCMU"]});
        assert!(in_sync(&json!({"status": "ENABLED"}), Some(&current)));
        assert!(!in_sync(&json!({"codecs": ["PCMA"]}), Some(&current)));
        assert!(in_sync(&json!({}), None));
    }

    #[test]
    fn changes_list_paths() {
        let current = json!({"sip": {"status": "DISABLED"}});
        let desired = json!({"sip": {"status": "ENABLED"}, "status": "ENABLED"});
        assert_eq!(
            changes(&desired, Some(&current)),
            vec![
                r#"sip.status: "DISABLED" -> "ENABLED""#.to_owned(),
                r#"status: (unset) -> "ENABLED""#.to_owned()
            ]
        );
    }
}
