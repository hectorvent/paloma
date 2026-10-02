//! camelCase (manifest specs) <-> snake_case (Graph API) key conversion. Meta config
//! keys are pure snake_case, so this round-trips losslessly. Values are never touched.

use serde_json::{Map, Value};

pub fn to_snake(v: &Value) -> Value {
    transform(v, &camel_to_snake)
}

pub fn to_camel(v: &Value) -> Value {
    transform(v, &snake_to_camel)
}

fn transform(v: &Value, key: &dyn Fn(&str) -> String) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.iter()
                .map(|(k, v)| (key(k), transform(v, key)))
                .collect::<Map<_, _>>(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(|v| transform(v, key)).collect()),
        other => other.clone(),
    }
}

pub fn camel_to_snake(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        if c.is_uppercase() {
            out.push('_');
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

pub fn snake_to_camel(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut upper = false;
    for c in s.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn round_trips_nested_keys_and_leaves_values() {
        let camel = json!({"callIconVisibility": "DEFAULT", "sip": {"servers": [{"requestUriUserParams": {"tgrp": "a_b"}}]}});
        let snake = to_snake(&camel);
        assert_eq!(
            snake,
            json!({"call_icon_visibility": "DEFAULT", "sip": {"servers": [{"request_uri_user_params": {"tgrp": "a_b"}}]}})
        );
        assert_eq!(to_camel(&snake), camel);
    }
}
