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
| [Billing currency](billing-currency.md) | Migrate to INR or BRL billing before Meta's deadlines, then update Paloma |
| [Architecture](architecture.md) | How the tool is built (for contributors) |
| [Planned commands](commands/planned.md) | Work that hasn't shipped yet |

## Command reference

| Group | Status | Reference |
|-------|--------|-----------|
| `paloma send` | text · template (automatic category routing, named variables, text and media headers, buttons, components file) · image · document · audio · video · sticker · location · contact · interactive (button, list, cta-url, location-request, address, carousel, call, request-contact) · flow · call-permission · product · product-list · catalog · reaction · raw | [commands/send.md](commands/send.md) |
| `paloma template` | list · status · get · create · submit · watch · edit · delete · namespace · archive · unarchive · unpause · library · compare · migrate · preview · upsert · group · analytics | [commands/template.md](commands/template.md) |
| `paloma media` | upload · get · download · delete | [commands/media.md](commands/media.md) |
| `paloma message` | read · typing · history | [commands/message.md](commands/message.md) |
| `paloma number` | list · get · register · deregister · set-pin · quality · health · request-code · verify-code · add · set-name · search-visibility · username · contacts · settings · oba · compliance | [commands/number.md](commands/number.md) |
| `paloma commerce` | get · set | [commands/commerce.md](commands/commerce.md) |
| `paloma profile` | get · set · photo | [commands/profile.md](commands/profile.md) |
| `paloma calling` | status · enable · disable · set · sip · codecs | [commands/calling.md](commands/calling.md) |
| `paloma webhook` | get · subscribe · unsubscribe · fields · fields available · fields set · listen · verify-url · simulate | [commands/webhook.md](commands/webhook.md) |
| `paloma flow` | list · get · create · update · publish · deprecate · delete · assets · metrics · migrate · key | [commands/flow.md](commands/flow.md) |
| `paloma block` | add · remove · list | [commands/block.md](commands/block.md) |
| `paloma auth` | login · status · whoami · logout | [commands/auth.md](commands/auth.md) |
| `paloma config`, `paloma configure` | init · list · get · set · unset · use · path; guided setup | [commands/config.md](commands/config.md) |
| `paloma version` | the installed version | |
| `paloma link` | create (offline) | [commands/link.md](commands/link.md) |
| `paloma export` · `apply` · `diff` | your WhatsApp setup as code (declarative YAML: Account, Calling, Profile, Number, PhoneSettings, Commerce, ConversationalAutomation, QrCode, Template, Webhook) | [commands/manifests.md](commands/manifests.md) |
| `paloma completion` | bash · zsh · fish · powershell · elvish | [commands/completion.md](commands/completion.md) |
| `paloma account` | get · set · limits · health · activities · subscribed-apps · billing-currency · analytics · users · welcome-sequence | [commands/account.md](commands/account.md) |
| `paloma qr` | create · list · get · update · delete | [commands/qr.md](commands/qr.md) |
| `paloma automation` | get · set · bot | [commands/automation.md](commands/automation.md) |

> Every command listed here is implemented. For work that hasn't shipped, see the
> [planned commands](commands/planned.md).

## Conventions

- The command shape is always `paloma <resource> <verb> [args] [flags]`, with
  `paloma send …` promoted as the hot path.
- Recipients are E.164 numbers (`+15551234567` or `15551234567`) or business-scoped
  user IDs (`US.13491208655302741918`).
- This tool talks to the **real** Graph API (`graph.facebook.com`). It is not an
  emulator and not the Web protocol.

---

<sub>Paloma is an independent open-source project and is not affiliated with, endorsed by, or
sponsored by WhatsApp LLC or Meta Platforms, Inc. WhatsApp is a trademark of WhatsApp LLC.</sub>
