//! `-o table`: lists become aligned columns, and a single object becomes key/value rows
//! with nested objects indented under their key.

use std::fmt::Write as _;

use super::{Cell, Table, Value, View};

pub(super) fn render_table(data: &Value, view: View) -> String {
    match view {
        View::Table { rows, columns } => match data.pointer(rows).and_then(Value::as_array) {
            Some(items) => columns_table(items, columns),
            None => key_value(data),
        },
        View::Detail => key_value(data),
    }
}

/// kubectl-style columns: uppercase headers, no borders, missing values as `-`. A column
/// is a field of the row, or a JSON pointer into it, titled by its last segment.
pub(super) fn columns_table(items: &[Value], columns: &[&str]) -> String {
    if items.is_empty() {
        return "(none)".into();
    }
    let mut table = Table::new();
    table.load_style(comfy_table::presets::NOTHING);
    table.set_header(
        columns
            .iter()
            .map(|c| Cell::new(column_title(c).replace('_', " ").to_uppercase())),
    );
    for item in items {
        table.add_row(
            columns
                .iter()
                .map(|c| Cell::new(cell(column_value(item, c)))),
        );
    }
    table
        .lines()
        .map(|l| l.trim().to_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn column_title(column: &str) -> &str {
    column.rsplit('/').next().unwrap_or(column)
}

pub(super) fn column_value<'a>(item: &'a Value, column: &str) -> Option<&'a Value> {
    if column.starts_with('/') {
        item.pointer(column)
    } else {
        item.get(column)
    }
}

pub(super) fn cell(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "-".into(),
        Some(Value::String(s)) if s.is_empty() => "-".into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) if a.iter().all(is_scalar) => {
            a.iter().map(scalar).collect::<Vec<_>>().join(",")
        }
        Some(other) => other.to_string(),
    }
}

pub(super) fn is_scalar(v: &Value) -> bool {
    !matches!(v, Value::Array(_) | Value::Object(_))
}

pub(super) fn scalar(v: &Value) -> String {
    match v {
        Value::Null => "-".into(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// An aligned, nesting-aware key/value rendering for single objects.
pub(super) fn key_value(v: &Value) -> String {
    if is_scalar(v) {
        return scalar(v);
    }
    let mut out = String::new();
    render_node(v, 0, &mut out);
    out
}

pub(super) fn render_node(v: &Value, indent: usize, out: &mut String) {
    let pad = "  ".repeat(indent);
    match v {
        Value::Object(m) if m.is_empty() => {
            let _ = writeln!(out, "{pad}(empty)");
        }
        Value::Object(m) => {
            let width = m.keys().map(String::len).max().unwrap_or(0);
            for (k, child) in m {
                match child {
                    Value::Array(a) if a.iter().all(is_scalar) => {
                        let inline = a.iter().map(scalar).collect::<Vec<_>>().join(", ");
                        let _ = writeln!(out, "{pad}{k:<width$}  [{inline}]");
                    }
                    c if is_scalar(c) => {
                        let _ = writeln!(out, "{pad}{k:<width$}  {}", scalar(c));
                    }
                    c => {
                        let _ = writeln!(out, "{pad}{k}");
                        render_node(c, indent + 1, out);
                    }
                }
            }
        }
        Value::Array(a) if a.is_empty() => {
            let _ = writeln!(out, "{pad}(none)");
        }
        Value::Array(a) => {
            for (i, el) in a.iter().enumerate() {
                if is_scalar(el) {
                    let _ = writeln!(out, "{pad}- {}", scalar(el));
                } else {
                    let _ = writeln!(out, "{pad}- [{i}]");
                    render_node(el, indent + 1, out);
                }
            }
        }
        other => {
            let _ = writeln!(out, "{pad}{}", scalar(other));
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn key_value_aligns_and_nests() {
        let v = json!({"id": "1", "display_name": "x", "tags": ["a", "b"], "nested": {"k": null}});
        assert_eq!(
            key_value(&v),
            "id            1\ndisplay_name  x\ntags          [a, b]\nnested\n  k  -\n"
        );
    }

    #[test]
    fn columns_table_uses_headers_and_dashes() {
        let rows = vec![
            json!({"name": "a", "status": "APPROVED"}),
            json!({"name": "bb"}),
        ];
        let t = columns_table(&rows, &["name", "status"]);
        let lines: Vec<&str> = t.lines().collect();
        assert!(lines[0].starts_with("NAME"));
        assert!(lines[0].contains("STATUS"));
        assert!(lines[2].contains('-'));
        assert_eq!(columns_table(&[], &["x"]), "(none)");
    }

    #[test]
    fn a_pointer_column_reads_a_nested_field_under_its_last_segment() {
        let rows = vec![json!({"node": {"delivery_status": "SENT"}}), json!({})];
        let t = columns_table(&rows, &["/node/delivery_status"]);
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines[0], "DELIVERY STATUS");
        assert_eq!(lines[1], "SENT");
        assert_eq!(lines[2], "-");
    }
}
