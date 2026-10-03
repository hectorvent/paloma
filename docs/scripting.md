# Scripting & automation

`paloma` is built for pipes and CI: clean stdout, diagnostics on stderr, machine
formats, and meaningful [exit codes](global-flags.md#exit-codes).

## Capture an ID

`--quiet` prints only the primary identifier to stdout:

```bash
MID=$(paloma send text 15551234567 "your order is ready" -q)
echo "sent $MID"
```

## Pipe JSON to jq

`--output json` emits the raw Cloud API response:

```bash
paloma template list -o json | jq -r '.data[] | "\(.name)\t\(.status)"'

paloma send text 15551234567 "hi" -o json | jq -r '.messages[0].id'
```

## Preview requests with --dry-run

Build and inspect the exact request body without sending — great for debugging
payloads or generating fixtures:

```bash
paloma send template 15551234567 order_ready --lang en_US \
    --body-var "Hector" --body-var "#4821" \
    --dry-run -o json > expected-request.json
```

## Branch on exit codes

```bash
if paloma auth status -q >/dev/null 2>&1; then
  echo "token OK"
else
  case $? in
    3) echo "auth problem — run paloma auth login" ;;
    4) echo "rate limited — back off" ;;
    *) echo "unexpected failure" ;;
  esac
fi
```

## Bulk sends with throttling

stdout vs. stderr separation makes loops safe to pipe:

```bash
while read -r number; do
  paloma send template "$number" promo --lang es -q
  sleep 1                      # be gentle with rate limits
done < recipients.txt > sent-ids.txt
```

## Use in CI

- The native `paloma` binary has no runtime dependencies — drop it into any image.
- Provide credentials via environment, not files:

  ```bash
  export PALOMA_TOKEN="$WHATSAPP_TOKEN"          # from your CI secret store
  export PALOMA_PHONE_NUMBER_ID="1556677889900"
  export PALOMA_WABA_ID="1029384756"
  paloma template status order_ready -o json
  ```

- On Linux/Windows runners, keyring storage isn't available — `PALOMA_TOKEN` is the
  intended path (see [Authentication](authentication.md)).

## Tips

- Set `PALOMA_OUTPUT=json` once to default every command to JSON.
- Set `NO_COLOR=1` (or `--no-color`) in logs to strip ANSI escapes.
- Combine `--profile` with per-environment secrets to target staging vs. prod
  from the same script.
