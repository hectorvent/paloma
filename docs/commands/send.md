# `paloma send`

The hot path. Send a WhatsApp message. Recipient `<to>` is an E.164 number
(`+15551234567` or `15551234567`). To send to a business-scoped user ID (BSUID), use
[`send raw`](#paloma-send-raw) with `recipient` in the payload.

```
paloma send <type> <to> [args] [flags]
```

Implemented types: **text**, **template**, **image**, **document**, **audio**,
**video**, **sticker**, **location**, **contact**, **interactive**
(button / list / cta-url), **reaction**, and **raw**. Still planned: **flow**
(visible in `--help`).

All sends honor the global `--dry-run` (print the request, don't send),
`--output`, and `--quiet` flags. See [Global flags](../global-flags.md).

Every type except `reaction` and `raw` takes `--reply-to <wamid>`, which quotes that
message in the reply. Meta doesn't allow a reaction to be a reply, and a `raw` payload
carries its own `context`. WhatsApp doesn't show the quote on a template, so Paloma warns
you and sends it anyway. The quote is also missing if the original message was deleted or
moved to long-term storage, which typically happens after 30 days unless you've enabled
local storage, and on KaiOS when the reply is audio, an image or a video. See Meta's
[contextual replies](https://developers.facebook.com/documentation/business-messaging/whatsapp/messages/contextual-replies).

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
| `--strict` | Block free-form sends outside the 24-hour window |

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
| `--body-var <value>` | Positional body text variable; repeat in order |
| `--var <name=value>` | Named body text variable; repeat with unique names; names contain lowercase letters and underscores |
| `--header-text <text>` | Text value for a positional header variable; combines with `--body-var`; conflicts with `--var` |
| `--header-image <id\|url>` | Image header: a media ID or an `http(s)` link |
| `--header-video <id\|url>` | Video header: a media ID or an `http(s)` link |
| `--header-document <id\|url>` | Document header: a media ID or an `http(s)` link |
| `--button <index:type=value>` | A button's parameter; repeat for each button |
| `--components-file <path>` | JSON array of send components, or `{"components": [...]}`; exclusive with the flags above except `--lang` |
| `--reply-to <wamid>` | Send as a reply to this message id; WhatsApp doesn't show the quote on a template |

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
requests and redact sensitive fields; real sends preserve supplied component values.
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
paloma send location <to> --lat <n> --long <n> [--name ...] [--address ...] [--reply-to <wamid>]
```

```bash
paloma send location 15551234567 --lat 18.4861 --long -69.9312 \
      --name "Caribbean Acme" --address "Santo Domingo"
```

## `paloma send contact`

Send contacts from a JSON file holding the Cloud API contacts array (or an object
with a `contacts` array).

```bash
paloma send contact 15551234567 --file contacts.json
```

## `paloma send reaction`

React to a prior message. Pass an empty `--emoji ""` to remove a reaction.

```bash
paloma send reaction 15551234567 --message-id wamid.XXX --emoji 👍
```

## `paloma send interactive`

Three kinds, each taking `--body` plus optional `--header`, `--footer` and `--reply-to`.

```bash
# Reply buttons (up to 3), given as id:Title
paloma send interactive button 15551234567 --body "Confirm your order?" \
      --button "yes:Yes" --button "no:No"

# A list picker; sections are "Title:id:Label,id2:Label2" (up to 10 rows total)
paloma send interactive list 15551234567 --body "Pick a flavor" --button "Choose" \
      --section "Cakes:choco:Chocolate,vanilla:Vanilla" \
      --section "Drinks:morir:Morir Soñando"

# A call-to-action that maps a URL to a tappable button
paloma send interactive cta-url 15551234567 --body "See the menu" \
      --display "Open menu" --url https://acme.do/menu
```

## `paloma send raw`

Escape hatch: send an already-formed Cloud API message object verbatim, from a
file or stdin. Useful for message shapes paloma does not model directly.

```bash
paloma send raw --file payload.json
cat payload.json | paloma send raw --stdin
```

There's one exception to "verbatim". If a [messaging account](../global-flags.md#global-flags)
is set and the payload is a message (it has a `type`) with no `messaging_account_id` of its
own, Paloma adds it. A value in the payload always wins, including the deprecated
`paid_messaging_account_id`.

---

## Compliance: the 24-hour window

Outside the 24-hour customer-service window, non-template sends are subject to a
guardrail:

- **Default mode:** permitted (a warning will be added once window tracking
  lands).
- **`--strict`:** free-form sends are blocked with exit code `6` unless you pass
  `--force`.
- **`--force`:** always permitted.
- **Templates:** always permitted.

Full window tracking (derived from events seen by `paloma webhook listen`) is
[planned](planned.md) for v1.0.

---

## Planned send types

Visible in `--help`, not yet implemented: `flow`. See the [planned commands](planned.md).
