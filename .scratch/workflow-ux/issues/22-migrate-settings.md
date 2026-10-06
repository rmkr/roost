# Move the user's hooks and status line into core

Type: task
Labels: workflow-ux
Status: open
Assignee: unassigned
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: 21

## Question

With the user present: split the 10 hooks in `~/.claude/settings.json` into fragments in `~/.agents/settings/` (atuin, cbm, herdr), move the status line script out of `~/.roost/profiles/personal/` into `~/.agents/` with a `statusline.json` fragment, add `--settings-from ~/.agents/settings` to `core`, remove the hand-added `statusLine` from `personal/settings.json` so Roost manages it, launch both profiles, and confirm with `/hooks` and the status line that everything appears once. Decide whether plain `claude` (`~/.claude/settings.json`) keeps its own copies.

## Comments
