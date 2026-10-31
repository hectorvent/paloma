# `paloma number`

Phone-number management on the WABA. Reads use GET; the write verbs
support `--dry-run`.

```
paloma number <verb> [args] [flags]
```

## `paloma number list`

List the phone numbers on the active WABA. A single page warns on stderr when more
exist.

| Flag | Meaning |
|------|---------|
| `--account-mode <SANDBOX\|LIVE>` | Only numbers in this account mode. Meta is still testing this filter, so not every developer has it |
| `--sort <last-onboarded-asc\|last-onboarded-desc>` | Order by when Embedded Signup onboarding finished. Newest first is Meta's default, so `last-onboarded-desc` sends no parameter |
| `--limit <n>` | Page size, 1 to 100; anything else exits `5` |
| `--all` | Follow every page and merge them |
| `--after <cursor>` | Start after this cursor |

```bash
paloma number list
paloma number list --all --account-mode LIVE --sort last-onboarded-asc
paloma number list -o json | jq -r '.data[] | "\(.id)\t\(.display_phone_number)\t\(.quality_rating)"'
```

## `paloma number get` / `quality`

Show details for a number (defaults to the active sender; pass an id to target
another). `get` includes the number's `status`, its display name's `name_status`, and
`health_status`, the summary of whether it can send messages. `quality` is a compact view
of rating, messaging limit, and throughput.

The limit comes from `whatsapp_business_manager_messaging_limit` (for example `TIER_250`).
[Meta sets it per business portfolio](https://developers.facebook.com/documentation/business-messaging/whatsapp/messaging-limits),
so every number in a portfolio shares it.

```bash
paloma number get
paloma number get 1556677889900
paloma number quality
```

Explicit phone-number Graph ids must contain only digits; invalid ids exit `5` before
context resolution or an API call.

## `paloma number health`

Read `health_status` for the active number, or pass another phone-number id. Returns
Meta's messaging and calling diagnostics. A `BLOCKED` or `LIMITED` state is response
data, so a successful read still exits `0`. With `-q`, prints the returned number id.

```bash
paloma number health
paloma number health 1556677889900 -o json
```

Tables show the overall status plus entity diagnostics and suggested fixes. JSON, YAML
and raw keep Meta's full response.

## `paloma number register` / `deregister`

Register the active number for Cloud API with its six-digit PIN, or deregister it.
`deregister` is disruptive, so it confirms unless `--yes`.

```bash
paloma number register              # prompts for the PIN
paloma number register --data-localization-region DE
paloma number deregister --yes
```

### Local storage

`--data-localization-region <CC>` enables
[local storage](https://developers.facebook.com/documentation/business-messaging/whatsapp/local-storage)
as part of the registration, keeping message data at rest in that country. It accepts
`AE`, `AU`, `BH`, `BR`, `CA`, `CH`, `DE`, `GB`, `ID`, `IN`, `JP`, `KR`, `SG` and `ZA`,
in uppercase. Any other value exits `5` before the PIN is read or a request is sent.

The flag applies only when first registering a number. To enable, change or disable local
storage on a registered number, deregister it, use the
[Settings API](https://developers.facebook.com/documentation/business-messaging/whatsapp/reference/whatsapp-business-phone-number/settings-api),
then register again without this flag. Use `number settings set --storage-region` for that change.
Deregistering doesn't turn local storage off.

## `paloma number pin set`

Replace the two-step verification PIN. The old PIN stops working, so it confirms first.

```bash
paloma number pin set               # prompts for the PIN and the confirmation
PALOMA_PIN="$PIN" paloma number pin set --yes
```

### Passing the PIN in scripts

Both commands read the PIN from one of these, and prompt if there's a terminal and none is set:

| Source | Example |
|--------|---------|
| `PALOMA_PIN` | `PALOMA_PIN="$PIN" paloma number register` |
| `--pin-stdin` | `printf '%s' "$PIN" \| paloma number register --pin-stdin` |

With neither and no terminal, the command exits `5`, including under `--dry-run`, where any
six digits will do because the output masks them. There is no `--pin <value>` flag,
because other processes can read a command line.

PIN values are redacted from dry-run output and verbose request diagnostics. The real request sends the supplied PIN.

## Verification and onboarding

Add a number to the WABA, verify ownership, then register it for Cloud API use:

```bash
paloma number add 15551234567 --cc 1 --name 'Acme Support'
paloma number verification request --method SMS --language en_US --yes
# Put the returned number ID in the selected profile before verifying/registering.
paloma number verification confirm --code-stdin
paloma number register --pin-stdin
```

`add <phone_number> --cc <code> --name <name>` takes digits without `+`, with or without
the country calling code, plus the calling code Meta requires, and prints the new number
ID under `-q`. Adding a number does not register it. `verification request`
uses the active phone-number ID, accepts `--method SMS|VOICE`, and defaults
`--language` to `en_US`. It confirms before triggering a text or phone call; an
already verified number can be refused by Meta.

`verification confirm` reads the numeric code from `PALOMA_VERIFICATION_CODE`, `--code-stdin`
or a hidden prompt. Codes never belong in argv. Dry-run and verbose request output
mask the code. The separate `register --pin-stdin` input is the two-step PIN.

All writes support `--dry-run`. Validation failures exit `5`; closed-set argument
errors exit `2`. Verification and registration success responses have no primary ID,
so `-q` produces no stdout.

## Display names and search visibility

```bash
paloma number name set 'Acme Support' --dry-run
paloma number visibility set HIDDEN --yes
```

`name set <name>` submits `new_display_name` for Meta's review. It does not promise
approval or immediate activation. `visibility set <VISIBLE|HIDDEN>` changes search
visibility without changing the username. Both confirm before writing.

## `paloma number username`

```text
paloma number username get
paloma number username list
paloma number username set <username> [--force-transfer]
paloma number username delete
```

`get -q` prints the current username; JSON/raw preserve approval status.
`list` reads the usernames reserved for the portfolio. Adoption, transfer and
deletion confirm and honor `--yes` and `--dry-run`.

Usernames must contain 3–35 ASCII letters, digits, periods or underscores, with at
least one letter. Leading/trailing periods, consecutive periods, `www.` prefixes and
Meta's documented domain suffixes are refused locally. Availability and eligibility
remain Meta decisions. `--force-transfer` can remove a username from another number
in the same portfolio and assign it here; it is never an automatic fallback.

Meta permits two changes per number in a rolling 14-day window (ten for a `reserved`
username). Deleting does not reset the limit. Excess changes return rate-limit exit
`4`; Meta does not provide a `Retry-After` header for this limit.

## `paloma number contacts`

```bash
paloma number contacts get US.13491208655302741918 -o json
paloma number contacts delete US.13491208655302741918 --yes
```

Checks or deletes one portfolio-scoped BSUID in Meta's contact book. Parent BSUIDs
and phone numbers are refused. The contact book is scoped to the whole portfolio;
deleting an entry affects all its numbers. Meta populates entries through qualifying
interactions; these commands do not create or enumerate contact-book entries.
Both preserve Meta's response, and deletion confirms before writing. If Meta reports
`deleted: false`, the command warns on stderr and still exits `0`.

## `paloma number settings`

```bash
paloma number settings get -o json
paloma number settings set --identity-check --dry-run
paloma number settings set --storage-region IN --dry-run
paloma number settings set --no-storage --dry-run
```

`get` reads settings without requesting SIP credentials. `set` changes exactly one
setting per call and confirms, because a storage change needs a deregistered number and
must not hold back an identity change. `--identity-check`/`--no-identity-check` controls Meta's
`user_identity_change.enable_identity_key_check`. Delivery with an identity hash is
still subject to Meta's identity comparison.

Storage takes `--storage-region <CC>` to turn it on, or `--no-storage` to turn it off. Supported countries
are the same as registration's `--data-localization-region`. **The number must already
be deregistered.** After changing storage, register it again without the localization
flag. Paloma does not automatically deregister or re-register it. The focused
[local-storage guide](https://developers.facebook.com/documentation/business-messaging/whatsapp/local-storage)
uses `status` and `data_localization_region`; these guide fields are used instead of
the conflicting generated reference's `enabled`/`region` shape.

## Official Business Account applications

```bash
paloma number oba get -o json
paloma number oba apply --file oba.json --dry-run
```

`get` reads `oba_status` from the number, and `get -q` prints just the status.

The application document contains `business_website_url` and
`primary_country_of_operation` (such as `US`). Optional `supporting_links` must contain 5–10 HTTP(S)
URLs. Other documented optional fields are `primary_language`,
`parent_business_or_brand`, and `additional_supporting_information`. Submission
confirms, and Meta determines eligibility, review and approval.

```json
{
  "business_website_url": "https://example.com",
  "primary_country_of_operation": "US"
}
```

## India compliance information

```bash
paloma number compliance get -o json
paloma number compliance set --file compliance.json --dry-run
```

The document requires a 2–128 character `entity_name`, a documented `entity_type`,
`grievance_officer_details` (name and email), and `customer_care_details` (email).
`entity_type_custom` is required only for `OTHER` and refused with other types.
`is_registered` is boolean and only applies to `OTHER` or `PARTNERSHIP`. Email fields
must be valid and shorter than 128 characters; optional mobile/landline numbers use
international format. Paloma supplies `messaging_product: whatsapp`. Submitting the
regional compliance document confirms and preserves Meta's response.

```json
{
  "entity_name": "Acme Support",
  "entity_type": "PRIVATE_COMPANY",
  "grievance_officer_details": {
    "name": "Support Officer",
    "email": "officer@example.com"
  },
  "customer_care_details": {"email": "support@example.com"}
}
```
