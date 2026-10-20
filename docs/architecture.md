# Architecture

For contributors. `paloma` is a Cargo workspace of five crates. Dependencies point one
way, from the binary down to the domain:

```
paloma-cli ──► paloma-manifest ──► paloma-graph ──► paloma-core
     │               │                                  ▲
     └───────────────┴──────► paloma-config ────────────┘
```

| Crate | Owns | Knows nothing about |
|-------|------|---------------------|
| `paloma-core` | `Error` + exit codes, pre-flight validation and limits, the 24h-window policy, the `Credentials` trait | HTTP, clap. Its only I/O is stat-ing files for validation |
| `paloma-config` | `config.toml` (read with `toml`, edited in place with `toml_edit`), profiles, the OS keyring, context resolution | HTTP, output |
| `paloma-graph` | the Graph API: `Request` (a call as data), `Client` (reqwest/rustls), error mapping, paging, one `api::*` module per resource, `callback` (calling webhook endpoints), `webhook_samples` | the CLI, config files |
| `paloma-manifest` | the YAML manifest envelope, `${...}` interpolation, camel/snake conversion, drift diff, the nine kinds | clap, output |
| `paloma-cli` | the clap tree, `App` wiring, the output contract, prompts | — |

The pure parts (`paloma-core` validation rules, `case`/`diff`/`interpolate`/`yaml` in
`paloma-manifest`) are synchronous and unit-test without a runtime.

## A request's life

```
commands/send.rs        parse args (clap) → validate locally (paloma_core::validate)
        │                 → build payload + Request (paloma_graph::api::messages)
        ▼
App::send(&Request)     --dry-run?  print Request::describe() and stop
        │               otherwise   lazily resolve Context → build Client → execute
        ▼
Client::execute         bearer from Credentials unless Auth::None (keyring read here,
        │               not earlier)
        │               non-2xx → error_map::map_error → Error with its exit code
        ▼
Printer::emit(Output)   stdout per --output / --quiet; diagnostics to stderr
```

**Requests are data.** Endpoint functions in `paloma_graph::api` are pure: they return
a `Request { method, target, query, headers, body, auth }`. That is what makes `--dry-run`
impossible to forget (`App::send` enforces it centrally), keeps dry-run output identical
to what is sent (query parameters included), and lets manifest kinds plan writes that
`apply` executes later through the same path.

**Laziness.** `App` holds `OnceLock`s for the config file, the resolved `Context` and the
`Client`. Offline commands (`link create`, `config path`, `--help`) never read config,
and nothing touches the keyring until a request is actually sent, or a listener starts.

## Cross-cutting concerns, one place each

| Concern | Where |
|---------|-------|
| Exit codes | `paloma_core::error::{Error, ExitCode}`; mapped once in `main.rs` |
| Graph error → exit code | `paloma_graph::error_map` |
| Context precedence (flag/env → profile → keyring) | `paloma_config::resolve::Context::resolve` |
| Secret storage | `paloma_config::secrets::SecretStore` (keyring; `MemoryStore` in tests) |
| Output (`--output`, `--quiet`, stdout vs stderr) | `paloma_cli::output::Printer` |
| `--dry-run`, confirmations | `paloma_cli::app::App::{send, fetch, confirm}`. Webhook endpoint probes aren't Graph calls, so they check `app.dry_run()` themselves |
| Cloud API limits | `paloma_core::limits` |
| 24-hour window | `paloma_core::compliance` (`WindowTracker` seam for future tracking) |
| Paging | `paloma_graph::paging` |

## Adding a command

1. Pick the group module in `crates/paloma-cli/src/commands/` (or add one and a variant
   in `commands/mod.rs`).
2. Add an `Args` struct (or inline fields) and a variant to the group's `Subcommand`
   enum; doc comments become `--help` text.
3. If it calls a new Graph node, add a pure builder to `paloma_graph::api::<resource>`.
4. In `run`: validate locally first, then `app.ctx()?.require_…()`, then
   `app.send_and_emit(&request, primary_id)` (or `app.send` / `app.fetch` for
   multi-step flows). Destructive writes call `app.confirm_or_abort(..)` first.
5. Not built yet? Return `planned("group verb", "vX.Y")` so it stays in `--help`.
6. Add a contract test in `crates/paloma-cli/tests/cli.rs` and a docs page.

## Adding a manifest kind

Add a variant to `paloma_manifest::Kind` (name, scope, and the three `match`es) and a
module under `kinds/` with `template()` (camelCase writable fields), `export()` and
`plan()`. Singleton kinds reuse `kind::singleton_plan` for the subset-diff reconcile.

## Async

One current-thread tokio runtime, built in `main`. The CLI is I/O-bound and runs one
request at a time, so a thread pool would only cost startup time. The same runtime hosts
`webhook listen` (axum, HTTP/1 only).

## Release build

`[profile.release]`: fat LTO, one codegen unit, `panic = "abort"`, stripped. The
`dist` profile trades speed for size (`opt-level = "s"`). See
[Installation](installation.md).

## Testing

- Unit tests next to the code (`cargo test --workspace`).
- Contract tests in `crates/paloma-cli/tests/cli.rs` run the real binary against a
  `wiremock` Graph API and assert exit codes, stdout vs stderr, and exact request
  bodies (`insta` snapshots for dry-run output).
