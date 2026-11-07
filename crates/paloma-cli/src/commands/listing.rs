//! Paged reads. One page warns when more exist, so a script never mistakes a first page
//! for the whole list; `--all` follows the cursor to the end. Under `-q` a list prints
//! one row id per line, so `paloma … list -q | xargs …` works on every group.

use clap::Args;
use paloma_core::{Result, validate};
use paloma_graph::paging;
use serde_json::Value;

use crate::app::App;
use crate::output::Output;

/// The paging flags every list takes.
#[derive(Debug, Clone, Args)]
pub struct Pagination {
    /// Rows per page. Meta picks when this is left out, and refuses more than the edge allows.
    #[arg(long, value_name = "N")]
    pub limit: Option<u32>,
    /// Follow the cursor to the last page.
    #[arg(long)]
    pub all: bool,
    /// Start after this cursor, from a previous page's "more results" note.
    #[arg(long, value_name = "CURSOR")]
    pub after: Option<String>,
}

impl Pagination {
    /// Refuse a zero `--limit`, or one over the edge's documented `max`, before any call.
    pub fn check(&self, max: Option<u32>) -> Result<()> {
        validate::page_size(self.limit, max)
    }
}

/// What a list shows: its table columns, and the field holding each row's id (a field name
/// or a JSON pointer) for `-q`.
#[derive(Debug, Clone, Copy)]
pub struct Rows {
    pub columns: &'static [&'static str],
    pub id: &'static str,
}

/// A listing: one page (with a note when more exist) or every page with `--all`.
pub async fn list(
    app: &App,
    paging: &Pagination,
    page: impl Fn(Option<&str>) -> paloma_graph::Request,
    rows: Rows,
) -> Result<()> {
    let after = paging.after.clone();
    if app.is_dry_run() {
        return app.print_dry_run(&[page(after.as_deref())]);
    }
    let out = if paging.all {
        Output::new(paging::collect_all(app.client()?, after, page).await?)
    } else {
        let resp = app.fetch(&page(after.as_deref())).await?;
        let out = Output::response(&resp)?;
        if let Some(next) = paging::next_cursor(&out.data) {
            app.out.note(format!(
                "more results exist: re-run with --all, or page with --after {next}"
            ));
        }
        out
    };
    let ids = row_ids(&out.data, "/data", rows.id);
    app.out.emit(&out.table("/data", rows.columns).id(ids));
    Ok(())
}

/// The ids of the rows at `rows` (a JSON pointer, `""` for a bare array), one per line, or
/// `None` when there are none.
pub fn row_ids(data: &Value, rows: &str, id: &str) -> Option<String> {
    let ids: Vec<String> = data
        .pointer(rows)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let value = if id.starts_with('/') {
                row.pointer(id)
            } else {
                row.get(id)
            }?;
            match value {
                Value::String(s) if !s.is_empty() => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            }
        })
        .collect();
    (!ids.is_empty()).then(|| ids.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn row_ids_take_a_field_or_a_pointer_and_skip_rows_without_one() {
        let data = json!({ "data": [
            { "id": "1", "app": { "id": 7 } },
            { "name": "no id" },
            { "id": "", "app": { "id": 8 } },
        ] });
        assert_eq!(row_ids(&data, "/data", "id").as_deref(), Some("1"));
        assert_eq!(row_ids(&data, "/data", "/app/id").as_deref(), Some("7\n8"));
        assert_eq!(row_ids(&json!({ "data": [] }), "/data", "id"), None);
        assert_eq!(
            row_ids(&json!([{ "id": "a" }]), "", "id").as_deref(),
            Some("a")
        );
    }
}
