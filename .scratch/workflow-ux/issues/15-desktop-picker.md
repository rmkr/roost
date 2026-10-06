# Open a profile picker from `roost desktop` without a name

Type: task
Labels: workflow-ux
Status: open
Assignee: unassigned
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: none

## Question

User request 2026-10-06 (Q56/Q57): `roost desktop [--foreground]` with no NAME always shows the profile picker and launches the choice exactly as `roost desktop NAME` would. Desktop is machine-wide, so the picker highlights the profile whose Desktop was launched most recently (`desktop_launched`), else the most recently launched profile, else the first row, and the choice is never recorded as a project selection. Picker rows add a Desktop column: `running` (that registration's Desktop folder holds a live `SingletonLock`), `signed in` (a Desktop folder exists), `—` (never launched); the default alias row is plain Desktop. Choosing a running profile fails with the existing desktop_running message. No terminal: usage error naming `roost desktop NAME`; cancel exits 130. Amend the spec's desktop synopsis, Desktop section and A18.

## Comments
