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

## `paloma number health`

Read `health_status` for the active number, or pass another phone-number id. Returns
Meta's messaging and calling diagnostics. A `BLOCKED` or `LIMITED` state is response
data, so a successful read still exits `0`. With `-q`, prints the returned number id.

```bash
paloma number health
paloma number health 1556677889900 -o json
```

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
then register again without this flag. Paloma doesn't call the Settings API yet.
Deregistering doesn't turn local storage off.

## `paloma number set-pin`

Set or update the two-step verification PIN.

```bash
paloma number set-pin               # prompts for the PIN
```

### Passing the PIN in scripts

Both commands read the PIN from one of these, and prompt if there's a terminal and none is set:

| Source | Example |
|--------|---------|
| `PALOMA_PIN` | `PALOMA_PIN="$PIN" paloma number register` |
| `--pin-stdin` | `printf '%s' "$PIN" \| paloma number register --pin-stdin` |

With neither and no terminal, the command exits `5`, including under `--dry-run`, where any
six digits will do because the output masks them. The older `--pin <value>` flag still
works, but it warns, because other processes can read a command line.

PIN values are redacted from dry-run output and verbose request diagnostics. The real request sends the supplied PIN.

Explicit phone-number Graph ids must contain only digits; invalid ids exit `5` before
context resolution or an API call.

`health` tables show the overall status plus entity diagnostics and suggested fixes.
JSON, YAML and raw keep Meta's full response, and quiet output remains the number id.
