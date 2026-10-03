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
kept). Never echoed, never stored in plaintext.

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

Validate the active token by probing the phone-number node, and show its context.
Exits with code `3` if the token is missing or invalid.

```bash
paloma auth status
paloma auth status -o json
```

---

## `paloma auth whoami`

Show the active WABA, sender number, and business name for the current profile.

```bash
paloma auth whoami
```

---

## `paloma auth logout`

Remove the stored token for a profile from the keyring. Other profile settings in
config are left intact.

```bash
paloma auth logout
paloma auth logout --profile staging
```

---

## Planned verbs

`refresh` — exchange/refresh a long-lived token where applicable. See the
[planned commands](planned.md).
