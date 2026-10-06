# Find the way to a Rust Claude profile manager

Labels: wayfinder:map
Status: resolved

## Destination

An implementation-ready specification for a Rust CLI version of CarlosTheory/claude-multi-account, supporting Linux, macOS, and Windows. The map ends when its scope, behavior, compatibility contract, distribution approach, and acceptance criteria are settled and captured in `spec.md`.

Completed on 2026-10-06: the user accepted the reviewed [Roost specification](spec.md). Implementation and deferred runtime/platform testing follow as separate work.

## Notes

- Confirmed with the user on 2026-10-05: a CLI; Linux, macOS, and Windows in the first release; equivalent upstream capabilities with targeted improvements; safe coexistence with existing upstream profiles.
- Tracking uses [local Markdown operations](../README.md). This effort plans the port; implementation follows after the specification is agreed.
- Rust project source repository: [rmkr/roost](https://github.com/rmkr/roost), supplied by the user on 2026-10-06. Keep this distinct from the upstream behavior/attribution baseline.
- Baseline: [CarlosTheory/claude-multi-account](https://github.com/CarlosTheory/claude-multi-account). Research must pin the inspected revision and distinguish source behavior, documented contracts, and unverified platform behavior.
- Consult Wayfinder for each session. Use Grilling and Domain Modeling for human decisions, Research for external facts, and Prototype when a concrete artifact is needed to choose behavior. Apply Ponytail to keep the eventual design small.
- Use the accepted profile, account, launcher, and default-alias definitions in the [glossary](../../GLOSSARY.md). The resolved [Define safe profile coexistence and ownership](issues/03-profile-compatibility.md) contract constrains later CLI, distribution, and Rust design decisions.
- Preserve the user's existing Claude installation and profile data. Investigations use source snapshots and disposable test directories.

## Decisions so far

<!-- Closed tickets only: linked title and a one-line gist; detail lives in the ticket. -->

- [Establish the upstream behavior baseline](issues/01-upstream-behavior.md): pinned v0.1.2 command and file contracts, with isolated Linux checks exposing compatibility and safety choices.
- [Verify Claude isolation and authentication contracts](issues/02-claude-contracts.md): documented profile isolation has a keyless Console exception; authentication precedence and update ownership need explicit treatment.
- [Define safe profile coexistence and ownership](issues/03-profile-compatibility.md): separate Rust ownership, unchanged-path upstream registration, explicit authentication conflicts, pass-through defaults, and guarded token/copy/removal operations preserve upstream data and the shared installation.
- [Choose the Rust command contract and targeted improvements](issues/04-command-contract.md): flat CLI with explicit registration/reuse, native OAuth and concurrent-session/resume workflows, strict forwarding, and guarded token/copy/purge/diagnostic/PATH/update behavior.

- [Choose command naming and cross-platform distribution](issues/05-distribution.md): Roost identity, separate configurable storage, source-only testing distribution, cross-platform/Fish support, native prerequisites, MIT license, and explicit manager switching.

- [Set the Rust design and specification acceptance criteria](issues/06-specification-readiness.md): accepted implementation-ready specification with exact state/recovery/launcher/process design, clean independent review, and local-first acceptance recipes with later platform evidence gates.

## Not yet specified

None within the completed planning destination. The [specification acceptance recipes](spec.md#implementation-acceptance-recipes) track future implementation and runtime evidence; failures exposing a new support trade-off require a subsequent explicit decision.

## Out of scope

- Implementing and shipping the Rust application in this planning map.
- A TUI or desktop GUI: the selected interface is a CLI.
- Rust-owned session orchestration, terminal automation, and a custom OAuth client: [Choose the Rust command contract and targeted improvements](issues/04-command-contract.md#answer) uses Claude's native session and browser-login capabilities.
