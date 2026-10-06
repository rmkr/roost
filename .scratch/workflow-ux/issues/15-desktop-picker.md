# Open a profile picker from `roost desktop` without a name

Type: task
Labels: workflow-ux
Status: claimed
Assignee: Claude subagent ticket-15
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: none

## Question

User request 2026-10-06 (Q56/Q57): `roost desktop [--foreground]` with no NAME always shows the profile picker and launches the choice exactly as `roost desktop NAME` would. Desktop is machine-wide, so the picker highlights the profile whose Desktop was launched most recently (`desktop_launched`), else the most recently launched profile, else the first row, and the choice is never recorded as a project selection. Picker rows add a Desktop column: `running` (that registration's Desktop folder holds a live `SingletonLock`), `signed in` (a Desktop folder exists), `—` (never launched); the default alias row is plain Desktop. Choosing a running profile fails with the existing desktop_running message. No terminal: usage error naming `roost desktop NAME`; cancel exits 130. Amend the spec's desktop synopsis, Desktop section and A18.

## Comments

2026-10-06 (Claude subagent ticket-15): Implemented on branch `ticket/15-desktop-picker`. `roost desktop [--foreground] [NAME]`: NAME is optional and desktop now refuses any `--`. Without NAME, `desktop::run` checks for a terminal before reading storage (usage error, next step `roost desktop NAME`). It then uses the new shared `select::choose` flow, which `switch` and bare `roost` also use now: active check, terminal check, lock released while picking, then reacquire and re-verify root ID and registration. The picker title is a parameter of `platform::pick` (Desktop shows "Choose a profile for Claude Desktop"). Rows come from `table::desktop_profiles`, which is the ls table plus a Desktop column: `running`, `signed in`, `—`, or `plain` for a default alias. The column is filled from `Store::desktop_locks` and `live_lock`. The highlight goes to the most recent `desktop_launched`, else `most_recent`, else row 0. The choice goes through the same `start` path as `desktop NAME`, so a running profile fails with desktop_running and no selection is recorded. Spec synopsis, CLI rules, Desktop section and A18 are amended. Tests: a parse unit test and four PTY/no-terminal integration tests in `tests/cli.rs`. Not resolved yet; waiting for review.
