# `paloma template`

Manage the full message-template lifecycle on the WABA: `list`, `status`, `get`,
`create`, `submit`, `watch`, `edit`, `delete`, `namespace`, `archive`, `unarchive`, `unpause`,
`library`, `compare`, `migrate`, `preview`, `upsert`, `group`, `analytics`.

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
paloma template get <name> [--language es] [--quality]
```

The result is `{"data": [...]}` holding only templates with exactly that name, from the first
page that has one; `-q` prints the first one's id. Later pages aren't fetched, so without
`--language` other translations may be on them. Paloma says so on stderr when more pages
exist. Exits non-zero if no page has the name.

`--quality` asks only for `name`, `language`, `status`,
[`quality_score`](https://developers.facebook.com/documentation/business-messaging/whatsapp/templates/template-quality) and
[`health_status`](https://developers.facebook.com/documentation/business-messaging/whatsapp/support/health-status), and the table shows the score and whether the
template can be sent.

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

## `paloma template namespace`

Print the WABA's `message_template_namespace`. `-q` prints only the namespace.

## `paloma template archive` / `unarchive`

[Archive](https://developers.facebook.com/documentation/business-messaging/whatsapp/templates/template-archival) up to 100 templates by id, or restore them.

```bash
paloma template archive --ids 1387372356726668,1304694804498707 --yes
paloma template unarchive --ids 1387372356726668
```

An archived template can't be sent, and Meta deletes it after 28 days unless it's unarchived,
so `archive` confirms first. `unarchive` restores the previous status and doesn't confirm.

Meta answers 200 even when some templates fail. Paloma prints the response, warns on stderr and
exits 1 if `failed_templates` isn't empty.

## `paloma template unpause`

[Unpause](https://developers.facebook.com/documentation/business-messaging/whatsapp/templates/template-pausing) a template by id. A template paused during template
pacing stays paused until you do this; other pauses also end on their own.

```bash
paloma template unpause 1387372356726668
```

## `paloma template library`

Browse [Meta's Template Library](https://developers.facebook.com/documentation/business-messaging/whatsapp/templates/template-library), and create a template from an
entry.

```bash
paloma template library list --search payments
paloma template library list --topic ORDER_MANAGEMENT --language en_US --all

paloma template library create --name my_delivery_update \
      --library-template delivery_update_1 --language en_US \
      --button-inputs-file buttons.json
```

`library list` takes `--search`, `--topic`, `--usecase`, `--industry`, `--language`, `--name`,
`--all` and `--after`. It doesn't need a WABA id, because the library is Meta's.

| `library create` flag | Description |
|-----------------------|-------------|
| `--name <name>` | **Required.** Name for your template |
| `--library-template <name>` | **Required.** The exact name of the library template |
| `--language <code>` | **Required.** Language code |
| `--button-inputs-file <path>` | JSON array for `library_template_button_inputs`. Meta requires it for a utility template with buttons |
| `--body-inputs-file <path>` | JSON object for `library_template_body_inputs` |

The category is always sent as `UTILITY`, which
[Meta's page](https://developers.facebook.com/documentation/business-messaging/whatsapp/templates/template-library)
requires when creating from the library.

## `paloma template compare`

[Compare](https://developers.facebook.com/documentation/business-messaging/whatsapp/templates/template-comparison) two templates on the same WABA: their sends, which
has the lower block rate, and each one's top reason for being blocked.

```bash
paloma template compare 5289179717853347 1533406637136032 --days 30
```

`--days` is the window back from now, and must be 7, 30, 60 or 90. Each template must have been
sent at least 1,000 times in it, or Meta returns an error.

## `paloma template migrate`

[Recreate templates](https://developers.facebook.com/documentation/business-messaging/whatsapp/templates/template-migration) from another WABA of the same business on
the active WABA, after confirming. The source keeps its templates. Only `APPROVED` templates with
a `quality_score` of `GREEN` or `UNKNOWN` qualify, and Meta wants a system or business token.

```bash
paloma template migrate --from 102290129340398 --yes
paloma template migrate --from 102290129340398 --page 1 --yes
paloma template migrate --from 102290129340398 --ids 35002248699842,351234565148 --yes
```

| Flag | Description |
|------|-------------|
| `--from <waba-id>` | **Required.** The WABA to copy from |
| `--ids <id,id,...>` | Only these templates, up to 500. Use it to retry the failed ones |
| `--page <n>` | Which set of 500 templates to migrate, from 0 |
| `--count <n>` | Batch size, up to 500. Lower it if the request times out |

As with `archive`, a response with `failed_templates` is printed and exits 1.

## `paloma template preview`

[Preview authentication template text](https://developers.facebook.com/documentation/business-messaging/whatsapp/templates/authentication-templates/template-preview)
in each language. `--auth` is required, because those are the only previews Meta offers.

```bash
paloma template preview --auth --language en_US,es_ES \
      --security-recommendation --code-expiration 10
```

| Flag | Description |
|------|-------------|
| `--language <a,b>` | Keep only these language codes. Paloma filters the result itself, and warns about a code that has no preview |
| `--security-recommendation` | Include the security recommendation in the body |
| `--code-expiration <minutes>` | Include the expiration footer, from 1 to 90 minutes |

## `paloma template upsert`

[Create or update an authentication template](https://developers.facebook.com/documentation/business-messaging/whatsapp/templates/authentication-templates/bulk-management)
in several languages. A template with a matching name and language is updated, and any other
language is created, with no confirmation.

```bash
paloma template upsert --file auth_code.json
```

The file is the template definition with `languages`, an array, in place of `language`. `-q`
prints the first template's id. These exit 5 before any API call: a file with `language`, without
`languages`, without a valid `name`, whose `category` isn't `AUTHENTICATION`, or with a `text` or
`autofill_text` property, which Meta's page says upsert doesn't support.

## `paloma template group`

[Template groups](https://developers.facebook.com/documentation/business-messaging/whatsapp/templates/template-groups) track the metrics of a set of templates together.
A template can be in one group at a time.

```bash
paloma template group create --name "Black Friday" --description "US sale" \
      --templates 278077987957091,1945418102598215
paloma template group get 9020555671393375
paloma template group set 9020555671393375 --add 844006344365270 --remove 1035843174854974
paloma template group delete 9020555671393375 --yes
```

| Verb | Notes |
|------|-------|
| `create` | `--name` up to 256 characters and unique, `--description` up to 512, `--templates` required. `-q` prints the group's id |
| `get` | The group and the templates in it |
| `set` | Any of `--name`, `--description`, `--add`, `--remove`. Metrics a removed template contributed stay with the group |
| `delete` | Confirms first. The templates stay |

There's no `list`, because Meta's docs don't describe a way to list groups.

## `paloma template analytics`

Daily [template and template group metrics](https://developers.facebook.com/documentation/business-messaging/whatsapp/analytics#template-analytics).

**This needs template analytics enabled on the WABA**, either in WhatsApp Manager or with
`paloma account set --insights true` (see [Turning on insights](account.md#paloma-account-set)).
Once enabled, it can't be disabled. It isn't available to accounts in the European Union or Japan.

```bash
paloma template analytics --ids 1421988012088524,2632273056924580 \
      --start 2026-10-01 --end 2026-10-08 --metrics sent,delivered,read
paloma template analytics --ids 1421988012088524 --start 2026-10-01 --end 2026-10-08 \
      --product-type MARKETING_MESSAGES_LITE_API
paloma template analytics --groups 1044106240855852 --start 1738465116 --end 1739559516
```

| Flag | Description |
|------|-------------|
| `--ids <id,id,...>` | Template ids, up to 10 (`template_analytics`) |
| `--groups <id,id,...>` | Template group ids instead, up to 10 (`template_group_analytics`) |
| `--start <t>`, `--end <t>` | **Required.** A UNIX timestamp or `YYYY-MM-DD`. The lookback window is 90 days. An impossible date, or a start after the end, exits `5`; the same day for both is fine |
| `--metrics <a,b>` | Metrics to return, e.g. `cost`, `clicked`, `delivered`, `read`, `sent`. All of them by default |
| `--product-type <t>` | `CLOUD_API` or `MARKETING_MESSAGES_LITE_API`. Meta counts only Cloud API sends by default, so **sends made with [`--via marketing`](send.md#category-aware-routing) need this**. Templates only |
| `--waba-timezone` | Use the WABA's timezone instead of UTC. `--start` and `--end` must then be `YYYY-MM-DD` |
| `--after <cursor>` | Return the page following this cursor |

The table shows sent, delivered and read counts. Use `-o json` for `clicked`, `cost` and the
conversion metrics.
