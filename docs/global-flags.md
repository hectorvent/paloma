# Global flags, output & exit codes

## Global flags

These are accepted on every command (you can place them before or after the
subcommand: `paloma --dry-run send text …` and `paloma send text … --dry-run` are
equivalent).

| Flag | Alias | Description |
|------|-------|-------------|
| `--profile <name>` | `-p` | Named credential/context profile (default: `default`) |
| `--output <fmt>` | `-o` | `table` (default) · `json` · `yaml` · `raw` |
| `--phone-number-id <id>` | | Override the active sender phone number ID |
| `--waba-id <id>` | | Override the active WABA ID |
| `--messaging-account-id <id>` | | Messaging account to bill for `send` commands. Set it if your token reaches more than one on the phone number |
| `--app-id <id>` | | Meta App ID (for app-level webhook field subscriptions and upload handles) |
| `--api-version <v>` | | Override Graph API version (e.g. `v26.0`) |
| `--dry-run` | | Print the request with sensitive fields redacted, without sending |
| `--quiet` | `-q` | Only essential output (IDs, errors) — script-friendly |
| `--verbose` | `-v` | Log each request to stderr; `-vv` adds redacted request/response bodies (`PALOMA_LOG` takes a full tracing filter) |
| `--timeout <dur>` | | Request timeout, e.g. `30s`, `2m` (default `60s`) |
| `--no-color` | | Disable ANSI colors (`NO_COLOR` is honored too) |
| `--config <path>` | | Config file path (default: `~/.config/paloma/config.toml`) |
| `--token <value>` | | Access token (hidden; prefer `PALOMA_TOKEN`/keyring) |
| `--app-secret <value>` | | App secret (hidden; prefer `PALOMA_APP_SECRET` or a keyring reference to keep it out of argv) |
| `--graph-url <url>` | | Graph API base URL for tests and proxies (hidden; also `PALOMA_GRAPH_URL`) |
| `--yes` | `-y` | Skip confirmation prompts for destructive ops |
| `--help` | `-h` | Show help for any command |
| `--version` | `-V` | Print version |

`--yes` is honored by commands that confirm before a disruptive write (e.g.
`webhook subscribe`/`unsubscribe`/`fields set`, `number deregister`): pass it to
proceed without the interactive prompt, which is also required when there is no TTY.

Most flags also read an environment variable: `PALOMA_PROFILE`, `PALOMA_TOKEN`,
`PALOMA_OUTPUT`, `PALOMA_PHONE_NUMBER_ID`, `PALOMA_WABA_ID`,
`PALOMA_MESSAGING_ACCOUNT_ID`, `PALOMA_APP_ID`, `PALOMA_APP_SECRET`,
`PALOMA_API_VERSION`, `PALOMA_CONFIG`. A flag beats its variable.

### What `--dry-run` means

`--dry-run` never **writes**. Commands that must read live state to build the real
request still read it: `apply --dry-run` compares against live config, and
`webhook fields set --dry-run`, `media upload --handle --dry-run` and
`profile photo --dry-run` look up the app id unless you set it, and `template get --status` and
`template watch` read the template, since reading is all they do. `webhook listen --dry-run` only notes
the bind address; it does not open a socket or read verification secrets. Other commands
print requests or a multi-step plan without sending them; command pages describe any
required live lookups. Multi-step commands (`send image --file`, `flow update`
with metadata and Flow JSON) print every request as a JSON array.

Dry-run output in every format, including `raw`, masks PINs, authorization values,
and fields ending in `token`, `secret`, or `password` (including camelCase names).
Verbose diagnostics use the same policy for JSON bodies and URL query parameters;
non-JSON response bodies are omitted from logs. The actual request retains its values,
and normal response stdout is unchanged.

## Output formats (`--output` / `-o`)

| Format | Behavior |
|--------|----------|
| `table` | Aligned columns for lists (`template list`, `number list`, ...), key/value for single objects (default) |
| `json` | The raw Cloud API response, pretty-printed — pipe to `jq` |
| `yaml` | YAML |
| `raw` | The response body verbatim |

`webhook listen` emits one JSON document per line regardless of `-o`; `-q` selects
primary message IDs instead.

Set a default with the `PALOMA_OUTPUT` environment variable. See
[Scripting & automation](scripting.md) for patterns.

## Quiet mode (`--quiet` / `-q`)

Prints only the primary identifier (a `wamid…`, a media id, a path) to **stdout**
and routes diagnostics to **stderr**, so command substitution stays clean:

```bash
MID=$(paloma send text 15551234567 "hi" -q)
```

A list prints one row id per line (a blocked user without a shared number is listed by
BSUID), and a bulk write prints the ids it acted on:

```bash
paloma template list --status PAUSED --all -q | xargs -n1 paloma template unpause
```

## Paging

Every paged list takes the same flags:

| Flag | What it does |
|------|--------------|
| `--limit <N>` | Rows per page. Meta picks when it's left out, and refuses more than the edge allows |
| `--all` | Follow the cursor to the last page |
| `--after <CURSOR>` | Start after this cursor |

Without `--all`, a page that has more after it ends with a note on stderr naming the
`--after` cursor.

## Exit codes

`paloma` follows a strict exit-code contract so scripts can branch on the outcome:

| Code | Meaning |
|------|---------|
| `0` | Success |
| `1` | Generic error; also `diff` when it found drift |
| `2` | Usage error (bad flags/arguments) |
| `3` | Auth/token error, including Meta's permission codes such as 10, 190 and 200-299, whatever the HTTP status |
| `4` | Rate-limited (HTTP 429, or a Meta throttling code such as 130429 or 131048), after any [retries](#retries) |
| `5` | Pre-flight validation failed (caught locally before the API call) |
| `6` | Policy/window block (e.g. the 24-hour customer-service window, a marketing opt-out, or a restricted account) |

Pre-flight checks — recipient format, text-body length, required template fields,
file readability — run **locally**, so invalid input fails fast with code `5`
without spending an API call.

```bash
paloma send text "not-a-number" "hi" ; echo $?    # → 5
```

## Retries

For Graph API requests, `paloma` tries up to 3 times when repeating the request is safe, and notes each retry on
stderr. Webhook callback probes (`verify-url` and `simulate`) make one attempt.
A repeated message would reach the user twice, so the rules depend on whether the
request changes anything:

| Failure | Reads (`GET`) | Writes (`POST`, `DELETE`) |
|---------|---------------|---------------------------|
| HTTP 429 with a valid `Retry-After` | Retry | Retry |
| HTTP 429 without one | Retry | **Fail** |
| HTTP 500, 502, 503, 504 | Retry | **Fail** |
| Can't connect, including a connection attempt that times out | Retry | **Fail** |
| Timeout (`--timeout`) | **Fail** | **Fail** |
| Connection lost after connecting | **Fail** | **Fail** |

Without a server hint, reads wait 1 second and then 2. `Retry-After` must be a number of
seconds; any other form counts as no hint. If the server asks for more than 30 seconds,
`paloma` fails straight away, so your script decides whether to wait that long.

A write that fails in a way that's not retried may still have been applied, so check
before you repeat it.

File transfers in `media upload --handle` and `profile photo` are the one exception. After
a 500, 502, 503 or 504, a timeout or a lost connection, Paloma asks Meta how much of the
file arrived and continues from there, so nothing is sent twice. That's up to 3 transfers
in total, each with its own `--timeout`, and the reads between them are retried as usual.

**Time limits.** `--timeout` applies to each try, and waits between tries are extra. A
request can take up to 3 × `--timeout` plus 60 seconds of waits, and commands that make
several requests, such as `template list --all`, can take that long per request. Connecting
has its own 15-second limit. Downloads also honor `--timeout`, including streamed bodies.

This doesn't throttle: each `paloma` run retries on its own, so it can't pace a shell loop.
