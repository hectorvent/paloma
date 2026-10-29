//! Paged reads. One page warns when more exist, so a script never mistakes a first page
//! for the whole list; `--all` follows the cursor to the end.

use paloma_core::Result;
use paloma_graph::paging;

use crate::app::App;
use crate::output::Output;

/// A listing: one page (warning when more exist) or every page with `--all`.
pub async fn list(
    app: &App,
    all: bool,
    after: Option<String>,
    page: impl Fn(Option<&str>) -> paloma_graph::Request,
    columns: &'static [&'static str],
) -> Result<()> {
    if app.dry_run() {
        return app.print_dry_run(&[page(after.as_deref())]);
    }
    if all {
        let merged = paging::collect_all(app.client()?, after, page).await?;
        app.out.emit(&Output::new(merged).table("/data", columns));
    } else {
        let resp = app.fetch(&page(after.as_deref())).await?;
        let out = Output::response(&resp)?.table("/data", columns);
        if let Some(next) = paging::next_cursor(&out.data) {
            app.out.note(format!(
                "more results exist: re-run with --all, or page with --after {next}"
            ));
        }
        app.out.emit(&out);
    }
    Ok(())
}
