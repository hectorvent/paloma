# `paloma auth`

Store and inspect credentials. For the full picture of token storage and
resolution, see [Authentication](../authentication.md).

```
paloma auth <verb> [flags]
```

---

## `paloma auth login`

Store a system-user token for a profile. The token is read from stdin with
`--with-token`, from `PALOMA_TOKEN`, or from a hidden prompt; it is saved to the OS
keyring and the profile is written to config (other keys already in the profile are
kept). Never echoed, never stored in plaintext. Logging in also makes that profile the
default, so `--profile staging` below switches later commands to `staging`.

```
paloma auth login [--with-token] [--profile <name>] [--phone-number-id <id>] [--waba-id <id>] [--api-version <v>]
```

```bash
paloma auth login --phone-number-id 1556677889900 --waba-id 1029384756
echo "$TOKEN" | paloma auth login --with-token --profile staging   # CI-friendly
```

The sender/WABA/version values come from the global flags
([Global flags](../global-flags.md)).

---

## `paloma auth status`

Validate the active token by probing the phone-number node, and show what it acts as: the
profile, WABA, sender number, its business name and its quality rating.
Exits with code `3` if the token is missing or invalid.

```bash
paloma auth status
paloma auth status -o json
```

Add `--scopes` to ask Meta about the token itself, using
[`debug_token`](https://developers.facebook.com/documentation/business-messaging/whatsapp/permissions).
It shows the token's type, app, granted scopes and expiry, and skips the phone-number
check, so it needs no phone number id and still answers when a missing permission is the
problem.

```bash
paloma auth status --scopes
paloma auth status --scopes -o json   # Meta's response, unchanged
```

It exits `3` if the token is missing or Meta doesn't report it as valid. With `-q` it prints
`valid`. Times are Unix timestamps as Meta returns them. Meta's example for a system user
token shows `expires_at` as `0`, but its docs don't define that value.

---

## `paloma auth logout`

Remove the stored token for a profile from the keyring. Other profile settings in
config are left intact.

```bash
paloma auth logout
paloma auth logout --profile staging
```
