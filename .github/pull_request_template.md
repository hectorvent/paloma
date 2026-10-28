<!--
Title: a Conventional Commit subject, e.g. `feat(webhook): add listen`.
release-plz reads it when the PR is squash-merged.
-->

## What

<!-- The change, in a sentence or two. -->

## Why

<!-- The problem it solves or the need behind it. Link the issue: Closes #123 -->

## How to try it

<!-- A command a reviewer can run, ideally with --dry-run. -->

```sh
paloma …
```

## Checklist

- [ ] Title and every commit follow Conventional Commits; every commit is signed off (`git commit -s`)
- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets` and `cargo test --workspace` pass
- [ ] New or changed commands have a contract test in `crates/paloma-cli/tests/cli/<group>.rs` (exit code, stdout vs stderr, request body)
- [ ] Behavior changes update `docs/commands/*.md` (and `docs/README.md` / `README.md` when the command list changes)
- [ ] Breaking changes are marked with `!` or a `BREAKING CHANGE:` footer
