# Authentication

`paloma` authenticates to the Graph API with a WhatsApp Cloud API **system-user
access token**. Tokens are stored in your OS keyring and referenced from config
by handle — never written to disk in plaintext, never echoed to the terminal.

## Logging in

Interactive wizard (also sets the sender context):

```bash
paloma configure
```

Or directly:

```bash
paloma auth login --phone-number-id <PHONE_NUMBER_ID> --waba-id <WABA_ID> [--profile <name>]
```

The token comes from, in order: the `--token` flag, the `PALOMA_TOKEN` environment
variable, or an interactive hidden prompt. On success the token is written to the
keyring as `keyring:paloma/<profile>` and the profile is saved to `~/.config/paloma/config.toml`.

## Where the token is stored

On macOS, `paloma` shells out to the `security` CLI and stores a generic password:

- **service:** `paloma`
- **account:** `paloma/<profile>` (e.g. `paloma/default`)

Inspect or remove it directly:

```bash
security find-generic-password -s paloma -a paloma/default -w     # print (prompts for access)
open -a "Keychain Access"                                  # search for "paloma"
```

The config file only stores the handle:

```toml
[profiles.default]
token_ref = "keyring:paloma/default"
```

> **Platform support:** the keyring is the macOS Keychain, the Windows Credential
> Manager, or the Linux Secret Service (GNOME Keyring / KWallet). Where none is
> available (containers, headless CI), `paloma auth login` says so; use `PALOMA_TOKEN`
> instead. The keyring is read lazily, only when a request is actually sent, so
> `--dry-run` and offline commands never trigger a keychain prompt.

## Commands

| Command | What it does |
|---------|--------------|
| `paloma auth login` | Store a token for a profile (prompts, never echoes) |
| `paloma auth status` | Validate the active token; show the profile, WABA, sender number and business name |
| `paloma auth status --scopes` | Show the active token's type, granted scopes and expiry |
| `paloma auth logout` | Remove the stored token for a profile (other settings stay) |

```bash
paloma auth status -o json     # machine-readable
paloma auth logout --profile staging
```

## Token resolution

When any command needs a token, `paloma` checks these in order and uses the first
present:

1. `--token <value>` (hidden flag; visible to other processes via argv — prefer the others)
2. `PALOMA_TOKEN` environment variable
3. OS keyring (`token_ref = "keyring:…"`)
4. A literal `token_ref` in config (plaintext — triggers a warning)

If none resolve, the command exits with code `3` (auth error) and a hint to run
`paloma auth login`, set `PALOMA_TOKEN`, or pass `--token`.
