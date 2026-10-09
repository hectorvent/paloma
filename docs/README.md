# paloma documentation

Full documentation for **Paloma**, an unofficial, single-binary CLI for the
WhatsApp Cloud API. *Carry your message.*

New here? Start with [Installation](installation.md) → [Getting started](getting-started.md).

## Guides

| Doc | What it covers |
|-----|----------------|
| [Installation](installation.md) | Build and install the `paloma` binary; platform notes |
| [Getting started](getting-started.md) | First-run setup and your first message |
| [Configuration](configuration.md) | `config.toml`, profiles, env vars, value precedence |
| [Authentication](authentication.md) | Tokens, the OS keyring, `paloma auth` |
| [Global flags](global-flags.md) | Flags on every command, output formats, exit codes |
| [Scripting & automation](scripting.md) | `--output json`, `--quiet`, `--dry-run`, CI usage |
| [Architecture](architecture.md) | How the tool is built (for contributors) |
| [Planned commands](commands/planned.md) | What's visible in `--help` but not implemented yet |

## Command reference

| Group | Status | Reference |
|-------|--------|-----------|
| `paloma send` | text · template (named variables, header text, components file) · image · document · audio · video · sticker · location · contact · interactive · reaction · raw | [commands/send.md](commands/send.md) |
| `paloma template` | list · status · get · create · submit · watch · edit · delete | [commands/template.md](commands/template.md) |
| `paloma media` | upload · get · download · delete | [commands/media.md](commands/media.md) |
| `paloma message` | read · typing | [commands/message.md](commands/message.md) |
| `paloma number` | list · get · register · deregister · set-pin · quality | [commands/number.md](commands/number.md) |
| `paloma profile` | get · set | [commands/profile.md](commands/profile.md) |
| `paloma calling` | status · enable · disable · set · sip · codecs | [commands/calling.md](commands/calling.md) |
| `paloma webhook` | get · subscribe · unsubscribe · fields · fields set | [commands/webhook.md](commands/webhook.md) |
| `paloma flow` | list · get · create · update · publish · deprecate · delete · assets | [commands/flow.md](commands/flow.md) |
| `paloma block` | add · remove · list | [commands/block.md](commands/block.md) |
| `paloma auth` | login · status · whoami · logout | [commands/auth.md](commands/auth.md) |
| `paloma config` | init · list · get · set · unset · use · path | [commands/config.md](commands/config.md) |
| `paloma link` | create (offline) | [commands/link.md](commands/link.md) |
| `paloma export` · `apply` · `diff` | your WhatsApp setup as code (declarative YAML: Calling, Profile, Number, Commerce, ConversationalAutomation, Template, Webhook) | [commands/manifests.md](commands/manifests.md) |
| `paloma completion` | bash · zsh · fish · powershell · elvish | [commands/completion.md](commands/completion.md) |
| `paloma account` · `qr` | planned | [commands/planned.md](commands/planned.md) |

> Anything marked **planned** is already visible in `paloma --help` and exits with a
> clear "not yet implemented (planned for vX.Y)" message. See the
> [planned commands](commands/planned.md).

## Conventions

- The command shape is always `paloma <resource> <verb> [args] [flags]`, with
  `paloma send …` promoted as the hot path.
- Recipients accept E.164 (`+15551234567` or `15551234567`) or a
  business-scoped user ID (BSUID).
- This tool talks to the **real** Graph API (`graph.facebook.com`). It is not an
  emulator and not the Web protocol.

---

<sub>Paloma is an independent open-source project and is not affiliated with, endorsed by, or
sponsored by WhatsApp LLC or Meta Platforms, Inc. WhatsApp is a trademark of WhatsApp LLC.</sub>
