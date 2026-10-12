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

## `paloma profile photo`

Upload a JPEG or PNG and make it the profile photo.

```
paloma profile photo <file> [--type <mime>]
```

```bash
paloma profile photo ./logo.png --app-id 1234567890
```

This takes two steps: a [handle upload](media.md#upload-handles), then a profile update with
that handle. The upload's rules apply, including how the app id is chosen and how an
interrupted transfer resumes. `--dry-run` prints all the requests and uploads nothing.

Paloma judges the file by its extension, or by `--type` if you set it, not by its contents.
Meta's docs give no size or dimension rules for profile photos, so Meta decides those.

If the upload works but the profile update fails, Paloma warns you. The update may still
have applied, so check with `paloma profile get` before you repeat the command.
