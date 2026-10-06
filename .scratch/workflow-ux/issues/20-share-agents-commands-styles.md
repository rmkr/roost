# Share agents, commands and output-styles through sets

Type: task
Labels: workflow-ux
Status: open
Assignee: unassigned
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: none

## Question

Extend shared sets with three more linked item kinds, reusing skill/instruction reconciliation unchanged (recording, conflicts, existing content wins, purge, missing-source skip, owned profiles only, group-writable folders tightened to 0700):

- Agents: `--agent FILE` / `--agents-from DIR`, `.md` files linked under `agents/`.
- Commands: `--command FILE` / `--commands-from DIR`, `.md` files linked under `commands/`.
- Output styles: `--output-style FILE` / `--output-styles-from DIR`, `.md` files linked under `output-styles/`.

Whole-directory sources take immediate `*.md` files only, skipping dot-names and links, like instruction sources. Show the new kinds in `set list` (text and JSON) and the `ls` Sets data. Amend the spec's set grammar, shared sets section and A16. Confirm against Claude Code docs that these folders under the config directory are loaded and follow symlinked files; note any exception in the ticket.

## Comments
