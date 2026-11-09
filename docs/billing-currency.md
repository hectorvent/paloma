# Migrating your billing currency

Meta is moving businesses in some countries to local-currency billing. A WABA's currency
can't be changed in place, so migrating creates a **new WABA with a new ID**, and Paloma
has to be pointed at it afterwards.

Use `paloma account billing-currency start|status|complete` for the three API steps,
then [update Paloma](#after-the-migration-update-paloma). The raw API examples below
show the same calls.

> Meta's newer docs call a WABA a **Messaging account**. This page uses WABA, as the rest
> of Paloma's docs do.

## Deadlines

| Billing currency | Migrate by | What happens after |
|---|---|---|
| **INR** (India) | **December 31, 2026** | From January 1, 2027, Meta no longer delivers messages from non-INR WABAs |
| **BRL** (Brazil) | **June 30, 2027** | From July 1, 2027, Meta no longer delivers messages from non-BRL WABAs |

These apply to Solution Providers and directly integrated businesses whose Sold-To country
is India or Brazil in [Billing Hub](https://business.facebook.com/billing_hub/legal_entities).
Every WABA in the business portfolio has to migrate. See
[Meta's pricing page](https://developers.facebook.com/documentation/business-messaging/whatsapp/pricing)
for the current terms.

## Before you start

- **Eligibility:** Coexistence and Authorized Agents accounts can't use this API.
- **Access:** you need admin access to the WABA and a token with the
  `whatsapp_business_management` and `whatsapp_business_messaging` permissions.
- **Timing:** the last step switches billing at once, and the old WABA can no longer send
  messages. Plan it for a quiet period.
- **Templates:** status and category are locked while the migration runs, and templates
  created on the old WABA after it starts aren't copied. Avoid creating any until it's done.

| | Moves to the new WABA | Stays behind |
|---|---|---|
| **Templates** | All statuses, with **new template IDs** | Templates with outdated formats |
| **Flows** | Only those used by a migrated template's Flow button | Flows no template uses |
| **Access** | App installation, users and permissions | |
| **Numbers** | Stay on the same WhatsApp account hierarchy, so their IDs don't change | |
| **Insights** | New data only | All historical data, still readable on the old WABA |

Export anything you'd lose, such as Flows no template uses, before you start.

## Run with Paloma

```bash
paloma account billing-currency start --currency INR --dry-run
paloma account billing-currency start --currency INR --yes -q
paloma account billing-currency status <migration-id> -o json
# Inspect destination assets, app access and payment method before this step:
paloma account billing-currency complete <migration-id> --yes
```

`start` accepts `--extended-credit-id` for Line of Credit billing. `complete` checks
readiness before switching billing. These commands do not change local profiles.
See [account commands](commands/account.md#paloma-account-billing-currency) for output,
confirmation and dry-run behavior.

## Run the migration through the Graph API

Set these first. Reading the token keeps it out of your shell history. The examples use
Graph API v27.0.

```bash
export WABA_ID=<current WABA ID>
read -rs WHATSAPP_TOKEN && export WHATSAPP_TOKEN
```

**1. Start it.** Use the target currency, here `INR`. Add `"extended_credit_id"` only if the
new WABA will bill to a Line of Credit.

```bash
curl -X POST "https://graph.facebook.com/v27.0/$WABA_ID/set_payment_method_migration_intent" \
  -H "Authorization: Bearer $WHATSAPP_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"currency": "INR"}'
```

Keep the `migration_id` from the response.

**2. Wait for it to be ready.** Repeat until `status` is `READY_TO_COMPLETE`.

```bash
export MIGRATION_ID=<migration_id>
curl "https://graph.facebook.com/v27.0/$MIGRATION_ID" \
  -H "Authorization: Bearer $WHATSAPP_TOKEN"
```

**3. Check the new WABA.** The response's `destination_waba.id` is your new WABA ID. Paloma
can check it without changing your config:

```bash
export NEW_WABA_ID=<destination_waba.id>
paloma template list --all --waba-id "$NEW_WABA_ID"   # templates were copied
paloma flow list --all --waba-id "$NEW_WABA_ID"       # Flows your templates use
paloma webhook get --waba-id "$NEW_WABA_ID"           # your app is installed
```

**4. Attach a payment method.** The migration won't complete without one. A Line of Credit
passed in step 1 is attached for you. A credit card can't be added through the API, so add
it to the new WABA in Business Manager.

**5. Complete it.** This is the switch: billing moves and the old WABA stops sending.

```bash
curl -X POST "https://graph.facebook.com/v27.0/$MIGRATION_ID/resume_migration" \
  -H "Authorization: Bearer $WHATSAPP_TOKEN"
```

Check the status again until it's `COMPLETED`.

If the status is `FAILED`, start again from step 1. If it's `FAILED_TO_COMPLETE`, repeat
step 5. If it's `REJECTED`, check your access, permissions and eligibility. See
[Meta's migration guide](https://developers.facebook.com/documentation/business-messaging/whatsapp/pricing/change-billing-currency)
for the full reference.

## After the migration: update Paloma

Point each profile that used the old WABA at the new one. Your phone number ID stays the same.

```bash
paloma config set waba_id "$NEW_WABA_ID"
```

Add `--profile <name>` for other profiles, and update `PALOMA_WABA_ID` wherever you set it,
such as CI. Then:

- **Replace saved template IDs.** Anything that stores a template ID needs the new one.
- **Send a test message** and confirm it's delivered and that its cost appears under the
  new WABA.
- **Read old data from the old WABA.** History doesn't move, so pass `--waba-id` with the
  old ID to reach it.

### If your token reaches several WABAs on one number

Meta then expects `messaging_account_id` on each send. Without it, a message
[may be billed to the wrong account or rejected](https://developers.facebook.com/documentation/business-messaging/whatsapp/account-model-evolution/messaging).
Name the one to bill with `--messaging-account-id`, or save it on the profile:

```bash
paloma config set messaging_account_id "$NEW_WABA_ID"
```
