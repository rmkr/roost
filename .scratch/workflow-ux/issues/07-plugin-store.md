# Build the plugin store and `roost plugin` commands

Type: task
Labels: workflow-ux
Status: open
Assignee: unassigned
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: 02, 05, 06

## Question

Implement [design: plugin store](../design.md#plugin-store) using the findings from [Investigate injected plugin behavior](06-injected-plugin-behavior.md): the store directory, `plugin marketplace add`, `plugin add` with set prompt/`--set`/`--no-set`, `plugin update` and the opt-in daily launch-time update, `plugin remove`, and `CLAUDE_CODE_PLUGIN_DIRS` injection for subscribed owned and upstream profiles. All store writes go through Claude's CLI under the Roost lock; output from Claude stays uncaptured or allowlisted.

## Comments
