# `paloma number`

Phone-number management on the WABA. Reads use GET; the write verbs
support `--dry-run`.

```
paloma number <verb> [args] [flags]
```

## `paloma number list`

List every phone number on the active WABA.

```bash
paloma number list
paloma number list -o json | jq -r '.data[] | "\(.id)\t\(.display_phone_number)\t\(.quality_rating)"'
```

## `paloma number get` / `quality`

Show details for a number (defaults to the active sender; pass an id to target
another). `quality` is a compact view of rating, messaging tier, and throughput.

```bash
paloma number get
paloma number get 1556677889900
paloma number quality
```

## `paloma number register` / `deregister`

Register the active number for Cloud API with its six-digit PIN, or deregister it.
`deregister` is disruptive, so it confirms unless `--yes`.

```bash
paloma number register              # prompts for the PIN
paloma number deregister --yes
```

## `paloma number set-pin`

Set or update the two-step verification PIN.

```bash
paloma number set-pin               # prompts for the PIN
```

### Passing the PIN in scripts

Both commands read the PIN from one of these, and prompt if there's a terminal and none is set:

| Source | Example |
|--------|---------|
| `PALOMA_PIN` | `PALOMA_PIN="$PIN" paloma number register` |
| `--pin-stdin` | `printf '%s' "$PIN" \| paloma number register --pin-stdin` |

With neither and no terminal, the command exits `5`, including under `--dry-run`, where any
six digits will do because the output masks them. The older `--pin <value>` flag still
works, but it warns, because other processes can read a command line.

PIN values are redacted from dry-run output and verbose request diagnostics. The real request sends the supplied PIN.
