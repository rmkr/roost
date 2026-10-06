# Split the user's global CLAUDE.md into instruction fragments

Type: task
Labels: workflow-ux
Status: claimed
Assignee: Claude (orchestrator, with the user)
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: 11

## Question

With the user present: split `~/.claude/CLAUDE.md` into one-topic fragments in a user-managed directory (for example `~/.agents/instructions/`), create a default set (for example `core`) containing them, subscribe the owned profiles the user chooses, and rewrite `~/.claude/CLAUDE.md` as imports of the same fragments. Confirm in a launched profile that each instruction loads once.

## Comments

### 2026-10-06: split done, load check pending

Backed up the original to `~/.claude/CLAUDE.md.pre-roost-split`. Fragments `~/.agents/instructions/git-commits.md` and `subagents.md` hold every original line (section headings became fragment titles). `core` gained `--instructions-from ~/.agents/instructions`; `personal` and `work` each link both into `rules/` (0700). `~/.claude/CLAUDE.md` is now an intro plus `@~/.agents/instructions/*.md` imports for plain `claude`. Remaining: in a new Roost session under `$HOME`, check `/memory` to confirm each fragment loads once (ancestor `~/.claude/CLAUDE.md` imports versus linked `rules/` files; deduplication was only inferred from Claude's source).
