//! Multi-document YAML in and out, including `export`'s commented-out stubs for fields
//! the live config does not set (comments are ignored by `apply`, so output round-trips).

use paloma_core::{Error, Result};
use serde::Deserialize;
use serde_json::{Map, Value};

/// Every non-empty document in a `---`-separated YAML text.
pub fn read_documents(text: &str, origin: &str) -> Result<Vec<Value>> {
    let mut docs = Vec::new();
    for de in serde_norway::Deserializer::from_str(text) {
        let v = Value::deserialize(de)
            .map_err(|e| Error::validation(format!("cannot parse YAML {origin}: {e}")))?;
        if !v.is_null() {
            docs.push(v);
        }
    }
    if docs.is_empty() {
        return Err(Error::validation(format!("no manifests found in {origin}")));
    }
    Ok(docs)
}

pub fn to_yaml(v: &Value) -> String {
    serde_norway::to_string(v).unwrap_or_default()
}

/// Render `manifest`, appending every field of `template` (camelCase) that its `spec`
/// does not set as a commented stub, in template order.
pub fn to_annotated_yaml(manifest: &Value, template: &Value) -> String {
    let empty = Map::new();
    let spec = manifest
        .get("spec")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let unset: Vec<(&String, &Value)> = template
        .as_object()
        .map(|t| t.iter().filter(|(k, _)| !spec.contains_key(*k)).collect())
        .unwrap_or_default();
    if unset.is_empty() {
        return to_yaml(manifest);
    }
    let mut envelope = manifest.clone();
    if let Some(m) = envelope.as_object_mut() {
        m.remove("spec");
    }
    let mut out = to_yaml(&envelope).trim_end().to_owned();
    out.push_str("\nspec:");
    for (k, v) in spec {
        let single = Value::Object(Map::from_iter([(k.clone(), v.clone())]));
        append_lines(&mut out, to_yaml(&single).trim_end(), "  ");
    }
    for (k, v) in unset {
        append_lines(&mut out, &stub(k, v, 0), "  # ");
    }
    out.push('\n');
    out
}

fn append_lines(out: &mut String, block: &str, prefix: &str) {
    for line in block.split('\n') {
        out.push('\n');
        out.push_str(prefix);
        out.push_str(line);
    }
}

/// scalar -> `key:`, array -> `key: []`, object -> nested stubs.
fn stub(key: &str, v: &Value, depth: usize) -> String {
    let indent = "  ".repeat(depth);
    match v {
        Value::Array(_) => format!("{indent}{key}: []"),
        Value::Object(m) => {
            let mut s = format!("{indent}{key}:");
            for (k, child) in m {
                s.push('\n');
                s.push_str(&stub(k, child, depth + 1));
            }
            s
        }
        _ => format!("{indent}{key}:"),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn profile(about: Option<&str>) -> Value {
        let mut spec = Map::new();
        if let Some(a) = about {
            spec.insert("about".into(), a.into());
        }
        json!({"apiVersion": "paloma/v1", "kind": "Profile", "metadata": {"phoneNumberId": "123"}, "spec": spec})
    }

    fn template() -> Value {
        json!({"about": "", "email": "", "vertical": "", "websites": []})
    }

    #[test]
    fn set_fields_render_normally_unset_as_comments_in_order() {
        let out = to_annotated_yaml(&profile(Some("We sell tacos")), &template());
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines.contains(&"spec:"), "{out}");
        assert!(lines.contains(&"  about: We sell tacos"), "{out}");
        assert!(lines.contains(&"  # email:"), "{out}");
        assert!(lines.contains(&"  # websites: []"), "{out}");
        assert!(!lines.contains(&"  # about:"), "{out}");
        assert!(out.find("# email:") < out.find("# vertical:"));
    }

    #[test]
    fn nested_stubs_indent_and_round_trip() {
        let m = json!({"apiVersion": "paloma/v1", "kind": "Calling", "metadata": {}, "spec": {}});
        let t = json!({"sip": {"status": "", "servers": []}, "audio": {"additionalCodecs": []}});
        let out = to_annotated_yaml(&m, &t);
        assert!(
            out.contains("  # sip:\n  #   status:\n  #   servers: []"),
            "{out}"
        );
        let back = read_documents(&out, "test").unwrap();
        assert_eq!(back[0]["kind"], "Calling");
        assert!(back[0]["spec"].is_null() || back[0]["spec"] == json!({}));
    }

    #[test]
    fn reads_multiple_documents() {
        let docs = read_documents("a: 1\n---\nb: 2\n---\n", "x").unwrap();
        assert_eq!(docs.len(), 2);
        assert!(read_documents("", "x").is_err());
        assert!(read_documents("a: [", "x").is_err());
    }
}
