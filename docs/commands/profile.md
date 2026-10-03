# `paloma profile`

Business-profile management on the sender number.

```
paloma profile <verb> [flags]
```

## `paloma profile get`

Show the business profile (about, address, description, email, websites, vertical,
profile photo URL).

```bash
paloma profile get
paloma profile get -o json | jq '.data[0]'
```

## `paloma profile set`

Update one or more profile fields. At least one field is required; `--dry-run`
prints the request body without sending. Repeat `--website` for more than one.

```bash
paloma profile set --about "Bakery & catering" \
      --email hola@acme.do --website https://acme.do \
      --address "Santo Domingo" --vertical FOOD --description "..."
```

## Planned

`profile photo` (upload the profile picture) needs the resumable upload handle
flow and lands in a later phase. See the [planned commands](planned.md).
