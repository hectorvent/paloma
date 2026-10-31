# `paloma automation`

Read and update conversational components on the active phone number: welcome messages,
ice breakers (prompts), and slash-command suggestions. Uses the same API as the
`ConversationalAutomation` manifest kind.

```bash
paloma automation get
paloma automation set --welcome --prompt 'Book a visit' --prompt 'Get support'
paloma automation set --command 'help:Talk to support' --command 'status:Track an order'
paloma automation set --clear-prompts --clear-commands --dry-run
```

| Flag on `set` | Meaning |
|---|---|
| `--welcome` / `--no-welcome` | Set `enable_welcome_message` to true or false |
| `--prompt <text>` | Replace the prompt list; repeat for up to 4 ice breakers of 80 characters each |
| `--command NAME:DESCRIPTION` | Replace the command list; repeat for up to 30 commands. Names omit `/`, with at most 32 characters; descriptions allow 256. The first colon or equals sign separates the name and description |
| `--clear-prompts` | Set prompts to an empty list; exclusive with `--prompt` |
| `--clear-commands` | Set commands to an empty list; exclusive with `--command` |
| `--file <path>` | Full conversational-automation JSON object; exclusive with typed settings |

`--ice-breaker` is an alias for `--prompt`.
Names must be unique within the update, and names and descriptions must be nonblank.

Omitted components are left out of the update; explicit clear flags send empty lists.
Files use Graph's snake_case keys:

```json
{
  "enable_welcome_message": false,
  "prompts": ["Get support"],
  "commands": [{"command_name": "help", "command_description": "Talk to support"}]
}
```

Empty updates, invalid JSON shapes and documented length/count violations exit `5`
before context resolution or an API call. Meta also disallows emojis in prompts and
commands; the API remains the authority on supported text.

`get -q` prints the returned phone-number id. Settings writes have no primary id, so
`set -q` produces no output. All data goes through the selected output format; diagnostics
go to stderr. All three verbs support `--dry-run` without reading a token or sending requests.

## `paloma automation bot get`

```bash
paloma automation bot get 123 -o json
```

Reads a bot by its WABA bot ID, which is distinct from the phone-number ID. The ID is
validated before resolving context. JSON/raw preserve prompts, commands and welcome
settings; `-q` prints the returned bot ID when Meta includes it. Dry-run is offline.
