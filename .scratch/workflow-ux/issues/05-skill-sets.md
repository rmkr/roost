# Add shared sets with linked skills

Type: task
Labels: workflow-ux
Status: open
Assignee: unassigned
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: 01

## Question

Implement the skill half of [design: shared sets](../design.md#shared-sets): `roost set` commands to create sets, add explicit items or whole-source entries, mark default, and subscribe/unsubscribe profiles; default-set subscription on `add` and copied subscriptions on `add --copy`; launch-time reconciliation of recorded links in owned profiles only; conflict rules; never touching `skills/synced`. Links are created through the existing `Directory` handles with identity checks, and purge must unlink without following them. Add the Sets column to `ls`.

## Comments
