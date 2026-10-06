# Share settings fragments by writing owned profiles' settings.json

Type: task
Labels: workflow-ux
Status: open
Assignee: unassigned
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: none

## Question

Implement [design: shared settings](../design.md#shared-settings) and [ADR 0003](../../../docs/adr/0003-shared-settings-written-into-owned-profiles.md): `--setting FILE` / `--settings-from DIR` set items (immediate `*.json`, skipping dot-names and links; refuse missing sources and keys other than `hooks`, `statusLine`, `outputStyle`); subscribe-time refusal when two fragments set the same single-value key; launch-time reconciliation into owned profiles' `settings.json` (record Roost-written hook handlers and keys per registration in `state.json`; write only on change; remove only recorded, unchanged entries; skip identical existing handlers; re-add hand-removed Roost hooks; back off `statusLine`/`outputStyle` the profile set itself; atomic compare-and-replace preserving key order and 2-space style; concurrent change → warn, retry next launch; invalid fragment → warn, skip). Unsubscribe removes Roost's entries at the next launch. Never write upstream or alias data. Desktop launches reconcile too. Amend the spec (set grammar, shared sets, state.json schema, A16).

## Comments
