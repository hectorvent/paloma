# message

Act on messages you received, and trace the delivery of ones you sent.

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

## `paloma message history`

```
paloma message history <message-id>
```

Shows a message's delivery status trail: each status it reached, when, and how the
webhook update went. It shows statuses, not the message's content.

```bash
$ paloma message history wamid.HBgL...
DELIVERY STATUS  OCCURRENCE TIMESTAMP  STATUS TIMESTAMP  ERROR DESCRIPTION  WEBHOOK UPDATE STATE  WEBHOOK URI
SENT             1640995200            1640995205        -                  SUCCESS               https://example.com/hook
DELIVERED        1640995260            1640995265        -                  SUCCESS               https://example.com/hook
```

Timestamps are Unix seconds, and rows keep the order Meta returns. `-o json` also has
the event ids and the application.

It uses two endpoints, both reads:

1. `GET /{phone-number-id}/message_history?message_id=<wamid>` finds the message's
   history entry, following pages until it does. No entry for that wamid exits 1.
2. `GET /{message-history-id}/events` returns the trail. Every page is fetched.

`--dry-run` prints both and sends neither, with `<history-id>` standing in for the id
the first call would return. `-q` prints the history id. See Meta's
[message history events](https://developers.facebook.com/documentation/business-messaging/whatsapp/reference/message-history/whatsapp-business-message-history-events-api)
reference.

Meta can return no history even for a delivered message. This does not prove the
message does not exist; the command reports the missing history on stderr and exits `1`.
