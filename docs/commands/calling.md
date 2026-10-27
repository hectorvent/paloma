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
for settings without a flag, such as `voicemail`, use the
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

# other settings through the escape hatch
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
`--phone-number`, `--country` and `--direction` (`user-initiated`, `business-initiated`), break
results down with `--dimension` (`phone`, `direction`, `country`), and choose `--metric` (`count`,
`cost`, `average-duration`). Each repeats or takes a comma-separated list. Tables show only the
time range and metrics, so use `-o json` to see a breakdown.
