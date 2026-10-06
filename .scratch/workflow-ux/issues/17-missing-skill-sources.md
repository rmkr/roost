# Refuse missing set item sources and skip missing link targets

Type: task
Labels: workflow-ux
Status: claimed
Assignee: Claude subagent ticket-17
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: none

## Question

Found while migrating `personal` (2026-10-06): `roost set add --skill DIR core` accepted a directory that does not exist, and launch-time reconciliation then created a dangling link. Make `set add` refuse a missing or non-directory `--skill`/`--skills-from`/`--instructions-from` source and a missing or non-regular `--instruction` file (not_found, naming the path), and make reconciliation skip any item whose source has since disappeared, with one warning naming the set and path and without removing or creating links for it beyond normal recorded-link cleanup. Amend the spec's shared sets section and A16.

## Comments

2026-10-06 (Claude subagent ticket-17): Implemented on branch `ticket/17-missing-skill-sources`. `set add` now refuses a skill/instruction item whose source is missing or the wrong type (skill, `--skills-from`, `--instructions-from`: directory; `--instruction`: regular file) with not_found naming the absolute path; `sets.json` is untouched and `set drop` never needs the source. The check follows symlinks, like source listing already does, because Claude reaches the source through the link; a dangling symlink counts as missing. At launch, an item whose source vanished is skipped with one warning `Skipped PATH from set SET: missing or not a directory|a regular file` (unreadable source directories now also name the set). Chosen behavior for an existing recorded link to a vanished source: the item contributes no desired links, so its recorded link is removed by the normal recorded-link cleanup (still only when it is the recorded symlink); the item stays in the set and relinks once the source returns. Vanished items also contribute nothing to subscribe-time conflict checks. Spec shared sets section and A16 amended; tests in `tests/sets.rs` (`add_refuses_missing_or_wrong_type_sources_and_leaves_sets_unchanged`, `launch_skips_vanished_sources_with_one_warning_and_drops_their_links`). Not resolved; awaiting review.
