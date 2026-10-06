# Move the user's hooks and status line into core

Type: task
Labels: workflow-ux
Status: resolved
Assignee: Claude (orchestrator, with the user)
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: 21

## Question

With the user present: split the 10 hooks in `~/.claude/settings.json` into fragments in `~/.agents/settings/` (atuin, cbm, herdr), move the status line script out of `~/.roost/profiles/personal/` into `~/.agents/` with a `statusline.json` fragment, add `--settings-from ~/.agents/settings` to `core`, remove the hand-added `statusLine` from `personal/settings.json` so Roost manages it, launch both profiles, and confirm with `/hooks` and the status line that everything appears once. Decide whether plain `claude` (`~/.claude/settings.json`) keeps its own copies.

## Comments

### 2026-10-06: migrated, live check pending

Plain `claude` (`~/.claude/settings.json`) left untouched, per the user. Fragments in `~/.agents/settings/`: `atuin.json` (3 Bash hooks), `cbm.json` (Grep/Glob gate, 4 SessionStart reminders, SubagentStart reminder), `herdr.json` (SessionStart state hook): 10 of 10 handlers, copied verbatim. Status line script copied to `~/.agents/statusline-command.sh` with `statusline.json`. Removed the hand-added `statusLine` from `personal/settings.json` (backup `settings.json.pre-roost-settings`). `core` gained `--settings-from ~/.agents/settings`. After `roost run NAME -- --version`, both profiles have 10 handlers and the shared status line, other keys intact, mode unchanged (0664); a repeat launch left `work/settings.json` untouched (same inode and mtime). `outputStyle` not shared yet: plain claude's effective style is `Concise` (settings.local.json) over `ELI5`; awaiting the user's choice. Remaining: `/hooks` check in a fresh session.

## Answer

2026-10-06: Done. The user confirmed with `/hooks` in a fresh Roost session: 15 hooks on 7 events, each of the 10 `[User]` hooks once, alongside the 5 hooks from the injected plannotator and ponytail plugins. Output styles, by the user's decision: no shared `outputStyle` setting (each profile chooses its own with `/config`), but style files are shared through sets, so `core` gained `--output-styles-from ~/.claude/output-styles` and both profiles link `ELI5.md`. "Concise" (plain claude's effective style) has no file anywhere, so it is built in or stale and needs no sharing. Plain `claude` was left untouched.
