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

use crate::cli::Format;
use anstyle::{AnsiColor, Style};
use comfy_table::{Cell, Table};
use paloma_core::Error;
use serde_json::Value;
use std::io::{IsTerminal, Write};
mod table;
use table::render_table;

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
