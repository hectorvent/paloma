# Getting started

This walks you from a fresh install to a sent message.

## 1. Install

```bash
cargo install --path crates/paloma-cli   # or download a release archive
paloma version
```

See [Installation](installation.md) if `paloma` isn't found.

## 2. Authenticate

You need a WhatsApp Cloud API **system-user token**, your **phone number ID**,
and your **WhatsApp Business Account (WABA) ID** from the Meta dashboard.

### Interactive

```bash
paloma configure
```

The wizard prompts for the token (input hidden) and the sender context, stores
the token in your OS keyring, and writes a profile to `~/.config/paloma/config.toml`.

### Non-interactive

```bash
paloma auth login --phone-number-id <PHONE_NUMBER_ID> --waba-id <WABA_ID>
# (prompts for the token without echoing; or pipe PALOMA_TOKEN / pass --token)
```

Confirm it works (this makes a live call to validate the token):

```bash
paloma auth status
paloma auth whoami      # active WABA, sender number, business name
```

## 3. Send your first message

```bash
paloma send text 15551234567 "hey Hector 🎉"
```

A successful send prints the Cloud API response, including the `wamid…` message
ID. For just the ID (handy in scripts):

```bash
MID=$(paloma send text 15551234567 "hi" -q)
echo "$MID"
```

## 4. Try it without sending

Every send supports `--dry-run`, which prints the exact request body and target
URL and never touches the network:

```bash
paloma send text 15551234567 "preview me" --dry-run -o json
```

## 5. A quick tour of the rest

```bash
# Templates: fill {{1}} {{2}} variables, and poll until approved
paloma send template 15551234567 order_ready --language en_US --body-var Hector --body-var "#4821"
paloma template list
paloma template watch order_ready          # exits 0 on APPROVED, non-zero on REJECTED

# Media: a local file auto-uploads, then sends
paloma send image 15551234567 --file ./flyer.jpg --caption "This week's specials"

# Account management
paloma number list
paloma profile get
paloma block add 15557654321 --yes

# Voice: read and configure WhatsApp calling (and SIP)
paloma calling get
paloma calling sip set --server pbx.example.com:5061 --enable-calling

# Your WhatsApp setup, as code: snapshot to YAML, version it, re-apply idempotently
paloma export calling > calling.yaml
paloma apply -f calling.yaml                # only writes what changed
paloma diff -f calling.yaml                 # exit 1 if live config has drifted
```

## Where to next

- Every message type (templates, media, location, interactive, reactions) → [commands/send.md](commands/send.md)
- Template lifecycle (create → watch → send) → [commands/template.md](commands/template.md)
- Phone numbers, business profile, block list → [number](commands/number.md), [profile](commands/profile.md), [block](commands/block.md)
- WhatsApp calling / SIP / codecs → [commands/calling.md](commands/calling.md)
- Your WhatsApp setup, as code (export/apply/diff) → [commands/manifests.md](commands/manifests.md)
- Managing multiple senders/accounts → [Configuration](configuration.md)
- Using `paloma` in CI and pipelines → [Scripting & automation](scripting.md)
- The full flag list and exit codes → [Global flags](global-flags.md)
