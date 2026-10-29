//! The single output sink: the scripting contract lives here.
//!
//! - Structured data goes to **stdout**; every diagnostic, warning and prompt goes to
//!   **stderr**, so pipes stay clean.
//! - `--quiet` prints only [`Output::primary_id`] (`MID=$(paloma send text … -q)`).
//! - `--output` picks the rendering. JSON and raw pass the Graph response through
//!   verbatim when there is one, so `jq` sees exactly what Meta sent.
//!
//! Commands never write to stdout directly; bypassing this breaks `-q` and `-o`.

use std::fmt::Write as _;
use std::io::{IsTerminal, Write};

use anstyle::{AnsiColor, Style};
use comfy_table::{Cell, Table};
use paloma_core::Error;
use serde_json::Value;

use crate::cli::Format;

/// How a result prefers to render as a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// Nested key/value (single objects).
    Detail,
    /// Columns over the array at a JSON pointer (lists).
    Table {
        rows: &'static str,
        columns: &'static [&'static str],
    },
}

/// A command's renderable result.
#[derive(Debug, Clone)]
pub struct Output {
    /// The one identifier echoed under `--quiet` (a wamid, a media id, ...).
    pub primary_id: Option<String>,
    pub data: Value,
    /// The raw response body, passed through for `json`/`raw`.
    pub raw: Option<String>,
    pub view: View,
}

impl Output {
    pub fn new(data: Value) -> Self {
        Self {
            primary_id: None,
            data,
            raw: None,
            view: View::Detail,
        }
    }

    /// A Graph response: parsed for `table`/`yaml`, verbatim for `json`/`raw`.
    pub fn response(resp: &paloma_graph::Response) -> paloma_core::Result<Self> {
        Ok(Self {
            primary_id: None,
            data: resp.json()?,
            raw: Some(resp.text.clone()),
            view: View::Detail,
        })
    }

    #[must_use]
    pub fn id(mut self, id: Option<String>) -> Self {
        self.primary_id = id;
        self
    }

    #[must_use]
    pub fn table(mut self, rows: &'static str, columns: &'static [&'static str]) -> Self {
        self.view = View::Table { rows, columns };
        self
    }
}

#[derive(Debug, Clone)]
pub struct Printer {
    format: Format,
    quiet: bool,
    color: bool,
}

const WARN: Style = AnsiColor::Yellow.on_default().bold();
const ERROR: Style = AnsiColor::Red.on_default().bold();
const DIM: Style = Style::new().dimmed();

impl Printer {
    pub fn new(format: Format, quiet: bool, no_color: bool) -> Self {
        let no_color_env = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        let color = !no_color && !no_color_env && std::io::stderr().is_terminal();
        Self {
            format,
            quiet,
            color,
        }
    }

    pub fn format(&self) -> Format {
        self.format
    }

    pub fn quiet(&self) -> bool {
        self.quiet
    }

    /// Render a result to stdout per `--output` / `--quiet`.
    pub fn emit(&self, out: &Output) {
        let text = if self.quiet {
            match &out.primary_id {
                Some(id) => id.clone(),
                None => return,
            }
        } else {
            match self.format {
                Format::Json => out
                    .raw
                    .as_deref()
                    .map_or_else(|| pretty_json(&out.data), pretty_raw),
                Format::Raw => out.raw.clone().unwrap_or_else(|| compact(&out.data)),
                Format::Yaml => to_yaml(&out.data),
                Format::Table => render_table(&out.data, out.view),
            }
        };
        let mut stdout = std::io::stdout().lock();
        // A closed pipe (`paloma … | head`) is not an error worth reporting.
        let _ = writeln!(stdout, "{}", text.trim_end());
    }

    /// Plain text straight to stdout (documents a command renders itself, e.g. YAML
    /// manifests or completion scripts).
    #[allow(clippy::unused_self)] // a method so every write goes through the Printer
    pub fn stdout(&self, text: &str) {
        let _ = std::io::stdout().lock().write_all(text.as_bytes());
    }

    /// One line straight to stdout, flushed, for a stream. A long-running command needs the
    /// error to stop, and to tell a reader that has gone from a write that failed.
    #[allow(clippy::unused_self)] // a method so every write goes through the Printer
    pub fn stream_line(&self, line: &str) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        writeln!(stdout, "{line}").and_then(|()| stdout.flush())
    }

    /// A human-facing diagnostic on stderr.
    #[allow(clippy::unused_self)] // a method so every diagnostic goes through the Printer
    pub fn info(&self, msg: impl AsRef<str>) {
        let _ = writeln!(std::io::stderr().lock(), "{}", msg.as_ref());
    }

    pub fn note(&self, msg: impl AsRef<str>) {
        self.styled(DIM, "", msg.as_ref());
    }

    pub fn warn(&self, msg: impl AsRef<str>) {
        self.styled(WARN, "warning: ", msg.as_ref());
    }

    pub fn error(&self, err: &Error) {
        if matches!(err, Error::Silent(_)) {
            return;
        }
        self.styled(ERROR, "error: ", &err.to_string());
    }

    fn styled(&self, style: Style, prefix: &str, msg: &str) {
        let mut err = std::io::stderr().lock();
        let _ = if self.color && !prefix.is_empty() {
            writeln!(err, "{style}{prefix}{style:#}{msg}")
        } else if self.color {
            writeln!(err, "{style}{msg}{style:#}")
        } else {
            writeln!(err, "{prefix}{msg}")
        };
    }
}

fn pretty_json(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}

/// Pretty-print a raw body when it is JSON; otherwise pass it through untouched.
fn pretty_raw(raw: &str) -> String {
    serde_json::from_str::<Value>(raw).map_or_else(|_| raw.to_owned(), |v| pretty_json(&v))
}

fn compact(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub fn to_yaml(v: &Value) -> String {
    serde_norway::to_string(v).unwrap_or_else(|_| pretty_json(v))
}

fn render_table(data: &Value, view: View) -> String {
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
fn columns_table(items: &[Value], columns: &[&str]) -> String {
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

fn column_title(column: &str) -> &str {
    column.rsplit('/').next().unwrap_or(column)
}

fn column_value<'a>(item: &'a Value, column: &str) -> Option<&'a Value> {
    if column.starts_with('/') {
        item.pointer(column)
    } else {
        item.get(column)
    }
}

fn cell(v: Option<&Value>) -> String {
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

fn is_scalar(v: &Value) -> bool {
    !matches!(v, Value::Array(_) | Value::Object(_))
}

fn scalar(v: &Value) -> String {
    match v {
        Value::Null => "-".into(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// An aligned, nesting-aware key/value rendering for single objects.
fn key_value(v: &Value) -> String {
    if is_scalar(v) {
        return scalar(v);
    }
    let mut out = String::new();
    render_node(v, 0, &mut out);
    out
}

fn render_node(v: &Value, indent: usize, out: &mut String) {
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
