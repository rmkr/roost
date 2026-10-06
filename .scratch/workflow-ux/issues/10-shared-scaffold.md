# Lay shared scaffolding for the workflow UX features

Type: task
Labels: workflow-ux
Status: claimed
Assignee: Claude subagent ticket-10
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: 01

## Question

Tickets 03, 05 and 09 run in parallel and otherwise collide in the same places. Land the shared pieces first, with no user-visible behavior change beyond what the amended spec allows:

- A table renderer module with a column list API (header, cell function, optional marker column, display-width alignment, terminal-only color honoring `NO_COLOR`, summary line), used by `list` with today's columns.
- Reserved manager-storage names from the amended spec (for example `state/`, `sets/`, `plugin-store/`, `desktop/`) admitted by the root residue check and created lazily, plus a small side-file helper for atomic private writes under the lock that does not go through the registry journal and tolerates its own temp leftovers, if the amended spec chooses that path.
- `launch::prepare` split so profile environment setup can be reused (for Desktop) and extended (for `CLAUDE_CODE_PLUGIN_DIRS`), and a generalized program resolver.
- `Directory` symlink creation and link reading through descriptor-relative no-follow calls, with fail-closed Windows stubs.

Implementer notes: `~/.cache/roost-impl/notes.md` (cross-cutting section and merge-conflict hotspots).

## Comments
