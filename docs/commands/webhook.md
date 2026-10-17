# `paloma webhook`

Read and manage WhatsApp webhook delivery for the active WhatsApp Business Account
(WABA), with optional alternate callbacks on individual phone numbers.

```
paloma webhook <verb> [flags]
```

`subscribe` and `unsubscribe` manage the WABA's app subscription and alternate callback
(`POST/DELETE /{waba-id}/subscribed_apps`). `subscribe --number` manages the active
number's alternate callback; `get` resolves the effective delivery URL. These use your
existing token; no app secret is needed. The receive-side bridge (`listen`, `verify-url`, `simulate`) is
planned for v1.0; those verbs exit non-zero with a not-implemented error.

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

Without `--callback-url`, subscribe the app using its app-level callback and remove any
WABA alternate callback. With a URL, set or change the per-WABA override. A verify token
is required for a new callback: Meta runs the `hub.challenge` verification handshake against the URL at set time, echoing the
token, so it must match what your endpoint expects.

| Flag | Meaning |
|------|---------|
| `--callback-url <url>` | HTTPS callback URL to deliver this WABA's webhooks to |
| `--verify-token-stdin` | Read the verify token from stdin (requires `--callback-url`) |
| `--number` | Set the active number's alternate callback; without a URL, clear that override |

The verify token comes from `--verify-token-stdin` or `PALOMA_VERIFY_TOKEN`, or from a
prompt if there's a terminal. When `--callback-url` is supplied, having none of these
sources and no terminal makes the command exit `5`. Default subscriptions need no token. The
older `--verify-token <value>` flag still works, but it warns, because other processes can
read a command line.

```bash
printf '%s' "$VERIFY_TOKEN" | paloma webhook subscribe \
      --callback-url https://example.com/webhooks/whatsapp \
      --verify-token-stdin --yes

# preview the POST body without sending; the token is masked
PALOMA_VERIFY_TOKEN=t paloma webhook subscribe --callback-url https://example.com/hook --dry-run
```

This re-routes the WABA's webhook delivery, so it asks for confirmation first. Skip
the prompt with `--yes` (e.g. in CI). `--verify-token-stdin` always needs `--yes`, because
piped input leaves no way to answer a prompt. A wrong token or an unreachable URL fails the
handshake and the call returns a Graph API error (the override is not applied).

### Default and per-number callbacks

```bash
paloma webhook subscribe --yes   # app default; clears the WABA alternate callback
PALOMA_VERIFY_TOKEN=t paloma webhook subscribe --number \
  --callback-url https://example.com/number --dry-run
paloma webhook subscribe --number --yes  # clear only the number's alternate callback
```

A per-number override requires phone-number context, with no WABA id needed. A default
WABA subscription needs no verify token. Explicit token flags without a callback URL
exit `5`. Callback URLs are limited to 200 characters and blank URLs are refused before
context resolution. `get` resolves the effective URL in order: number override, WABA
override, app callback. `get -q` prints that effective URL.

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
that enumerates subscribable fields); `set` accepts these known names and rejects
unknown names locally with exit `2`. Common WhatsApp Business Account fields:

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
| `group_lifecycle_update` · `group_participants_update` · `group_settings_update` · `group_status_update` | group lifecycle, membership, settings and status |
| `business_username_updates` | business username changes |
| `message_template_components_update` | template content changes |
| `account_settings_update` | phone-number settings changes |
| `automatic_events` · `partner_solutions` · `payment_configuration_update` · `smb_app_state_sync` | automatic events, partner solutions, payment configuration and app sync |

The reference retains `messaging_handovers` because Meta's override guide documents it,
even though it is absent from the general field-reference index. New fields require an
updated Paloma reference before they can be selected. `set` may require `--callback-url`
and a verify token to reactivate the subscription; pass them when Meta asks. The token comes
from the same sources as for `subscribe`, and it's only used with `--callback-url`. As there,
`--verify-token-stdin` needs `--yes`. Setting
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

Verification tokens are redacted from dry-run output and verbose diagnostics, including app-subscription query parameters. The real subscription request sends the supplied token.
