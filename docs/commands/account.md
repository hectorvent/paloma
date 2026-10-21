# `paloma account`

Read the active WhatsApp Business Account (WABA), its health, subscribed apps and
messaging limits. Select the account with `--waba-id`, `PALOMA_WABA_ID` or the active
profile. No active phone-number id is required.

```console
paloma account get
paloma account health
paloma account limits --all
paloma account subscribed-apps
```

## `paloma account get`

By default, requests `id`, `name`, `timezone_id`, `message_template_namespace`,
`account_review_status` and `business_verification_status`. Use `--fields` to replace
that selection with a comma-separated Graph field selection. Available fields and
permissions depend on the account and Graph version; Graph validates the selection.
A blank selection exits `5` before context resolution or an API call.

```bash
paloma account get --waba-id 123456789 -o json
paloma account get --fields id,name,service_pricing_policy
paloma account get -q  # WABA id only
```

`service_pricing_policy` reports the account's service-message pricing policy. It is
opt-in because newer account fields may not be available in older Graph versions.

## `paloma account health`

Requests the WABA's `health_status`, including the messaging and calling diagnostics
Meta returns for related entities. Table output lists the overall status and each entity,
including error descriptions and suggested fixes; other formats keep the response.
A reported `BLOCKED` or `LIMITED` state remains response data: a successful read exits `0`. With `-q`, prints the returned WABA id.

```bash
paloma account health -o json | jq '.health_status'
```

## `paloma account limits`

Lists the WABA's phone numbers with their ids, display numbers and
`whatsapp_business_manager_messaging_limit` (for example `TIER_250`). Meta exposes this
field through the phone-number API. The limit is **shared across the business
portfolio**, including numbers on other WABAs in that portfolio: do not add the rows
as separate allowances. This read does not report remaining capacity or usage.

```bash
paloma account limits --all -o json
paloma account limits --business-id 123456789
```

`--business-id` reads the shared limit directly from a business portfolio without WABA
context. It is exclusive with paging flags; the id must contain only digits.

## `paloma account subscribed-apps`

Lists apps subscribed to the WABA, showing each app's id, name, link and any override
callback URL. Uses the same `subscribed_apps` endpoint as `webhook get`; `webhook get` also
resolves the effective callback. Use `webhook fields` to read the app's field
subscriptions.

```bash
paloma account subscribed-apps -o json
```

Both list commands accept:

| Flag | Meaning |
|------|---------|
| `--all` | Follow all pages and merge the results |
| `--after <cursor>` | Start after this cursor |

Without `--all`, a single page is returned and a note on stderr says if more results
exist. Lists have no primary id, so `-q` produces no output. JSON/raw preserve the
Graph body for a single page; `--all` renders the merged response.

All four reads support `--dry-run`: print the GET request without sending it or reading
a token. Data goes to stdout; diagnostics go to stderr. Missing WABA context or rejected
credentials exit `3`; rate limits exit `4`. Reads require a token with access to the
WABA and appropriate WhatsApp business management permissions.

## `paloma account set`

Update common account settings with typed flags, or supply a full writable JSON object
with `--file`. The file is exclusive with typed flags. At least one setting is required;
empty/non-object JSON and blank names/timezone ids exit `5` before an API call.

```bash
paloma account set --name 'Acme Support' --timezone-id 1 --dry-run
paloma account set --enable-insights --yes
paloma account set --file account-settings.json --dry-run
```

`--enable-insights` (also `--insights true`) permanently enrolls the account in template
insights. It requires confirmation or `--yes` and must be sent separately from other
settings. Meta does not support disabling enrollment, so `--insights false` exits `5`.
The same checks apply to JSON files. Dry-run previews the enrollment query parameter.
`--timezone-id` takes Meta's numeric Graph timezone identifier, not an IANA timezone
name. Supported settings and permissions are account-dependent.

`--disable-cloud-marketing true|false` (alias `--disable-marketing-messages-on-cloud-api`) controls
`disable_marketing_messages_on_cloud_api`. Enabling it requires confirmation (or `--yes`)
because `/messages` then refuses marketing templates, including Cloud API fallback from
the Marketing Messages API. Complete MM API onboarding first. To inspect eligibility:

```bash
paloma account get --fields id,marketing_messages_onboarding_status
paloma account set --disable-cloud-marketing true --dry-run
```

A successful settings write has no primary id, so `-q` produces no output.

## `paloma account activities`

Read the WABA activity log, including event type, timestamp, actor ids and category.
Supports `--all`, `--after` and `--limit <1..100>`, with the same paging and
quiet-output behavior as the other account lists.

```bash
paloma account activities --all -o json
```

## `paloma account billing-currency`

Three separate commands cover asset cloning, progress and the final billing switch:

```bash
paloma account billing-currency start --currency INR --dry-run
paloma account billing-currency start --currency INR --yes -q
paloma account billing-currency status <migration-id> -o json
paloma account billing-currency complete <migration-id> --dry-run
```

`start` uses the active WABA as the source Messaging account. It requires a three-letter
uppercase currency code and accepts `--extended-credit-id` for Line of Credit billing.
Supported currencies and account eligibility are checked by Meta. `-q` prints the
returned migration id. Starting requires confirmation, or `--yes`.

`status` and `complete` take the migration id directly and need no configured WABA or
phone number. Their quiet output is the returned migration id.

Before completion, inspect the destination account's templates, Flows, app installation,
permissions and payment method. Credit cards must be attached in Business Manager.
`complete` requires confirmation (or `--yes`), checks the current status, and only posts
`resume_migration` for `READY_TO_COMPLETE` or a retryable `FAILED_TO_COMPLETE`. Other states
exit `1` without a completion write. Successful completion makes the new account the
billing account and stops the old account from sending.

Dry-run prints the readiness GET and completion POST without executing either. A start
dry-run prints its single POST. A real completion makes a GET followed by the POST. Neither command changes your
Paloma configuration. See [the billing currency guide](../billing-currency.md) for the
prerequisites, destination checks and profile updates.

## `paloma account analytics`

Read messaging counts by default, or delivered-volume and approximate-cost breakdowns
with `--pricing`. Both are GET field expansions on the active WABA/Messaging account.
Use `--waba-id`, `PALOMA_WABA_ID` or the profile to choose it. No phone-number id is
required. The send-only `--messaging-account-id` does not override this selection, so
`--waba-id <old-account>` still reaches pre-migration history.

```bash
paloma account analytics --start 2026-11-01 --end 2026-11-02
paloma account analytics --start 1793491200 --end 1793577600 --product-type template
paloma account analytics --pricing --start 2026-11-01 --end 2026-11-02 \
  --metric cost,volume --dimension country,pricing-category,pricing-type
paloma account analytics --pricing --start 2026-11-01 --end 2026-11-02 \
  --dimension country,pricing-category,tier --country IN --pricing-category utility -o json
```

### Time and common filters

| Flag | Meaning |
|---|---|
| `--start <time>` | Required. Nonnegative UNIX **seconds**, or `YYYY-MM-DD` at midnight UTC |
| `--end <time>` | Required, in the same formats; must be later than `--start` |
| `--granularity half-hour\|day\|month` | Reporting interval; defaults to `day` |
| `--phone-number <number>` | Actual E.164 number, not a Graph id or BSUID; repeat or comma-separate. A leading `+` is accepted and omitted in the Graph filter |
| `--country <code>` | Uppercase two-letter country code; repeat or comma-separate |

Granularity maps to `HALF_HOUR`, `DAY`, `MONTH` for messaging, and `HALF_HOUR`, `DAILY`,
`MONTHLY` for pricing. Date inputs are always UTC; they do not inherit your machine's
timezone. Meta controls retention and bucket boundaries. Its documented lookback for
these reports is one year; Paloma does not invent a fixed-day equivalent or silently
truncate a requested range.

Omitted filters are omitted from the request, letting Meta select all available data.
Only currently associated phone numbers contribute, and historical data does not move
when a Messaging account is cloned for billing migration.

### Messaging filters

`--product-type template|non-template|incoming` selects template sends (`0`), non-template
sends (`2`) or incoming messages (`100`). Repeat or comma-separate to combine them.
This flag cannot be used with `--pricing`.

The table shows `start`, `end`, `sent` and `delivered` for each returned data point.

### Pricing filters

These flags require `--pricing` and can be repeated or comma-separated:

| Flag | Values |
|---|---|
| `--metric` | `cost`, `volume` |
| `--pricing-type` | `free-customer-service`, `free-entry-point`, `regular` |
| `--pricing-category` | `authentication`, `authentication-international`, `marketing`, `marketing-lite`, `marketing-lite-dynamic`, `service`, `utility`, `referral-conversion` |
| `--dimension` | `country`, `phone`, `pricing-category`, `pricing-type`, `tier` |

For volume-tier information, request `country`, `pricing-category` and `tier` together.
The `marketing-lite-dynamic` category covers the newer max-price pricing category.

The table includes all data points from all returned groups, with time boundaries,
phone number, country, pricing category/type, tier, volume and cost. Missing fields
show as `-`. Cost is approximate, in the selected account's currency; Meta may omit it
for accounts billed through a partner credit line. Missing costs remain absent, and
Paloma neither replaces them with zero nor adds totals across dimensions.

### Output and failures

JSON, YAML and raw preserve Meta's response envelope and any extra fields. Only table
output flattens pricing groups. `-o raw` preserves decimal text exactly as received;
`-q` is silent because a report has no primary resource id. An empty report succeeds
and renders `(none)` as a table; malformed report structures exit `1`.

All local date, range, phone and country checks run before context resolution or API
calls and exit `5`. Missing flags, invalid enum values and incompatible filter modes
exit `2`. Missing WABA context exits `3`. API failures keep the shared Graph error/exit
mapping, including HTTP `401`/`403` authentication failures (`3`) and throttling (`4`).

`--dry-run` prints the GET, including the complete encoded field expansion, without
reading a token or sending a request:

```bash
paloma account analytics --pricing --start 2026-11-01 --end 2026-11-02 --dry-run -o json
```

The deprecated `conversation_analytics` field is not exposed. Template and calling
analytics belong to their own resources and are outside this command.

## Assigned users and system users

```bash
paloma account users --business-id 123 --all -o json
paloma account users --system -q
```

Assigned-user reads require `--business-id` to identify the business context.
`--system` selects the WABA's system-users endpoint instead and conflicts with that
flag. Both accept `--limit` (1–100), `--after`, and `--all`. A single page warns when
more results exist; `--all` follows cursors. Table output includes ID/name/user type,
`-q` prints IDs, and JSON/raw retain returned metadata. Dry-run prints the first
request offline. These are read-only commands and do not grant or remove access.

## Welcome message sequences

```bash
paloma account welcome-sequence list --limit 5
paloma account welcome-sequence list --id 123 -o json
paloma account welcome-sequence create --name Support --file sequence.json --dry-run
paloma account welcome-sequence set 123 --name 'Support updated' --yes
paloma account welcome-sequence delete 123 --yes
```

These are WABA-scoped assets for click-to-WhatsApp ads. The JSON file is the sequence
content, with `text`, `autofill_message.content`, and/or `ice_breakers` containing
`title` fields. `create` requires `--name` and `--file`; `set` accepts either or both.
`list` accepts optional `--id` and positive `--limit`. JSON/raw preserve Meta's array
response. `create -q` prints the sequence ID; updates and deletes have no primary ID.
Updates and deletes confirm, and all writes support dry-run. Meta refuses deletion
of sequences used in active ads.

```json
{
  "text": "Welcome to Acme",
  "autofill_message": {"content": "I'd like some help"},
  "ice_breakers": [{"title": "Track an order"}]
}
```
