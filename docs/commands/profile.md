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
      --address "Santo Domingo" --vertical RESTAURANT --description "..."
```

Paloma checks these limits before any API call and exits 5 if one is exceeded:

| Flag | Limit |
|------|-------|
| `--about` | 1 to 139 characters |
| `--address` | 256 characters |
| `--description` | 512 characters |
| `--email` | 128 characters |
| `--website` | 256 characters each, two websites at most |

`--vertical` takes one of the following, in either case; anything else exits 2. `ALCOHOL`,
`APPAREL`, `AUTO`, `BEAUTY`, `EDU`, `ENTERTAIN`, `EVENT_PLAN`, `FINANCE`, `GOVT`, `GROCERY`,
`HEALTH`, `HOTEL`, `MATRIMONY_SERVICE`, `NONPROFIT`, `ONLINE_GAMBLING`, `OTHER`, `OTC_DRUGS`,
`PHYSICAL_GAMBLING`, `PROF_SERVICES`, `RESTAURANT`, `RETAIL`, `TRAVEL`.

See Meta's [business profile fields](https://developers.facebook.com/documentation/business-messaging/whatsapp/reference/whatsapp-business-phone-number/whatsapp-business-profile-api).

## `paloma profile photo set`

Upload a JPEG or PNG and make it the profile photo.

```
paloma profile photo set <file> [--type <mime>]
```

```bash
paloma profile photo set ./logo.png --app-id 1234567890
```

This takes two steps: a [handle upload](media.md#upload-handles), then a profile update with
that handle. The upload's rules apply, including how the app id is chosen and how an
interrupted transfer resumes. `--dry-run` prints all the requests and uploads nothing.

Paloma judges the file by its extension, or by `--type` if you set it, not by its contents.
Meta's docs give no size or dimension rules for profile photos, so Meta decides those.

If the upload works but the profile update fails, Paloma warns you. The update may still
have applied, so check with `paloma profile get` before you repeat the command.
