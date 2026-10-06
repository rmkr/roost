# Build the plugin store and `roost plugin` commands

Type: task
Labels: workflow-ux
Status: resolved
Assignee: Claude subagent ticket-07
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: 02, 05, 06

## Question

Implement [design: plugin store](../design.md#plugin-store) using the findings from [Investigate injected plugin behavior](06-injected-plugin-behavior.md): the store directory, `plugin marketplace add`, `plugin add` with set prompt/`--set`/`--no-set`, `plugin update` and the opt-in daily launch-time update, `plugin remove`, and `CLAUDE_CODE_PLUGIN_DIRS` injection for subscribed owned and upstream profiles. All store writes go through Claude's CLI under the Roost lock; output from Claude stays uncaptured or allowlisted.

## Comments

### 2026-10-06: decisions from research

[Injected plugin research](../research/injected-plugins.md) settled the open questions; ADR 0001 is accepted. The user chose: prune store versions orphaned more than 14 days after `plugin update`, and detect native duplicates in `doctor` only. Both are in the spec next to auto-update. Learn each `installPath` from `claude plugin list --json` in the store after mutations, never by parsing `installed_plugins.json`.

### 2026-10-06: implementation on `ticket/07-plugin-store` (Claude subagent ticket-07)

Implemented, not resolved; awaiting merge and review.

- `src/plugins.rs`: `roost plugin list|marketplace add|add|update|auto-update|remove`, launch-time injection (`plugins::prepare` for `run`, launchers, `switch`/bare `roost`; `plugins::at_launch` for `desktop`), the daily auto-update, orphan pruning and doctor's `plugin_shadowed`. `status` and `list --full` keep `launch::prepare` (no injection).
- `src/store/plugin_store.rs`: journaled `store_create` with `.roost-store.json` and recovery; `Operation::StoreCreate`/`Role::Store` hooks in `store.rs`.
- Store children: PATH Claude (version-checked) with `CLAUDE_CONFIG_DIR=<store>`, `DISABLE_AUTOUPDATER=1`, minus `CLAUDE_CODE_OAUTH_TOKEN`, `CLAUDE_CODE_PLUGIN_DIRS` and also `CLAUDE_CODE_OAUTH_REFRESH_TOKEN`, `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN` (the store has no account). They run under `plugin-store/.roost-store-lock` with inherited stdio; the root lock is held only to create/validate the store and to update `sets.json`.
- Roost's store record is `plugin-store/.roost-store-plugins.json` (ID, install path, version, manifest name, resolved dependency IDs), rebuilt from `claude plugin list --json` after every store mutation under the store lock. The spec names no file for this record; this is my choice. Dependencies come from each installed plugin's documented manifest `dependencies`, not from Claude's state.
- Choices the spec leaves open: `plugin update` with no PLUGIN runs `claude plugin update ID` once per recorded plugin rather than a bare `claude plugin update`, because I could not confirm the bare form against real Claude. Bare `plugin add NAME` resolves through `claude plugin list --json --available`, using `available[].pluginId` (or `name` plus `marketplaceName`) and installed IDs. Those field names are unverified against real Claude. After a failed update the record is still re-learned, because Claude may already have moved versions. `plugin list` also shows recorded plugins that belong to no set. Auto-update runs only for isolated launches. The set prompt is skipped when no sets exist.
- Tests: `tests/plugins.rs` (A17), with a Python fake Claude that emulates `plugin` subcommands in its config directory and logs argv, environment and lock state. Plus unit tests in `store::plugin_store`.


## Answer

2026-10-06: Merged into `workflow-ux` (branch head 57faae8). `src/plugins.rs` adds `roost plugin list|marketplace add|add|update|auto-update|remove`, the journaled store with its own lock, a Roost store record built from `claude plugin list --json`, launch-time `CLAUDE_CODE_PLUGIN_DIRS` injection on every launch path (run, launchers, switch/bare, desktop), opt-in daily auto-update, orphan pruning and the `plugin_shadowed` doctor check. Verified against real Claude 2.1.291 in a disposable config dir: `plugin list --json` is an array of `{id, installPath, version, …}`, `--available` returns `{installed, available[{pluginId, name, marketplaceName}]}`, `plugin update` takes exactly one plugin, and the manifest is `<installPath>/.claude-plugin/plugin.json`.
