# `paloma webhook`

Read and manage WhatsApp webhook delivery for the active WhatsApp Business Account
(WABA), with optional alternate callbacks on individual phone numbers.

```
paloma webhook <verb> [flags]
```

`subscribe` and `unsubscribe` manage the WABA's app subscription and alternate callback
(`POST/DELETE /{waba-id}/subscribed_apps`). `subscribe --number` manages the active
number's alternate callback; `get` resolves the effective delivery URL. These use your
existing token; no app secret is needed. `listen`, `verify-url` and `simulate` provide local receive-side development and callback checks.

## `paloma webhook get`

Show the current webhook configuration: the effective callback URL (read from the
phone-number `webhook_configuration`), the active number's
`number_override_callback_uri` (null when unset), the per-WABA `override_callback_uri` (empty
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
sources and no terminal makes the command exit `5`. Default subscriptions need no token. There is
no `--verify-token <value>` flag, because other processes can read a command line.

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

## `paloma webhook version`

Shows the Graph version Meta uses for the app's WhatsApp webhooks, and moves them to another
one. It only touches the `whatsapp_business_account` subscription; an app's Instagram or Page
subscriptions keep their version. It needs an app id and the app secret, like `fields`.

```bash
paloma webhook version
paloma webhook version -q                    # just the version, such as v26.0
paloma webhook version set v26.0             # confirms; --yes to skip
paloma webhook version set v26.0 --dry-run -o json
```

| Flag | Meaning |
|------|---------|
| `--reverify` | Also resend the current callback URL and a verify token, if Meta asks for them |
| `--verify-token-stdin` | Read that token from stdin (requires `--reverify`). Otherwise uses `PALOMA_VERIFY_TOKEN`, or prompts |

`set` resubmits the same fields through the requested version, so the subscribed set doesn't
change. Already on that version, it writes nothing. Afterward it reads the subscription back
and exits `1` if Meta still reports another version, because Meta doesn't document this route
for changing versions; if that happens, use the App Dashboard. Payloads can differ between
versions, so check your endpoint handles the new one first.

This is the webhook version, which isn't the same as `--api-version` (the version Paloma calls
Graph with) or the app's default API version in the App Dashboard, which Paloma can't change.
All output describes the WhatsApp subscription rather than passing Graph's response through.

## `paloma webhook verify-url`

Check that your endpoint would pass Meta's verification, before you point Meta at it. It sends
what Meta sends, and passes only on what Meta accepts: status `200` and a body that is exactly
the challenge.

```
GET <url>?hub.mode=subscribe&hub.challenge=<random digits>&hub.verify_token=<your token>
```

```bash
PALOMA_VERIFY_TOKEN=t paloma webhook verify-url https://example.com/webhooks/whatsapp
printf '%s' "$VERIFY_TOKEN" | paloma webhook verify-url https://example.com/hook --verify-token-stdin

# show the request without sending it; the token is masked
PALOMA_VERIFY_TOKEN=t paloma webhook verify-url https://example.com/hook --dry-run
```

The verify token comes from the same sources as for `subscribe`. It's never printed, and
neither is the endpoint's response body, which could echo it. Redirects aren't followed, so the
token can't reach another host.

stdout shows `url`, `passed`, `status` and `challenge_echoed`, and stderr says why it failed.
`-q` prints nothing, so use the exit code:

| Exit | Meaning |
|------|---------|
| `0` | Passed |
| `1` | Failed, or the endpoint couldn't be reached (stdout is empty then) |
| `5` | Not an http or https URL, or no verify token |

Plain `http://` works for local testing but warns: Meta only calls endpoints with a valid TLS
certificate, and self-signed ones aren't accepted, so a pass on `http://` doesn't tell you
about that.

## `paloma webhook simulate`

POST a sample webhook to your endpoint, signed as Meta signs it: `X-Hub-Signature-256` is
`sha256=` and the HMAC-SHA256 of the body, with your app secret as the key. Only a `200` passes,
because Meta treats [any other status](https://developers.facebook.com/documentation/business-messaging/whatsapp/webhooks/overview#webhook-delivery-failure)
as a failed delivery and retries it for up to 7 days.

| Flag | Meaning |
|------|---------|
| `--event <name>` | Which sample to send (default `text`) |
| `--file <path>` | Send this JSON file instead, byte for byte (at most 3 MB) |
| `--signature <mode>` | `valid` (default), `wrong` or `missing` |

| `--event` | What it tests |
|-----------|---------------|
| `text` | An incoming message from a user with no username: `wa_id`, `from` and the BSUID in `user_id`, `from_user_id` |
| `text-username` | A username user whose number can't be shared: **no `wa_id` or `from`**, only `user_id`, `from_user_id`, and the parent BSUIDs |
| `status-delivered` | A delivered status with `contacts`, `recipient_user_id` and `pricing` |
| `status-failed` | A failed status with `errors`, and no `contacts` |
| `status-paid-exempt` | A free service message: `pricing.type` `free_customer_service`, `pricing.subtype` `paid_exempt` |
| `status-marketing-lite-dynamic` | A max-price marketing message: `pricing.category` `marketing_lite_dynamic`, with `cost` |
| `user-changed-number` | The `user_changed_number` system message, with the new and previous BSUIDs |
| `user-changed-user-id` | The `user_changed_user_id` system message: BSUIDs only, **no `from` or `wa_id`** |
| `template-status` | `message_template_status_update`, which has a different `value` from a message |

Samples use your configured WABA and phone number ids when there are any, the current time,
and a new message id each time, so an endpoint that deduplicates won't drop a repeat.

To test that your endpoint checks signatures, send a bad one. `--signature wrong` signs with a
random key, and `--signature missing` leaves the header out. Neither needs the app secret, and
they pass when the endpoint answers anything but `200`:

```bash
export PALOMA_APP_SECRET=...   # or an app_secret_ref in your profile
paloma webhook simulate https://example.com/hook
paloma webhook simulate https://example.com/hook --event text-username
paloma webhook simulate https://example.com/hook --file ./captured-event.json
paloma webhook simulate https://example.com/hook --signature wrong

# show the request without sending it or reading the app secret
paloma webhook simulate https://example.com/hook --event status-paid-exempt --dry-run
```

stdout shows `url`, `event`, `signature`, `status` and `passed`, and `-q` prints nothing:

| Exit | Meaning |
|------|---------|
| `0` | Passed |
| `1` | Failed (including a bad signature answered with `200`), or the endpoint couldn't be reached |
| `3` | `--signature valid` with no app secret |
| `5` | Not an http or https URL, or a `--file` that's missing, over 3 MB or not JSON |

## `paloma webhook listen`

Run a temporary webhook receiver on your machine. It answers Meta's verification handshake,
checks each POST's signature, and prints each event to stdout as one line of JSON.

| Flag | Meaning |
|------|---------|
| `--host <ip>` | IP address to bind (default `127.0.0.1`). Anything but loopback warns, because it's reachable from your network without TLS |
| `--port <port>` | Port to bind (default `8080`; `0` picks a free one, shown on stderr) |
| `--verify-token-stdin` | Read the verify token from stdin |

```bash
export PALOMA_VERIFY_TOKEN=t PALOMA_APP_SECRET=...
paloma webhook listen --port 9000 | jq -c '.entry[].changes[].value.messages[]?'

# in another terminal
paloma webhook simulate http://127.0.0.1:9000/webhook --event text-username
```

It only listens locally, and it doesn't create a tunnel. For Meta to reach it you need your own
public HTTPS address in front of it.

**Any path works.** How it answers:

| Request | Answer |
|---------|--------|
| `GET` with `hub.mode=subscribe` and the right `hub.verify_token` | `200` and the challenge |
| `GET` with anything else | `403` |
| `POST` with a good signature, or any `POST` if there's no app secret | `200`, and the event goes to stdout |
| `POST` with a bad or missing signature | `403`, and a warning on stderr |
| `POST` whose body isn't JSON | `400`, and a warning on stderr |
| `POST` over 3 MB, Meta's payload limit | `413`, and a warning on stderr |
| Any other method | `405` |

**stdout is one JSON document per line, whatever `-o` says.** A line is one delivery, exactly as
Meta sent it apart from the whitespace between tokens. Nothing is modelled, so the BSUID fields,
`user_changed_user_id`, `pricing.subtype`, `marketing_lite_dynamic` and anything Meta adds later
pass through. Meta can batch several changes into one delivery, so read `entry[].changes[]`
rather than assuming one event per line. A username user's event has no `wa_id` or `from`. With
`-q`, each delivery prints only its message and status ids, one per line. Everything else,
including the listening address and rejections, goes to stderr.

**Secrets.** The verify token comes from the same sources as for `subscribe`, so without
`PALOMA_VERIFY_TOKEN` or `--verify-token-stdin` it prompts, or exits `5` with no terminal. The
app secret comes from `PALOMA_APP_SECRET` or `app_secret_ref`. Without one, the listener warns
and accepts every POST unchecked. Neither is ever printed.

**Stopping.** Ctrl-C exits `0`. So does the reader going away, as in `… | head -1`: that event
gets a `503` so it isn't lost, and the listener stops. Any other failure writing to stdout, such
as a full disk, also answers that event with `503`, but reports the error and exits `1`.

**A stalled reader blocks the listener.** Events are written to stdout as they arrive, with no
queue, so a reader that stops reading without closing the pipe holds up the listener, Ctrl-C
included, until it reads again. Deliveries wait meanwhile, and Meta retries those that time out.

`--dry-run`: a listener has nothing to send, so it says what it would bind on stderr, binds
nothing, reads no secrets and exits `0`.

A port already in use exits `1`.

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
