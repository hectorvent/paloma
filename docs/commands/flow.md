# `paloma flow`

Manage WhatsApp Flows — the interactive, form-like experiences you send into a chat.

```
paloma flow <verb> [args] [flags]
```

Flows live on the **WhatsApp Business Account**, and this is the Business Management
side of the Graph API: the token needs `whatsapp_business_management`, not just the
messaging scope. A system-user token is the usual choice.

Sending a flow to a recipient is a separate command, [`paloma send flow`](send.md#paloma-send-flow).
This group builds and manages the flow itself.

## The lifecycle

A flow moves `DRAFT → PUBLISHED → DEPRECATED`, and each step closes a door:

| Status | Can edit? | Can delete? | Can send? |
|--------|-----------|-------------|-----------|
| `DRAFT` | yes | yes | only in draft-preview mode |
| `PUBLISHED` | no | no (deprecate instead) | yes |
| `DEPRECATED` | no | no | no — recipients cannot open it |

`BLOCKED` and `THROTTLED` are set by Meta, not by these commands, when a flow's data
endpoint is unhealthy.

Because `publish`, `deprecate`, and `delete` are all one-way, each asks for confirmation
first. Skip the prompt with `--yes` (e.g. in CI).

## `paloma flow list`

```bash
paloma flow list
paloma flow list --all                    # follow every page
paloma flow list -o json | jq '.data[] | select(.status == "DRAFT") | .id'

# expand fields beyond the default projection
paloma flow list --fields id,name,status,endpoint_uri
```

Graph returns `id`, `name`, `status`, `categories`, and `validation_errors` on each flow
unless you ask for more with `--fields`.

| Flag | Meaning |
|------|---------|
| `--all` | Follow paging cursors and return every flow as one merged `data` array |
| `--after <cursor>` | Return the single page following this cursor |
| `--limit <n>` | Flows **per request** — Graph's page size, not a total |
| `--fields <csv>` | Expand each flow with more fields |

See [Paging](#paging) below. `--limit` is a page size, so `--all --limit 100` means "every
flow, 100 per request".

## `paloma flow get`

```bash
paloma flow get 1234567890
paloma flow get 1234567890 --preview        # also mint/return the shareable preview URL
paloma flow get 1234567890 --fields id,name,status,data_api_version
```

| Flag | Meaning |
|------|---------|
| `--preview` | Also return `preview.preview_url` and its expiry |
| `--fields <csv>` | Replace the default field set entirely |

`get` requests a useful default projection (`id`, `name`, `status`, `categories`,
`validation_errors`, `json_version`, `endpoint_uri`, `whatsapp_business_account`,
`application`). Two fields are deliberately left out:

- **`preview`** — asking for it mints a share link that stays live for 30 days, so it is
  opt-in via `--preview` (which reuses an existing link rather than invalidating it).
- **`data_api_version`** — it only exists on endpoint-backed flows; request it explicitly
  with `--fields` when you need it.

## `paloma flow create`

```bash
# a draft with no layout yet
paloma flow create --name "Signup" --category SIGN_UP

# with the Flow JSON inline, published straight away
paloma flow create --name "Signup" --category SIGN_UP,LEAD_GENERATION \
      --flow-json ./flow.json --publish

# copy an existing flow's layout
paloma flow create --name "Signup v2" --category SIGN_UP --clone-from 1234567890

# scriptable: print just the new flow id
paloma flow create --name "Survey" --category SURVEY -q
```

| Flag | Meaning |
|------|---------|
| `--name <name>` | **Required.** Flow name |
| `--category <cat>` | **Required.** Repeatable, or comma-separated |
| `--flow-json <file>` | Flow JSON layout to create the flow with |
| `--clone-from <flow-id>` | Copy an existing flow's layout instead |
| `--endpoint-uri <uri>` | Data endpoint URL (Flow JSON v3.0+) |
| `--publish` | Publish immediately rather than leaving it in `DRAFT` |

Categories are validated locally, so a typo fails before the request. The valid values are
`SIGN_UP`, `SIGN_IN`, `APPOINTMENT_BOOKING`, `LEAD_GENERATION`, `CONTACT_US`,
`CUSTOMER_SUPPORT`, `SURVEY`, `OTHER`. `--flow-json` and `--clone-from` both set the
layout, so pass only one.

Under `--quiet` the new flow id is the only thing on stdout.

## `paloma flow update`

Only a `DRAFT` flow can be updated.

```bash
paloma flow update 1234567890 --name "Signup (EU)"
paloma flow update 1234567890 --category LEAD_GENERATION
paloma flow update 1234567890 --flow-json ./flow.json         # re-upload the layout
paloma flow update 1234567890 --name "Signup v2" --flow-json ./flow.json
```

| Flag | Meaning |
|------|---------|
| `--name <name>` | New flow name |
| `--category <cat>` | Replace the categories (repeatable, or comma-separated) |
| `--endpoint-uri <uri>` | Data endpoint URL (Flow JSON v3.0+) |
| `--flow-json <file>` | Upload this file as the flow's layout |

Metadata and the layout are two different Graph requests — a body update on the flow node,
and a multipart asset upload. Passing both flags runs both, and reports each under a
`metadata` and `flow_json` key:

```json
{
  "metadata":  { "success": true },
  "flow_json": { "success": true, "validation_errors": [] }
}
```

They are not atomic: if the metadata update succeeds and the upload then fails, the name
change stands. Re-run with just `--flow-json` to finish.

Passing no flag at all is an error rather than a no-op request.

## Flow JSON validation

`paloma` parses `--flow-json` before sending, so a malformed file fails immediately, naming
the file and the offending line.

Meta validates it again, more deeply, on arrival — and it will happily **store** a layout it
refuses to publish, answering `{"success": true, "validation_errors": [...]}`. Paloma does
not paper over that. The response still goes to stdout, the errors are explained on stderr,
and the command exits `5` (validation):

```bash
$ paloma flow update 1234567890 --flow-json ./flow.json
{"success":true,"validation_errors":[{"error":"INVALID_PROPERTY_VALUE", ...}]}
1 validation error (the flow cannot be published until they are fixed):
  line 14:3 INVALID_PROPERTY_VALUE: unknown property 'titel'

$ echo $?
5
```

So a bad layout fails a CI step instead of quietly passing. The upload did happen, though —
the flow now holds a layout it cannot publish until you fix it.

## `paloma flow publish`

Move a `DRAFT` to `PUBLISHED`. One-way: it can no longer be edited or deleted, only
deprecated.

```bash
paloma flow publish 1234567890
paloma flow publish 1234567890 --yes      # skip the confirmation
paloma flow publish 1234567890 --dry-run  # show the POST without sending
```

## `paloma flow deprecate`

Retire a `PUBLISHED` flow. Irreversible, and recipients can no longer open it — including
flows already sitting in their chat history.

```bash
paloma flow deprecate 1234567890 --yes
```

## `paloma flow delete`

Delete a `DRAFT` flow. A published flow cannot be deleted; deprecate it instead.

```bash
paloma flow delete 1234567890 --yes
```

## `paloma flow assets`

List the flow's assets, or download its Flow JSON — the round trip for
`flow update --flow-json`.

```bash
paloma flow assets 1234567890
paloma flow assets 1234567890 -o json | jq '.data[].download_url'

# download the layout (--out or -O, since -o is the global --output flag)
paloma flow assets 1234567890 --out ./flow.json
paloma flow assets 1234567890 --out ./flows/          # a directory receives flow.json
```

The `download_url` in the listing is short-lived, so prefer `--out` over saving the URL.
Under `--quiet`, `--out` prints only the path it wrote.

## Paging

Graph returns flows one page at a time. Paloma never truncates silently: when a listing has
more pages and you did not pass `--all`, it says so on **stderr**, naming the cursor to
resume from.

```bash
$ paloma flow list
{"data":[ ...one page... ],"paging":{...}}
more results exist: re-run with --all, or page with --after QVFIU1VWbTh4UGMt...

$ paloma flow list --all
{"data":[ ...every flow, merged... ]}
```

The warning goes to stderr, so `paloma flow list -o json | jq …` stays clean, and it still
fires under `--quiet`. With `--all` the `paging` block is dropped from the output: once
followed, it describes nothing.

Two details worth knowing, both of which bite naive paging code:

- Graph returns `paging.cursors.after` on **every** page, including the last. The signal that
  another page exists is `paging.next`. Looping on the cursor alone never terminates.
- Paloma pages by re-requesting the edge with `after=<cursor>`, rather than fetching the
  absolute `paging.next` URL Graph hands back. That URL can carry an embedded access token,
  and following a server-supplied URL verbatim is a request-forgery footgun.

If the API ever returns a repeating cursor, `--all` aborts with an error rather than looping
or quietly returning a partial list.

## Exit codes

Beyond the [standard contract](../global-flags.md), the one worth knowing here:

| Code | When |
|------|------|
| `5` | A Flow JSON failed validation — locally before sending, or reported by Meta after it stored it |

## Migration

```bash
paloma flow migrate --source-waba-id 123 --name booking --name survey --dry-run
```

The selected WABA is the destination. This **copies** Flows between WABAs owned by
the same business; source Flows remain. Repeat `--name` to select up to 100 names,
or omit it to request all. Existing destination names can be skipped, and published
source Flows remain published in the destination. Migration confirms and preserves
Meta's per-Flow result. Any `failed_flows` entries cause exit `1` with a warning;
`-q` prints newly migrated IDs. It never silently retries with
an expanded selection. Source and destination must differ.

## Endpoint encryption key

```bash
paloma flow key get -o json
paloma flow key set --public-key public.pem --dry-run
```

These phone-scoped commands read or replace the business public key used for Flow
endpoint encryption. Supply a PEM **public** key file generated from a 2048-bit RSA
pair. Private-key input is rejected locally; keep that key on your endpoint server.
Meta validates the public key and returns its signature status. Installation confirms
before writing. Dry-run shows the public-key form field without sending it.
