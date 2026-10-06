# Color the profile picker like `roost ls`

Type: task
Labels: workflow-ux
Status: resolved
Assignee: Claude subagent ticket-13-14
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: none

## Question

User feedback 2026-10-06: `roost ls` colors names (bold) and launcher state, but the picker rows shown by bare `roost`/`switch` are plain. Render picker rows with the same table styles (terminal-only, honoring `NO_COLOR`), keeping the highlighted row readable. Confirm the intended direction with the user if unclear.

## Comments

2026-10-06 (Claude subagent ticket-13-14): Implemented on branch `ticket/13-14-switch-picker`. Picker rows now come from `table::profiles` with `table::stderr_color()` (stderr is a terminal and `NO_COLOR` unset/empty), so names are bold and launcher state colored like `roost ls`. The highlighted row (`picker_frame` in src/platform/unix.rs) re-applies reverse video after every embedded reset so the whole row stays highlighted and readable; it ends with a reset. Spec Picker paragraph and A15 amended. Tests: unit `platform::unix::tests::picker_highlight_survives_styled_cells_and_plain_rows_stay_plain`, PTY `picker_rows_use_list_styles_on_a_terminal_unless_no_color`. Not resolved; awaiting user confirmation of the look.

## Answer

2026-10-06: Merged into `workflow-ux` (e0acea1). Picker rows use the `ls` styles on a terminal (stderr) honoring `NO_COLOR`, with reverse video re-applied after each style reset on the highlighted row. The user confirmed the colors look good.
