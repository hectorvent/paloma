# `paloma calling`

Read and update the WhatsApp Business Calling configuration on the sender number,
via the Cloud API `/{phone-number-id}/settings` node (the `calling` object).

```
paloma calling <verb> [flags]
```

This covers the calling **configuration**. Real-time call control (placing and
answering calls) is a separate, larger surface and is not implemented.

## `paloma calling get`

Show the current calling config (status, call-icon visibility, callback
permission, call hours, SIP, audio codecs, voicemail).

```bash
paloma calling get
paloma calling get -o json | jq '.calling'
paloma calling get --sip-credentials   # include SIP passwords in the output
```

## `paloma calling enable` / `disable`

Turn calling on or off for the active number.

```bash
paloma calling enable
paloma calling disable              # confirms; --yes to skip
```

## `paloma calling set`

Update part of the config. For settings without a flag, such as `video`, use the `--file`
escape hatch with a full `calling` object. Enum values are case-insensitive. Like the
matching commands, it confirms when it turns calling off (`--status DISABLED`), and a
`--file` that turns calling, SIP or voicemail off or empties the codecs confirms too.

| Flag | Values |
|------|--------|
| `--status` | `ENABLED` \| `DISABLED` |
| `--call-icon` | `DEFAULT` \| `DISABLE_ALL` |
| `--callback-permission` | `ENABLED` \| `DISABLED` |
| `--icon-countries <CC,...>` | Show the call button only to people whose phone numbers are from these countries |
| `--icon-everywhere` | Show the call button in every country again |
| `--srtp` | `DTLS` (Meta's default) \| `SDES`, the SRTP key exchange for SIP calls |
| `--file <json>` | A full `calling` object (mutually exclusive with the flags above) |

```bash
paloma calling set --call-icon DISABLE_ALL
paloma calling set --status ENABLED --callback-permission ENABLED
paloma calling set --icon-countries DO,US
paloma calling set --srtp SDES

# other settings through the escape hatch
paloma calling set --file calling.json
```

`--icon-countries` takes two-letter codes, repeated or comma-separated, and a bad code exits `5`
before any request. It goes by the country of the person's phone number, not where they are.
Meta doesn't return `srtp_key_exchange_protocol` in `calling get` until it's been set.

All writes support `--dry-run`, which prints the exact POST body without sending.
Partial updates are merged server-side, so you only send the fields you change.

## `paloma calling sip`

Configure the SIP server(s) calls are routed to. `set` does a full replace of the
server list; `--server` takes `host` or `host:port` (port defaults to 5061) and
repeats. `--enable-calling` flips `calling.status` to ENABLED in the same request.

```bash
paloma calling sip set --server pbx.acme.com:5061 --enable-calling
paloma calling sip set --server pbx.acme.com --tgrp sales --context support
paloma calling sip disable          # confirms
```

> Enabling SIP routes inbound calls as SIP INVITEs to your server, and the `calls`
> webhook stops being delivered (it is one or the other). The Meta-generated SIP
> password is read with `paloma calling get --sip-credentials`; `app_id` and the
> password are managed by Meta and are never sent on a write.

## `paloma calling codecs`

Set the additional audio codecs (a full replace). Valid values are `PCMA` and
`PCMU`; OPUS is the default codec and is always present, so it is not listed.

```bash
paloma calling codecs set PCMA PCMU
paloma calling codecs clear          # additional_codecs -> [], confirms
```

## `paloma calling voicemail`

```bash
GREETING=$(paloma media upload ./greeting.ogg --voicemail-announcement -q)
paloma calling voicemail set --trigger reject,timeout --ring-seconds 20 --announcement "$GREETING"
paloma calling voicemail disable    # confirms
```

When voicemail is on, Meta answers missed or rejected calls, plays the greeting, records a
message, and delivers it as an audio message on the `messages` webhook. That message's id is the
call's id (`wacid.…`), not a `wamid`. The triggers are:

- `reject`: you reject the call.
- `timeout`: nobody accepts or rejects it within `--ring-seconds` (0 to 30).

`--trigger timeout` needs `--ring-seconds`, because Meta ignores the trigger without it, and
`--ring-seconds` without that trigger exits `5` too. The greeting comes from
[`media upload --voicemail-announcement`](media.md#voicemail-announcements). Calling must be
enabled, and Meta advises turning [call hours](#paloma-calling-hours) off, because people can't
call outside them; `set` notes this on stderr.

## `paloma calling hours`

```bash
paloma calling hours get
paloma calling hours set --timezone America/Santo_Domingo \
  --open monday=0900-1200 --open monday=1300-1700 --open tuesday=0900-1700 \
  --holiday 2026-12-25=0000-2359 --yes
paloma calling hours disable --yes
```

`set` replaces the whole schedule and confirms first. People can't call outside these hours, and
any holiday you leave out is deleted, because that's how Meta applies the setting. Each `--open`
is `DAY=HHMM-HHMM` in 24-hour time, with at most 2 non-overlapping periods per day. Each `--holiday`
is `YYYY-MM-DD=HHMM-HHMM`, at most 20 in total. Bad schedules exit `5` before any request.

`disable` accepts calls at any time. It reads the current schedule first and sends it back, even
under `--dry-run`, because Meta requires it and would otherwise delete your holidays. `get -q`
prints the status.

## `paloma calling permission`

```bash
paloma calling permission 15551234567 -o json
paloma calling permission US.13491208655302741918 -q     # just the status
```

Shows whether you can call this person, and how much of each limit is used for sending permission
requests and starting calls. A successful read exits `0` even when the status is `no_permission`.

## `paloma calling analytics`

```bash
paloma calling analytics --start 2026-10-01 --end 2026-11-01
paloma calling analytics --start 2026-10-01 --end 2026-11-01 --granularity month \
  --direction user-initiated --dimension country --metric count,cost -o json
```

Reads the WABA's call counts, costs and average durations. `--start` and `--end` take UNIX seconds
or `YYYY-MM-DD`, and `--granularity` is `half-hour`, `day` (default) or `month`. Filter with
`--phone`, `--country` and `--direction` (`user-initiated`, `business-initiated`), break
results down with `--dimension` (`phone`, `direction`, `country`), and choose `--metric` (`count`,
`cost`, `average-duration`). Each repeats or takes a comma-separated list. Tables show only the
time range and metrics, so use `-o json` to see a breakdown.
