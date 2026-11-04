---
name: add-manifest-kind
description: >-
  Add a manifest kind so `paloma export`, `apply` and `diff` manage another piece of
  WhatsApp setup as YAML. Use when the user says "add a manifest kind", "support X in
  export/apply", "make Y declarative", or "manage <setting> as code".
---

# Add a manifest kind

`crates/paloma-manifest/src/kind.rs` holds the `Kind` enum; each kind is a module in
`crates/paloma-manifest/src/kinds/`. Reference for a singleton: `kinds/calling.rs`.
Collections (many objects per WABA): `kinds/template.rs`.

## 1. The module (`kinds/<name>.rs`)

- `pub fn template() -> Value` — every **writable** field, camelCase, empty values.
  `export` renders the unset ones as comments, so this is the user's field list.
- `async fn current(cx: Cx<'_>) -> Result<Value>` — read live state with
  `kind::read(cx, &api::<resource>::get(..))`; strip server-assigned and secret fields.
- `pub async fn export(cx) -> Result<Vec<Value>>` — one value per object.
- `pub async fn plan(cx, spec) -> Result<KindPlan>` — compare spec to live, return the
  changes, a `preview` body for `--dry-run`, and the write `requests` in order.
  Singletons use `kind::singleton_plan(..)`. An empty plan is `KindPlan::synced()`.

Specs arrive **snake_case** (`case` converts from the manifest's camelCase after
`${...}` interpolation); keep the module in snake_case. Requests come from pure
builders in `paloma-graph/src/api/` (add one if missing). Read ids from `cx.scope`
(`require_phone_number_id`, `require_waba_id`, `app_id`), never from CLI settings.

**Guard like the command.** If the matching `paloma <group>` command confirms or checks a
write, the kind must too: use the same guard from `paloma_graph::api::<resource>` (e.g.
`calling::confirm_for`, `webhook::check_callback_url`) and pass its text to
`KindPlan::confirming`, so `apply` is never a way around a command's confirmation.

## 2. Register it (`kind.rs`)

Add the variant, then every `match`: `ALL`, `name`, `cli_name`, `phone_scoped` (true when
the node hangs off a phone number id), `spec_template`, `export`, `plan`. The compiler
finds the ones you miss. Add `pub mod` in `kinds.rs`.

## 3. Tests

- Unit tests in the module for plan output (in sync, drift, field removal).
- Contract tests in `crates/paloma-cli/tests/cli/<group>.rs`, modeled on
  `diff_exits_1_on_drift_and_0_in_sync`, `apply_dry_run_reads_but_never_writes` and
  `apply_writes_only_drift`: `diff` exits 1 on drift and 0 in sync, `apply --dry-run`
  reads but never writes, `apply` writes only the drift.

## 4. Docs

`docs/commands/manifests.md` (the kind, its fields, an example), and the kind list in
`docs/README.md` and `README.md`.

Commit as `feat(manifest): add <Kind> kind` via the `git-workflow` skill, after
`preflight`.
