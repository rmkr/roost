# Share instruction fragments through sets

Type: task
Labels: workflow-ux
Status: resolved
Assignee: Claude subagent ticket-11
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: 01, 05, 06

## Question

Implement [design: shared instructions](../design.md#shared-instructions): instruction items in sets (explicit fragments and whole-directory entries), linked into owned profiles' `rules/` and reconciled at launch with the same machinery as skills. If [Investigate injected plugin behavior](06-injected-plugin-behavior.md) finds that config-dir `rules/` does not load like `CLAUDE.md`, implement the delimited `@import` block fallback instead and record that in the design. Add instruction items to the Sets column and `--json` set data.

## Comments

- 2026-10-06 (Claude subagent ticket-11): Implemented on branch `ticket/11-shared-instructions`. `Instruction`/`InstructionSource` join `sets::LINKED`, so instruction items reconcile into owned profiles' `rules/` with the skill machinery (recording, existing-content-wins, subscribe-time conflict refusal, launch-time conflict skip, recorded-only removal, purge without following). Source expansion takes immediate regular `*.md` files only, skipping dot-names, links, directories and `synced`. A profile's `CLAUDE.md` is never touched. The delimited `@import` block fallback was not needed and was not built: the [research addendum](../research/injected-plugins.md#addendum-user-level-rules-and-path-imports-orchestrator-question) found config-dir `rules/` is the per-profile equivalent of `~/.claude/rules/`; recorded in [design](../design.md#shared-instructions) and the spec. `set list` (text and `--json`) already shows `instruction`/`instruction_source` items; `ls` Sets lists subscribed set names. Tests in `tests/sets.rs`: explicit and whole-directory fragments, conflict skip against an existing same-named rule, unsubscribe removal, same-name subscribe refusal, private `rules/` creation, upstream/alias get nothing, purge unlinks without following.

## Answer

2026-10-06: Merged into `workflow-ux` (branch head e50d623). Instruction and instruction-source items are linked into owned profiles' `rules/` by the same reconciliation as skills. The `@import` fallback was not needed: config-dir `rules/` loads like `~/.claude/rules/` (see the research addendum). Known issue carried to the review fix pass: linking refuses group-writable `skills/`/`rules/` folders, which Claude creates under a 002 umask.
