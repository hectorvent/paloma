# message

Act on messages you received.

## `paloma message read`

```
paloma message read <message-id>
```

Marks an inbound message as read (the sender sees blue ticks). Messages before it in
the thread are marked read too.

## `paloma message typing`

```
paloma message typing <message-id>
```

Marks the message as read and shows a typing indicator in the chat. It disappears when
you send a reply or after about 25 seconds.

```bash
paloma message typing wamid.HBgL... && sleep 2 && paloma send text 15551234567 "Here you go" --reply-to wamid.HBgL...
```

Both POST to `/{phone-number-id}/messages` and honor `--dry-run`.
