# `paloma media`

Handle the upload → media-id → send dance, plus metadata and download.

```
paloma media <verb> [args] [flags]
```

---

## `paloma media upload`

Upload a local file and print its media id.

```
paloma media upload <file> [--type <mime>] [--handle]
```

| Flag | Description |
|------|-------------|
| `<file>` | Local file to upload (positional) |
| `--type <mime>` | Content type, e.g. `image/jpeg`. Auto-detected if omitted. |
| `--handle` | Print a resumable upload handle instead of a media id |

```bash
ID=$(paloma media upload ./flyer.jpg -q)
paloma send image 15551234567 --id "$ID" --caption "Specials"
```

### Upload handles

Sending a message takes a media id, but creating a template with a media header takes a
handle, which comes from Meta's [Resumable Upload API](https://developers.facebook.com/docs/graph-api/guides/upload).
Put it in the header's `example.header_handle`:

```bash
HANDLE=$(paloma media upload ./flyer.jpg --handle -q)
```

- **File types:** PDF, JPEG, PNG and MP4. Anything else exits 5 before any API call.
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
