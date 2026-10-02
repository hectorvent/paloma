//! One parsed manifest document: the K8s-style `apiVersion`/`kind`/`metadata`/`spec`
//! envelope. `spec` stays camelCase as authored.

use paloma_core::{Error, Result};
use serde_json::{Map, Value, json};

pub const API_VERSION: &str = "paloma/v1";

#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub kind: String,
    pub metadata: Value,
    pub spec: Value,
}

impl Manifest {
    pub fn from_doc(doc: &Value) -> Result<Self> {
        let Value::Object(m) = doc else {
            return Err(Error::validation("manifest is not a YAML mapping"));
        };
        let api = m.get("apiVersion").and_then(Value::as_str).unwrap_or("");
        if api != API_VERSION {
            return Err(Error::validation(format!(
                "unsupported apiVersion '{api}' (expected {API_VERSION})"
            )));
        }
        let kind = m.get("kind").and_then(Value::as_str).unwrap_or("").trim();
        if kind.is_empty() {
            return Err(Error::validation("manifest is missing 'kind'"));
        }
        let spec = match m.get("spec") {
            None | Some(Value::Null) => Value::Object(Map::new()),
            Some(s) => s.clone(),
        };
        Ok(Self {
            kind: kind.to_owned(),
            metadata: m.get("metadata").cloned().unwrap_or(Value::Null),
            spec,
        })
    }

    /// The target from `metadata.phoneNumberId`, if any.
    pub fn phone_number_id(&self) -> Option<String> {
        match self.metadata.get("phoneNumberId")? {
            Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    }

    /// Build the envelope for export.
    #[allow(clippy::needless_pass_by_value)]
    pub fn document(kind: &str, phone_number_id: Option<&str>, spec_camel: Value) -> Value {
        let mut metadata = Map::new();
        if let Some(id) = phone_number_id {
            metadata.insert("phoneNumberId".into(), id.into());
        }
        json!({ "apiVersion": API_VERSION, "kind": kind, "metadata": metadata, "spec": spec_camel })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_envelope_and_rejects_bad_ones() {
        let m = Manifest::from_doc(&json!({"apiVersion": "paloma/v1", "kind": "Calling", "metadata": {"phoneNumberId": 123}})).unwrap();
        assert_eq!(m.kind, "Calling");
        assert_eq!(m.phone_number_id().as_deref(), Some("123"));
        assert_eq!(m.spec, json!({}));
        assert!(Manifest::from_doc(&json!({"apiVersion": "v2", "kind": "X"})).is_err());
        assert!(Manifest::from_doc(&json!({"apiVersion": "paloma/v1"})).is_err());
        assert!(Manifest::from_doc(&json!([1])).is_err());
    }
}
