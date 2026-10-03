# `paloma media`

Handle the upload → media-id → send dance, plus metadata and download.

```
paloma media <verb> [args] [flags]
```

---

## `paloma media upload`

Upload a local file and print its media id.

```
paloma media upload <file> [--type <mime>]
```

| Flag | Description |
|------|-------------|
| `<file>` | Local file to upload (positional) |
| `--type <mime>` | Content type, e.g. `image/jpeg`. Auto-detected if omitted. |

```bash
ID=$(paloma media upload ./flyer.jpg -q)
paloma send image 15551234567 --id "$ID" --caption "Specials"
```

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
