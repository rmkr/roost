# Remember the selected profile per project and add `roost switch`

Type: task
Labels: workflow-ux
Status: resolved
Assignee: Claude subagent ticket-04
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: 01, 03, 10

## Question

Implement [design: selected profile and switching](../design.md#selected-profile-and-switching): project key from the git common directory, bare `roost` launch, arrow-key picker with last-used highlight, non-terminal failure, `switch` with `--no-launch`/`--forget`, default-alias selection, last-used recording from run/switch/launchers, stale-selection handling, and clearing on remove/purge. Add the `@`/`^` markers and Last used column to `ls`.

## Comments

2026-10-06 (Claude subagent ticket-04): Implemented on branch `ticket/04-selected-profile`, not yet resolved.
- `src/select.rs`: project key (walks to `.git`, `gitdir:`/`commondir`, fallback to the current directory with a warning, unsafe_path for non-Unicode/CRLF keys), bare `roost` (`[--allow-auth-env] [-- tail]`), `switch` (`--no-launch`, `--forget`), picker flow (lock released while waiting, then reacquire, revalidate, record), last-use recording (`launched`, called from Run/Internal next to `sets::reconcile_at_launch`; aliases record nothing), `annotate` for `last_used`/`selected`/`most_recent`, and `forget_removed` (Remove arm).
- `platform::pick` (unix raw mode on stdin, drawn on stderr; Up/Down/k/j/Enter; Esc/q/Ctrl-C/Ctrl-D/EOF return 130) plus a fail-closed Windows stub.
- `ls`: `@`/`^` marker, Last used column, legend in the summary; list JSON `project` filled. Doctor records are annotated as well.
- `Store::read_state_unfiltered` added to `store::side` so a stale selection that names a dropped registration can still be reported.
- Interpretation: `switch NAME` (launching) warns and still launches when the selection write fails (Launch-time mutation, A15); `switch --no-launch` and `--forget` fail with the side file's error category.
- Test note: this machine has an empty `/tmp/.git`, so every temp directory shares one project key; CLI tests give each project its own `.git` (`repo()` helper).

## Answer

2026-10-06: Merged into `workflow-ux` (branch head a6570f2). `src/select.rs` adds the git-common-dir project key, bare `roost` and `roost -- args`, `switch` with `--no-launch`/`--forget`, the arrow-key picker (`platform::pick`), last-use recording from run/switch/bare/launchers, stale-selection handling, clearing on remove/purge, and the `@`/`^` markers, Last used column and JSON fields in `ls`. A failed selection write warns and still launches for `switch NAME`, but fails `--no-launch`/`--forget`.
