# Let a profile borrow the existing Claude Desktop data folder

Type: task
Labels: workflow-ux
Status: open
Assignee: unassigned
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
