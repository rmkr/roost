# Accept group-writable folders shared only with the user's private group

Type: task
Labels: workflow-ux
Status: resolved
Assignee: Claude subagent ticket-25
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: none

## Question

Found 2026-10-06: `roost setup-path --apply` refused `~/.config/fish/conf.d` (mode 0775) and owned-profile links needed a 0700 fix, because the namespace check rejects any group-writable directory. On Ubuntu's default umask 002 every user directory is group-writable by the user's private group (same name as the user, no other members), so this is not a real exposure. Decide with the user whether the check should accept a group-writable directory whose group is the user's primary group with no other members (and/or one whose ancestor is 0700), then implement and amend the spec.

## Comments

### 2026-10-06: user decision (Q80)

Accept a group-writable (not world-writable) directory owned by the user whose group is the user's private group: the user's primary group, with no supplementary members and no other account using it as its primary group. Reject anything else as today, including world-writable directories and groups with other members. The "0700 ancestor" rule was considered and not adopted.

### 2026-10-06: implementation (Claude subagent ticket-25)

Implemented Q80 on branch `ticket/25-private-group-namespace`. `check_namespace` in `src/platform/unix.rs` now delegates to `namespace_protected(owner, gid, mode, euid, private_gid)`: owner must be the effective user or root; sticky is accepted as before; world-writable without sticky is rejected; group-writable is accepted only when the directory's gid equals `private_group(&dyn AccountDatabase)`, which requires the user's primary group, a member list that is empty or names only the user, and no other uid in the passwd database with that primary gid. The real `SystemAccounts` uses `getpwuid_r`, `getgrgid_r` and `getpwent_r` (glibc; other targets report the passwd database unreadable). Any unreadable database fails closed. The answer is computed lazily once per process, so ordinary 0755 ancestors never consult the databases. `Directory::restrict_to_owner` is unchanged, and `setup-path --apply` picks up the rule through `verify_namespace`. Unit tests inject account data through a fake `AccountDatabase`. A host-independent test checks that the real bindings resolve the current user. I did not add an integration test that depends on the host having a private group, because CI runners may not have one. The spec's Unix filesystem paragraph is amended. Not resolved.

## Answer

2026-10-06: Merged into `main` (1dca707, plus a follow-up). `check_namespace` accepts a user-owned, group-writable, not world-writable directory whose group is the user's private group (primary gid, no other supplementary members, no other account using it as primary); unreadable account databases fail closed. Verifying on the user's real `~/.config/fish/conf.d` (restored to its original 0775) found a second check in `setup-path` (`protected_parent`) that still required mode 0755; it now uses the same rule through `user_private_directory`. `roost setup-path --shell fish --apply` then succeeded and `roost doctor` reports no warnings.
