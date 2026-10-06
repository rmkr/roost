# Share instruction fragments through sets

Type: task
Labels: workflow-ux
Status: open
Assignee: unassigned
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: 01, 05, 06

## Question

Implement [design: shared instructions](../design.md#shared-instructions): instruction items in sets (explicit fragments and whole-directory entries), linked into owned profiles' `rules/` and reconciled at launch with the same machinery as skills. If [Investigate injected plugin behavior](06-injected-plugin-behavior.md) finds that config-dir `rules/` does not load like `CLAUDE.md`, implement the delimited `@import` block fallback instead and record that in the design. Add instruction items to the Sets column and `--json` set data.

## Comments
