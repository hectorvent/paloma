# `paloma qr`

Create and manage Meta-hosted message QR codes and their short links on the active
phone number. Unlike an offline `link create` URL, a QR code's prefilled message can
be changed later and the link can be retired.

```bash
paloma qr create 'Tell me more' -q
paloma qr list --all
paloma qr get 4O4YGZEG3RIVE1
paloma qr update 4O4YGZEG3RIVE1 'Book an appointment'
paloma qr delete 4O4YGZEG3RIVE1 --yes
```

`create` and `update` take the message as a positional argument, limited to 140
characters; blank messages are refused. Codes are opaque
14-character alphanumeric identifiers. Invalid codes, oversized messages and invalid
page sizes exit `5` before context resolution or an API call.

`list` accepts `--all`, `--after <cursor>` and `--limit <1..25>`. Without `--all`, it
returns one page and notes on stderr if more results exist.

## Images

Create a new code with a generated image, or save an existing code's image:

```bash
paloma qr create 'Support' --image svg --out support.svg
paloma qr get 4O4YGZEG3RIVE1 --image png --out support.png
```

`--image svg|png` accepts either case and defaults to `CODE.svg` or `CODE.png`.
Use `--out` (or `-O`) to choose a destination. `--out` requires `--image`. The destination
must be new and its parent directory must exist; existing files and symlinks are refused. The response
continues to describe the Graph resource, while the saved-file diagnostic goes to stderr.
The signed/public image URL is downloaded without forwarding the Graph token.

If the image download fails after creating a code, stderr reports the created code so
you can retry with `qr get --image ... --out ...` instead of creating another. HTTP
errors before a download starts do not create the output file; an interrupted transfer
can leave a partial file. `--timeout` bounds each download request and its body transfer.

## Output and confirmation

`create`, `get`, `update` and `delete` print the QR code under `-q`. Lists have no primary
id, so `list -q` produces no output. JSON/raw preserve the Graph response on a single
request; `--all` returns merged data. Meta wraps `get` responses in `data`.

Deleting requires confirmation, or `--yes`, and expires the short link. Noninteractive
confirmation refusals exit `5`. All verbs support `--dry-run`; an image dry-run prints
both the Graph request and a placeholder image GET without creating a code or file.
