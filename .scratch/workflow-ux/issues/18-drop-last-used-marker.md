# Drop the `^` last-used marker

Type: task
Labels: workflow-ux
Status: claimed
Assignee: Claude subagent ticket-18-19
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: none

## Question

User decision 2026-10-06 (Q62): remove the `^` most-recently-launched marker from `roost ls` and every picker; keep `@` (selected for this project). The legend becomes `@ selected here`. Pickers keep their starting highlight rules; `ls` keeps the Last used column. JSON `most_recent` stays (data, not display). Amend the spec's `roost ls` and picker text.

## Comments

- 2026-10-06 (Claude subagent ticket-18-19): Implemented on branch `ticket/18-19-marker-desktop-close` (commit 74042b6). `table::profile_table` marker is `@` or space; summary legend `@ selected here`. Pickers keep their highlight rules (selection, else `most_recent`; Desktop: last Desktop launch, else `most_recent`); JSON `most_recent` unchanged. Spec `roost ls` and picker text amended. Tests: ls layout/legend expectations updated; Desktop picker rows assert no `^`. `.scratch/workflow-ux/design.md` still shows the old legend in its example (historical design, left as is). Not resolved: awaiting review.
