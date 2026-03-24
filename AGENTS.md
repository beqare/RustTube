# Repository Rules

## Git Safety

- Never switch branches unless the user explicitly asks for it.
- Never run `git checkout`, `git switch`, `git reset`, `git restore`, `git revert`, `git rebase`, or `git cherry-pick` unless the user explicitly asks for it.
- Never create commits, amend commits, push, pull, merge, or force-push unless the user explicitly asks for it.
- Treat all existing uncommitted changes as user-owned unless clearly created in the current task.
- Before any potentially destructive Git action, stop and ask the user.

## Workflow

- Prefer read-only Git commands when inspecting repository state.
- When deployment or server configuration is involved, explain the likely impact before changing workflow files.
