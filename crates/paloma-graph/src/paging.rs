//! Cursor paging over a Graph API edge, shared by every `list` command.
//!
//! Graph signals "another page exists" with `paging.next`, but *always* returns
//! `paging.cursors.after`, even on the last page, so looping on the cursor alone never
//! terminates. We key off `paging.next` and only then read the cursor.
//!
//! We page by re-requesting the same edge with `after=<cursor>` rather than following the
//! absolute `paging.next` URL: that URL can embed an access token, and following a
//! server-supplied URL verbatim is a request-forgery footgun.
//!
//! `block_users` is the exception: its documented `paging` holds only `cursors`, so a
//! missing `paging.next` proves nothing there and stopping on it could truncate the list.
//! That edge is walked until a page comes back empty instead.

use std::collections::HashSet;

use paloma_core::{Error, Result};
use serde_json::{Value, json};

use crate::client::Client;
use crate::request::Request;

/// A runaway-loop backstop, far above a real result set at normal page sizes. A very small
/// `--limit` can reach it on a valid list, which is an error rather than a partial result.
pub const MAX_PAGES: usize = 100;

/// The cursor for the next page, or `None` on the last one.
pub fn next_cursor(page: &Value) -> Option<String> {
    page.pointer("/paging/next")?;
    page.pointer("/paging/cursors/after")
        .and_then(Value::as_str)
        .filter(|c| !c.trim().is_empty())
        .map(str::to_owned)
}

/// While walking: `paging.next` without a usable cursor means more results exist that we
/// can't reach, which must not look like the last page.
fn advance(page: &Value) -> Result<Option<String>> {
    if page.pointer("/paging/next").is_none() {
        return Ok(None);
    }
    next_cursor(page).map(Some).ok_or_else(|| {
        Error::other("the Graph API reported more results but gave no cursor to reach them")
    })
}

/// [`advance`] for an edge that doesn't document `paging.next`, which still wins if Meta
/// sends it.
fn advance_until_empty(page: &Value) -> Result<Option<String>> {
    if page.pointer("/paging/next").is_some() {
        advance(page)
    } else {
        Ok(cursor_unless_empty(page))
    }
}

/// For an edge that doesn't document `paging.next`: the cursor after a page with data,
/// or `None` once a page is empty.
pub fn cursor_unless_empty(page: &Value) -> Option<String> {
    page.get("data")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())?;
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
    walk(client, start_after, MAX_PAGES, page, advance).await
}

/// [`collect_all`] for an edge that doesn't document `paging.next`, ending on an empty
/// page. `max_pages` is the caller's, because the edge's size limit sets the backstop.
pub async fn collect_until_empty(
    client: &Client,
    start_after: Option<String>,
    max_pages: usize,
    page: impl Fn(Option<&str>) -> Request,
) -> Result<Value> {
    walk(client, start_after, max_pages, page, advance_until_empty).await
}

async fn walk(
    client: &Client,
    start_after: Option<String>,
    max_pages: usize,
    page: impl Fn(Option<&str>) -> Request,
    next: fn(&Value) -> Result<Option<String>>,
) -> Result<Value> {
    let mut merged = Vec::new();
    let mut seen = HashSet::new();
    let mut cursor = start_after;
    for _ in 0..max_pages {
        let body = client.execute(&page(cursor.as_deref())).await?.json()?;
        let Some(Value::Array(items)) = body.get("data") else {
            return Err(Error::other("the Graph API returned a page with no data"));
        };
        merged.extend(items.iter().cloned());
        match next(&body)? {
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
        "gave up after {max_pages} pages; narrow the query or page manually with --after"
    )))
}

/// Follow pages from the start until `found` accepts one, and return that page; a lookup
/// shouldn't pay for the pages after its answer. `None` means every page was read without
/// a match. The guards match [`collect_all`], so a broken cursor is never "not found".
pub async fn find_page(
    client: &Client,
    page: impl Fn(Option<&str>) -> Request,
    found: impl Fn(&Value) -> bool,
) -> Result<Option<Value>> {
    let mut seen = HashSet::new();
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let body = client.execute(&page(cursor.as_deref())).await?.json()?;
        if found(&body) {
            return Ok(Some(body));
        }
        match advance(&body)? {
            None => return Ok(None),
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
    Err(Error::other(format!("gave up after {MAX_PAGES} pages")))
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

    #[test]
    fn next_without_a_cursor_is_an_error_not_the_last_page() {
        let broken = json!({"data": [], "paging": {"next": "https://..."}});
        assert!(advance(&broken).is_err());
        assert!(advance_until_empty(&broken).is_err());
        let last = json!({"data": [], "paging": {"cursors": {"after": "abc"}}});
        assert_eq!(advance(&last).unwrap(), None);
    }

    #[test]
    fn an_empty_page_with_next_doesnt_end_a_walk() {
        let page =
            json!({"data": [], "paging": {"cursors": {"after": "abc"}, "next": "https://..."}});
        assert_eq!(advance_until_empty(&page).unwrap().as_deref(), Some("abc"));
    }

    #[test]
    fn an_empty_page_ends_a_walk_that_has_no_next() {
        let full = json!({"data": [{"wa_id": "1"}], "paging": {"cursors": {"after": "abc"}}});
        assert_eq!(cursor_unless_empty(&full).as_deref(), Some("abc"));
        let empty = json!({"data": [], "paging": {"cursors": {"after": "abc"}}});
        assert_eq!(cursor_unless_empty(&empty), None);
        assert_eq!(
            cursor_unless_empty(&json!({"data": [{"wa_id": "1"}]})),
            None
        );
    }
}
