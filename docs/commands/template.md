# `paloma template`

Manage the full message-template lifecycle on the WABA: `list`, `status`, `get`,
`create`, `submit`, `watch`, `edit`, `delete`. Only `namespace` is still planned.

```
paloma template <verb> [args] [flags]
```

---

## `paloma template list`

List templates on the active WABA, with optional filters.

| Flag | Description |
|------|-------------|
| `--status <s>` | Filter by `APPROVED`, `PENDING`, or `REJECTED` |
| `--category <c>` | Filter by `MARKETING`, `UTILITY`, or `AUTHENTICATION` |
| `--language <code>` | Filter by language code, e.g. `es` |
| `--name <name>` | Filter by name. Meta's docs don't say the match is exact, so `-o json \| jq` on the name if you need that |
| `--fields <a,b>` | Comma-separated [fields](https://developers.facebook.com/documentation/business-messaging/whatsapp/reference/whatsapp-business-account/message-template-api#get-version-waba-id-message-templates) to return. In a table they replace the default columns |
| `--limit <n>` | Templates **per request** — Graph's page size, not a total |
| `--all` | Follow paging cursors and return every template |
| `--after <cursor>` | Return the single page following this cursor |

Results are paged. Without `--all`, a listing that has more pages says so on stderr and names
the cursor to resume from, so a partial result never looks complete. See
[Paging](flow.md#paging) for the details — the behavior is identical across list commands, except
[`block list`](block.md#paloma-block-list).

```bash
paloma template list
paloma template list --status APPROVED --category UTILITY
paloma template list --fields name,status,quality_score
paloma template list --fields name,category,correct_category
paloma template list -o json | jq -r '.data[] | "\(.name)\t\(.status)"'
```

The raw Cloud API response is returned, so `-o json` pipes cleanly to `jq`.

There's no quality filter, because Meta's docs define `quality_score` only as a field. Ask
for it with `--fields` and filter the result yourself.

---

## `paloma template status`

Compact approval view for a single template by name.

```
paloma template status <name>
```

```bash
paloma template status order_ready
paloma template status order_ready -o json
```

Output includes `name`, `language`, `category`, `status`, and `rejected_reason` (null if
Meta returned none). Exits non-zero if no template by that name exists on the WABA.

The lookup follows paging until it reaches a template with exactly that name, and fetches
nothing after that page.

---

## `paloma template get`

Show the full definition of a template by name, optionally narrowed to one
language.

```
paloma template get <name> [--language es]
```

The result is `{"data": [...]}` holding only templates with exactly that name, from the first
page that has one; `-q` prints the first one's id. Later pages aren't fetched, so without
`--language` other translations may be on them. Paloma says so on stderr when more pages
exist. Exits non-zero if no page has the name.

---

## `paloma template create` / `submit`

Submit a template for approval, either from a full `--file` or from inline flags.
`submit` is an alias of `create`.

```bash
paloma template create --file template.json

paloma template create --name order_ready --category UTILITY --language en_US \
      --body "Hi {{1}}, order {{2}} is ready." --example "Hector,#4821"

HANDLE=$(paloma media upload ./flyer.jpg --handle -q)
paloma template create --name summer_sale --category MARKETING --language en_US \
      --header-media IMAGE --header-handle "$HANDLE" \
      --body "Use {{1}} at checkout." --example 25OFF \
      --footer "Reply STOP to opt out" \
      --button "url:Shop=https://example.com/shop?promo={{1}}" --url-example summer \
      --button "quick_reply:Unsubscribe"
```

Use `--dry-run` to print the exact request body without submitting.

| Flag | Description |
|------|-------------|
| `--file <path>` | The full template definition as JSON; can't be combined with the other flags |
| `--name <name>` | Lowercase letters, digits and underscores, up to 512 characters |
| `--category <c>` | `MARKETING`, `UTILITY`, or `AUTHENTICATION` |
| `--language <code>` | Language code, e.g. `en_US` |
| `--body <text>` | BODY text, up to 1024 characters |
| `--example <a,b>` | Comma-separated example for each body variable; write a comma in a value as `\,`. With named parameters, each is `name=value` |
| `--parameter-format <f>` | `positional` (`{{1}}`, Meta's default) or `named` (`{{first_name}}`) |
| `--header-text <text>` | Text header, up to 60 characters, with at most one variable |
| `--header-example <v>` | Example for that variable. With named parameters, `name=value` |
| `--header-media <m>` | Media header: `IMAGE`, `VIDEO`, `GIF`, or `DOCUMENT`. Needs `--header-handle` |
| `--header-handle <h>` | Sample media handle from [`media upload --handle`](media.md#upload-handles). A media id won't work |
| `--footer <text>` | Footer, up to 60 characters |
| `--button <spec>` | A button, in display order; repeatable (see below) |
| `--url-example <v>` | Example for a URL button's variable; repeat in the order of those buttons |
| `--allow-category-change` | Sends `allow_category_change`. Meta has reassigned categories by default since April 9, 2025 |
| `--ttl-seconds <n>` | Sends `message_send_ttl_seconds` (see below) |

Each variable needs an example, and positional variables are numbered from `{{1}}` without
gaps. Otherwise the command exits 5 before any API call.

GIF headers are only for the [Marketing Messages API](https://developers.facebook.com/documentation/business-messaging/whatsapp/marketing-messages/features).

### Buttons

| `--button` | Limit per template | Notes |
|------------|--------------------|-------|
| `quick_reply:TEXT` | 10 | Quick replies must sit together, before or after the other buttons |
| `url:TEXT=URL` | 2 | URL up to 2000 characters, with at most one variable, at its end |
| `phone_number:TEXT=NUMBER` | 1 | Number up to 20 characters |
| `copy_code:EXAMPLE` | 1 | Example up to 20 characters |

Labels are up to 25 characters, and a template can have 10 buttons in total. Paloma checks all
of this and exits 5.

### Time-to-live

| Category | `--ttl-seconds` |
|----------|-----------------|
| `AUTHENTICATION` | 30 to 900, or `-1` for 30 days |
| `UTILITY` | 30 to 43200, or `-1` for 30 days |
| `MARKETING` | 43200 to 2592000 (applies to the Marketing Messages API) |

### What needs `--file`

Inline flags don't cover authentication templates' footers and one-time password buttons, so
`AUTHENTICATION` with a header, footer or button exits 5. Also use `--file` for carousels,
limited-time offers, location headers, and voice call, flow, catalog and other button types.

## `paloma template watch`

Poll a template's approval status until it resolves. CI-friendly: exits `0` on
APPROVED, non-zero on REJECTED or timeout.

```
paloma template watch <name> [--interval 10s] [--max-wait 10m]
```

## `paloma template edit`

Edit an existing template's components/category. The template is resolved by name
(and `--language` when it exists in more than one).

Without `--language`, Paloma reads every page of the lookup first, and exits 5 if any page
shows the name in more than one language. With `--language`, it stops at the first match.

```bash
paloma template edit order_ready --file template.json
```

## `paloma template delete`

Delete a template by name, one template by id, or several by id. Every form confirms first,
naming what it will delete, unless `--yes` (or `--dry-run`).

```
paloma template delete <name> [--language <code> | --id <id>]
paloma template delete --ids <id,id,...>
```

| Form | Deletes |
|------|---------|
| `<name>` | Every language of the name |
| `<name> --language <code>` | That language, after looking up its id |
| `<name> --id <id>` | The template with that id. Meta requires the name as well |
| `--ids <id,id,...>` | Up to 100 templates. It can't be combined with a name, and if any id is invalid Meta deletes none |

```bash
paloma template delete order_ready
paloma template delete order_ready --language es --yes
paloma template delete order_ready --id 1407680676729941 --yes
paloma template delete --ids 1387372356726668,1304694804498707 --yes
```

`-q` prints the id with `--id`, the name otherwise, and nothing with `--ids`.

After you delete an approved template, its name can't be reused for 30 days. Templates in a
disabled status can't be deleted.

---

## Planned verbs

`namespace` (fetch the WABA `message_template_namespace`). See the
[planned commands](planned.md).
