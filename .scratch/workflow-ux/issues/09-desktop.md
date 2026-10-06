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

### 2026-10-06: implementation (Claude subagent ticket-09)

Implemented on branch `ticket/09-desktop`: `roost desktop [--foreground] NAME` (experimental, Linux only) in `src/desktop.rs`, with the folder lifecycle in `src/store/desktop.rs`. Owned and upstream profiles get `<root>/desktop/<REGISTRATION_ID>/`, created through the journaled `desktop_create` operation with the `.roost-desktop.json` marker. Launch uses the isolated `run` environment plus set-link reconciliation, records `last_used` and `desktop_launched`, and passes `--user-data-dir=<folder>`. Default aliases start plain `claude-desktop` with the caller's environment. Launch is detached by default (setsid, null stdio, 2-second early-exit watch, `Started Claude Desktop for NAME`); `--foreground` execs. A live same-profile `SingletonLock` fails `desktop_running`; any other live Roost folder or the conventional `Claude` config folder triggers the Cowork warning. Purge deletes the folder as a journaled `desktop_data` artifact, with the marker deleted last. Ordinary remove of an upstream profile deletes it after confirmation (`--yes` skips; without a terminal and without `--yes`, it is a usage error). `--yes` without `--purge` is now accepted only for upstream registrations. Doctor reports `desktop_data_not_isolated`. Plugin injection is not wired yet (ticket 07).

The ticket stays open. Still to do with the user: check whether Cowork works in a second concurrent Desktop instance, and record the result in ADR 0002.
