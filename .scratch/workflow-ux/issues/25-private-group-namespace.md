# Accept group-writable folders shared only with the user's private group

Type: task
Labels: workflow-ux
Status: open
Assignee: unassigned
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: none

## Question

Found 2026-10-06: `roost setup-path --apply` refused `~/.config/fish/conf.d` (mode 0775) and owned-profile links needed a 0700 fix, because the namespace check rejects any group-writable directory. On Ubuntu's default umask 002 every user directory is group-writable by the user's private group (same name as the user, no other members), so this is not a real exposure. Decide with the user whether the check should accept a group-writable directory whose group is the user's primary group with no other members (and/or one whose ancestor is 0700), then implement and amend the spec.

## Comments

### 2026-10-06: user decision (Q80)

Accept a group-writable (not world-writable) directory owned by the user whose group is the user's private group: the user's primary group, with no supplementary members and no other account using it as its primary group. Reject anything else as today, including world-writable directories and groups with other members. The "0700 ancestor" rule was considered and not adopted.
