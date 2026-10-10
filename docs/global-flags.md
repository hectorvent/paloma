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
| `--app-id <id>` | | Meta App ID (for app-level webhook field subscriptions) |
| `--api-version <v>` | | Override Graph API version (e.g. `v26.0`) |
| `--dry-run` | | Print the request with sensitive fields redacted, without sending |
| `--quiet` | `-q` | Only essential output (IDs, errors) — script-friendly |
| `--verbose` | `-v` | Log each request to stderr; `-vv` adds redacted request/response bodies (`PALOMA_LOG` takes a full tracing filter) |
| `--timeout <dur>` | | Request timeout, e.g. `30s`, `2m` (default `60s`) |
| `--no-color` | | Disable ANSI colors (`NO_COLOR` is honored too) |
| `--config <path>` | | Config file path (default: `~/.config/paloma/config.toml`) |
| `--token <value>` | | Access token (hidden; prefer `PALOMA_TOKEN`/keyring) |
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
`webhook fields set --dry-run` looks up the app id. Everything else prints the request
without any network call. Multi-step commands (`send image --file`, `flow update`
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

Set a default with the `PALOMA_OUTPUT` environment variable. See
[Scripting & automation](scripting.md) for patterns.

## Quiet mode (`--quiet` / `-q`)

Prints only the primary identifier (a `wamid…`, a media id, a path) to **stdout**
and routes diagnostics to **stderr**, so command substitution stays clean:

```bash
MID=$(paloma send text 15551234567 "hi" -q)
```

## Exit codes

`paloma` follows a strict exit-code contract so scripts can branch on the outcome:

| Code | Meaning |
|------|---------|
| `0` | Success |
| `1` | Generic error |
| `2` | Usage error (bad flags/arguments) |
| `3` | Auth/token error |
| `4` | Rate-limited (HTTP 429) |
| `5` | Pre-flight validation failed (caught locally before the API call) |
| `6` | Policy/window block (e.g. the 24-hour customer-service window) |

Pre-flight checks — recipient format, text-body length, required template fields,
file readability — run **locally**, so invalid input fails fast with code `5`
without spending an API call.

```bash
paloma send text "not-a-number" "hi" ; echo $?    # → 5
```
