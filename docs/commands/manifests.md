# Your WhatsApp setup, as code

Declarative, K8s-style config with `export` / `apply` / `diff`.

Manage a number's configuration the way you manage Kubernetes: describe the desired
state in a versionable YAML manifest, and apply it idempotently. One manifest
replaces the several imperative commands it would otherwise take (calling setup
alone is 3-7 commands).

## Manifest format

K8s-style envelope with a camelCase `spec`. `metadata.phoneNumberId` is optional;
targeting falls back to `--phone-number-id` or the active profile.

```yaml
apiVersion: paloma/v1
kind: Calling
metadata:
  phoneNumberId: "1556677889900"
spec:
  status: ENABLED
  callIconVisibility: DEFAULT
  callbackPermissionStatus: ENABLED
  sip:
    status: ENABLED
    servers:
      - hostname: pbx.acme.com
        port: 5061
  audio:
    additionalCodecs: [PCMA, PCMU]
```

Supported `kind`s: **Calling**, **Profile**, **Number**, **PhoneSettings**, **Commerce**,
**ConversationalAutomation**, **Account**, **QrCode**, **Template**, **Webhook**.
A file may hold several manifests separated by `---`.

## Variables & secrets

`apply` and `diff` expand `${...}` placeholders in the string values of a manifest
before applying it, so you can keep environment-specific values and secrets out of the
file. (`export` writes literal values and never emits placeholders.)

| Form | Resolves to |
|------|-------------|
| `${NAME}` / `${env:NAME}` | the `NAME` environment variable |
| `${keyring:ACCOUNT}` | a secret from the OS keyring (the same `keyring:` store as `token_ref`/`app_secret_ref`) |
| `${...:-default}` | the value, or `default` when it is unset/blank |
| `$${...}` | a literal `${...}` (escape) |

Placeholders may appear anywhere in a value, including partially
(`https://${HOST}/webhook`) and more than once. A reference that resolves to nothing
**and** has no default fails the command with a validation error naming the field, e.g.
`manifest field 'spec.verifyToken' references unset ${keyring:acme-verify}`.

```yaml
apiVersion: paloma/v1
kind: Webhook
spec:
  callbackUrl: ${WEBHOOK_URL}
  verifyToken: ${keyring:acme-verify}
  apiVersion: ${PALOMA_API_VERSION:-v26.0}
```

```bash
WEBHOOK_URL=https://tunnel.hectorvent.dev/whatsapp/webhook paloma diff -f webhook.yaml
```

## `paloma export <kind>`

Render the live config as a manifest (YAML by default, `-o json` for JSON).
`<kind>` is kebab-case: `account`, `calling`, `commerce`, `conversational-automation`,
`number`, `phone-settings`, `profile`, `qr-code`, `template` or `webhook`. The manifest's
own PascalCase name (`PhoneSettings`) works too. `--skeleton` emits the field skeleton with
no API call.

Fields a kind supports but that are **not set** are emitted as commented-out stubs, so
the manifest documents what else you can configure:

```yaml
spec:
  status: ENABLED
  # callIconVisibility:
  # sip:
  #   status:
  #   servers: []
```

The comments are ignored by `apply`, so the file still round-trips. Pass `--minimal`
to emit only the fields that are set (the previous behaviour). Annotation is YAML-only;
`-o json` is unaffected. The annotation covers every kind's writable fields, including those
of a Template.

```bash
paloma export calling > calling.yaml
paloma export profile                 # set fields + commented unset fields
paloma export profile --minimal       # only the fields that are set
paloma export calling --skeleton      # full commented skeleton for authoring
```

## `paloma apply -f <file>`

`-f` is short for `--file`, here and on `diff`.

Apply one or more manifests. For each kind it reads the current config, compares,
and **only sends a request when something differs** (idempotent). Re-applying an
unchanged manifest does nothing.

```bash
paloma apply -f calling.yaml
paloma apply -f all.yaml --dry-run     # print the POST bodies, change nothing
```

Output (to stderr) is one line per kind: `Calling/<id> unchanged` or
`Calling/<id> configured:` followed by the changed fields.

Every document is planned, and every confirmation is asked, before the first write. Without
a terminal or `--yes`, a document that needs confirmation exits `5` with nothing written.
Writes aren't atomic across documents: if one fails, stderr lists the documents not
attempted, and the ones before it stay applied.

## `paloma diff -f <file>`

Show drift between manifests and live config without writing. Exits `0` when
everything is in sync and `1` when there is drift, so CI can gate on config drift.

```bash
paloma diff -f calling.yaml
```

## How idempotency works

`apply`/`diff` GET the current config, then check whether the spec is already
satisfied (objects compared field by field; arrays like SIP servers and codecs
compared whole, since Meta replaces them wholesale). Read-only, Meta-managed fields
(`app_id`, the SIP password) are excluded, so an exported manifest re-applies as
`unchanged`.

## Per-kind notes

- **Calling** maps to the `/settings` `calling` object. Remember that enabling SIP
  routes calls as SIP INVITEs and stops the `calls` webhook.
- **Profile** maps to the business profile. The profile photo is read-only here and
  is not exported or applied.
- **Number** exports an observational snapshot plus writable search visibility.
  `pin` and `newDisplayName` are write-only submissions and always count as drift.
  A PIN is a secret, so export never includes it; use interpolation instead of
  committing one. Observational fields are ignored on apply.
- **Commerce** maps to `whatsapp_commerce_settings`: `isCartEnabled`, `isCatalogVisible`.
  Disabling either confirms unless `--yes` is set.
- **ConversationalAutomation** maps to `conversational_automation`: `enableWelcomeMessage`,
  `prompts` (ice breakers) and `commands`. Arrays are full-replace; send the complete
  desired set.
- **Template** is a managed *set* (WABA-scoped), one manifest per template keyed by
  `name` + `language`. `export template` snapshots every template as a multi-document
  file. `apply` creates a missing template or edits a changed one (an **edit
  resubmits it for approval**); unchanged templates are skipped. Templates absent from
  a manifest are never deleted (use `paloma template delete`). A `spec` may give full
  `components` or the `body`/`example` shorthand, and that choice sets what drift means:
  - With **`components`**, every component and field you declare is compared:
    header, body, footer, buttons and their order. Adding or removing a component
    counts as drift. `example` blocks are not compared: they are review samples that
    Meta rewrites on read (an uploaded header handle comes back as a CDN URL). Fields
    Meta returns that you didn't declare are ignored, and `type`/`format` match in any
    case.
  - An edit sends only what changed. Meta can't change an approved template's category and
    only edits `APPROVED`, `REJECTED` or `PAUSED` templates, so `apply` exits `5` with nothing
    written if a document asks for either. `diff` still reports it as drift.
  - With the **`body`** shorthand, only `category` and the BODY text are compared.
    Meta's edit replaces every component, so a shorthand edit resends the live
    header, footer and buttons unchanged around the new BODY. A template with a
    media header or a carousel can't be edited this way, because its media must be
    re-uploaded. `apply` refuses it (exit 5) and asks you to declare its full
    `components`.
- <a name="webhook"></a>**Webhook** is the webhook configuration (WABA-scoped), with two
  aspects in one spec: `callbackUrl` (mapped to the per-WABA `override_callback_uri`) and
  an optional `fields` list (the app-level subscribed event types). `apply` reconciles
  each independently and writes only what changed. `verifyToken` is a write-only secret:
  never exported, required **only when the callback URL changes** (so export -> apply is a
  tokenless no-op). Do not commit a verify token; pull it from the keyring or an env
  var with a [placeholder](#variables--secrets) (`${keyring:...}` / `${WHATSAPP_VERIFY_TOKEN}`),
  supply it at change time, or use `paloma webhook subscribe`. `fields` are app-level, so reading/diffing them needs an app
  access token: configure `PALOMA_APP_SECRET` (or `app_secret_ref`) and `--app-id` /
  `PALOMA_APP_ID` (or rely on auto-discovery). `export` omits `fields` when no app secret
  is configured. A newly added field must appear in
  [`webhook fields list`](webhook.md#paloma-webhook-fields), or `diff` and `apply`
  exit `5`. Already-subscribed fields remain valid even if the local available list
  does not recognize them, so exporting and reapplying does not drop newer fields.
  Phone-number callback overrides are outside this WABA-scoped kind; manage them with
  `paloma webhook subscribe --number`. There is no deletion semantics; remove a webhook
  with `paloma webhook unsubscribe`.

  ```yaml
  apiVersion: paloma/v1
  kind: Webhook
  spec:
    callbackUrl: https://example.com/webhooks/whatsapp
    verifyToken: ${keyring:acme-verify}   # only needed when changing the URL; never commit a secret
    fields:                                  # app-level event subscriptions (needs app creds)
      - messages
      - calls
  ```

Dry-run previews and drift diagnostics mask sensitive fields, including nested SIP passwords. Sensitive-only updates still report drift and are applied with their original values.

### Account

`Account` manages only the WABA name and numeric Graph timezone identifier. It is WABA
scoped; `metadata.phoneNumberId` is unnecessary. Bare numeric timezone values are
normalized to strings, avoiding perpetual drift against Meta's response. Unknown fields
are refused before a read. Insights enrollment, the Cloud API marketing toggle and
billing stay explicit commands because they affect enrollment, sending or billing.

```yaml
apiVersion: paloma/v1
kind: Account
spec:
  name: Acme Support
  timezoneId: 7
```

### QrCode

`QrCode` manages the prefilled message of an existing phone-scoped code. Meta assigns
code identities, so create one with `paloma qr create`, then `paloma export qrcode`.
Apply updates only that code; it never creates or deletes a code. Ambiguous, malformed
or mismatched lookup responses fail before any write. Export includes only
the code and message, leaving out generated links and image URLs.

```yaml
apiVersion: paloma/v1
kind: QrCode
metadata:
  phoneNumberId: '111'
spec:
  code: WOMVT6TJ2BP7A1
  prefilledMessage: Tell me more
```

### PhoneSettings

This phone-scoped kind manages local storage and the customer identity-key check.
Calling remains in the `Calling` kind. Export excludes calling credentials, encryption
keys and unrelated settings. Each changed feature becomes a separate Settings API
request, identity first; matching fields produce no writes.

```yaml
apiVersion: paloma/v1
kind: PhoneSettings
metadata:
  phoneNumberId: '111'
spec:
  storageConfiguration:
    status: IN_COUNTRY_STORAGE_ENABLED
    dataLocalizationRegion: IN
  userIdentityChange:
    enableIdentityKeyCheck: true
```

For storage disablement use `status: IN_COUNTRY_STORAGE_DISABLED` and omit the region.
Storage changes require the number to be deregistered and re-registered afterward;
apply does neither automatically. Applying either feature confirms unless `--yes`
is set; dry-run reads current settings and prints the proposed changes without writes.
Graph can't apply both atomically, so if a request fails, stderr names what was applied,
what failed and what wasn't attempted.
A dry-run prints one body per request, leaving out features already in sync.
Meta's settings schema doesn't list `userIdentityChange` as readable. If Graph doesn't
return it, the plan says `not returned by Graph` and the feature counts as drift on every
`apply` and `diff`, like `pin` on a Number.
Unknown settings and malformed feature values fail before any read. Meta's default
storage status is normalized to disabled to avoid repeated drift.

### Additional Number fields

`Number` also supports `searchVisibility: VISIBLE|HIDDEN` and `newDisplayName`.
Visibility is compared with live state, so it only writes on drift; Meta may report a hidden
number as `NON_VISIBLE`, which Paloma treats as `HIDDEN`. A display name is
submitted for review only if it isn't already the verified name or a pending request, or
if Meta declined or expired the earlier request. The existing write-only `pin` behavior
is unchanged, and its value remains redacted from previews and diagnostics. Identity/visibility submissions
confirm before apply. Exported observational fields remain read-only.
