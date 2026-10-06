# Investigate injected plugin behavior

Type: research
Labels: workflow-ux
Status: resolved
Assignee: Claude subagent ticket-06
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: none

## Question

Before the plugin store is built, settle what ADR 0001 leaves open, using disposable config directories only: how `name@inline` plugins from `CLAUDE_CODE_PLUGIN_DIRS` interact with a profile's own `enabledPlugins` and same-named native installs; where plugin user config and `${CLAUDE_PLUGIN_DATA}` live for injected plugins; which directory inside the store to inject per plugin and how that survives `claude plugin update` and the 14-day orphan sweep; whether claude.ai organization marketplaces work in an account-less store. Record findings in `../research/` and update the ADR's status.

## Comments

## Answer

2026-10-06. Detail and evidence are in [the injected plugin research](../research/injected-plugins.md). [ADR 0001](../../../docs/adr/0001-shared-plugins-injected-at-launch.md) is now accepted.

- Injected plugins are `<manifest name>@inline`, on by default. A profile opts out with `"<name>@inline": false` in its own settings (verified). They replace a same-named native install silently (docs; both rows show as enabled in `plugin list`), and managed settings beat both.
- Plugin user config is saved under `pluginConfigs["<name>@inline"]` in the profile's `settings.json` (verified). `${CLAUDE_PLUGIN_DATA}` is `<profile>/plugins/data/<name>-inline/` (docs + binary). Both are per profile; neither carries over from a native `name@marketplace` install.
- Inject `installPath` from `<store>/plugins/installed_plugins.json` (`plugins/cache/<mkt>/<name>/<version>/`), never the marketplace checkout. `claude plugin update` makes a new version directory, rewrites `installPath`, and writes `.orphaned_at` into the old one (verified), so recompute it per launch. To satisfy the spec's "no undocumented state" rule: after each Roost store mutation, record `installPath` from `claude plugin list --json` run in the store (public output, about 0.5 s), and read only that record at launch. Validate the path (inside the store cache, no-follow directory, no `.orphaned_at`, no `:`), inject the dependency closure, and refuse duplicate manifest names. Profile sessions write no `.in_use` markers into the store (binary). Store CLI commands did not sweep an aged orphan (verified), so orphaned versions accumulate unless Roost prunes them.
- claude.ai organization marketplaces cannot be added to an account-less store (verified error: sign-in with plugin sync required). Those plugins stay per account as `@synced`.
- Addendum for the orchestrator: `$CLAUDE_CONFIG_DIR/rules/**/*.md` is the per-profile equivalent of `~/.claude/rules/` (docs + binary). Symlinks are followed outside Cowork, with no import approval. `@path` imports take relative, absolute and `~` paths, up to four hops. A file reached twice is loaded once (binary, not observed live).
