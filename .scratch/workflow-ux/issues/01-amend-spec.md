# Amend the specification for workflow UX

Type: task
Labels: workflow-ux
Status: open
Assignee: unassigned
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: none

## Question

Update the [Roost specification](../../rust-port/spec.md) for every item under [Spec changes](../design.md#spec-changes): bare `roost`, new commands and their errors/exit codes, launch-time mutation and its recovery/lock behavior, `CLAUDE_CODE_PLUGIN_DIRS` injection, the 2.1.280 floor, the new manager storage with remove/purge rules, and added `list` JSON fields. Keep the existing ownership invariants: nothing is written into registered upstream or default data. Get the user's acceptance of the amended sections before implementation tickets start.

## Comments
