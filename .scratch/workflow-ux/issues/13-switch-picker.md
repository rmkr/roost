# Open the picker from `roost switch` without a name

Type: task
Labels: workflow-ux
Status: resolved
Assignee: Claude subagent ticket-13-14
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: none

## Question

User feedback 2026-10-06: `roost switch` with no NAME fails ("switch requires NAME"), unlike `wt switch`. Make `roost switch` without NAME always show the profile picker (even when the project already has a selection), highlighting the current selection (else last used), then select and launch the choice; `--no-launch` only selects. No terminal: usage error naming `roost switch NAME`. Amend the spec's switch synopsis and parse rules, and add A15 cases.

## Comments

2026-10-06 (Claude subagent ticket-13-14): Implemented on branch `ticket/13-14-switch-picker`. `roost switch [--allow-auth-env|--no-launch]` without NAME always shows the picker (src/select.rs `switch` → `pick`), highlighting the valid selection, else the most recently launched profile, else the first row; the choice is recorded and launched (or only recorded with `--no-launch`, where a recording failure is an error as for `switch --no-launch NAME`). The tail without NAME starts after `--` (`switch -- --resume`). The terminal check runs before storage is read, so no terminal is usage (exit 2) naming `roost switch NAME` and `roost run NAME`; cancel is 130. Spec synopsis, parse rules, Selected profile section and A15 amended. Tests: `cli::tests::switch_name_is_optional_and_a_separator_starts_the_tail`, PTY tests `switch_without_name_opens_the_picker_on_the_current_selection_and_launches`, `switch_picker_cancel_exits_130_and_keeps_the_selection`, `switch_without_name_and_terminal_is_usage_naming_switch_name`. Not resolved; awaiting review.

## Answer

2026-10-06: Merged into `workflow-ux` (e0acea1) and installed. `roost switch [--no-launch] [-- ARGS]` without NAME always opens the picker, highlighting this project's selection, else the most recently launched profile, then records and launches the choice. No terminal is a usage error naming `roost switch NAME`; cancel exits 130 and keeps the selection. Spec synopsis, parse rules and A15 amended.
