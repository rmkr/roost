# Raise the minimum Claude version to 2.1.280

Type: task
Labels: workflow-ux
Status: resolved
Assignee: Claude subagent ticket-02
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: none

## Question

Change the minimum supported Claude version from 2.1.268 to 2.1.280 in `src/launch.rs`, fake-Claude fixtures, tests and docs. Unsupported-version errors keep their existing code and guidance.

## Comments

- 2026-10-06 (Claude subagent ticket-02): Raised the floor to 2.1.280 on branch `ticket/02-raise-claude-floor`. `parse_version` in `src/launch.rs` now rejects anything below (2, 1, 280); `claude_unsupported` keeps its code and the guidance now reads "Use Claude Code 2.1.280 or later with supported native status". Unit test `version_floor_is_exact` accepts 2.1.280 and rejects 2.1.279 and 2.1.268. The fake Claude in `tests/cli.rs` defaults to 2.1.280, and the refusal test uses 2.1.279 and also checks the guidance text. Updated README, CLAUDE.md and the version number in `.scratch/rust-port/spec.md` (only the number; the "status `configDirectory`" rationale wording around it is unchanged). Historical research notes under `.scratch/rust-port/research/` and `issues/06` still say 2.1.268 on purpose. `cargo test`, clippy `-D warnings` and fmt check all pass.

## Answer

2026-10-06: Merged into `workflow-ux` at cf0b71e. The minimum supported Claude version is 2.1.280 in `src/launch.rs`, the fake Claude fixtures, README, CLAUDE.md and the spec.
