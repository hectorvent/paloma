---
name: preflight
description: >-
  Run Paloma's CI checks plus the repo's contract checks locally and report one
  pass/fail table. Use when the user says "check before PR", "is this ready", "run the
  checks", "preflight", "will CI pass", or after finishing a feature and before the
  git-workflow skill's PR step.
---

# Preflight

Mirror CI (`.github/workflows/ci.yml`), then check what CI can't: the contracts in
`AGENTS.md`. Report every row; don't stop at the first failure.

## 1. CI checks

Run in this order (cheap first). If the environment has a remote runner for heavy
commands, use it for the test run.

| Check | Command |
|-------|---------|
| fmt | `cargo fmt --all --check` |
| clippy | `RUSTFLAGS=-D\ warnings cargo clippy --workspace --all-targets` |
| test | `cargo test --workspace` |
| deny | `cargo deny check` (skip with a note if `cargo-deny` isn't installed) |
| msrv | `cargo +1.88 check --workspace` (skip with a note if that toolchain isn't installed; the version is `rust-version` in `Cargo.toml`) |
| commits | each commit in `main..HEAD` through `sh .githooks/commit-msg` |

On a failure, show the first relevant lines of output, not the whole log. Fix fmt
with `cargo fmt --all` only if the user asked for fixes.

## 2. Contract checks

Scope them to the change: `git diff --name-only main...HEAD` plus uncommitted files.

- **stdout only through `Printer`.** `grep -rn 'println!\|print!(' crates/*/src`
  must find nothing; `eprintln!` is allowed only in `crates/paloma-cli/src/main.rs`
  (before the runtime exists).
- **Docs move with commands.** Each changed `crates/paloma-cli/src/commands/<group>.rs`
  with a behavior change has a changed `docs/commands/<group>.md`. If a verb was added
  or un-planned: `docs/README.md` table, `docs/commands/planned.md`, and the README
  command table also updated.
- **Contract test.** A new verb or flag has a test in `crates/paloma-cli/tests/cli/<group>.rs`
  asserting exit code, stdout vs stderr, and the request body.
- **Pure builders.** New code in `crates/paloma-graph/src/api/` does no I/O and takes
  no `App`/`Context`.
- **Limits are named.** New numeric Cloud API limits are constants in
  `paloma_core::limits`, not literals in commands.
- **Lint allows carry a reason.** Each new `#[allow(clippy::…)]` has a comment saying why.
- **No internal links.** Nothing tracked links into `local/`.

For a deeper review of the contracts, hand the diff to the `contract-reviewer` agent;
for docs drift across the whole tree, the `docs-auditor` agent.

## 3. Report

```
| Check     | Result | Notes |
|-----------|--------|-------|
| fmt       | pass   |       |
| clippy    | FAIL   | 2 warnings in crates/paloma-cli/src/commands/qr.rs |
| …         |        |       |
```

End with one line: ready for a PR, or the blocking items.
