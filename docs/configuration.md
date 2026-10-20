# Configuration

`paloma` reads its settings from `~/.config/paloma/config.toml`, organized into named
**profiles**. Secrets are never stored in this file — only a reference to the OS
keyring (see [Authentication](authentication.md)).

## The config file

```toml
default_profile = "acme"

[profiles.acme]
token_ref       = "keyring:paloma/acme"        # handle into the OS keyring
waba_id         = "1029384756"
phone_number_id = "1556677889900"
api_version     = "v26.0"
app_secret_ref  = "keyring:paloma/acme-secret" # optional; for webhook field subscriptions and signatures
```

Find the resolved path on your system:

```bash
paloma config path
```

Override the location for one command with `--config <path>`, or with the
`PALOMA_CONFIG` environment variable.

## Profiles

A profile bundles a token, sender number, WABA, and API version under a name, so
you can switch between businesses or environments instantly.

```bash
paloma config list                       # all profiles (secrets redacted)
paloma config use acme                # set the default profile
paloma send text … --profile staging     # use a profile for one command
```

Create or update profiles with `paloma auth login --profile <name>` (stores a token)
or `paloma config set` (individual keys). See [commands/config.md](commands/config.md).

## Settable keys

`paloma config get/set <key>` operates on these keys:

| Key | Meaning |
|-----|---------|
| `token_ref` | Keyring handle (`keyring:paloma/<profile>`) or, discouraged, a literal token |
| `waba_id` | WhatsApp Business Account ID |
| `phone_number_id` | Sender phone number ID |
| `api_version` | Graph API version, e.g. `v26.0` (defaults to `v26.0`) |
| `app_secret_ref` | Keyring handle for the app secret (app-level webhook field subscriptions, and signing or checking webhooks in `webhook simulate` and `listen`) |
| `messaging_account_id` | Messaging account to bill for `send` commands, if your token reaches more than one on the phone number |

```bash
paloma config set phone_number_id 1556677889900 --profile acme
paloma config get waba_id --profile acme
```

## Value precedence

For any setting, the first source that provides a value wins:

```
flag  >  environment variable  >  OS keyring  >  config file
```

For example, the token resolves as `--token` → `PALOMA_TOKEN` → keyring → config.

## Environment variables

These override the config file (and are themselves overridden by flags):

| Variable | Overrides |
|----------|-----------|
| `PALOMA_PROFILE` | active profile |
| `PALOMA_TOKEN` | access token |
| `PALOMA_PHONE_NUMBER_ID` | sender phone number ID |
| `PALOMA_WABA_ID` | WABA ID |
| `PALOMA_MESSAGING_ACCOUNT_ID` | Messaging account to bill for `send` commands |
| `PALOMA_PIN` | two-step verification PIN for `number register` and `number set-pin` |
| `PALOMA_VERIFY_TOKEN` | verify token for `webhook subscribe`, `webhook fields set --callback-url`, `webhook verify-url` and `webhook listen` |
| `PALOMA_API_VERSION` | Graph API version |
| `PALOMA_APP_ID` | Meta App ID (app-level webhook field subscriptions and upload handles; also `--app-id`) |
| `PALOMA_APP_SECRET` | app secret (app-level webhook field subscriptions); also signs and checks webhooks in `webhook simulate` and `webhook listen` |
| `PALOMA_OUTPUT` | default `--output` format |
| `PALOMA_CONFIG` | config file path |
| `NO_COLOR` | disables ANSI color |

> `PALOMA_APP_ID` has no config-file key yet; set it via the env var or `--app-id`
> (or let `paloma webhook fields` auto-discover it from the subscribed app). See
> [webhook fields](commands/webhook.md#paloma-webhook-fields).

## Security notes

- Tokens belong in the keyring; a `token_ref` is just a handle. If a profile ever
  contains a **plaintext** token, `paloma` still uses it but prints a warning.
- `paloma config list` redacts secrets — keyring handles are shown as-is, plaintext
  values appear as `<plaintext:redacted>`.
- Writes (`config set`, `auth login`) edit the file in place, so comments and key
  order you added by hand survive.

## Location and migration

The default path is `$XDG_CONFIG_HOME/paloma/config.toml` (`~/.config/paloma/config.toml`
on Linux and macOS, `%APPDATA%\paloma\config.toml` on Windows). `--config` or
`PALOMA_CONFIG` override it.

If you used the Java build of paloma, its `~/.paloma/config.toml` is copied to the new
location the first time paloma runs without one. Keyring entries keep the same names
(service `paloma`, account `paloma/<profile>`), so stored tokens keep working.
