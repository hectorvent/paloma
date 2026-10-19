# Planned commands

These resource groups are wired into the command tree (they appear in
`paloma --help` and `paloma <group> --help`) but the verbs are not implemented yet.
Each exits non-zero with a clear `not yet implemented (planned for vX.Y)` message,
so the published surface stays discoverable and honest. All are planned for v1.0.

| Group | Verbs | Purpose |
|-------|-------|---------|
| `paloma webhook` | `listen`, `verify-url`, `simulate` | Receive-side: ephemeral listener, hub.challenge check, local signed-payload simulator (`get` / `subscribe` / `unsubscribe` / `fields` already ship, see [webhook](webhook.md)) |

## The differentiators on the way

The features that make this more than "curl with flags," in order of value:

1. **`webhook listen` + `simulate`**: turn the hardest part of Cloud API
   development (receiving) into one command, including signed
   payloads that exercise your endpoint's signature check.
2. **Compliance guardrails**: full 24-hour window tracking.

The **template lifecycle** (`create → watch → send`) and advanced management
(archive, library, migration, authentication previews, groups and analytics) are
implemented. Template sends
support positional/named body text, text and media headers, button parameters, and a
JSON components file; see [send template](send.md#paloma-send-template).
