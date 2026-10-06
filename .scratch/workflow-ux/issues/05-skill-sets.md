# Add shared sets with linked skills

Type: task
Labels: workflow-ux
Status: claimed
Assignee: Claude subagent ticket-05
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: 01, 10

## Question

Implement the skill half of [design: shared sets](../design.md#shared-sets): `roost set` commands to create sets, add explicit items or whole-source entries, mark default, and subscribe/unsubscribe profiles; default-set subscription on `add` and copied subscriptions on `add --copy`; launch-time reconciliation of recorded links in owned profiles only; conflict rules; never touching `skills/synced`. Links are created through the existing `Directory` handles with identity checks, and purge must unlink without following them. Add the Sets column to `ls`.

## Comments

2026-10-06 (Claude subagent ticket-05): Implemented on branch `ticket/05-skill-sets` (merged with workflow-ux after ticket 03).
- `roost set list|create|delete|default|add|drop|subscribe|unsubscribe` per the spec grammar in `src/sets.rs` (CLI in `cli::SetAction`/`ItemArgs`). All five item kinds are stored in `sets.json`; `--plugin` validates the ID and is refused not_found until the plugin store exists (`sets::plugin_installed`, ticket 07).
- `add` subscribes owned profiles to default sets (`--no-sets` skips; conflicts with `--link-default`); `add --copy-default` from an owned profile's directory copies its subscriptions. `Store::add` now admits a copy source that is exactly an owned registration's verified directory (it previously failed the root-overlap check, which made the spec's copy-from-owned case impossible).
- `run` and profile launchers open `OpenMode::Launch` and call `sets::reconcile_at_launch` after `launch::prepare`: owned active only; only kinds in `sets::LINKED` (skills) are linked; conflicts, existing content, replaced records and side-file errors warn and the launch proceeds. Only recorded links that are still the recorded symlink (identity and text) are removed; `synced` is never linked or touched.
- `remove`/`purge` call `sets::forget_removed` after commit (retained owned profiles keep subscriptions and link records; purge unlinks through `purge_children`).
- `ls`: the `sets` placeholder is filled by `sets::annotate`; Sets column after Launchers.
- Tests: `tests/sets.rs` (A16 cases, except instruction linking, which is ticket 11).
- Extension: ticket 11 adds `ItemKind::Instruction`/`InstructionSource` to `sets::LINKED` (placement `rules/` and `.md` expansion already exist in `PLACEMENTS`); ticket 07 replaces `plugin_installed` and uses `sets::plugin_items(&SetsFile, registration_id)` for injection.
