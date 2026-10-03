# `paloma link`

Build `wa.me` click-to-chat deep links entirely offline — no token, no API call.

```
paloma link create --phone <e164> [--text <message>]
```

| Flag | Description |
|------|-------------|
| `--phone <e164>` | **Required.** Destination phone, e.g. `+15551234567` (non-digits are stripped) |
| `--text <message>` | Prefilled message text (URL-encoded into the link) |

```bash
paloma link create --phone +15551234567 --text "Hi, I'd like to order"
# → https://wa.me/15551234567?text=Hi%2C+I%27d+like+to+order

# Just the URL, for embedding:
paloma link create --phone +15551234567 -q
```

Because this makes no network call, it works without any configured profile or
token — handy for generating links in static sites, emails, or QR codes.
