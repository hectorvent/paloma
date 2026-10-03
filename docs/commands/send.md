# `paloma send`

The hot path. Send a WhatsApp message. Recipient `<to>` accepts E.164
(`+15551234567` or `15551234567`) or a business-scoped user ID (BSUID).

```
paloma send <type> <to> [args] [flags]
```

Implemented types: **text**, **template**, **image**, **document**, **audio**,
**video**, **sticker**, **location**, **contact**, **interactive**
(button / list / cta-url), **reaction**, and **raw**. Still planned: **flow**
(visible in `--help`).

All sends honor the global `--dry-run` (print the request, don't send),
`--output`, and `--quiet` flags. See [Global flags](../global-flags.md).

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

The differentiator: variable filling without hand-writing JSON. Body variables
fill `{{1}}, {{2}}, …` positionally, in the order the flags are given.

```
paloma send template <to> <name> --lang <code> [--body-var V]...
```

| Flag | Description |
|------|-------------|
| `<name>` | Template name (positional) |
| `--lang <code>` | **Required.** Template language, e.g. `en_US`, `es` |
| `--body-var <value>` | A body variable; repeat in order for `{{1}}`, `{{2}}`, … |

```bash
paloma send template 15551234567 order_ready --lang en_US \
    --body-var "Hector" --body-var "#4821"
```

Templates are always allowed by the 24-hour window guardrail. Header/button
variables and the `--component-file` escape hatch are [planned](planned.md).

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
takes `--filename`; `audio` and `sticker` take neither.

```bash
paloma send document 15551234567 --link https://x.com/invoice.pdf --filename invoice.pdf
paloma send audio    15551234567 --file ./voice.ogg
paloma send video    15551234567 --file ./promo.mp4 --caption "New batch!"
paloma send sticker  15551234567 --file ./sticker.webp
```

## `paloma send location`

```
paloma send location <to> --lat <n> --long <n> [--name ...] [--address ...]
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

Three kinds, each taking `--body` plus optional `--header` and `--footer`.

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
