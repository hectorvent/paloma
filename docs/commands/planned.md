# Planned commands

These resource groups are wired into the command tree (they appear in
`paloma --help` and `paloma <group> --help`) but the verbs are not implemented yet.
Each exits non-zero with a clear `not yet implemented (planned for vX.Y)` message,
so the published surface stays discoverable and honest.

| Group | Verbs | Purpose |
|-------|-------|---------|
| `paloma account` | `get`, `limits`, `analytics`, `subscribed-apps` | WABA-level reads and messaging analytics |
| `paloma webhook` | `listen`, `verify-url`, `simulate` | Receive-side: ephemeral listener, hub.challenge check, local signed-payload simulator (`get` / `subscribe` / `unsubscribe` / `fields` already ship, see [webhook](webhook.md)) |
| `paloma qr` | `create`, `list`, `get`, `update`, `delete` | Message QR codes |

Also planned within implemented groups:

- **`paloma send`**: only `flow` remains — sending a flow into a chat. Building and
  managing the flow itself already ships, see [flow](flow.md). The rest of the send
  surface, including `interactive` (button/list/cta-url), is implemented.
- **`paloma template`**: only `namespace` remains. The rest of the lifecycle
  (get/create/submit/watch/edit/delete) is implemented.
- **`paloma profile`**: only `photo` remains (needs the resumable upload flow).
- **`paloma auth`**: `refresh`.

## The differentiators on the way

The features that make this more than "curl with flags," in order of value:

1. **`webhook listen` + `simulate`**: turn the hardest part of Cloud API
   development (receiving) into one command, including a local tunnel and signed
   payloads that exercise your endpoint's signature check.
2. **Compliance guardrails**: full 24-hour window tracking, rate limiting.

The **template lifecycle** (`create → watch → send`) is implemented. Template sends
support positional/named body text, text and media headers, button parameters, and a
JSON components file; see [send template](send.md#paloma-send-template).
