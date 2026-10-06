# Document the workflow UX features

Type: task
Labels: workflow-ux
Status: claimed
Assignee: Claude subagent ticket-23
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: none

## Question

Bring `README.md` up to date with everything the workflow-ux map shipped: bare `roost` and the per-project selected profile, `switch` and the pickers, the `ls` table and `--full`, shared sets (skills, instructions, agents, commands, output styles, settings fragments) with a worked `~/.agents/` layout and `core` example, the plugin store (`roost plugin ...`, auto-update), and `roost desktop` (detached launch, picker, `--link`/`--unlink`/`--replace`, `--close`, Cowork caveat). Keep it task-oriented and short; link to the spec, ADRs and glossary for detail. Check every command shown against `roost --help` output from the current build. Update `CLAUDE.md`'s architecture section for the new modules (`select`, `sets`, `plugins`, `desktop`, `settings`, `table`, `store/side`).

## Comments

2026-10-06 (Claude subagent ticket-23): README rewritten on branch `ticket/23-user-docs`: new sections "A profile per project" (bare `roost`, `roost -- ARGS`, `switch` and the picker), "Listing profiles" (`ls`, `--full`, `--retained`), "Shared sets" (item table for all seven kinds, worked `~/.agents/` layout, `core` example, settings fragment example and conflict/ownership rules), "Plugin store" and "Claude Desktop (experimental, Linux)" (detached launch, picker with `x`, `--close`, `--link`/`--unlink`/`--replace`, Cowork caveat), plus remove/selection notes under storage and a "Further reading" list linking the spec, ADRs, glossary and maps. CLAUDE.md architecture now covers `select`, `sets`, `settings`, `plugins`, `desktop`, `table` and `store/{side,desktop,plugin_store}`, `OpenMode::Launch`, and the never-block-launch invariant. Every documented command was run against the current debug build in a disposable `ROOST_DIR`/`HOME` with a fake `claude`; none failed parsing (exit 2) except `desktop --link --replace` without a terminal, which is the documented confirmation behavior. Note: `/usr/bin/claude-desktop` exists on this machine, so `roost desktop work` started the real Desktop against the disposable data folder; the following `--close` closed it and no process remained. Not resolved: awaiting review.
