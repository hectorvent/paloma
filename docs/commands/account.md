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
Meta returns for related entities. A reported `BLOCKED` or `LIMITED` state remains
response data: a successful read exits `0`. With `-q`, prints the returned WABA id.

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
```

## `paloma account subscribed-apps`

Lists apps subscribed to the WABA, showing each app's id, name and any override callback
URL. Uses the same `subscribed_apps` endpoint as `webhook get`; `webhook get` also
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

`account analytics` remains planned for v1.0.

## `paloma account set`

Update common account settings with typed flags, or supply a full writable JSON object
with `--file`. The file is exclusive with typed flags. At least one setting is required;
empty/non-object JSON and blank names/timezone ids exit `5` before an API call.

```bash
paloma account set --name 'Acme Support' --timezone-id 1 --dry-run
paloma account set --insights true
paloma account set --file account-settings.json --dry-run
```

`--insights true|false` controls `is_enabled_for_insights`. `--timezone-id` takes Meta's
Graph timezone identifier. Supported settings and permissions are account-dependent.

`--disable-cloud-marketing true|false` controls
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
Supports `--all` and `--after`, with the same paging and quiet-output behavior as the
other account lists.

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
