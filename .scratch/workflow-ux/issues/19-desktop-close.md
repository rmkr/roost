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
