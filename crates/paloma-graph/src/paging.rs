//! Cursor paging over a Graph API edge, shared by every `list` command.
//!
//! Graph signals "another page exists" with `paging.next`, but *always* returns
//! `paging.cursors.after`, even on the last page, so looping on the cursor alone never
//! terminates. We key off `paging.next` and only then read the cursor.
//!
//! We page by re-requesting the same edge with `after=<cursor>` rather than following the
//! absolute `paging.next` URL: that URL can embed an access token, and following a
//! server-supplied URL verbatim is a request-forgery footgun.

use std::collections::HashSet;

use paloma_core::{Error, Result};
use serde_json::{Value, json};

use crate::client::Client;
use crate::request::Request;

/// A runaway-loop backstop far above any real result set. Hitting it is an error.
pub const MAX_PAGES: usize = 100;

/// The cursor for the next page, or `None` on the last one.
pub fn next_cursor(page: &Value) -> Option<String> {
    page.pointer("/paging/next")?;
    page.pointer("/paging/cursors/after")
        .and_then(Value::as_str)
        .filter(|c| !c.trim().is_empty())
        .map(str::to_owned)
}

/// Follow every page from `start_after`, merging each `data` array into one
/// `{"data": [...]}`. Refuses to truncate silently: a repeated cursor or an implausible
/// page count is an error rather than a partial list that looks complete.
pub async fn collect_all(
    client: &Client,
    start_after: Option<String>,
    page: impl Fn(Option<&str>) -> Request,
) -> Result<Value> {
    let mut merged = Vec::new();
    let mut seen = HashSet::new();
    let mut cursor = start_after;
    for _ in 0..MAX_PAGES {
        let body = client.execute(&page(cursor.as_deref())).await?.json()?;
        if let Some(Value::Array(items)) = body.get("data") {
            merged.extend(items.iter().cloned());
        }
        match next_cursor(&body) {
            None => return Ok(json!({ "data": merged })),
            Some(next) => {
                if !seen.insert(next.clone()) {
                    return Err(Error::other(format!(
                        "Graph API returned cursor {next} twice; refusing to loop"
                    )));
                }
                cursor = Some(next);
            }
        }
    }
    Err(Error::other(format!(
        "gave up after {MAX_PAGES} pages; narrow the query or page manually with --after"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_page_has_cursor_but_no_next() {
        let last = json!({"data": [], "paging": {"cursors": {"after": "abc"}}});
        assert_eq!(next_cursor(&last), None);
        let more =
            json!({"data": [], "paging": {"cursors": {"after": "abc"}, "next": "https://..."}});
        assert_eq!(next_cursor(&more).as_deref(), Some("abc"));
    }
}
