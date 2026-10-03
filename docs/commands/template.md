# `paloma template`

Manage the full message-template lifecycle on the WABA: `list`, `status`, `get`,
`create`, `submit`, `watch`, `edit`, `delete`. Only `namespace` is still planned.

```
paloma template <verb> [args] [flags]
```

---

## `paloma template list`

List templates on the active WABA, with optional filters.

| Flag | Description |
|------|-------------|
| `--status <s>` | Filter by `APPROVED`, `PENDING`, or `REJECTED` |
| `--category <c>` | Filter by `MARKETING`, `UTILITY`, or `AUTHENTICATION` |
| `--language <code>` | Filter by language code, e.g. `es` |
| `--limit <n>` | Templates **per request** — Graph's page size, not a total |
| `--all` | Follow paging cursors and return every template |
| `--after <cursor>` | Return the single page following this cursor |

Results are paged. Without `--all`, a listing that has more pages says so on stderr and names
the cursor to resume from, so a partial result never looks complete. See
[Paging](flow.md#paging) for the details — the behavior is identical across list commands.

```bash
paloma template list
paloma template list --status APPROVED --category UTILITY
paloma template list -o json | jq -r '.data[] | "\(.name)\t\(.status)"'
```

The raw Cloud API response is returned, so `-o json` pipes cleanly to `jq`.

---

## `paloma template status`

Compact approval view for a single template by name.

```
paloma template status <name>
```

```bash
paloma template status order_ready
paloma template status order_ready -o json
```

Output includes `name`, `language`, `category`, `status`, and, when present,
`rejected_reason`. Exits non-zero if no template by that name exists on the WABA.

---

## `paloma template get`

Show the full definition of a template by name, optionally narrowed to one
language.

```
paloma template get <name> [--language es]
```

---

## `paloma template create` / `submit`

Submit a template for approval, either from a full `--file` or from inline flags
that build a single-BODY template. `submit` is an alias of `create`.

```bash
paloma template create --file template.json

paloma template create --name order_ready --category UTILITY --language en_US \
      --body "Hi {{1}}, order {{2}} is ready." --example "Hector,#4821"
```

`--example` is a comma-separated list of sample values for `{{1}}, {{2}}, ...`.
Use `--dry-run` to print the exact request body without submitting.

## `paloma template watch`

Poll a template's approval status until it resolves. CI-friendly: exits `0` on
APPROVED, non-zero on REJECTED or timeout.

```
paloma template watch <name> [--interval 10s] [--max-wait 10m]
```

## `paloma template edit`

Edit an existing template's components/category. The template is resolved by name
(and `--language` when it exists in more than one).

```bash
paloma template edit order_ready --file template.json
```

## `paloma template delete`

Delete a template. Without `--language` it removes every language of the name, so
it confirms first unless `--yes` (or `--dry-run`).

```bash
paloma template delete order_ready
paloma template delete order_ready --language es --yes
```

---

## Planned verbs

`namespace` (fetch the WABA `message_template_namespace`). See the
[planned commands](planned.md).
