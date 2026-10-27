# `paloma media`

Handle the upload → media-id → send dance, plus metadata and download.

```
paloma media <verb> [args] [flags]
```

---

## `paloma media upload`

Upload a local file and print its media id.

```
paloma media upload <file> [--type <mime>] [--handle | --voicemail-announcement]
```

| Flag | Description |
|------|-------------|
| `<file>` | Local file to upload (positional) |
| `--type <mime>` | Content type, e.g. `image/jpeg`. Auto-detected from the extension if omitted, and used for the checks below if set. |
| `--handle` | Print a resumable upload handle instead of a media id |
| `--voicemail-announcement` | Upload a greeting for [`calling voicemail`](calling.md#paloma-calling-voicemail) |

```bash
ID=$(paloma media upload ./flyer.jpg -q)
paloma send image 15551234567 --id "$ID" --caption "Specials"
```

### File checks

Without `--handle`, before any API call, Paloma checks the file against Meta's
[supported media types](https://developers.facebook.com/documentation/business-messaging/whatsapp/business-phone-numbers/media#supported-media-types).
The type decides the limit, and a file over it exits 5.

| MIME types | Maximum size |
|------------|--------------|
| `image/jpeg`, `image/png` | 5 MB |
| `video/mp4`, `video/3gpp` | 16 MB |
| `audio/aac`, `audio/amr`, `audio/mpeg`, `audio/mp4`, `audio/ogg` | 16 MB |
| `image/webp` (stickers) | 500 KB |
| `text/plain`, `application/pdf`, and the Word, Excel and PowerPoint types | 100 MB |

- **Other `image/`, `video/` or `audio/` types** exit 5, for example `image/gif`.
- **Any other type** could still go out as a document, because Meta says other file types may
  be sent but aren't supported. Paloma warns on stderr and uploads it, up to 100 MB.
- **Parameters and case** are ignored in the check and sent as you wrote them, so
  `--type "audio/ogg; codecs=opus"` counts as `audio/ogg`.
- **Sizes** count 1 MB as 1,048,576 bytes and allow 500 KB for any sticker. Meta can still
  refuse a file within these limits, such as a static sticker over 100 KB or an OGG file that
  isn't OPUS.

Paloma judges the file by its extension, or by `--type` if you set it, not by its contents.

### Voicemail announcements

```bash
GREETING=$(paloma media upload ./greeting.ogg --voicemail-announcement -q)
```

The greeting must be `audio/ogg` with the Opus codec, and any other type exits 5. Meta also
requires it to be under 60 seconds, which it checks itself. Meta keeps it for as long as voicemail
uses it, past the usual 30 days, and it can't be sent as a message.

### Upload handles

Sending a message takes a media id, but creating a template with a media header takes a
handle, which comes from Meta's [Resumable Upload API](https://developers.facebook.com/docs/graph-api/guides/upload).
Put it in the header's `example.header_handle`:

```bash
HANDLE=$(paloma media upload ./flyer.jpg --handle -q)
```

- **File types:** PDF, JPEG, PNG and MP4. Anything else exits 5 before any API call, and
  `--type` must match exactly. An empty file also exits 5. No size limit is checked.
- **App id:** the session belongs to a Meta app. Set `--app-id` or `PALOMA_APP_ID`.
  Otherwise Paloma looks up the first app subscribed to your WABA, even under `--dry-run`,
  and tells you which one it used. It warns if there are several.
- **Interrupted transfers:** if the connection drops, times out or Meta answers with a 500,
  502, 503 or 504, Paloma asks Meta how much arrived and continues from there, up to 3 tries
  in total. `--timeout` applies to each try. A later command can't pick up an earlier upload.
- **Don't change the file during the upload:** if its size or modified time changes, the
  command exits 5 and prints no handle.

---

## `paloma media get`

Show media metadata, including the short-lived download URL.

```
paloma media get <media-id>
```

```bash
paloma media get 1284756... -o json
```

---

## `paloma media download`

Download media to a local file. `paloma` resolves the temporary URL and follows it
with the auth header for you.

```
paloma media download <media-id> --out <path>
```

| Flag | Description |
|------|-------------|
| `--out <path>` / `-O` | **Required.** Output file path. (`-o` is the global output-format flag, so download uses `--out`/`-O`.) |

```bash
paloma media download 1284756... --out ./received.jpg
```

---

## `paloma media delete`

```
paloma media delete <media-id> [--yes]
```

Deletes an uploaded media object (scoped to the active phone number). Confirms first
unless `--yes`.

Downloads honor the global `--timeout` for the request and response-body transfer. A
transfer interrupted after data arrives may leave a partial output file.

For a download dry-run, the second GET uses a labelled placeholder because its URL
comes from the metadata response. Placeholders are readable in the plan and cannot
be executed by the Graph client.
