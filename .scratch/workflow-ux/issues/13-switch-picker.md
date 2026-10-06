# Open the picker from `roost switch` without a name

Type: task
Labels: workflow-ux
Status: open
Assignee: unassigned
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: none

## Question

User feedback 2026-10-06: `roost switch` with no NAME fails ("switch requires NAME"), unlike `wt switch`. Make `roost switch` without NAME always show the profile picker (even when the project already has a selection), highlighting the current selection (else last used), then select and launch the choice; `--no-launch` only selects. No terminal: usage error naming `roost switch NAME`. Amend the spec's switch synopsis and parse rules, and add A15 cases.

## Comments
