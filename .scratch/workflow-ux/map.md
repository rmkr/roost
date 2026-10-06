# Make Roost simpler to work with day to day

Labels: workflow-ux
Status: open

## Destination

Roost remembers which profile each project uses, shows profiles in a readable table, shares skills and plugins across profiles through shared sets and a plugin store, and launches Claude Desktop per profile. The map ends when every ticket below is resolved and the amended specification is accepted.

## Notes

- Decisions come from a grilling session with the user on 2026-10-06 and are recorded in [design.md](design.md), [ADR 0001](../../docs/adr/0001-shared-plugins-injected-at-launch.md) and [ADR 0002](../../docs/adr/0002-desktop-per-profile-via-user-data-dir.md). New terms are in the [glossary](../../GLOSSARY.md).
- These features amend the accepted [Roost specification](../rust-port/spec.md); [Amend the specification for workflow UX](issues/01-amend-spec.md) goes first.
- `roost desktop` launches Claude's own Desktop app; it is not a Roost GUI, so it does not conflict with the rust-port map's "CLI, not a GUI" scope.
- Build order agreed with the user: `ls` table, selected profile, skill sets, plugin store, Desktop.
- Tracking follows the [local Markdown operations](../README.md).

## Decisions so far

<!-- Closed tickets only: linked title and a one-line gist; detail lives in the ticket. -->

- [Amend the specification for workflow UX](issues/01-amend-spec.md): spec amended and accepted, with a separate plugin store lock and Desktop-folder deletion on upstream remove.
- [Lay shared scaffolding for the workflow UX features](issues/10-shared-scaffold.md): table renderer, link helpers, reusable profile environment, side-file store API and `Launch` open mode.
- [Investigate injected plugin behavior](issues/06-injected-plugin-behavior.md): injection works as ADR 0001 assumes (`name@inline`, per-profile opt-out, config and data per profile); inject `installPath` from the store's `installed_plugins.json` at every launch; no claude.ai org marketplaces in the store; ADR accepted.

## Not yet specified

- Shared agents, commands and output-styles (linked like skills), and hooks/settings through a shared `--settings` file: agreed as later item kinds, not yet ticketed.

## Out of scope

- Roost installing or owning skills: skills stay with the user's own installer and are only linked.
- Sharing a whole `CLAUDE.md`, `.claude.json`, credentials, or Claude's synced skill and plugin folders.
- Writing into registered upstream or default-alias data.
