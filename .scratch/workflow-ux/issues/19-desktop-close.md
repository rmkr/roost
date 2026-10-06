# Close a profile's Desktop from the picker or `--close`

Type: task
Labels: workflow-ux
Status: claimed
Assignee: Claude subagent ticket-18-19
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: 18

## Question

User decisions 2026-10-06 (Q63, Q64):

- In the Desktop picker, `x` on a `running` row closes that Desktop; the picker stays open and the row refreshes (e.g. to `signed in`). `x` on a non-running row does nothing (or a brief hint). Footer: `↑/↓, Enter launch, x close, Esc cancel`. Other pickers do not get `x`.
- `roost desktop --close NAME` does the same non-interactively; not running is a success no-op with a message. Usage errors for `--close` without NAME or with `--link`/`--unlink`/`--replace`/`--foreground`.
- Close gracefully only: send SIGTERM to the PID named by that registration's Desktop `SingletonLock` (its own Roost folder, or the conventional folder when linked; the default alias row closes the conventional instance), only after verifying the lock names this host, the PID is alive, and `/proc/PID/exe` resolves to the `claude-desktop` binary resolved through PATH (or its real path). Wait up to 10 seconds for exit; if still running, report that and do not escalate. No force-kill.
- Never signal anything else; Linux only like the rest of Desktop.
- Amend the spec's Desktop section and A18. Tests use the fake `claude-desktop` long-running mode (it must exit on SIGTERM; add a mode that ignores SIGTERM to test the timeout path).

## Comments

- 2026-10-06 (Claude subagent ticket-18-19): Implemented on branch `ticket/18-19-marker-desktop-close` (commits 967de5f, 2a90d0b). `roost desktop --close NAME` (clap: requires NAME, conflicts with --foreground/--link/--unlink/--replace/--yes → usage). Lock chosen: own Roost folder, or the conventional folder for a linked registration or default alias. No live lock → success "Claude Desktop is not running for NAME". Otherwise (store dropped first) `pidfd_open(PID)` pins the process, then verification: `/proc/PID/exe` == canonical resolved `claude-desktop`, OR the resolved program is a `#!` script whose canonical interpreter == `/proc/PID/exe` and the kernel-inserted script argument in `/proc/PID/cmdline` (argv[1], or argv[2] with a shebang argument) canonicalizes to the program. This second form is exactly how the kernel runs a script, so the Python fake is verified honestly; the real `/usr/bin/claude-desktop` is a symlink to the ELF `/usr/lib/claude-desktop/claude-desktop`, matched by the first form. Failure → `ownership` ("…names process PID, which is not Claude Desktop…; nothing was signalled"). SIGTERM via `pidfd_send_signal` only; poll the pidfd up to 10 s → "Closed Claude Desktop for NAME"; timeout → `desktop_running` "still running after 10 seconds…", never SIGKILL. Picker: `platform::pick_with(title, hint, header, rows, initial, actions)` returns `Picked::{Chosen, Action(key,row)}`; `pick` wraps it unchanged for switch/bare (letters other than k/j/q still ignored). `select::choose_with(.., Some(Actions{hint, keys, initial}), ..)` returns `Choice::{Chosen, Action}`; `choose` unchanged for callers. Desktop picker loops: on `x` it re-reads the store (Read), revalidates the listed registration, closes or reports, prints the outcome above the list and re-shows on the same row. Hint is on the title line like other pickers: `(↑/↓, Enter launch, x close, Esc cancel)`. Spec Desktop section (picker + new Closing bullet) and A18 amended. Tests: fake handles SIGTERM (removes lock, exits), `FAKE_DESKTOP_TERM=ignore`, `FAKE_DESKTOP_SECONDS`; `pty_session` gained `WAIT:TEXT` steps. Not resolved: awaiting review.
