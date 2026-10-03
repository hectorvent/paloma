# `paloma calling`

Read and update the WhatsApp Business Calling configuration on the sender number,
via the Cloud API `/{phone-number-id}/settings` node (the `calling` object).

```
paloma calling <verb> [flags]
```

This covers the calling **configuration**. Real-time call control (placing and
answering calls) is a separate, larger surface and is not implemented.

## `paloma calling status`

Show the current calling config (status, call-icon visibility, callback
permission, call hours, SIP, audio codecs).

```bash
paloma calling status
paloma calling status -o json | jq '.calling'
paloma calling status --sip-credentials   # include SIP passwords in the output
```

## `paloma calling enable` / `disable`

Turn calling on or off for the active number.

```bash
paloma calling enable
paloma calling disable
```

## `paloma calling set`

Update part of the config. The typed flags cover the confirmed scalar fields;
for `call_hours` and `sip` (whose exact JSON shape paloma does not model), use the
`--file` escape hatch with a full `calling` object. Enum values are case-insensitive.

| Flag | Values |
|------|--------|
| `--status` | `ENABLED` \| `DISABLED` |
| `--call-icon` | `DEFAULT` \| `DISABLE_ALL` |
| `--callback-permission` | `ENABLED` \| `DISABLED` |
| `--file <json>` | A full `calling` object (mutually exclusive with the flags above) |

```bash
paloma calling set --call-icon DISABLE_ALL
paloma calling set --status ENABLED --callback-permission ENABLED

# call hours / SIP via the escape hatch
paloma calling set --file calling.json
```

All writes support `--dry-run`, which prints the exact POST body without sending.
Partial updates are merged server-side, so you only send the fields you change.

## `paloma calling sip`

Configure the SIP server(s) calls are routed to. `set` does a full replace of the
server list; `--server` takes `host` or `host:port` (port defaults to 5061) and
repeats. `--enable-calling` flips `calling.status` to ENABLED in the same request.

```bash
paloma calling sip set --server pbx.acme.com:5061 --enable-calling
paloma calling sip set --server pbx.acme.com --tgrp sales --context support
paloma calling sip disable
```

> Enabling SIP routes inbound calls as SIP INVITEs to your server, and the `calls`
> webhook stops being delivered (it is one or the other). The Meta-generated SIP
> password is read with `paloma calling status --sip-credentials`; `app_id` and the
> password are managed by Meta and are never sent on a write.

## `paloma calling codecs`

Set the additional audio codecs (a full replace). Valid values are `PCMA` and
`PCMU`; OPUS is the default codec and is always present, so it is not listed.

```bash
paloma calling codecs set PCMA PCMU
paloma calling codecs clear          # additional_codecs -> []
```
