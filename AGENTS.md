# AGENTS.md

Guidance for coding agents (and humans) working in this repository. This file is the
source of truth; tool-specific instruction files only import it.

## What this is

**Paloma**: an unofficial, single-binary CLI for the **real** WhatsApp Cloud API
(`graph.facebook.com`). Not an emulator, not the Web protocol. Rust (2024 edition),
clap + tokio (current-thread) + reqwest/rustls.

The command shape is always `paloma <resource> <verb> [args] [flags]`.

## Commands

| Task | Command |
|------|---------|
| Compile check | `cargo check --workspace` |
| Lint (must be clean) | `cargo clippy --workspace --all-targets` |
| Format | `cargo fmt --all` |
| All tests | `cargo test --workspace` |
| One crate's tests | `cargo test -p paloma-manifest` |
| Run | `cargo run -q -- send text 15551234567 hi --dry-run --phone-number-id 1` |
| Release binary | `cargo build --release` → `target/release/paloma` |
| Man pages + completions | `cargo xtask dist-assets` |
| Supply chain | `cargo deny check` |

## Architecture

Read [docs/architecture.md](docs/architecture.md) first. The short version:

```
paloma-core      errors + exit codes, validation, limits, 24h policy, Credentials trait
paloma-config    config.toml (toml_edit writes), profiles, keyring, Context resolution
paloma-graph     Request (a call as data), Client, error_map, paging, api::<resource>, lookup,
                 callback (calling webhook endpoints), webhook_samples
paloma-manifest  YAML manifests: interpolate, case, diff, Kind enum + kinds/*
paloma-cli       clap tree (commands/*), App wiring, output::Printer
```

Each cross-cutting concern lives in exactly one place, and that is load-bearing:

- **Exit codes** only in `paloma_core::error`; `main.rs` maps `Result` to the process
  exit code once. Graph responses map in `paloma_graph::error_map`.
- **Context precedence** (flag/env → profile → keyring) only in
  `paloma_config::resolve::Context::resolve`. clap merges flag and env (`env = "PALOMA_…"`).
- **All stdout** goes through `paloma_cli::output::Printer`. Never `println!`.
- **`--dry-run`** is enforced by `App::send`; commands build a `Request` and hand it over.
  Webhook endpoint probes aren't Graph calls, so they check `app.dry_run()` themselves.
- **Cloud API limits** are named constants in `paloma_core::limits`.

## Conventions that matter

**Output is a contract.** Data → stdout; diagnostics, warnings, prompts → stderr
(`app.out.info/note/warn`). `-q` prints only `Output::primary_id`, so
`MID=$(paloma send text … -q)` works. `-o table|json|yaml|raw`; json/raw pass the raw
Graph body through. Lists set `.table("/data", COLUMNS)`.
`template get` is one exception: it prints only exact matches as `{"data":[...]}`, because
Graph's name filter can return other templates. `webhook listen` is another: it always
prints JSON lines, whatever `-o` says, unless `-q` selects only primary ids.

**Exit codes are a contract.** `0` ok, `1` generic, `2` usage (clap), `3` auth,
`4` rate-limited, `5` validation, `6` policy. Return the right `Error` variant; use
`Error::Silent(code)` when the command already reported the outcome (e.g. `diff`).

**Requests are data.** Endpoint builders in `paloma_graph::api` are pure and return a
`Request`. Use `app.send(&req)` (honors `--dry-run`), `app.fetch(&req)` (a read needed to
build the real request; runs even under `--dry-run`), or `app.send_and_emit(&req, id_fn)`.
Multi-request commands print all of them under dry-run with `app.print_dry_run(&[..])`.

**Fail before spending an API call.** Validate with `paloma_core::validate` (exit 5)
before `app.ctx()`. Prefer clap `ValueEnum`s for closed sets (bad values exit 2 with
suggestions).

**Destructive actions confirm** with `app.confirm_or_abort(what)`: passes under `--yes`
or `--dry-run`, refuses without a TTY.

**Secrets never touch argv, config or logs.** Tokens live in the keyring
(`paloma_config::secrets`), service `paloma`, account `paloma/<profile>`. `--token` is
hidden; `PALOMA_TOKEN` and `auth login --with-token` are the scripting paths. Other
secrets a command needs go through `commands::util::read_secret` (env, stdin or a
prompt; there is no flag for the value). Secrets are `SecretString`s; `Request::describe`
never includes them.

**Lazy context.** `ctx.require_phone_number_id()` / `require_waba_id()` at the point of
use; the keyring is read only when a request is sent, or a listener starts.

**Docs explain why, not what.** Each module opens with a short `//!` on the constraint
it exists to satisfy. Match that; don't narrate code.

## CLI grammar

Every command follows these rules, so a user who knows one group can guess the next.
A new command that can't follow one says why in its doc comment.

- **Verbs.** A resource with an id takes `list`, `get`, `create`, `update` and `delete`.
  A singleton setting takes `get` and `set`. `enable`/`disable` turn a whole feature on
  or off; `clear` empties a list setting.
- **Groups aren't commands.** `paloma webhook fields` lists its verbs; reading is
  `webhook fields get`. A sub-resource is a nested group: `number pin set`, never a
  hyphenated verb like `number set-pin`.
- **Arguments.** The resource's own id is positional; every other reference is a flag.
  The recipient of a send is `<TO>`; any other person is `<USER>`.
- **Booleans** are `--x`/`--no-x` pairs, never `--x <true|false>`.
- **Lists** are a singular flag that repeats and also takes commas (`--id 1 --id 2`, or
  `--id 1,2`), shown as `<NAME,...>`.
- **Files.** `-f, --file <FILE>` reads one; `-O, --out <OUT>` writes one.
- **Time.** `--start`/`--end <TIME>` take UNIX seconds or `YYYY-MM-DD`; durations are
  humantime (`--ttl 10m`).
- **Paging.** Every paged read flattens `listing::Pagination` (`--limit`, `--all`,
  `--after`) and goes through `listing::list`.
- **`-q`.** A create prints the new id, a list prints one row id per line, and a bulk
  write prints the ids it acted on.
- **Confirmation.** Anything that deletes, disables, or overwrites what Meta holds
  calls `app.confirm_or_abort`.
- **Help.** A verb's doc comment is one imperative sentence ("Show the …", not
  "Read …"); clap drops its period. Every argument has a doc comment.

Known exceptions, each for a reason; don't add more without one:

- **Read-only views are noun leaves**: `account limits`, `account health`,
  `number quality`, `number health`, `template namespace`, `calling permission`. Each
  shows one computed view, not a resource you could list, create or change.
- **Not every list pages by cursor.** `template analytics` takes only `--after`, because
  Meta pages the data points, not resources. `welcome-sequence list` takes only `--limit`,
  because Meta documents no cursor there. `template migrate --page` is Meta's batch
  number, not a cursor.
- **`--fields`** stays plural and comma-separated: it is Graph's own parameter, passed
  through as written.
- **Names that keep Meta's word**: `number oba apply` (Meta calls it applying for OBA),
  and `send call-permission` beside `calling permission`, because one sends a message and
  the other reads a setting.

## How to add a command

1. Add a variant (and an `Args` struct if it has several flags) to the group's
   `Subcommand` enum in `crates/paloma-cli/src/commands/<group>.rs`; doc comments become
   `--help`. New top-level group → new module + variant in `commands.rs`.
2. New Graph node → a pure builder in `crates/paloma-graph/src/api/<resource>.rs`.
3. In `run`: validate → resolve ids from `app.ctx()?` → `app.send_and_emit(..)`.
   Payload shaping belongs in `paloma-graph`, not in the command.
4. Contract test in `crates/paloma-cli/tests/cli/<group>.rs` (exit code, stdout vs stderr,
   exact request body); docs page under `docs/commands/`, linked from `docs/README.md`.

## How to add a manifest kind

Add a variant to `paloma_manifest::Kind` (name, `phone_scoped`, and the `match`es in
`spec_template`/`export`/`plan`) and a module in `crates/paloma-manifest/src/kinds/`.
Specs are **camelCase** in manifests, **snake_case** inside kinds (`case` converts);
`${...}` is expanded before conversion. Singleton kinds use `kind::singleton_plan`.
`template()` lists writable fields; `export` renders unset ones as comments.

## Testing

Unit tests sit next to the code. Contract tests run the built binary against a
`wiremock` Graph API with an isolated env (`PALOMA_CONFIG` in a temp dir,
`PALOMA_TOKEN`, `PALOMA_GRAPH_URL`), so they never touch the real keyring or network.
Test what the user is promised: exit code, what lands on stdout vs stderr, and the exact
request body.

## Git workflow

The full rules, with examples, are in [CONTRIBUTING.md](CONTRIBUTING.md). The ones an
agent must never get wrong:

- **Branches** are `<type>/<short-kebab>` off an up-to-date `main`
  (`feat/webhook-listen`, `fix/send-media-id`, `docs/roadmap-section`).
- **Conventional Commits**: `type(scope): imperative summary`, at most 72 characters.
  release-plz derives versions and `CHANGELOG.md` from them, so the type matters:
  `feat` bumps minor, `fix`/`perf` bump patch, `!` or `BREAKING CHANGE:` bumps major.
  Don't hand-edit versions or the changelog.
- **Scopes** are a crate (`core`, `config`, `graph`, `manifest`, `cli`), a command
  group (`send`, `template`, `webhook`, `flow`, …), or `deps`, `ci`, `build`, `xtask`.
- **Every commit is signed off and signed**: `git commit -s -S`. The `Signed-off-by:`
  trailer is the DCO; the hook and CI reject commits without it.
- **No AI attribution**: no `Co-Authored-By:` for an AI, no "Generated with …" lines,
  no mention of an assistant in commit messages, PR titles or PR bodies.
- One logical change per commit; when a command's behavior changes, its
  `docs/commands/*.md` page changes in the same commit.
- Enable the hooks once per clone: `git config core.hooksPath .githooks`.
- Never push, open a PR, or rewrite published history unless asked.

## Code hygiene

- clippy runs with `pedantic`; keep it at zero warnings. Allow a lint locally with a
  comment saying why, not crate-wide.
- `local/` is git-ignored scratch space for internal notes (the working roadmap lives
  there). Never link to it from tracked files.

## Agent tooling

Shared project tooling lives in `.agents/`:

- Skills: `.agents/skills/<name>/SKILL.md`, discovered directly by Codex.
- Agent instructions: `.agents/agents/<name>.md`, shared by both tools.
- Codex agent definitions: `.agents/codex-agents/<name>.toml`, exposed through
  `.codex/agents`. These adapters read the shared Markdown instructions.
- Edit the canonical files under `.agents`; a tool's own discovery path may link
  to them locally.

When adding an agent, add its shared Markdown file and a matching Codex TOML adapter.
Keep task instructions in the Markdown file; tool-specific configuration stays in
the adapter. The front matter's `tools` list does not configure Codex permissions.
See [.agents/README.md](.agents/README.md) for the layout and discovery checks.

| Skill | Use it to |
|-------|-----------|
| `git-workflow` | Start a branch, write a commit, prepare or open a PR the project's way |
| `preflight` | Run the CI checks plus the repo's contract checks before a PR |
| `add-command` | Add a `paloma <group> <verb>` end to end |
| `add-manifest-kind` | Add a kind to `export` / `apply` / `diff` |

| Agent | Use it to |
|-------|-----------|
| `contract-reviewer` | Review a diff against the output, exit-code, dry-run and secrets contracts |
| `docs-auditor` | Find drift between `--help`, `docs/` and `README.md` |
| `test-writer` | Write contract tests in `crates/paloma-cli/tests/cli/<group>.rs` |
