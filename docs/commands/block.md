# `paloma block`

Block-list management on the sender number, via the Cloud API
`/{phone-number-id}/block_users` endpoint.

```
paloma block <verb> [args] [flags]
```

## `paloma block list`

List currently blocked users.

| Flag | Meaning |
|------|---------|
| `--limit <n>` | Page size, at least 1 |
| `--all` | Follow every page and merge them |
| `--after <cursor>` | Start after this cursor |

```bash
paloma block list
paloma block list --all
paloma block list --all -o json | jq -r '.data[] | .wa_id // .user_id'
```

The table shows `wa_id` and `user_id`, the user's
[business-scoped user ID](https://developers.facebook.com/documentation/business-messaging/whatsapp/business-scoped-user-ids)
(BSUID). Meta leaves out `wa_id` when it can't share the user's phone number, and the column
then shows `-`. Meta's parent BSUID, `parent_user_id`, isn't in the table; use `-o json`, `-o yaml` or `-o raw`.

Meta's documented response for this list has cursors but no link to a next page, so a
page doesn't always show whether it's the last one. When it doesn't, a single page notes
on stderr that more results may exist. `--all` keeps going until a page comes back empty,
which can take one request more than the list needs. It exits `1` if Meta repeats a
cursor, and also if the block list changes during the walk (error `139102`). Run it again
in that case.

## `paloma block add` / `remove`

Block or unblock one or more users by E.164 number or BSUID, in any mix. Paloma tells them
apart [the same way `send` does](send.md#recipients) and writes `user` for a number and
`user_id` for a BSUID. Both verbs support `--dry-run`.

```bash
paloma block add 15551234567 US.13491208655302741918
paloma block remove 15551234567
```

Anything else exits `5` before any API call. That includes a parent BSUID
(`US.ENT.11815799212886844830`), because Meta fails those on this endpoint.

> Blocking only works for users who messaged within the customer-service window,
> and there is a cap on how many users can be blocked.
