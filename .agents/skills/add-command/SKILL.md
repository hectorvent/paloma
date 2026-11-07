---
name: add-command
description: >-
  Add a `paloma <group> <verb>` command end to end: clap variant, pure Graph builder, run
  wiring, contract test and docs. Use when the user says "add a command", "implement
  paloma qr create", "add a --flag to send", "new resource group", or "wire up the
  <endpoint> endpoint".
---

# Add a command

`AGENTS.md` ("How to add a command" and "Conventions that matter") is the rulebook.
This is the order of work, with the real names to copy.

## 0. Learn the endpoint

Confirm the Graph node, method, fields and permissions from Meta's WhatsApp Cloud API
docs (the local mirror when you have one). Note any limits
(lengths, counts) for `paloma_core::limits`.

## 1. Graph builder (`crates/paloma-graph/src/api/<resource>.rs`)

Pure functions returning a `Request`, no I/O. Reference: `api/block.rs`.

```rust
pub fn add(phone_number_id: &str, users: &[String]) -> Request {
    Request::post(format!("{phone_number_id}/block_users")).json(body(users))
}
```

Payload shaping (JSON bodies, `MESSAGING_PRODUCT`, multipart) lives here. New module →
`pub mod` in `api.rs`. Add a unit test for any non-trivial body.

## 2. clap variant (`crates/paloma-cli/src/commands/<group>.rs`)

- Doc comments become `--help`; write them for users.
- Several flags → an `Args` struct. Closed sets → `clap::ValueEnum`.
- Env-backed flags use `env = "PALOMA_…"`. Never take a secret as an argument.
- New top-level group → module + variant in `commands.rs`.

## 3. `run`

Order is the contract:

1. **Validate** with `paloma_core::validate` (exit 5) before touching context.
2. **Resolve ids** lazily: `app.ctx()?.require_phone_number_id()?` /
   `require_waba_id()?`.
3. **Destructive?** `app.confirm_or_abort("delete … ")?` first.
4. **Send**:
   - single write: `app.send_and_emit(&req, field_id)` (or `no_id` when there's no id
     to print for `-q`).
   - list: `if let Some(resp) = app.send(&req).await? { app.out.emit(&Output::response(&resp)?.table("/data", &[..])) }`
   - read needed to build the write: `app.fetch(&req)` (runs under `--dry-run`).
   - several writes: `app.print_dry_run(&[..])` under `app.is_dry_run()`.
5. Diagnostics via `app.out.info/note/warn` (stderr). Never `println!`.

Return the right `paloma_core::Error` variant: `Validation` (5), `Auth` (3),
`Policy` (6), `Usage` (2); `Api` comes from `error_map`.

## 4. Contract test (`crates/paloma-cli/tests/cli/<group>.rs`)

Use the `paloma(Some(&graph), &home)` helper and `MockServer`; model on
`send_text_posts_with_bearer_and_quiet_prints_wamid`. Assert:

- the exact request (`method`, `path("/v26.0/…")`, `body_json(..)`, `.expect(1)`),
- exit code and what is on stdout vs stderr (`-q` prints only the id),
- a `--dry-run` case prints the request and sends nothing, when the verb writes,
- validation failures exit 5 without a request.

The `test-writer` agent can draft these.

## 5. Docs (same commit)

- `docs/commands/<group>.md`: usage, flags, an example, exit codes worth knowing.
- `docs/README.md` command table; `README.md` command table if the verb list changed.

## 6. Check

Run the `preflight` skill; commit with the `git-workflow` skill
(`feat(<group>): add <verb>`).
