# Verify Claude isolation and authentication contracts

Type: research
Labels: wayfinder:research
Status: resolved
Assignee: codex/claude-research
Parent: [Find the way to a Rust Claude profile manager](../map.md)
Blocked by: none

## Question

Which current, first-party Claude Code contracts support independent profiles across Linux, macOS, and Windows, and where does upstream rely on undocumented behavior?

Verify configuration-directory isolation, authentication and credential storage, environment precedence, token limitations, shared versus profile-scoped state, executable discovery, and update controls. Separate documented guarantees from implementation observations and assumptions, especially macOS Keychain behavior. Save a source-cited report at `../research/claude-contracts.md` on the throwaway branch `research/claude-contracts`; identify any disposable experiments still needed rather than inspecting the user's credentials.

## Answer

### 2026-10-05: research resolution

The [Claude contract research](../research/claude-contracts.md) identifies the supported isolation boundary, authentication precedence, official status inspection, and update controls. Keyless Console logins are outside configuration-directory isolation; Keychain naming internals and broad binary-update concurrency remain unverified. These facts inform the profile compatibility decision; supported authentication modes and improvements remain open.

Evidence: current first-party documentation inspected, with consequential isolation and environment claims checked by the coordinating agent. No account authentication, credential inspection, or cross-platform runtime tests were performed. Upstream observations are attributed to the separate baseline investigation.

Research context: local branch `research/claude-contracts`, commit `786454e1cf183927de88f997c018b516a4e1acb2`; the linked report is also present in this checkout.
