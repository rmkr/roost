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
- [Raise the minimum Claude version to 2.1.280](issues/02-raise-claude-floor.md): floor raised for `CLAUDE_CODE_PLUGIN_DIRS`.
- [Lay shared scaffolding for the workflow UX features](issues/10-shared-scaffold.md): table renderer, link helpers, reusable profile environment, side-file store API and `Launch` open mode.
- [Render `roost ls` as a worktrunk-style table](issues/03-ls-table.md): aligned table with summary line and width fitting; `--full` runs parallel probes.
- [Launch Claude Desktop per profile with `roost desktop`](issues/09-desktop.md): per-profile Desktop sign-in, detached launch; only one running Desktop can use Cowork.
- [Remember the selected profile per project and add `roost switch`](issues/04-selected-profile.md): per-repo selection with picker, bare `roost`, `switch`, last-use recording and `ls` markers.
- [Add shared sets with linked skills](issues/05-skill-sets.md): `set` commands, default and copied subscriptions, launch-time skill links in owned profiles.
- [Share instruction fragments through sets](issues/11-shared-instructions.md): fragments linked into owned profiles' `rules/`; no import fallback needed.
- [Build the plugin store and `roost plugin` commands](issues/07-plugin-store.md): account-less store via Claude's installer, injected on every launch path; JSON shapes verified against real Claude.
- [Move the personal profile's hand-made skill links onto a set](issues/08-migrate-personal-skills.md): `personal` and `work` subscribe to default set `core` (65 skills, plannotator and ponytail).
- [Split the user's global CLAUDE.md into instruction fragments](issues/12-split-global-instructions.md): fragments in `~/.agents/instructions` via `core`; verified each loads once.
- [Open the picker from `roost switch` without a name](issues/13-switch-picker.md): `roost switch` alone picks and launches, like `wt switch`.
- [Color the profile picker like `roost ls`](issues/14-picker-colors.md): picker rows share the `ls` styles; confirmed by the user.
- [Open a profile picker from `roost desktop` without a name](issues/15-desktop-picker.md): Desktop picker with running/signed-in state, launching detached.
- [Let a profile borrow the existing Claude Desktop data folder](issues/16-desktop-link-existing.md): `roost desktop --link NAME` opens the signed-in Desktop with that profile's configuration.
- [Investigate injected plugin behavior](issues/06-injected-plugin-behavior.md): injection works as ADR 0001 assumes (`name@inline`, per-profile opt-out, config and data per profile); inject `installPath` from the store's `installed_plugins.json` at every launch; no claude.ai org marketplaces in the store; ADR accepted.

## Not yet specified

None in scope; see Later.

## Later

- Shared agents, commands and output-styles (linked like skills), and hooks/settings (status line, hooks) through a shared `--settings` file: agreed as later item kinds, deferred by the user on 2026-10-06 to a future round.

## Out of scope

- Roost installing or owning skills: skills stay with the user's own installer and are only linked.
- Sharing a whole `CLAUDE.md`, `.claude.json`, credentials, or Claude's synced skill and plugin folders.
- Writing into registered upstream or default-alias data.
