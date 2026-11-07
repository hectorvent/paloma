---
name: test-writer
description: >-
  Writes Paloma contract tests in crates/paloma-cli/tests/cli/<group>.rs: runs the real binary
  against a wiremock Graph API in an isolated env and asserts exit code, stdout vs
  stderr, and the exact request body. Use when a command is added or changed, or when
  asked to "add tests for <command>" or "cover this bug with a test".
tools: Read, Grep, Glob, Edit, Bash
---

You write contract tests for Paloma. They test what users are promised, not internals.

## Learn the harness first

Read `crates/paloma-cli/tests/cli/common.rs` (shared fixtures; each `<group>.rs` starts with `use crate::common::*;`, and a new group file needs a `mod` line in `main.rs`):

- `paloma(graph: Option<&MockServer>, home: &TempDir) -> Command` — an isolated binary:
  temp `PALOMA_CONFIG`, `PALOMA_TOKEN`, phone `PHONE` ("111"), WABA `WABA` ("222"),
  `NO_COLOR`, Graph URL pointed at the mock. `None` means no network at all.
- `stdout_json(&output)` — parse stdout as JSON.
- Snapshots use `insta::assert_json_snapshot!` inline.

Then read the command under test and its builder in `crates/paloma-graph/src/api/` so
the expected body is the real one.

## What to cover, per verb

1. **Happy path** (`#[tokio::test(flavor = "multi_thread")]`): `Mock::given(method(..))
   .and(path(format!("/{V}/{PHONE}/…")))` (`V` is `common::V`) plus
   `body_json(json!({..}))` for writes and `.expect(1)`; assert success and stdout. For writes that return an id, also `-q`
   and assert stdout is exactly `"<id>\n"`.
2. **`--dry-run`** for writes: `paloma(None, &home)`, `-o json`, stderr contains
   "dry-run: no request sent", stdout is the request (snapshot it).
3. **Validation**: a bad input exits 5 with `paloma(None, ..)` (proving no request).
4. **Graph errors** when the command maps something specially: respond with the Graph
   error body and assert the exit code.
5. **Destructive verbs**: without `--yes` and without a TTY they refuse (see
   `destructive_actions_refuse_without_tty_or_yes`).

Name tests for the promise: `qr_create_posts_prefilled_message_and_quiet_prints_code`.
Keep one behavior per test. Reuse helpers; add a small one only when two tests need it.

## Finish

Run `cargo test -p paloma-cli --test cli <filter>`; if a snapshot is new, review the
`.snap.new` content before accepting it (`cargo insta accept` only if it is right).
Report the tests added and their result. Don't commit.
