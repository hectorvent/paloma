---
name: paloma
description: >-
  Drive the WhatsApp Cloud API with the `paloma` CLI: send messages and templates,
  manage templates, media, numbers, the business profile, calling and SIP, webhooks and
  Flows, and keep WhatsApp Business settings as YAML with export/diff/apply. Use when the
  user wants to send a WhatsApp message, check or create a template, configure calling,
  webhooks or the profile, look up phone numbers or account health, catch config drift,
  or script any of this. Also use when they mention paloma, wamid, WABA, or
  graph.facebook.com WhatsApp calls.
---

# Paloma

`paloma` is a single-binary CLI for Meta's **real** WhatsApp Cloud API. Every command
hits a live business account. Real messages reach real people and cost real money, and
real settings change. Treat it like a production deploy tool.

Commands always take the form `paloma <resource> <verb> [args] [flags]`. Don't guess
flags. Ask the binary:

```bash
paloma --help                  # groups
paloma send --help             # verbs in a group
paloma send template --help    # flags for one verb
```

## Before anything else

1. **Check it's installed and configured.** Run `paloma version`, then `paloma auth
   status`. That shows the token state, active WABA, sender number and business name.
   If it's not configured, tell the user to run `paloma configure` themselves. It
   prompts for the token.
2. **Know which account you're on.** `paloma config list` shows the profiles. Use
   `--profile <name>` (or `-p`) to target one explicitly. When the user has several,
   name the profile you're about to use.

## Safety rules

- **Never handle the token in argv.** Don't pass `--token`, and don't echo or log a
  token. The token lives in the OS keyring. For scripts and CI, use the `PALOMA_TOKEN`
  env var or `echo "$T" | paloma auth login --with-token`.
- **Dry-run every write first.** `--dry-run` prints the exact method, URL and body
  without sending, with secrets redacted. Show it to the user and get a yes before you
  run it for real. This covers every `send`, `create`, `update`, `delete`, `set`,
  `enable`/`disable` and `apply`.
- **Don't add `--yes` on your own.** Destructive commands (delete, disable, deregister,
  overwrite) prompt for confirmation and refuse without a TTY. Pass `--yes` only when
  the user has explicitly approved that specific action.
- **Reads are safe.** `list`, `get`, `auth status`, `diff` and the report views
  (`account health`, `number quality`, `template analytics`, …) change nothing.
- **Messaging rules still apply.** Free-form messages only reach users inside the
  24-hour customer-service window. Outside it, send an approved template. A policy
  rejection exits `6`. Don't retry it as free text.

## Output and exit codes

Data goes to stdout and diagnostics go to stderr.

- `-q` prints only the primary id (a `wamid…`, media id, template id):
  `MID=$(paloma send text 15551234567 "hi" -q)`.
- `-o json` prints Graph's raw response for `jq`. `-o table` (the default) is for
  people.
- `-v` logs each request to stderr. `-vv` adds redacted bodies. Use it to debug.

| Exit | Meaning | What to do |
|-----:|---------|------------|
| 0 | ok | |
| 1 | generic error, or `diff` found drift | read stderr |
| 2 | usage: bad flag or value | read the suggestion, check `--help` |
| 3 | auth: token missing, invalid or expired | ask the user to re-run `paloma configure` |
| 4 | rate-limited by Meta | wait and retry later. Don't loop tight |
| 5 | validation, caught locally before any API call | fix the input. Nothing was sent |
| 6 | policy, e.g. outside the 24h window | use an approved template |

## Common tasks

```bash
# Messages
paloma send text 15551234567 "Your order is ready" --dry-run
paloma send template 15551234567 order_ready --language en_US --body-var Alex --body-var "#4821"
paloma send image 15551234567 --file ./flyer.jpg --caption "This week's specials"

# Templates
paloma template list --status APPROVED -o json | jq -r '.data[].name'
paloma template get order_ready --status
paloma template create --name order_ready --category UTILITY --language en_US \
  --body "Hi {{1}}, order {{2}} is ready" --example "Alex,#4821"
paloma template watch order_ready       # exits 0 on APPROVED, non-zero on REJECTED

# Account and numbers
paloma number list
paloma account health
paloma calling get
paloma webhook get

# Local webhook testing (no Meta traffic)
paloma webhook listen --port 9000
paloma webhook simulate http://127.0.0.1:9000/webhook --event text-username
```

Recipients are in international format: country code plus number, digits only, with an
optional leading `+`. No spaces or dashes. `paloma link` builds `wa.me` links offline.

## Setup as code (export / diff / apply)

To change calling, SIP, the profile, templates, webhooks and similar settings, prefer
manifests over one-off commands. The change is then reviewable and repeatable.

```bash
paloma export calling > calling.yaml   # snapshot what's live (read-only)
# edit calling.yaml
paloma diff  -f calling.yaml           # what would change? exit 1 on drift, writes nothing
paloma apply -f calling.yaml --dry-run # the exact requests
paloma apply -f calling.yaml           # only after the user approves the diff
```

- `apply` reads live state and only sends requests for fields that differ. Running it
  twice changes nothing.
- It never deletes. A template missing from the manifest stays in the account.
- Never write secrets into a manifest. Use `${VAR}` for env vars and
  `${keyring:name}` for keyring entries.
- Kinds: `Calling`, `Profile`, `Template`, `Webhook`, `Account`, `PhoneSettings`,
  `QrCode`, `ConversationalAutomation`, `Commerce` and `Number`. Several kinds can share
  one file, separated by `---`.

## When something fails

1. Read stderr. Paloma names the missing value or the rule that was broken.
2. Map the exit code using the table above.
3. Re-run with `-v` to see the request. Never paste the token into the conversation.
4. For command details, see the docs at
   https://github.com/hectorvent/paloma/tree/main/docs.
