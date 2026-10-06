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

- 2026-10-06 (Claude subagent ticket-10): Scaffold implemented on branch `ticket/10-shared-scaffold`, not yet resolved.
  - Table: `src/table.rs` `Table<R>` with `.marker(fn)`, `.column(header, fn(&R)->Cell)`, `.summary(text)`, `.render(rows, color)`; `Cell`/`Style`; `color_enabled`/`stdout_color` (terminal + `NO_COLOR`); `display_width`; `table::profiles(records, color)` is the `list` table that 03/04/05 append columns to. Width-based column hiding is left to 03.
  - Side files: `src/store/side.rs` (`pub mod side` of store). Schema types `StateFile`, `SetsFile`, `Selection`, `Timestamp`, `LinkRecord`, `SharedSet`, `Item`, `ItemKind`, `Subscription` follow the amended spec exactly. `Store::read_state`/`read_sets` (any mode, absent means empty, never create; unknown registration IDs filtered out) and `Store::update_state`/`update_sets(|doc| ...)` (read-modify-write, under the held lock, no journal, refused in Read mode or with a pending intent, validation failures are `ownership`, unknown registrations dropped on write). Replacement goes through an exclusive `.roost-state-<ID>.tmp` with the destination identity re-verified before rename; private leftover temps are removed by the next write and others are left in place. `Store::reserved_dir(PLUGIN_STORE | DESKTOP, create)` opens or lazily creates (0700) the reserved directories.
  - Root validation admits `state.json`, `sets.json`, `.roost-state-<32hex>.tmp` (regular files) and `plugin-store/`, `desktop/` (real directories). The same names with the wrong object type or a link are refused.
  - New `OpenMode::Launch`: behaves like Read (no recovery and no journaled mutation) but permits side-file writes. Run/Internal still open in Read mode; 04 switches them over.
  - Launch: `launch::profile_env(store, reg, allow_auth_env) -> ProfileEnv` holds the validation and env part of `prepare`. `ProfileEnv::{is_isolated, set, append_paths, apply, command}`, where `append_paths` builds `CLAUDE_CODE_PLUGIN_DIRS` after the inherited nonempty value and is a no-op for aliases. `launch::resolve_program(name)` replaces `resolve_claude`, and `prepare` = `profile_env` + `resolve_program("claude")`.
  - `Directory::symlink(target, name) -> FileIdentity` / `read_link(name) -> Option<PathBuf>` (descriptor-relative, no-follow) have fail-closed Windows stubs.
  - Not done: the journaled `store_create`/`desktop_create` operations and the `store`/`desktop_data` roles are left to tickets 07 and 09.
