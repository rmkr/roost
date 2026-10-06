# Launch Claude Desktop per profile with `roost desktop`

Type: task
Labels: workflow-ux
Status: claimed
Assignee: Claude subagent ticket-09
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: 01, 10

## Question

Implement [design: Desktop](../design.md#desktop) and [ADR 0002](../../../docs/adr/0002-desktop-per-profile-via-user-data-dir.md): per-registration data folder in Roost's tree, `claude-desktop --user-data-dir` plus the profile's `CLAUDE_CONFIG_DIR`, plain launch for the default alias, refusal when the same profile's Desktop holds its lock, concurrent-instance Cowork warning, remove/purge lifecycle, `doctor` check, experimental labelling. Tests use a fake `claude-desktop`; never launch the real app in tests. With the user, check whether Cowork works in a second concurrent instance and record the result in the ADR.

## Comments

### 2026-10-06: launch detached

The user wants `roost desktop` to return the prompt immediately instead of holding the terminal and printing Chromium's console errors. The spec's Desktop section now makes detached launch the default (new session, null stdio, 2-second early-exit check) with `--foreground` for debugging. Tests: the fake `claude-desktop` must cover early exit and a long-running child.
