# Share settings fragments by writing owned profiles' settings.json

Type: task
Labels: workflow-ux
Status: claimed
Assignee: Claude subagent ticket-21
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: none

## Question

Implement [design: shared settings](../design.md#shared-settings) and [ADR 0003](../../../docs/adr/0003-shared-settings-written-into-owned-profiles.md): `--setting FILE` / `--settings-from DIR` set items (immediate `*.json`, skipping dot-names and links; refuse missing sources and keys other than `hooks`, `statusLine`, `outputStyle`); subscribe-time refusal when two fragments set the same single-value key; launch-time reconciliation into owned profiles' `settings.json` (record Roost-written hook handlers and keys per registration in `state.json`; write only on change; remove only recorded, unchanged entries; skip identical existing handlers; re-add hand-removed Roost hooks; back off `statusLine`/`outputStyle` the profile set itself; atomic compare-and-replace preserving key order and 2-space style; concurrent change → warn, retry next launch; invalid fragment → warn, skip). Unsubscribe removes Roost's entries at the next launch. Never write upstream or alias data. Desktop launches reconcile too. Amend the spec (set grammar, shared sets, state.json schema, A16).

## Comments

2026-10-06 (Claude subagent ticket-21): Implemented on branch `ticket/21-shared-settings`.

- New `src/settings.rs`: fragment validation, merge and compare-and-replace of `settings.json`; item kinds `setting`/`setting_source` (`--setting FILE`, `--settings-from DIR`); `state.json` gains optional `settings` records (omitted while empty, so existing documents read unchanged). Called from `sets::reconcile_at_launch`, so run, launchers, switch, bare `roost` and `desktop` all reconcile. Spec amended (grammar, `set list` ItemKind, state schema, Shared sets "Shared settings", launch-time mutation, remove/purge table, A16).
- Key order: serde_json `preserve_order` was not used. `indexmap` is in the offline cache but not in `Cargo.lock`, and the feature would reorder every `--json` envelope in the crate. Instead `settings::Json` is a small order-preserving document (custom `Deserialize`, refuses duplicate keys) with a 2-space writer.
- Choices for review: (1) a `statusLine`/`outputStyle` that Roost recorded and the user deleted by hand is re-added, like hooks (only a different value counts as the profile's own); (2) a recorded hook handler the user edited becomes theirs and the set's handler is added again beside it; (3) a rewritten `settings.json` is mode 0600 (the profile directory is already private); (4) a new `settings.json` has no trailing newline, an existing one keeps whether it had one; (5) numbers are re-emitted through serde_json (`1.0` style floats that JavaScript would print as `1` could change form on a rewrite).
- Tests: `tests/settings.rs` (7 integration tests) plus unit tests for compare-and-replace, rendering and old-format `state.json`.
