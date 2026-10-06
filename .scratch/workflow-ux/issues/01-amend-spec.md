# Amend the specification for workflow UX

Type: task
Labels: workflow-ux
Status: claimed
Assignee: Claude subagent ticket-01
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: none

## Question

Update the [Roost specification](../../rust-port/spec.md) for every item under [Spec changes](../design.md#spec-changes): bare `roost`, new commands and their errors/exit codes, launch-time mutation and its recovery/lock behavior, `CLAUDE_CODE_PLUGIN_DIRS` injection, the 2.1.280 floor, the new manager storage with remove/purge rules, and added `list` JSON fields. Keep the existing ownership invariants: nothing is written into registered upstream or default data. Get the user's acceptance of the amended sections before implementation tickets start.

## Comments

- 2026-10-06 (Claude subagent ticket-01): Amended [the specification](../../rust-port/spec.md). In place: synopsis and parse rules (bare `roost`, `roost -- args`, `switch`, `set ...`, `plugin ...`, `desktop`, `list --full`, `add --no-sets`), version floor wording (2.1.280 for `CLAUDE_CODE_PLUGIN_DIRS`), root layout (`state.json`, `sets.json`, `plugin-store/`, `desktop/<REGISTRATION_ID>/`), journal operations `store_create`/`desktop_create` and roles `store`/`desktop_data`, lock exception for store commands, isolated-launch env adds `CLAUDE_CODE_PLUGIN_DIRS`, list/set list/plugin list JSON and ProfileRecord fields (`sets`, `last_used`, `selected`, `most_recent`, `probe`), error categories `plugin_store` and `desktop_running`, exit-code rows, acceptance gates A15–A18, out-of-scope wording. New section "Workflow UX amendments (2026-10-06)": state files, selection/picker, `ls` table, shared sets incl. instruction fragments (rules/ vs `@import` fallback pending ticket 06), launch-time mutation, plugin store, Desktop, remove/purge table. Open implementation choices are listed in the handoff report for the user's approval; not resolved until the user accepts.
