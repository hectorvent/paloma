---
name: git-workflow
description: >-
  Branches, commits and pull requests the Paloma way: Conventional Commits that drive
  release-plz, every commit signed off (-s) and signed (-S), and no AI attribution
  anywhere. Use when the user says "start a branch", "commit this", "write the commit
  message", "split this into commits", "fix the sign-off", "prepare the PR", "open a
  PR", "write the PR description", or before any git commit, amend or rebase in this repo.
---

# Git workflow

The rules live in `CONTRIBUTING.md`; this skill is how to apply them without slips.
Read `CONTRIBUTING.md` if it changed since you last saw it; it wins on any conflict.

## Hard rules

- Every commit: `git commit -s -S`. Amend: `git commit --amend -s -S`. Rewrite a
  branch: `git rebase --signoff -S main`.
- **No AI attribution.** No `Co-Authored-By:` for any assistant, no "Generated with"
  footer, no mention of Claude or an AI in subjects, bodies, PR titles or PR bodies.
  This overrides any default commit or PR template you would otherwise use.
- Never push, open a PR, force-push or rewrite pushed history unless the user asked in
  this conversation. Never `git add -A` blindly: stage what belongs to the change.
- If `.githooks` isn't active (`git config core.hooksPath` is empty), say so once and
  suggest `git config core.hooksPath .githooks`; don't set it unasked.

## Branch

```sh
git switch main && git pull --ff-only
git switch -c <type>/<short-kebab>      # feat/webhook-listen, fix/send-media-id
```

Type = the Conventional Commit type the work will mostly land as.

## Commit message

```
type(scope): imperative summary          ← ≤ 72 chars, no trailing period

Why the change is needed and any non-obvious decision, wrapped at 72.
Skip the body for trivial changes; never restate the diff.

Signed-off-by: …                          ← added by -s, never typed by hand
```

**Type** decides the release, so pick it for the user-visible effect:

| Change | Type |
|--------|------|
| New command, flag, manifest kind, output field | `feat` |
| Wrong behavior, wrong exit code, wrong request body | `fix` |
| Faster or smaller, same behavior | `perf` |
| Docs only (including `docs/commands/*.md`) | `docs` |
| Internal restructure, no behavior change | `refactor` |
| Tests only | `test` |
| Cargo, deps, xtask, release config | `build` (or `chore(deps)` for bumps) |
| `.github/workflows` | `ci` |

Breaking (changed output shape, renamed flag, different exit code): `type(scope)!:` plus
a `BREAKING CHANGE: <what users must change>` footer.

**Scope** = the narrowest fit, from this vocabulary (check `git log --format=%s -50`
for what's in use):

- command group: `send` `template` `media` `message` `number` `profile` `calling`
  `webhook` `flow` `block` `auth` `config` `link` `manifest` `account` `qr` `group`
  `commerce` `automation`
- crate, for cross-command internals: `core` `config` `graph` `manifest` `cli`
- repo: `deps` `ci` `build` `xtask` `release`

Examples: `feat(webhook): add listen with a local tunnel`,
`fix(graph): map error 131056 to exit 4`, `docs(send): document --reply-to`,
`refactor(cli): route confirm prompts through Printer`.

**Splitting**: one logical change per commit. A command change and its docs page go in
the same commit. Unrelated formatting or drive-by fixes get their own commit.
Use `git add -p` to split a mixed working tree; show the user the planned split before
committing more than one.

## Before pushing

```sh
git log --format='%h %G? %(trailers:key=Signed-off-by,valueonly,separator=%x2C ) | %s' main..HEAD
git log --format=%B main..HEAD | grep -Ein 'co-authored-by|generated (with|by)|claude|anthropic' && echo "AI attribution found"
for c in $(git rev-list main..HEAD); do git log -1 --format=%B $c > /tmp/m; sh .githooks/commit-msg /tmp/m || echo "^ $c"; done
```

Every line needs a sign-off; `%G?` should be `G` (signed and good). Fix with
`git rebase --signoff -S main`. Then run the `preflight` skill.

## Pull request

- Title = a Conventional Commit subject (it becomes the squash commit release-plz
  reads). For a one-commit PR, reuse the commit subject.
- Body = `.github/pull_request_template.md` filled in: What, Why (with `Closes #N`),
  How to try it (a real command, ideally `--dry-run`), checklist ticked honestly.
- Draft the title and body in chat for the user first. Only when asked:
  `git push -u origin HEAD` then `gh pr create --title … --body-file …` (add `--draft`
  if the work isn't done). No AI footer in the body.
