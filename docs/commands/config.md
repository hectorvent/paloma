# `paloma config` & `paloma configure`

Manage profiles and the config file (`~/.config/paloma/config.toml`). For the file format,
keys, and precedence rules, see [Configuration](../configuration.md).

---

## `paloma configure`

Interactive first-run wizard. Prompts for the token (hidden) and the sender
context, stores the token in the OS keyring, and writes the profile.

```bash
paloma configure
paloma configure --profile staging
```

Requires an interactive terminal; in scripts, use `paloma auth login` + `paloma config set`.

---

## `paloma config init`

Write a skeleton config file for a profile.

```
paloma config init [--profile <name>] [--force]
```

| Flag | Description |
|------|-------------|
| `--force` | Overwrite an existing config file |

---

## `paloma config list`

Show all profiles with secrets redacted (keyring handles shown as-is, plaintext
values masked).

```bash
paloma config list
paloma config list -o json
```

---

## `paloma config get` / `paloma config set`

Read or write a single key on a profile (defaults to the active profile; target
another with `--profile`).

```
paloma config get <key>
paloma config set <key> <value>
```

Settable keys: `token_ref`, `waba_id`, `phone_number_id`, `api_version`,
`app_secret_ref`.

```bash
paloma config set phone_number_id 1556677889900 --profile acme
paloma config get waba_id --profile acme
```

> Prefer `paloma auth login` for tokens — it stores them in the keyring rather than
> as a plaintext value.

---

## `paloma config use`

Set the active default profile.

```bash
paloma config use acme
```

---

## `paloma config path`

Print the resolved config file path (honors `--config` and `PALOMA_CONFIG`).

```bash
paloma config path
```
