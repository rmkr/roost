# Refuse missing set item sources and skip missing link targets

Type: task
Labels: workflow-ux
Status: open
Assignee: unassigned
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: none

## Question

Found while migrating `personal` (2026-10-06): `roost set add --skill DIR core` accepted a directory that does not exist, and launch-time reconciliation then created a dangling link. Make `set add` refuse a missing or non-directory `--skill`/`--skills-from`/`--instructions-from` source and a missing or non-regular `--instruction` file (not_found, naming the path), and make reconciliation skip any item whose source has since disappeared, with one warning naming the set and path and without removing or creating links for it beyond normal recorded-link cleanup. Amend the spec's shared sets section and A16.

## Comments
