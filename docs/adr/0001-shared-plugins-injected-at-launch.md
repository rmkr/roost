---
status: accepted
---

# Shared plugins live in a plugin store and are injected at launch

Shared plugins are installed once into a Roost-owned plugin store by delegating to Claude's own installer (`claude plugin install` with the store as its config directory), and reach each subscribed profile at launch through Claude's plugin directories (`CLAUDE_CODE_PLUGIN_DIRS`). Roost never links or redirects a profile's `plugins/` directory: Claude rewrites `installed_plugins.json` (absolute paths, no visible lock), sweeps old versions, and keeps per-account synced buckets there, so sharing it risks concurrent-write corruption and cross-account mixing. Only Roost writes the store, under its lock, and injection writes nothing into profiles, so registered upstream profiles can receive shared plugins without breaking the rule that borrowed data is never written.

## Considered Options

- Symlink, or `CLAUDE_CODE_PLUGIN_CACHE_DIR` pointing every profile at one shared plugins root: install once through `/plugin`, but shares Claude's unlocked mutable state across profiles and accounts.
- Fan-out: run `claude plugin install` inside each subscribed profile. Native ids and Claude's auto-update, but one install per profile, writes into Claude's state in each profile, and cannot reach upstream profiles.
- Roost fetching plugins itself (git clone, marketplace parsing): duplicates Claude's installer and trust prompts.

## Consequences

- Injected plugins appear as `name@inline` and Claude does not auto-update them; updates are `roost plugin update`, or an opt-in launch-time update at most once per day that warns and launches on failure.
- The store has no account. Verified: marketplace add and install from GitHub succeed with no Claude credentials, using the user's own git credentials. Marketplaces hosted on claude.ai (organization libraries) cannot be added to the store: they need a signed-in claude.ai session (verified 2026-10-06); those plugins stay per account as `@synced`.
- Requires Claude 2.1.280 or later; Roost's minimum supported version is raised to match.
- Skills, by contrast, are linked per item into owned profiles from directories the user manages, because Claude documents symlinked skill directories and does not rewrite them.

## Open

Resolved 2026-10-06 by [injected plugin research](../../.scratch/workflow-ux/research/injected-plugins.md); status moved to accepted because the evidence settles every question this section listed:

- Identity is `<manifest name>@inline`. A profile can opt out of one store plugin with `"<name>@inline": false` in its own settings, and Roost respects that. An injected plugin silently replaces a same-named native install (only `--debug` logs it), and managed settings override both.
- Plugin user config (`pluginConfigs["<name>@inline"]`) and `${CLAUDE_PLUGIN_DATA}` (`plugins/data/<name>-inline/`) live in each profile's config directory, written by Claude, not Roost. Nothing a session does writes into the store.
- Inject each plugin's `installPath` (the `plugins/cache/<mkt>/<name>/<version>/` directory) from the store's `installed_plugins.json`, recomputed at every launch, never the marketplace checkout. Updates create a new version directory and mark the old one `.orphaned_at`.

Rules carried into the plugin-store work: inject the dependency closure, refuse duplicate manifest names, skip missing or orphaned directories, and decide whether Roost prunes orphaned store versions, since Claude's CLI subcommands did not sweep them.
