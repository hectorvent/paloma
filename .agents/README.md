# Shared agent tooling

Project skills and agent instructions live here, so every coding agent reads the same
source. `AGENTS.md` remains the source of repository-wide rules.

```text
.agents/
  skills/<name>/SKILL.md       Shared skills; Codex discovers these directly
  agents/<name>.md             Shared agent instructions, with front matter
  codex-agents/<name>.toml     Codex adapters pointing to the shared instructions
  settings.local.json          Local agent settings and hooks; git-ignored
.codex/agents -> ../.agents/codex-agents
```

## Skills

Each skill has a `SKILL.md` with `name` and `description` front matter. Add or edit
it under `skills/`. Keep instructions portable and identify tool-specific steps
explicitly.

## Agents

Keep the task checklist in `agents/<name>.md`. Add a matching
`codex-agents/<name>.toml` with `name`, `description` and `developer_instructions`
directing Codex to read the shared file. Do not duplicate the checklist in TOML.
Leave model selection to the parent session unless a particular agent requires an
override.

The front matter's `tools` list is not a Codex permission. Codex agents inherit their
parent's permissions; instructions such as “do not edit files” are behavioral
constraints, not a separate sandbox. Creating a definition does not run an agent;
follow the session's delegation rules.

## Discovery checks

- Confirm `.codex/agents` resolves to `.agents/codex-agents` after checkout.
- In Codex, check the skill selector for the four project skills. Restart the
  session if newly added skills or agent definitions do not appear.
- Keep `settings.local.json` local; it is not Codex configuration.

Git must preserve symlinks for the discovery alias to work. On systems where Git
checks out symlinks as text files, enable symlink support and restore the alias.
