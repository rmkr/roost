# Move the personal profile's hand-made skill links onto a set

Type: task
Labels: workflow-ux
Status: open
Assignee: unassigned
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: 05

## Question

The `personal` profile has 60 unrecorded links in `skills/` pointing at `~/.claude/skills/*`, made by hand on 2026-10-06. With the user present: remove those links, create a set covering `~/.claude/skills`, subscribe `personal`, launch, and confirm the skills (for example `grill-me`) are available. Decide with the user which set, if any, the `work` profile gets.

## Comments

### 2026-10-06: personal migrated

With the user: installed the `workflow-ux` build, deleted the 60 hand-made links, created default set `core` = `--skills-from ~/.agents/skills` plus explicit `redbuilder` (red-skills repo) and `codebase-memory` (`~/.claude/skills`), subscribed `personal`, and verified 65 links with none broken after `roost run personal -- --version`. `skills/` was tightened from 0775 to 0700 as designed. `clipsheet`, `taskboard` and `jira-taskboard` were dropped: their rmkr-skills sources no longer exist. Remaining: decide whether `work` subscribes to `core`, and which plugins join `core`.
