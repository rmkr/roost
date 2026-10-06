# Let a profile borrow the existing Claude Desktop data folder

Type: task
Labels: workflow-ux
Status: claimed
Assignee: Claude subagent ticket-16
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: none

## Question

User request 2026-10-06 (Q58–Q61): the user wants `roost desktop work` to use their existing, signed-in Claude Desktop instead of a fresh Roost data folder.

- `roost desktop --link NAME` records that NAME's Desktop is the conventional Desktop data folder (Linux: `$XDG_CONFIG_HOME/Claude`, default `~/.config/Claude`), borrowed in place. Never copy or move it (copying would duplicate Desktop's sign-in, which Roost must not extract, and the copies would fight over one refresh token). `roost desktop --unlink NAME` removes the link. Owned and upstream registrations only; the default alias already uses that folder.
- A linked launch runs plain `claude-desktop` (no `--user-data-dir`) with NAME's profile environment (`CLAUDE_CONFIG_DIR`, plugin injection, set reconciliation, last-use and `desktop_launched` recording), detached as usual.
- Only one registration may link the conventional folder at a time; a second link is collision naming the current holder.
- If NAME already has its own Roost Desktop folder, `--link` refuses (collision) unless `--replace`, which deletes Roost's own folder after a confirmation naming it (`--yes` skips; same journaled `desktop_data` deletion and Q51 running check) and then links.
- Pickers: the linked row's Desktop column reflects the conventional folder's state; the default alias row shows `shared with NAME`; both report `running` together, and launching either while the shared instance runs is desktop_running.
- Remove and purge of a linked registration drop the link only and never touch the borrowed folder; the Q51 running refusal applies to the borrowed folder too. Doctor's `desktop_data_not_isolated` check does not apply to linked registrations.
- Record the link in Roost's side files keyed by registration ID. Amend the spec (synopsis, Desktop section, remove/purge table, A18) and ADR 0002.

## Comments

2026-10-06 (Claude subagent ticket-16): implemented on branch `ticket/16-desktop-link-existing`. `roost desktop --link NAME [--replace [--yes]]` / `--unlink NAME` (clap rules: NAME required; link/unlink/foreground mutually exclusive; replace requires link; yes requires replace). The link lives in `state.json` as optional `desktop_links: [{registration_id}]` (at most one; omitted while empty, so older files and older binaries stay compatible). Linked launches run plain `claude-desktop` with the profile environment and record last use/`desktop_launched`; no Roost folder is created. `--replace` deletes the own folder via a new journaled `desktop_delete` operation (recovery abandons, never continues, deletion). Linked row/alias row in the picker show the conventional folder's state / `shared with NAME`; both refuse while the shared instance runs. Remove/purge drop the link only; Q51 covers the borrowed folder; doctor skips linked registrations. Spec (synopsis, parse rules, journal ops, state schema, Desktop, remove/purge table, Q51, A18) and ADR 0002 amended. Choice for review: an unreadable `state.json` now fails owned/upstream Desktop launches (the link decides which sign-in is used) instead of warning and launching. Not resolved; awaiting review.
