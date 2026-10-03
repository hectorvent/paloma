# `paloma block`

Block-list management on the sender number, via the Cloud API
`/{phone-number-id}/block_users` endpoint.

```
paloma block <verb> [args] [flags]
```

## `paloma block list`

List currently blocked users.

```bash
paloma block list
```

## `paloma block add` / `remove`

Block or unblock one or more users by `wa_id`. Both accept multiple ids and
support `--dry-run`.

```bash
paloma block add 15551234567 15557654321
paloma block remove 15551234567
```

> Blocking only works for users who messaged within the customer-service window,
> and there is a cap on how many users can be blocked.
