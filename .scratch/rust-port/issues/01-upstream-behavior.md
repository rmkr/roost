# Establish the upstream behavior baseline

Type: research
Labels: wayfinder:research
Status: resolved
Assignee: codex/upstream-research
Parent: [Find the way to a Rust Claude profile manager](../map.md)
Blocked by: none

## Question

At a pinned upstream revision, what observable commands, profile formats, generated launchers, authentication handling, update behavior, PATH changes, and removal semantics define equivalent capabilities for this port?

Trace the implementation and relevant tests, including platform differences and failure cases. Distinguish advertised behavior from source behavior and tests actually run. Identify source reuse attribution requirements and gaps that require later decisions. Save a source-cited report at `../research/upstream-behavior.md` on the throwaway branch `research/upstream-behavior`; investigate in disposable directories.

## Answer

### 2026-10-05: research resolution

The [upstream behavior baseline](../research/upstream-behavior.md) inventories the CLI, disk layout, launchers, and platform behavior at upstream `12909e6fc76cfc3b36861c2b33f998585f2690f8` (v0.1.2). It identifies coexistence and safety choices around inherited authentication, linked launchers, copy recovery, token permissions, launcher ownership, and argument handling. Equivalent capabilities are now enumerated; which behavior the Rust port changes remains a human decision.

Evidence: inspected runtime source and tests; on Linux, the upstream suite passed 47 tests with 5 platform skips. Six disposable fake-Claude probes reproduced the specified edge cases. Real Claude authentication and Windows/macOS integration remain unverified. The report records MIT attribution for reused material.

Research context: local branch `research/upstream-behavior`, commit `7a611de617c334a090b41748f5df119dec2acac8`; the linked report is also present in this checkout.
