# Render `roost ls` as a worktrunk-style table

Type: task
Labels: workflow-ux
Status: open
Assignee: unassigned
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: 01, 10

## Question

Implement the table and `--full` from [design: roost ls](../design.md#roost-ls): aligned columns, color only on a terminal, summary line naming hidden columns, plain output when piped, `--json` unchanged apart from spec-approved fields. `--full` runs bounded, allowlisted status probes in parallel. Leave the `@`/`^` markers, Last used and Sets columns to their own tickets, and design the renderer so those columns slot in.

## Comments
