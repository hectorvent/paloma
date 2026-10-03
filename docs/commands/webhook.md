# `paloma webhook`

Read and manage the WhatsApp webhook callback for the active WhatsApp Business
Account (WABA), via the Cloud API `subscribed_apps` edge.

```
paloma webhook <verb> [flags]
```

`get`, `subscribe`, and `unsubscribe` manage the **per-WABA override callback**
(`POST/GET/DELETE /{waba-id}/subscribed_apps`), using your existing token. No app
secret is needed. The receive-side bridge (`listen`, `verify-url`, `simulate`) and
app-level field subscriptions are planned for v1.0 and currently print a notice.

## `paloma webhook get`

Show the current webhook configuration: the effective callback URL (read from the
phone-number `webhook_configuration`), the per-WABA `override_callback_uri` (empty
when the WABA uses the app-level callback rather than an override), and the
subscribed app.

```bash
paloma webhook get
paloma webhook get -o json | jq '.effective_callback_url'
```

## `paloma webhook subscribe`

Set or change the per-WABA override callback URL. Both flags are required: Meta runs
the `hub.challenge` verification handshake against the URL at set time, echoing the
verify token, so the token must match what your endpoint expects.

| Flag | Meaning |
|------|---------|
| `--callback-url <url>` | HTTPS callback URL to deliver this WABA's webhooks to |
| `--verify-token <token>` | Verify token your endpoint echoes during the handshake |

```bash
paloma webhook subscribe \
      --callback-url https://example.com/webhooks/whatsapp \
      --verify-token my-verify-token

# preview the exact POST body without sending
paloma webhook subscribe --callback-url https://example.com/hook --verify-token t --dry-run
```

This re-routes the WABA's webhook delivery, so it asks for confirmation first. Skip
the prompt with `--yes` (e.g. in CI). A wrong token or an unreachable URL fails the
handshake and the call returns a Graph API error (the override is not applied).

## `paloma webhook unsubscribe`

Remove the override and unsubscribe the app from the WABA. This stops webhook
delivery for the WABA, so it confirms first (skip with `--yes`).

```bash
paloma webhook unsubscribe
paloma webhook unsubscribe --dry-run   # show the DELETE without sending
```

## `paloma webhook fields`

Read and set **which event types** the webhook receives. These are the app-level
field subscriptions, distinct from the callback URL above: `get`/`subscribe` control
*where* webhooks go, `fields` controls *what* fires.

Because fields live on the app, they use an **app access token** (`{app-id}|{app-secret}`).
So an app secret must be configured (`PALOMA_APP_SECRET` or an `app_secret_ref` in the
profile), and the app id comes from `--app-id` / `PALOMA_APP_ID`, or is auto-discovered
from the subscribed app shown in `webhook get`.

```bash
# show the currently subscribed events
paloma webhook fields

# list the known field names you can subscribe to (no API call, no app creds)
paloma webhook fields available
paloma webhook fields available -q          # just the names, one per line (scriptable)
paloma webhook fields available -o json

# replace the subscribed set (full replace, not additive)
paloma webhook fields set messages calls account_update

# preview the request without sending
paloma webhook fields set messages --dry-run -o json
```

`paloma webhook fields available` is a static reference (the Graph API has no endpoint
that enumerates subscribable fields); Meta may add more, and `set` passes names through
unvalidated. Common WhatsApp Business Account fields:

| Field | Fires on |
|-------|----------|
| `messages` | inbound messages **and** outbound delivery/read statuses (the main one) |
| `calls` | call events (the Calling API) |
| `account_update` | account status, bans, policy decisions |
| `account_alerts` | proactive account alerts |
| `business_capability_update` | messaging-limit tier changes |
| `phone_number_name_update` · `phone_number_quality_update` | display-name approval, quality |
| `message_template_status_update` · `message_template_quality_update` · `template_category_update` | template approved/rejected, quality, category |
| `messaging_handovers` · `user_preferences` | handover protocol, marketing opt-out |

Field names are passed through to Meta (not validated locally), so newly added fields
keep working; an unknown name is rejected by the API. `set` may require `--callback-url`
and `--verify-token` to reactivate the subscription; pass them when Meta asks. Setting
fields changes which events production receives, so it confirms first (`--yes` to skip).

## Declarative config

The webhook is also a manifest kind, so you can version and `apply` it (both the
callback URL and the event `fields`). See
[`Webhook` in the manifests reference](manifests.md#webhook). The verify token is a
write-only secret: it is never exported, and is only needed when the URL actually
changes, so an exported manifest re-applies as a no-op without it. When you do need
it, keep it out of the file with a [placeholder](manifests.md#variables--secrets) —
`${keyring:ACCOUNT}` for a keyring secret or `${WHATSAPP_VERIFY_TOKEN}` for an env var.

```yaml
apiVersion: paloma/v1
kind: Webhook
spec:
  callbackUrl: ${WEBHOOK_URL}
  verifyToken: ${keyring:acme-verify}   # only needed when changing the URL
  fields:
    - messages
    - calls
```

```bash
paloma export webhook > webhook.yaml
paloma apply -f webhook.yaml            # only writes what changed
```

`export` omits `fields` when no app secret is configured (the URL still exports);
managing `fields` via a manifest needs the app credentials, same as the command.
