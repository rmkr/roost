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

2026-10-06 (Claude subagent fix-pass-2): Review fixes on branch `fix/shared-settings-review` (items 1–11 of fix pass 2; Q78 (a) and Q79 accepted).

- New files: a `settings.json` Roost creates ends with a newline, like Claude's; an existing file keeps its own ending (supersedes choice 4).
- Numbers: `settings::Json` keeps each number's exact source text (`1e2`, `1e21`, `18446744073709551616`, `0.1000000000000000055511151231257827` round-trip unchanged). serde_json `arbitrary_precision` was not used: Cargo.lock would not change, but it changes how every `Value` in the crate compares and deserializes numbers. Instead the text is taken from a pre-scan of number tokens while serde_json still validates the document (supersedes choice 5).
- Fragment reads: non-blocking open, regular files only, at most 4 MiB + 1 bytes read; a source's children are opened without following links. A FIFO, `/dev/zero` or oversized file swapped in warns and is skipped instead of hanging or growing memory.
- Fragment dedupe is by file identity (device, inode), so a symlinked `--setting` and its real path in a `--settings-from` directory count once (no false conflict at subscribe or launch).
- Mode: a rewrite keeps the replaced file's permission bits; a new file is 0600 (supersedes choice 3).
- Crash safety: records are written ahead. Before the replace, `state.json` holds the union of the old and new records (each single-value key keeps its old recorded value, else the new one); after it, the exact record; if the replace fails, the old record is restored, and if the write-ahead save fails, `settings.json` is not written. A recorded key already holding the desired value is now adopted, so an interrupted replacement recovers. An interruption can leave only recorded entries (re-added or removed at the next launch). A launch interrupted before its replace can trigger one re-add warning at the next launch.
- The window between the unchanged check and the rename is documented in `replace` and the spec. POSIX has no compare-and-rename to close it.
- Q78 (a): re-adding a recorded hook that is missing (removed or edited by hand) warns once, naming the fragment file to edit in its fragment directory.
- Q79: the spec, ADR 0003 and the design now say that a different value is the profile's own (Roost backs off) and that absence means the value is re-added. A test now covers re-adding a hand-deleted `statusLine`.
- Refactors: linked kinds come from `PLACEMENTS`, and every path-valued kind's source type is set in one `source_kind` next to it. Settings reuse `sets::source_present`/`source_type`, and `ItemKind::is_setting` replaces `settings::is_setting`. There is one `type_of` for handler/statusLine types, and `group_matcher` is reused by parsing. `statusLine`/`outputStyle` are indexed `SINGLE_KEYS`. Renames: `Hook::matches_record`, `MAX_READ_BYTES`, `parse_fragment`/`parse_hooks`.
- Item 11 (concurrent-change refusal through a real launch) was not added. Nothing runs between Roost's snapshot and its re-check except Roost's own `state.json` write: the fake claude runs after reconciliation, and a pre-launch modification only changes what is read. The only ways to test it would be a test-only hook in production code or a timing race, which would be flaky. The refusal stays covered at the `replace` seam by `replace_refuses_a_settings_file_changed_since_read`.
- Tests: four new integration tests in `tests/settings.rs` (link dedupe, mode, interrupted launch recovery, number text) and two new unit tests (FIFO/device/oversized fragment reads, plus number text added to rendering). Existing tests were extended for the trailing newline, re-add warning and `statusLine` re-add.
