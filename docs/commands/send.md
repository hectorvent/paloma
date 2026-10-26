# `paloma send`

The hot path. Send a WhatsApp message. Recipient `<to>` is an E.164 number
(`+15551234567` or `15551234567`) or a business-scoped user ID (BSUID), such as
`US.13491208655302741918`. See [Recipients](#recipients).

```
paloma send <type> <to> [args] [flags]
```

Implemented types: **text**, **template**, **image**, **document**, **audio**,
**video**, **sticker**, **location**, **contact**, **interactive**
(button / list / cta-url / location-request / address / carousel / call /
request-contact), **flow**, **call-permission**, **product**, **product-list**,
**catalog**, **reaction**, and **raw**.

All sends honor the global `--dry-run` (print the request, don't send),
`--output`, and `--quiet` flags. See [Global flags](../global-flags.md).

Every type except `reaction` and `raw` takes `--reply-to <wamid>`, which quotes that
message in the reply. Meta doesn't allow a reaction to be a reply, and a `raw` payload
carries its own `context`. WhatsApp doesn't show the quote on a template, so Paloma warns
you and sends it anyway. The quote is also missing if the original message was deleted or
moved to long-term storage, which typically happens after 30 days unless you've enabled
local storage, and on KaiOS when the reply is audio, an image or a video. See Meta's
[contextual replies](https://developers.facebook.com/documentation/business-messaging/whatsapp/messages/contextual-replies).

## Recipients

Every type except `raw` takes a phone number or a
[BSUID](https://developers.facebook.com/documentation/business-messaging/whatsapp/business-scoped-user-ids)
as `<to>`. A BSUID is a two-letter country code, a period and up to 128 letters and digits,
so it can't be mistaken for a number. Paloma writes the matching field:

| `<to>` | Example | Payload field |
|--------|---------|---------------|
| E.164 number | `15551234567` | `to` |
| BSUID | `US.13491208655302741918` | `recipient` |
| Parent BSUID | `US.ENT.11815799212886844830` | `recipient` |

```bash
paloma send text US.13491208655302741918 "Your order is ready"
```

- Pass the whole value. Meta fails a request if any part of it was changed, so Paloma
  refuses any other shape, including a lowercase country code, with exit `5`.
- A BSUID only works from a phone number in the business portfolio it belongs to. A parent
  BSUID works from any portfolio enrolled in its parent account.
- Meta also accepts both fields at once, in which case `to` wins. Paloma writes one, so use
  [`send raw`](#paloma-send-raw) if you want to send both.
- One-tap, zero-tap and copy code authentication templates need a phone number.
  Meta enforces BSUID eligibility and returns error `131062` with exit `1` and a hint.
  A marketing template sent with a max price can fail the same way. Default `--via auto`
  looks up the latest template category to select the sending endpoint; that lookup
  does not replace Meta's eligibility checks.
- `-q` still prints the wamid. The rest of the response differs: `contacts[]` carries
  `user_id` and no `wa_id` when you sent to a BSUID, and `wa_id` and no `user_id` when you
  sent to a number. Read `.contacts[0].wa_id` only if you know which you sent.

---

## `paloma send text`

```
paloma send text <to> [<body>] [flags]
```

| Flag | Description |
|------|-------------|
| `<body>` | Message text (positional). Omit when using `--body-file`. |
| `--body-file <path>` | Read the body from a file instead of the argument |
| `--preview-url` | Enable link preview for URLs in the body |
| `--reply-to <wamid>` | Reply in the thread of this message id (Cloud API `context`) |
| `--force` | Send even if outside the 24-hour customer-service window |
| `--strict` | Block free-form sends unless the 24-hour window is known to be open. Nothing tracks the window yet, so today it blocks all of them. Conflicts with `--force` |

```bash
paloma send text 15551234567 "Your order is ready 🎉"
paloma send text 15551234567 --body-file note.txt --preview-url
paloma send text 15551234567 "On it!" --reply-to wamid.HBgL...
```

---

## `paloma send template`

Send an approved template with positional body variables, named body variables,
or complete send components from JSON. Positional values fill `{{1}}, {{2}}, …` in
flag order; named values use the names in the approved template.

```
paloma send template <to> <name> --lang <code> [flags]
```

| Flag | Description |
|------|-------------|
| `<name>` | Template name (positional) |
| `--lang <code>` | **Required.** Template language, e.g. `en_US`, `es` |
| `--via auto\|cloud\|marketing` | Endpoint selection; defaults to `auto`, which reads the template's latest category |
| `--body-var <value>` | Positional body text variable; repeat in order |
| `--var <name=value>` | Named body text variable; repeat with unique names; names contain lowercase letters and underscores; conflicts with `--body-var` |
| `--header-text <text>` | Text value for a positional header variable; combines with `--body-var`; conflicts with `--var` |
| `--header-image <id\|url>` | Image header: a media ID or an `http(s)` link |
| `--header-video <id\|url>` | Video header: a media ID or an `http(s)` link |
| `--header-document <id\|url>` | Document header: a media ID or an `http(s)` link |
| `--button <index:type=value>` | A button's parameter; repeat for each button |
| `--components-file <path>` | JSON array of send components, or `{"components": [...]}`; exclusive with typed component flags; combines with `--lang` and `--via` |
| `--reply-to <wamid>` | Send as a reply to this message id; WhatsApp doesn't show the quote on a template |

### Category-aware routing

By default, `send template` reads the current category for the exact template name and
language, then chooses the endpoint:

| Current category | Endpoint |
|---|---|
| `MARKETING` | `/{phone-number-id}/marketing_messages` |
| `UTILITY` or `AUTHENTICATION` | `/{phone-number-id}/messages` |

The lookup uses `--messaging-account-id` when configured, otherwise the active WABA.
Automatic routing therefore requires account context and permission to read its templates,
in addition to messaging permission. The selected Messaging account id stays on the send
payload. Each invocation fetches a fresh category; no persistent category cache is used.
The impending `correct_category` field never controls routing.

**Compatibility:** older versions sent every template through `/messages` without a
lookup. Use `--via cloud` to retain that behavior, or `--via marketing` to select the
Marketing Messages API directly. Explicit routes need no WABA context or template-management
permission, and Paloma never overrides or refreshes an explicit route after rejection.
Both endpoints preserve the same recipient, language, reply context and component values.

```bash
paloma send template 15551234567 order_ready --lang en_US --waba-id 123456789
paloma send template 15551234567 sale --lang en_US --via marketing
paloma send template 15551234567 login_code --lang en_US --via cloud
```

Automatic routing refreshes the category only after a structured Graph HTTP `400`
rejection with code `131061` or `131063` on Cloud API, or code `100` (invalid parameter)
on the Marketing Messages API. It sends once more only if the completed category now
selects the other endpoint. The payload is unchanged, and a note on stderr identifies the reroute. A second
failure stops immediately. No same-endpoint resend or reroute follows an auth failure,
rate limit, policy block, timeout, server error, malformed response or successful send.
The client's existing bounded retry policy for explicit rate-limit responses still applies.

Missing templates, malformed metadata, multiple exact matches on a lookup page, and
missing/unsupported categories exit `1` without sending. Missing account context and
HTTP `401`/`403` lookup failures exit `3`; other Graph failures retain their existing
error-code mapping. Local parameter errors still exit `5` before
any lookup or credential resolution; invalid `--via` values exit `2`.

Dry-run remains **offline** and reads no token. In automatic mode it prints the initial
category GET plus a POST with the explicit placeholder
`<messages-or-marketing_messages-from-category>` in the URL. Later lookup pages and the
conditional rejection/refresh path depend on live responses and are not expanded in that
initial plan. Use `--via cloud` or `--via marketing` for a single POST with a fixed endpoint.
For a raw send-body preview, use an explicit route with `--dry-run -o raw`.

```bash
paloma send template 15551234567 sale --lang en_US --dry-run -o json
paloma send template 15551234567 sale --lang en_US --via marketing --dry-run -o raw
```

`-q` still prints only the final `wamid`; JSON/raw preserve the final Graph response.
Category lookups and a rejected attempt never produce data on stdout. Marketing API
onboarding, Terms of Service, eligibility and Meta-side Cloud API fallback remain Meta's
responsibility. This command does not enroll an account or change its marketing toggle.
Inspect those settings with [account commands](account.md#paloma-account-set).

### Template components

Use one header flag at most. Media headers combine with `--body-var` or `--var`.

`--button` takes the button's zero-based position in the template (0 to 9), then one of these:

| Type | Value | Used for |
|------|-------|----------|
| `url` | The URL's variable part, sent as written, so percent-encode special characters first | Dynamic URL buttons |
| `url` | The one-time password | The copy-code button of an authentication template |
| `quick_reply` | The payload returned to your webhook when tapped | Quick reply buttons |
| `copy_code` | The coupon code, up to 20 characters | Coupon buttons; a template has at most one |
| `flow` | The flow token | Flow buttons |
| `catalog` | The retailer ID of the thumbnail product | Catalog buttons |

```bash
paloma send template 15551234567 order_ready --lang en_US \
    --header-text "Order update" --body-var "Hector" --body-var "#4821"
paloma send template 15551234567 order_ready --lang en_US \
    --var first_name=Hector --var order_number=4821
paloma send template 15551234567 order_ready --lang en_US \
    --header-image 1234567890 --var first_name=Hector --button 0:url=order-4821
paloma send template 15551234567 login_code --lang en_US \
    --body-var 123456 --button 0:url=123456
paloma send template 15551234567 order_ready --lang en_US \
    --components-file components.json --dry-run
```

Use a components file for what the flags don't cover: currency and date values, location
headers, a document's filename, named header or button parameters, carousels, limited-time
offers, multi-product buttons, a Flow's first-screen data, and a Flow or catalog button sent
without a token or thumbnail. Use it too if your flow tokens are sensitive, because command
lines are visible to other processes. It sends component shapes as written, without
constructing the message envelope. For example:

```json
[
  {"type": "header", "parameters": [
    {"type": "image", "image": {"id": "MEDIA_ID"}}
  ]},
  {"type": "body", "parameters": [
    {"type": "text", "text": "Hector"}
  ]},
  {"type": "button", "sub_type": "url", "index": "0", "parameters": [
    {"type": "text", "text": "order-4821"}
  ]}
]
```

Use **send-time parameters**, rather than template-creation components. Media headers
accept an existing media ID or a public link; upload a local file with `media upload`
first. No resumable upload is needed to send an existing template.

Paloma checks the file envelope, component types, and parameter-array shape locally
(exit 5 on malformed input). Meta validates component-specific requirements and their
match to the approved template. Empty component arrays omit the field. Tap-target overrides are a component (`tap_target_configuration`), so they go in this
file too. A custom TTL isn't a send option: `message_send_ttl_seconds` is set on the
template when it's created.

Templates are always allowed by the 24-hour window guardrail. Dry-runs send no
requests and redact sensitive fields, including a Flow button's `flow_token`; real sends
preserve supplied component values.
See Meta's [named parameters](https://developers.facebook.com/documentation/business-messaging/whatsapp/templates/overview#named-parameters)
and [media template sends](https://developers.facebook.com/documentation/business-messaging/whatsapp/templates/template-media).

---

## `paloma send image`

Send an image by local file (auto-uploaded), an existing media id, or a public
link. Provide **exactly one** source.

```
paloma send image <to> (--file <path> | --id <media-id> | --link <url>) [--caption ...]
```

| Flag | Description |
|------|-------------|
| `--file <path>` | Local image file (uploaded to a media id first, then sent) |
| `--id <media-id>` | Reuse an already-uploaded media id |
| `--link <url>` | Public `https` link to the image |
| `--caption <text>` | Caption shown under the image |
| `--reply-to <wamid>` | Quote this message in the reply |
| `--force` / `--strict` | 24-hour window controls (as for `send text`) |

```bash
paloma send image 15551234567 --file ./flyer.jpg --caption "This week's specials"
paloma send image 15551234567 --id 1284756...
paloma send image 15551234567 --link https://example.com/promo.jpg
```

With `--dry-run`, a local `--file` is **not** uploaded; the body shows a
placeholder media id so you can inspect the request offline.

### Local file checks

Before any API call, Paloma checks a `--file` against Meta's
[supported media types](https://developers.facebook.com/documentation/business-messaging/whatsapp/business-phone-numbers/media#supported-media-types)
and exits 5 if it doesn't fit. It judges the type by the file's extension, not its contents,
and doesn't check codecs. `--id` and `--link` aren't checked, because Paloma can't see the file.

| Send type | File types | Maximum size |
|-----------|------------|--------------|
| `image` | JPEG, PNG | 5 MB |
| `video` | MP4, 3GPP | 16 MB |
| `audio` | AAC, AMR, MP3, M4A, OGG | 16 MB |
| `sticker` | WebP | 500 KB |
| `document` | TXT, XLS, XLSX, DOC, DOCX, PPT, PPTX, PDF | 100 MB |

- **Sizes** count 1 MB as 1,048,576 bytes, because Meta doesn't say which it means. A file
  just under that limit can still be refused by Meta.
- **Stickers:** Meta allows 500 KB for an animated sticker but only 100 KB for a static one.
  Paloma can't tell them apart, so it applies 500 KB and leaves the rest to Meta.
- **Documents:** Meta says other file types may be sent but aren't supported and may not
  display correctly. Paloma warns on stderr and sends them, up to the same size.
- **A file with the wrong extension:** these commands have no `--type` flag, so run
  `paloma media upload <file> --type <mime>` and send the result with `--id`.

---

## `paloma send document` / `audio` / `video` / `sticker`

The other media types share the image flow: one of `--file` (auto-uploaded),
`--id`, or `--link`. `document` and `video` also take `--caption`; `document`
takes `--filename`; `audio` and `sticker` take neither. `audio` takes `--voice`.

```bash
paloma send document 15551234567 --link https://x.com/invoice.pdf --filename invoice.pdf
paloma send audio    15551234567 --file ./song.mp3
paloma send audio    15551234567 --file ./note.ogg --voice
paloma send video    15551234567 --file ./promo.mp4 --caption "New batch!"
paloma send sticker  15551234567 --file ./sticker.webp
```

`--voice` sends the audio as a [voice message](https://developers.facebook.com/documentation/business-messaging/whatsapp/messages/audio-messages#voice-messages),
which downloads automatically and shows your profile picture with a microphone icon. It
also gets a `played` status webhook the first time the user plays it. Voice messages must
be mono `.ogg` files encoded with OPUS, and transcription fails on anything else. Paloma
doesn't check the file. The play icon only appears at 512 KB or smaller; a larger file
shows a download icon.

## `paloma send location`

```
paloma send location <to> --lat <n> --long <n> [--name ...] [--address ...] [--reply-to <wamid>] [--force | --strict]
```

```bash
paloma send location 15551234567 --lat 18.4861 --long -69.9312 \
      --name "Caribbean Acme" --address "Santo Domingo"
```

## `paloma send contact`

Send one contact from flags, or any number from a file. Give either `--name` or
`--file`; they can't be combined and one is required (exit 2). It takes `--reply-to`,
and `--force` or `--strict`.

```bash
paloma send contact 15551234567 --name "Ana Diaz" --first-name Ana --last-name Diaz \
  --phone +16505559999 --wa-id 16505559999 --email ana@example.com

paloma send contact 15551234567 --file contacts.json
```

| Flag | Meaning |
|------|---------|
| `--name` | Full name, shown on the message (Meta's `formatted_name`) |
| `--first-name`, `--last-name` | The name's parts. Give at least one: Meta's docs mark them optional, but the API rejects a contact with `--name` alone (exit 5 here, before any call) |
| `--phone` | Phone number; repeatable |
| `--wa-id` | WhatsApp id of the first `--phone`, so it needs `--phone`. It adds the **Message** and **Save contact** buttons; without it the message shows an **Invite to WhatsApp** button |
| `--email` | Email address; repeatable |
| `--file` | JSON file holding the Cloud API contacts array, or an object with a `contacts` array, up to 257 contacts. Use it for addresses, birthdays, organizations, URLs and phone or email types |

Blank values, or a file with no contacts or too many, exit 5 before any request. See
Meta's [contacts messages](https://developers.facebook.com/documentation/business-messaging/whatsapp/messages/contacts-messages)
page.

## `paloma send reaction`

React to a prior message. Pass an empty `--emoji ""` to remove a reaction.

```bash
paloma send reaction 15551234567 --message-id wamid.XXX --emoji 👍
```

## `paloma send interactive`

Every kind takes `--body`, `--reply-to`, and `--force` or `--strict`. `button`, `list`,
`cta-url` and `address` also take `--header` and `--footer`. Meta's pages don't show either
on `location-request`, `call` or `request-contact`, and rule them out on `carousel`.

| Kind | What it sends | Needs |
|------|---------------|-------|
| `button` | Up to 3 reply buttons | |
| `list` | A list of sectioned rows | |
| `cta-url` | A button that opens a URL | |
| [`location-request`](#location-request) | A button that asks for the user's location | |
| [`address`](#address) | A form that asks for a shipping address | A business and customers in India |
| [`carousel`](#carousel) | 2 to 10 media or product cards | A catalog, for product cards |
| [`call`](#call) | A button that calls your business | Calling enabled on the number |
| [`request-contact`](#request-contact) | A button that asks for the user's phone number | |

```bash
# Reply buttons (up to 3), given as id:Title, or just Title
paloma send interactive button 15551234567 --body "Confirm your order?" \
      --button "yes:Yes" --button "no:No"
paloma send interactive button 15551234567 --body "Confirm your order?" \
      --header-image 1284756 --button "yes:Yes" --button "no:No"

# A list picker; sections are "Title:id:Label::Description,id2:Label2" (up to 10 rows total)
paloma send interactive list 15551234567 --body "Pick a flavor" --button "Choose" \
      --section "Cakes:choco:Chocolate::Dark and rich,vanilla:Vanilla" \
      --section "Drinks:morir:Morir Soñando"

# A call-to-action that maps a URL to a tappable button
paloma send interactive cta-url 15551234567 --body "See the menu" \
      --display "Open menu" --url https://acme.do/menu \
      --header-image https://acme.do/menu.jpg
```

### Media headers

`--header-image`, `--header-video` and `--header-document` replace the text `--header`. Use
one header flag at most. What each kind takes follows Meta's page for it:

| Kind | Header | Media source |
|------|--------|--------------|
| [`button`](https://developers.facebook.com/documentation/business-messaging/whatsapp/messages/interactive-reply-buttons-messages) | Text, image, video or document | A media ID or an `http(s)` link |
| [`cta-url`](https://developers.facebook.com/documentation/business-messaging/whatsapp/messages/interactive-cta-url-messages) | Text, image, video or document | An `http(s)` link only; a media ID exits 5 |
| [`list`](https://developers.facebook.com/documentation/business-messaging/whatsapp/messages/interactive-list-messages) | Text only | None |

### List rows

A row is `id:Label`, or `id:Label::Description` to add a description. A label can contain `:`,
but can't contain `::` or start with `:`; use `send raw` for one that must. Rows are separated
by commas, so write a comma in a row's text as `\,`. A section title can hold a plain comma, and
is required, with at most 24 characters.

```bash
paloma send interactive list 15551234567 --body "Shipping?" --button "Options" \
      --section 'Fast:express:Express::Next day\, by 5 pm,priority:Priority'
```

| Row part | Maximum characters |
|----------|--------------------|
| ID | 200 |
| Label | 24 |
| Description | 72 |

Anything longer, or a blank part, exits 5 before any API call.

### `location-request`

A button that opens WhatsApp's location sharing screen. The location comes back in a
`messages` webhook. The body is at most 1024 characters. See Meta's
[location request messages](https://developers.facebook.com/documentation/business-messaging/whatsapp/messages/location-request-messages).

```bash
paloma send interactive location-request 15551234567 --body "Where should we pick you up?"
```

### `address`

A form that asks for a shipping address. The address comes back in a `messages` webhook.

**India only:** Meta offers this only to businesses based in India and their customers in
India. Paloma doesn't check the region and sends what you ask for. A recipient whose number
doesn't match the country can't get the message, and an app that doesn't support it makes
Meta drop the message and report error `1026` in a webhook.

Meta requires a country, so pass `--country`, or use `--parameters-file` to also prefill the
form, offer saved addresses, or report validation errors. Paloma checks that the file holds
a `country`, and the shape of the other three, and passes it through as written.

```bash
paloma send interactive address 919876543210 --body "Where should we deliver this?" --country IN

paloma send interactive address 919876543210 --body "That pin code looks wrong." \
      --parameters-file address.json
```

```json
{
  "country": "IN",
  "values": { "name": "Ana", "in_pin_code": "666666", "city": "Delhi" },
  "validation_errors": { "in_pin_code": "We could not locate this pin code." }
}
```

`saved_addresses` is an array of `{ "id": ..., "value": { ... } }`. See Meta's
[address messages](https://developers.facebook.com/documentation/business-messaging/whatsapp/messages/address-messages)
for the fields.

### `carousel`

2 to 10 horizontally scrolling cards under a body of at most 1024 characters. Meta doesn't
allow a header or footer on the message itself. Pass exactly one source.

**Media cards** come from `--card-file`, as a JSON array or `{"cards": [...]}` of Cloud API
card objects, since a card is too rich for flags. `card_index` defaults to the card's
position, and `type` to `cta_url`, which is what Meta's page uses even for quick-reply cards.

```bash
paloma send interactive carousel 15551234567 --body "Here are our new arrivals" \
      --card-file cards.json
```

```json
[
  {
    "header": { "type": "image", "image": { "link": "https://acme.do/echeveria.jpg" } },
    "body": { "text": "*Blue Echeveria*" },
    "action": {
      "name": "cta_url",
      "parameters": { "display_text": "Buy now", "url": "https://acme.do/echeveria" }
    }
  },
  {
    "header": { "type": "video", "video": { "link": "https://acme.do/haworthia.mp4" } },
    "action": {
      "name": "cta_url",
      "parameters": { "display_text": "Buy now", "url": "https://acme.do/haworthia" }
    }
  }
]
```

For quick replies, an `action` is instead
`{ "buttons": [{ "type": "quick_reply", "quick_reply": { "id": "...", "title": "..." } }] }`.
Paloma exits 5 before any API call unless each card follows Meta's rules:

| Part | Rule |
|------|------|
| Header | Required. An image or video, as an `http(s)` link |
| Body | Optional. At most 160 characters and 2 line breaks |
| Buttons | One URL button, or one or more quick-reply buttons, the same type and number on every card |
| URL button label, quick-reply title | At most 20 characters |
| Quick-reply ID | At most 256 characters |

**Product cards** come from `--products`, with one card per SKU, and need a catalog. Meta
requires every card to use the same one, so there is one `--catalog-id`.

```bash
paloma send interactive carousel 15551234567 --body "Our favorites" \
      --products abc123,def456,ghi789 --catalog-id 123456789
```

See Meta's
[media carousel](https://developers.facebook.com/documentation/business-messaging/whatsapp/messages/interactive-media-carousel-messages)
and [product carousel](https://developers.facebook.com/documentation/business-messaging/whatsapp/catalogs/interactive-product-carousel-messages)
messages.

### `call`

A button that starts a WhatsApp call to the number that sent the message. **Calling must be
enabled** on that number; see [calling](calling.md).

| Flag | Meaning | Limit | Meta's default |
|------|---------|-------|----------------|
| `--display` | Button label | 20 characters | `Call Now` |
| `--ttl-minutes` | How long the button lasts | 1 to 43200 (30 days) | 10080 (7 days) |
| `--payload` | A string returned as `cta_payload` in the `calls` webhook, for tracking; needs WhatsApp 2.25.27 or later | 512 characters | |

```bash
paloma send interactive call 15551234567 --body "Call us now for faster service" \
      --display "Call on WhatsApp" --ttl-minutes 1440 --payload campaign-7
```

An app too old for the button makes Meta report error `131026` in a webhook. See Meta's
[call button messages](https://developers.facebook.com/documentation/business-messaging/whatsapp/calling/call-button-messages-deep-links).

### `request-contact`

A button that asks the user to share their phone number. Meta doesn't let you customize the
button. Useful with a business-scoped user ID, when you don't have the number. It comes back
in a `contacts` webhook with `origin` set to `contact_request`. See Meta's
[requesting phone numbers](https://developers.facebook.com/documentation/business-messaging/whatsapp/business-scoped-user-ids#requesting-phone-numbers-from-users).

```bash
paloma send interactive request-contact US.13491208655302741918 \
      --body "Can we have your number for delivery updates?"
```

## `paloma send flow`

Send a [Flow](flow.md) inside the customer service window, opened from a button on the
message. To start a conversation with a Flow, send a [template](#paloma-send-template) with a
Flow button instead. Name the Flow with exactly one of `--flow-id` or `--flow-name`.

```bash
paloma send flow 15551234567 --body "Book your table" --flow-name booking_v1 --cta "Book!"

paloma send flow 15551234567 --body "Book your table" --flow-id 123456 --cta "Book!" \
      --flow-token order-7 --screen START --data '{"plan":"gold"}'
```

| Flag | Meaning | Meta's default |
|------|---------|----------------|
| `--cta` | Required. Button text; Meta advises 30 characters or less and no emoji, so a longer one warns | |
| `--flow-token` | Your ID for this send. Meta treats it like a session id, so dry runs mask it. As a flag it can land in your shell history, so don't pass a value that must stay secret | `unused` |
| `--action` | `navigate`, or `data_exchange` to get the first screen from your endpoint | `navigate` |
| `--mode` | `draft` or `published`. An edited Flow's draft needs `draft` | `published` |
| `--screen` | ID of the first screen, which must be an entry screen | `FIRST_ENTRY_SCREEN` |
| `--data`, `--data-file` | Input for the first screen, as a non-empty JSON object | |

`--screen`, `--data` and `--data-file` only apply to `navigate`, since Meta says to leave them
out otherwise: with `--action data_exchange` they exit 5. Paloma sends `--data` as a serialized string, as
Meta's page specifies. Dry runs mask secrets inside it, as they do in any other field.

The first screen has to declare the fields you pass. If it doesn't, Meta accepts the message
but the Flow shows "Something went wrong" when opened, so check the Flow's `data` block first.

Also takes `--body`, `--footer`, `--reply-to`, and `--force` or `--strict`. The header is one
of `--header`, or `--header-image`, `--header-video` or `--header-document`, each taking a media
ID or an `http(s)` link. The body is at most 1024 characters, and the header text and footer at
most 60. See Meta's
[Send a Flow message](https://developers.facebook.com/documentation/business-messaging/whatsapp/flows/guides/sendingaflow).

## `paloma send call-permission`

Ask the user for permission to call them, inside the customer service window. To ask outside
it, use a [call permission template](https://developers.facebook.com/documentation/business-messaging/whatsapp/calling/user-call-permissions#create-and-send-call-permission-request-template-messages).
The answer comes back in a `messages` webhook.

```bash
paloma send call-permission 15551234567 --body "May we call you about order ON-12853?"
```

`--body` is optional, though Meta recommends it, to give the user context. Meta doesn't allow a
header or footer. Also takes `--reply-to`, and `--force` or `--strict`.

**Calling must be enabled** on the number; see [calling](calling.md). This only works in
[regions with business-initiated calling](https://developers.facebook.com/documentation/business-messaging/whatsapp/calling#availability),
which leaves out business numbers in the United States, Canada, Egypt, Vietnam and Nigeria.

Meta allows 1 request in 24 hours and 2 in 7 days to each user, counting templates, and a
connected call resets both. Meta charges for these messages. See Meta's
[user call permissions](https://developers.facebook.com/documentation/business-messaging/whatsapp/calling/user-call-permissions).

## `paloma send product`

One product from your catalog. **Needs a catalog** connected to your WhatsApp account, as do
`product-list` and `catalog`; see Meta's
[upload inventory](https://developers.facebook.com/documentation/business-messaging/whatsapp/catalogs/upload-inventory).
The body is optional, and Meta doesn't allow a header. Also takes `--footer`, `--reply-to`, and
`--force` or `--strict`.

```bash
paloma send product 15551234567 --catalog-id 123456789 --product SKU1 --body "This one's back"
```

`--product` is the retailer ID: the SKU, labeled Content ID in Commerce Manager. Meta returns
an error if the product isn't in the catalog. See Meta's
[single-product messages](https://developers.facebook.com/documentation/business-messaging/whatsapp/catalogs/single-product-messages).

## `paloma send product-list`

Up to 30 products from your catalog, in up to 10 sections. `--header` and `--body` are required,
and the header can only be text. Also takes `--footer`, `--reply-to`, and `--force` or `--strict`.

```bash
paloma send product-list 15551234567 --catalog-id 123456789 \
      --header "Our picks" --body "Have a look" \
      --section "Cakes:choco-1,vanilla-2" --section "Drinks:morir-3"
```

A section is `Title:sku1,sku2`, with a title of at most 24 characters, and no `:` in the title.
Write a comma in a SKU as `\,`. If some products aren't in the catalog, Meta drops them and
sends the rest, and reports an error if none match. See Meta's
[multi-product messages](https://developers.facebook.com/documentation/business-messaging/whatsapp/catalogs/multi-product-messages).

## `paloma send catalog`

A **View catalog** button that opens your catalog in WhatsApp. `--body` is required. `--thumbnail` names the SKU whose
image heads the message; without it, Meta uses the first item in the catalog. Also takes
`--footer`, `--reply-to`, and `--force` or `--strict`. See Meta's
[catalog messages](https://developers.facebook.com/documentation/business-messaging/whatsapp/catalogs/catalog-messages).

```bash
paloma send catalog 15551234567 --body "Ordering is easy: browse and add to your cart" \
      --thumbnail 2lc20305pt --footer "Best deals on WhatsApp"
```

## `paloma send raw`

Escape hatch: send an already-formed Cloud API message object verbatim, from a
file or stdin. Useful for message shapes paloma does not model directly. The payload
addresses itself with `to`, `recipient` or both, and Paloma doesn't check or change them.

```bash
paloma send raw --file payload.json
cat payload.json | paloma send raw --stdin
```

There's one exception to "verbatim". If a [messaging account](../global-flags.md#global-flags)
is set and the payload is a message (it has a `type`) with no `messaging_account_id` of its
own, Paloma adds it. A value in the payload always wins, including the deprecated
`paid_messaging_account_id`.

---

## Marketing Messages options

With `--via marketing`, `send template` also takes two options that only `/marketing_messages`
accepts. Any other route exits `5`, including the default `--via auto`, which could pick
`/messages`.

| Flag | Meaning |
|------|---------|
| `--product-policy <cloud-api-fallback\|strict>` | Whether Meta may send through Cloud API when onboarding is incomplete. Meta defaults to falling back |
| `--message-activity-sharing <true\|false>` | Whether to share this message's activity with Meta, overriding the account's setting |

```bash
paloma send template 15551234567 sale --lang en_US --via marketing \
  --product-policy strict --message-activity-sharing false
```

## Sending to a group

`send text`, `send template` and the media commands (`image`, `document`, `audio`, `video`,
`sticker`) take `--group`, which treats the recipient as a group ID from
[`paloma group`](group.md).

```bash
paloma send text "$GROUP_ID" 'Hello everyone' --group
paloma send image "$GROUP_ID" --link https://example.com/a.jpg --group
paloma send template "$GROUP_ID" order_ready --lang en_US --group
```

The 24-hour check doesn't apply, because that window is per customer. Templates always go through
`/messages`, the route Meta documents for groups, so `--via marketing` exits `5` and no category
lookup happens. Meta supports text and media templates here, and Cloud API can't send to a group
that has only one member (#131020).

## Compliance: the 24-hour window

Outside the 24-hour customer-service window, non-template sends are subject to a
guardrail:

- **Default mode:** permitted (a warning will be added once window tracking
  lands).
- **`--strict`:** free-form sends are blocked with exit code `6` unless the window is
  known to be open. Nothing tracks it yet, so today that's all of them. Use `--force`
  instead to send anyway; combining the two exits 2.
- **`--force`:** always permitted.
- **Templates:** always permitted.

Full window tracking (derived from events seen by `paloma webhook listen`) is
[future work](planned.md).
