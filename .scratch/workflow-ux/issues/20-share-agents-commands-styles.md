# Share agents, commands and output-styles through sets

Type: task
Labels: workflow-ux
Status: claimed
Assignee: Claude subagent ticket-20
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: none

## Question

Extend shared sets with three more linked item kinds, reusing skill/instruction reconciliation unchanged (recording, conflicts, existing content wins, purge, missing-source skip, owned profiles only, group-writable folders tightened to 0700):

- Agents: `--agent FILE` / `--agents-from DIR`, `.md` files linked under `agents/`.
- Commands: `--command FILE` / `--commands-from DIR`, `.md` files linked under `commands/`.
- Output styles: `--output-style FILE` / `--output-styles-from DIR`, `.md` files linked under `output-styles/`.

Whole-directory sources take immediate `*.md` files only, skipping dot-names and links, like instruction sources. Show the new kinds in `set list` (text and JSON) and the `ls` Sets data. Amend the spec's set grammar, shared sets section and A16. Confirm against Claude Code docs that these folders under the config directory are loaded and follow symlinked files; note any exception in the ticket.

## Comments

### 2026-10-06 — Claude subagent ticket-20

Docs check (inspection only):
- code.claude.com/docs `claude-directory`: user `~/.claude/agents/` ("personal subagents available in every project"), `commands/` ("each markdown file becomes a command available everywhere") and `output-styles/` ("each markdown file defines an output style", read at startup). `sub-agents`: user agents load from `~/.claude/agents/`, scanned recursively. `slash-commands`: `commands/*.md` is the older format, merged into skills but still works. `output-styles`: user styles live in `~/.claude/output-styles`; the file name is the style name. None of these pages mentions symlinks or `CLAUDE_CONFIG_DIR` for these folders.
- Local Claude Code 2.1.291 bundle (strings only, not run): `agents`, `commands` and `output-styles` are among the userConfigDir directory names, and an internal note says the user output-styles folder is resolved from the config home honoring `CLAUDE_CONFIG_DIR`. All three load through the same `loadMarkdownFilesForSubdir` path, whose directory walker follows a symlink by stat-ing its target and keeps it when that is a regular `.md` file (the ripgrep fallback runs with `--follow --glob *.md`); same-inode duplicates are deduplicated. So symlinked `.md` files are followed for all three kinds; no exception found. Not verified with a real launch.

Implementation (branch `ticket/20-share-agents-commands-styles`): new `ItemKind`s `agent`, `agent_source`, `command`, `command_source`, `output_style`, `output_style_source` (older `sets.json` still reads; unit test), CLI flags `--agent`/`--agents-from`, `--command`/`--commands-from`, `--output-style`/`--output-styles-from`, three `PLACEMENTS` (Markdown select) and `LINKED` entries; reconciliation, recording, conflicts, purge and missing-source handling unchanged. `set list` text/JSON show the kinds through the existing item rendering; the `ls` Sets column/field is set names only, so it needs no change. The upstream subscribe note now names all linked kinds. Spec amended: ITEM grammar, set list ItemKind, side-file link paths, shared sets section, A16. Tests in `tests/sets.rs` cover each kind via explicit file and whole-dir source, dot-names/links/non-`.md`/directories skipped, existing file wins, a 0775 folder tightened to 0700, unsubscribe removes recorded links, add validation, listing, conflicts, upstream/alias get nothing, and purge unlinks without following.
