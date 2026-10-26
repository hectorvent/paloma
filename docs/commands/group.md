# `paloma group`

Create and manage WhatsApp groups on the active phone number. Meta only offers the Groups API
to an [Official Business Account](number.md#official-business-account-applications); without
one, these commands exit `6`. Check with `paloma number oba get`.

Subscribe your app to the `group_lifecycle_update`, `group_participants_update`,
`group_settings_update` and `group_status_update` webhook fields first, because Meta reports
much of the outcome there.

To message a group, use [`paloma send … --group`](send.md#sending-to-a-group).

## `paloma group create`

```bash
paloma group create 'New Purchase Inquiry' --description 'Options for this year' \
  --join-approval approval-required
```

The subject has at most 128 characters and the description at most 2048. `--join-approval` is
`approval-required` or `auto-approve`, and Meta defaults to auto-approve. Meta creates the group
asynchronously: `-q` prints the request id, and the group id and invite link arrive in a
`group_lifecycle_update` webhook.

## `paloma group list` and `get`

```bash
paloma group list --all
paloma group list -q                 # group ids, one per line
paloma group get "$GROUP_ID" -o json
paloma group get "$GROUP_ID" --fields subject,total_participant_count
```

`list` accepts `--limit` (1–1024), `--after` and `--all`. `get` asks for the subject, description,
suspension state, creation time, participants and join approval mode by default, because Graph
returns only the id without `fields`.

## `paloma group set`

```bash
paloma group set "$GROUP_ID" --subject 'Watch Enthusiasts'
paloma group set "$GROUP_ID" --photo square.jpg
```

Pass any of `--subject`, `--description` and `--photo`. A photo must be a JPEG of at most 5 MB.
Meta also requires it to be square and at least 192 x 192, which it checks itself. The outcome,
including a partial failure, arrives in a `group_settings_update` webhook.

## `paloma group delete`

```bash
paloma group delete "$GROUP_ID" --yes
```

Removes every participant, including the business. It confirms first.

## `paloma group invite`

```bash
paloma group invite get "$GROUP_ID" -q
paloma group invite reset "$GROUP_ID" --yes
```

`-q` prints the link. `reset` confirms first, because every earlier link stops working.

## `paloma group members remove`

```bash
paloma group members remove "$GROUP_ID" 15551234567 --yes
```

Takes up to 8 people per request and confirms first. Someone removed can't rejoin through an
invite link. There's no command for adding people, because Meta only lets them join through the
invite link. If Meta can't remove everyone, the response is printed, stderr warns and the command
exits `1`.

## `paloma group requests`

```bash
paloma group requests list "$GROUP_ID" --all
paloma group requests approve "$GROUP_ID" "$REQUEST_ID"
paloma group requests reject "$GROUP_ID" "$REQUEST_ID"
```

For groups created with `--join-approval approval-required`. Someone rejected can ask again. As
with `members remove`, if Meta can't process every request, the command exits `1`.

## `paloma group pin` and `unpin`

```bash
paloma group pin "$GROUP_ID" "$MESSAGE_ID" --days 7
paloma group unpin "$GROUP_ID" "$MESSAGE_ID"
```

`--days` is 1 to 30. Up to 3 messages can be pinned, and Meta unpins the oldest when a fourth is
pinned. `-q` prints the message id.

## Exit codes worth knowing

| Code | When |
|------|------|
| `1` | Part of a `members remove` or `requests` call failed |
| `4` | Too many group operations in a short time (Meta #131208) |
| `5` | A bad group id, subject, description, photo or `--days` value, before any request |
| `6` | The number isn't eligible (#131215), the group is suspended (#131207), or group creation is temporarily disabled (#131214) |
