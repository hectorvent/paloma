<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/paloma-wordmark-dark.svg">
  <img alt="Paloma" src="assets/paloma-wordmark-light.svg" width="340">
</picture>

**Carry your message.**

The WhatsApp Cloud API from your terminal: send messages and templates, and manage
your whole WhatsApp Business setup as code.

*One 4 MB binary · tokens in your OS keyring · `apply` it twice, nothing changes*

</div>

---

```console
$ paloma send text 15551234567 "Your order is ready 🎉" -q
wamid.HBgLMTU1NTEyMzQ1NjcVAgARGBI5QTNDQTVCM0I2RjI0RTg4AA==
```

Talking to WhatsApp through the Graph API by hand means building nested JSON, copying
phone-number IDs between browser tabs, pasting tokens into `curl`, and clicking
through Meta's dashboard to set up calling or webhooks. Then doing it all again for
staging. Paloma turns each of those jobs into one command, and your account settings
into a file you can review in a pull request.

It talks to the **real** WhatsApp Cloud API (`graph.facebook.com`). It's not an
emulator, and it doesn't use the WhatsApp Web protocol.

## A 30-second tour

```bash
# Send anything: text, templates, media, locations, contacts, buttons, lists, reactions
paloma send text 15551234567 "On it!" --reply-to wamid.HBgL...
paloma send template 15551234567 order_ready --lang en_US --body-var Hector --body-var "#4821"
paloma send image 15551234567 --file ./flyer.jpg --caption "This week's specials"   # uploads it for you

# Templates, end to end: create it, wait for Meta's approval, then send it
paloma template create --name order_ready --category UTILITY --language en_US \
      --body "Hi {{1}}, order {{2}} is ready" --example "Hector,#4821"
paloma template watch order_ready      # exits 0 on APPROVED, non-zero on REJECTED

# Your WhatsApp setup, as code
paloma export calling > calling.yaml   # snapshot what's live
paloma diff  -f calling.yaml           # what would change? (exit 1 on drift)
paloma apply -f calling.yaml           # change only that

# Look before you leap: every write can print its exact request instead of sending it
paloma send text 15551234567 "test" --dry-run
```

## Why paloma

- **One small binary, nothing to install around it.** No Node, Python, or JVM. A 4 MB
  executable that starts in ~7 ms and runs as-is in CI or a Docker image, on macOS,
  Linux and Windows (x86_64 and arm64).
- **Your WhatsApp setup becomes a reviewable file.** Calling, SIP, the business
  profile, templates, webhooks: export them to YAML, commit them, and `apply`
  them idempotently. [Details below](#your-whatsapp-setup-as-code).
- **Secrets stay in the OS keyring.** Tokens go into the macOS Keychain, Windows
  Credential Manager, or Linux Secret Service. The config file holds only a reference.
- **Templates without hand-built JSON.** Fill `{{1}} {{2}}` with `--body-var`,
  create from a one-line body, and `watch` until Meta approves it.
- **Nothing is a surprise.** `--dry-run` on every write shows the method, URL and
  exact body. Commands that delete things ask first. `-v` logs each request.
- **Script-friendly.** Data goes to stdout and messages to stderr. `-q` prints just
  the ID, `-o json` pipes into `jq`, and each kind of failure has its own
  [exit code](#built-for-scripts).

## Your WhatsApp setup, as code

Setting up calling with a SIP server takes several API calls, which most people
make by clicking through a dashboard and nobody writes down. With paloma you describe
the state you want in a manifest, and paloma works out what has to change.

**1. Export what's live.** You get a manifest you can commit. Fields this kind supports
but that you haven't set appear as comments, so the file also shows what else you could configure:

```console
$ paloma export calling > calling.yaml
$ cat calling.yaml
apiVersion: paloma/v1
kind: Calling
metadata:
  phoneNumberId: '1556677889900'
spec:
  status: ENABLED
  callIconVisibility: DEFAULT
  callbackPermissionStatus: DISABLED
  sip:
    status: DISABLED
    servers: []
  # audio:
  #   additionalCodecs: []
```

**2. Edit it** to turn on SIP and callback permission, **then ask what would change.**
`diff` writes nothing and exits `1` when the live config differs from the file:

```console
$ paloma diff -f calling.yaml
Calling/1556677889900 drift:
  callback_permission_status: "DISABLED" -> "ENABLED"
  sip.status: "DISABLED" -> "ENABLED"
  sip.servers: [] -> [{"hostname":"pbx.example.com","port":5061}]
```

**3. Apply it.** Add `--dry-run` first if you want to see the exact request body:

```console
$ paloma apply -f calling.yaml
Calling/1556677889900 configured:
  callback_permission_status: "DISABLED" -> "ENABLED"
  sip.status: "DISABLED" -> "ENABLED"
  sip.servers: [] -> [{"hostname":"pbx.example.com","port":5061}]
```

**4. Run it again. Nothing happens.** That's the point:

```console
$ paloma apply -f calling.yaml
Calling/1556677889900 unchanged
```

### How paloma decides what to change

- It **reads the live config first** and compares only the fields your manifest sets.
  Anything you leave out stays as it is.
- It **sends a request only for kinds that actually differ.** Read-only and
  Meta-managed fields, like the app ID or the generated SIP password, don't count,
  so an exported file re-applies as `unchanged`.
- It **never deletes.** Templates missing from a manifest stay in your account. Deleting
  is always an explicit command (`paloma template delete`).

### What you can manage

One file can hold several manifests separated by `---`, so a whole number's setup can
live in one `whatsapp.yaml`.

| Kind | Manages |
|------|---------|
| `Account` | WABA name and timezone (excludes irreversible enrollment and billing) |
| `QrCode` | Prefilled messages for existing QR codes |
| `Calling` | Voice calling: status, call icon, callback permission, SIP servers, codecs |
| `Profile` | Business profile: about, address, description, email, websites, vertical |
| `Template` | Message templates as a managed set. Missing ones are created; changed ones are edited and go back to Meta for approval |
| `Webhook` | Callback URL override and the event fields your app subscribes to |
| `ConversationalAutomation` | Ice breakers and slash commands |
| `Commerce` | Cart and catalog visibility |
| `Number` | A versioned snapshot of the number. The only field it can change is the two-step-verification PIN |

### Keep secrets and environments out of the file

`apply` and `diff` fill in `${...}` placeholders from environment variables or the OS
keyring, so one manifest serves every environment and no secret gets committed:

```yaml
apiVersion: paloma/v1
kind: Webhook
spec:
  callbackUrl: https://${API_HOST}/whatsapp/webhook
  verifyToken: ${keyring:acme-verify}        # write-only, pulled from the keyring
  fields: [messages, calls]
```

`${VAR:-default}` supplies a fallback value, and `$${...}` writes a literal `${...}`.
A placeholder that resolves to nothing fails with exit code `5` and names the field.

### Catch drift in CI

Because `diff` exits `1` on drift, it works as a CI check. Someone changed a setting in
the dashboard? The build tells you:

```yaml
- name: WhatsApp config has not drifted
  env:
    PALOMA_TOKEN: ${{ secrets.WHATSAPP_TOKEN }}
    PALOMA_PHONE_NUMBER_ID: ${{ vars.PHONE_NUMBER_ID }}
  run: paloma diff -f whatsapp.yaml
```

More in [Your WhatsApp setup, as code](docs/commands/manifests.md).

## Configuration

Most people set it up once and never think about it again:

```bash
paloma configure            # interactive: prompts for the token (hidden) and your IDs
```

That writes a **profile** to `~/.config/paloma/config.toml` (`%APPDATA%\paloma` on
Windows) and puts the token in your OS keyring. The file holds IDs and keyring
references, never secrets:

```toml
# Production sender for Acme
default_profile = "acme"

[profiles.acme]
token_ref       = "keyring:paloma/acme"     # just a handle into the keyring
waba_id         = "1029384756"
phone_number_id = "1556677889900"

[profiles.staging]
token_ref       = "keyring:paloma/staging"
waba_id         = "5647382910"
phone_number_id = "1998877665544"
api_version     = "v25.0"                      # pin a Graph version per profile
```

**One CLI, many businesses.** Switch for a single command with `--profile staging`, or
change the default with `paloma config use staging`.

**Where each value comes from.** For every setting, the first source that has a value
wins. Everything is optional except where a command needs it, and paloma tells you
which value is missing and how to set it.

```
--flag   →   PALOMA_* environment variable   →   active profile   →   OS keyring
```

So `PALOMA_PHONE_NUMBER_ID=… paloma send …` overrides the profile for one run, and a CI
job needs no config file at all: just set `PALOMA_TOKEN` plus the IDs.

**Built-in safeguards:**

- The keyring is read when a request is sent or a listener starts. `--help`
  and offline commands never touch it; `--dry-run` reads secrets only for a command
  that [reads live state](docs/global-flags.md#what---dry-run-means) to build its request.
- `--token` is hidden from `--help`. The scripting paths are `PALOMA_TOKEN` and
  `echo "$T" | paloma auth login --with-token`, so tokens stay out of shell history.
- `paloma config list` hides secret values, and a plaintext token in the file triggers
  a warning.
- `paloma config set` edits the file in place, **keeping your comments and key order**.
- If you used the original Java build, `~/.paloma/config.toml` is moved to the new
  location automatically, and stored tokens keep working.

See [Configuration](docs/configuration.md) and [Authentication](docs/authentication.md).

## Built for scripts

```bash
MID=$(paloma send text 15551234567 "hi" -q)                  # just the wamid
paloma template list --all -o json | jq -r '.data[].name'     # json/raw pass Graph's body through
paloma media delete 123 --yes                                 # skip the confirmation in automation
```

Exit codes are part of the contract, so scripts can tell one failure from another:

| Code | Meaning |
|-----:|---------|
| `0` | success |
| `1` | generic error, or `diff` found drift |
| `2` | usage error (bad flag or value, with suggestions) |
| `3` | auth: missing, invalid or expired token |
| `4` | rate-limited by Meta |
| `5` | validation, caught locally **before** any API call |
| `6` | policy, e.g. outside the 24-hour customer-service window |

Checks like recipient format, the 4096-character text limit, caption length, and
button and row counts run before paloma resolves your token. A bad message fails fast
and costs you no API call:

```console
$ paloma send text 15551234567 "$(cat novel.txt)"
error: text body is 5000 chars; max is 4096        # exit 5
```

## Everything it covers

| Group | Commands |
|-------|----------|
| `send` | text · template (category routing) · image · document · audio · video · sticker · location · contact · interactive (button, list, cta-url, location-request, address, carousel, call, request-contact) · flow · call-permission · product · product-list · catalog · reaction · raw |
| `template` | list · status · get · create · submit · watch · edit · delete · namespace · archive · unarchive · unpause · library · compare · migrate · preview · upsert · group · analytics |
| `media` | upload · get · download · delete |
| `message` | read · typing · history |
| `calling` | status · enable · disable · set · sip · codecs |
| `flow` | list · get · create · update · publish · deprecate · delete · assets |
| `webhook` | get · subscribe · unsubscribe · fields · listen · verify-url · simulate |
| `number` | list · get · register · deregister · set-pin · quality · health · request-code · verify-code · add · set-name · search-visibility · username · contacts · settings · oba · compliance |
| `profile` | get · set · photo |
| `account` | get · set · limits · health · activities · subscribed-apps · billing-currency · analytics |
| `block` | add · remove · list |
| `qr` | create · list · get · update · delete |
| `automation` | get · set |
| `commerce` | get · set |
| `export` · `apply` · `diff` | your setup as code: declarative YAML for the nine kinds above |
| `auth` | login · status · whoami · logout |
| `config` · `configure` | profiles and settings |
| `link` | `wa.me` click-to-chat links, offline, no token needed |
| `completion` | bash · zsh · fish · powershell · elvish |
| `version` | the installed version |

Template sends look up the latest category and route marketing templates through the
Marketing Messages API. Use `send template --via cloud|marketing` for an explicit
endpoint, or keep the default `--via auto`. See [template sends](docs/commands/send.md#category-aware-routing)
for permissions, offline dry-runs and handling a category change.

Develop receive-side integrations with `webhook listen`, check the verification
handshake with `webhook verify-url`, and send signed test payloads with
`webhook simulate`. See [webhook tools](docs/commands/webhook.md).

## Install

Download the archive for your platform from the latest GitHub Release. Each one ships
the binary, man pages and shell completions. Or build from source with a Rust
toolchain:

```bash
cargo install --path crates/paloma-cli
paloma completion zsh > "${fpath[1]}/_paloma"   # optional: tab completion
```

Then run `paloma configure` and you're ready. Full walkthrough:
[Getting started](docs/getting-started.md).

## Documentation

- **Guides:** [Installation](docs/installation.md) · [Getting started](docs/getting-started.md) · [Configuration](docs/configuration.md) · [Authentication](docs/authentication.md) · [Global flags](docs/global-flags.md) · [Scripting](docs/scripting.md) · [Billing currency](docs/billing-currency.md)
- **Commands:** [send](docs/commands/send.md) · [template](docs/commands/template.md) · [media](docs/commands/media.md) · [message](docs/commands/message.md) · [calling](docs/commands/calling.md) · [flow](docs/commands/flow.md) · [webhook](docs/commands/webhook.md) · [number](docs/commands/number.md) · [account](docs/commands/account.md) · [profile](docs/commands/profile.md) · [block](docs/commands/block.md) · [qr](docs/commands/qr.md) · [automation](docs/commands/automation.md) · [auth](docs/commands/auth.md) · [config](docs/commands/config.md) · [link](docs/commands/link.md) · [completion](docs/commands/completion.md)
- **Setup as code:** [export / apply / diff](docs/commands/manifests.md)
- **Project:** [Architecture](docs/architecture.md) · [Contributing](CONTRIBUTING.md)

## Contributing

Paloma is written in Rust: [clap](https://docs.rs/clap) for the command tree,
[reqwest](https://docs.rs/reqwest) and rustls on a single-threaded
[tokio](https://tokio.rs) runtime, and [keyring-core](https://docs.rs/keyring-core) for
secrets. There's no OpenSSL dependency, so it cross-compiles cleanly.

```bash
cargo test --workspace         # unit tests, plus contract tests against a mocked Graph API
cargo clippy --workspace --all-targets
cargo build --release          # → target/release/paloma
```

Start with [Architecture](docs/architecture.md). [AGENTS.md](AGENTS.md) has the
step-by-step for adding a command or a manifest kind. Commits follow
[Conventional Commits](https://www.conventionalcommits.org), and
[release-plz](https://release-plz.dev) turns them into versions, a changelog and
GitHub Releases.

---

<div align="center">
<sub>

Paloma is an independent open-source project and is **not affiliated with, endorsed by, or sponsored by** WhatsApp LLC or Meta Platforms, Inc. WhatsApp is a trademark of WhatsApp LLC.

</sub>
</div>
