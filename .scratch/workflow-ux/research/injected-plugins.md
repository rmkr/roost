# Injected plugin behavior (research for ticket 06)

Date: 2026-10-06. Claude Code 2.1.291. Answers [Investigate injected plugin behavior](../issues/06-injected-plugin-behavior.md) for [ADR 0001](../../../docs/adr/0001-shared-plugins-injected-at-launch.md).

## Method

- Every `claude` run used a disposable config directory under `~/.cache/roost-impl/research-06/` (mode 700), with `CLAUDE_CODE_OAUTH_TOKEN`, `ANTHROPIC_API_KEY` and `ANTHROPIC_AUTH_TOKEN` removed from the environment. No account, no session, no `claude -p`. The directories were deleted afterwards.
- Directories: `store` (the stand-in plugin store), `profA` (a profile that also has a native install of the same plugin), `profB` (a clean profile), plus local fixtures: a plugin `cfgdemo` with `userConfig`, and a git-backed local marketplace `localmkt` with a plugin `upd` that has no `version`, so each commit is a new version.
- Only non-interactive commands were used: `plugin marketplace add/list`, `plugin install/update/disable/configure/details/list [--json]`.
- Sources: the official docs at code.claude.com ([plugin loading reference](https://code.claude.com/docs/en/plugins/loading), [manifest reference](https://code.claude.com/docs/en/plugins-reference), [env vars](https://code.claude.com/docs/en/env-vars), [install](https://code.claude.com/docs/en/plugins/install), [dependencies](https://code.claude.com/docs/en/plugins/dependencies), [memory](https://code.claude.com/docs/en/memory)), and in a few places strings from the installed 2.1.291 binary (minified JS). Findings that rest only on minified source are marked **source-inferred**.

Labels: **verified** means observed in the disposable directories. **docs** means the official docs state it. **inferred** means reasoning that was not observed.

## Findings

### 1. Identity: `<manifest name>@inline`

- **verified**: with `CLAUDE_CODE_PLUGIN_DIRS=<store>/plugins/cache/plannotator/plannotator/0.28.5`, `plugin list --json` in a profile shows `{"id":"plannotator@inline","scope":"session","enabled":true,"installPath":"<that dir>"}`. The text listing puts it under "Session-only plugins (--plugin-dir / --plugin-url)".
- **docs**: for `@inline` the name part is the `name` in the plugin's manifest, not the marketplace entry name. `inline` is a reserved origin.
- **verified**: when the manifest has no `version`, the inline plugin's version shows as `unknown`. The cache directory name (`d1d808d07351`) is not used. Roost's `ls` and `plugin` output should take the version from the store's `installed_plugins.json`, not from Claude's listing of the profile.
- **verified**: a missing path in `CLAUDE_CODE_PLUGIN_DIRS` gives an error row (`inline[0]: ✘ Path not found`), and the other entries still load. **verified**: two injected directories with the same manifest name both appear as `upd@inline`. Which of the two a session actually loads was not observed. Roost should skip missing directories itself and refuse to inject two store plugins with the same manifest name.
- **docs**: entries must be absolute (or start with `~`), separated by `:` on Unix. Relative entries are skipped. A store path that contains `:` cannot be expressed, so Roost must refuse one.

### 2. Interaction with the profile's own `enabledPlugins` and same-named native installs

- **docs + verified**: an injected plugin is on by default. A settings file setting `"<name>@inline": false` turns it off. `claude plugin disable plannotator@inline`, run with the variable set, wrote `"plannotator@inline": false` into the profile's `settings.json`, and the next listing showed `✘ disabled`. So a profile can opt out of one store plugin by itself, and Roost should respect that rather than override it. The key is per name, so it disables any inline plugin with that name.
- **docs**: a manifest with `defaultEnabled: false` stays off unless a settings source sets `"<name>@inline": true`.
- **docs**: precedence when names collide, from highest: managed-settings ids (which lock the name and ignore the `--plugin-dir` copy) > an enabled inline plugin > an installed marketplace plugin > a skills-dir plugin > an `@synced` claude.ai plugin.
- **verified**: when `profA` also has `plannotator@plannotator` installed natively, `plugin list` shows both as enabled. **docs**: the inline copy silently replaces the native one at load time. Only `--debug` logs record `Plugin "<name>" from --plugin-dir overrides installed version`. Roost's `doctor` or `ls` could warn about this shadowing by comparing names against the profile's `installed_plugins.json`, which it may read but must not write.
- **docs**: comparison is by manifest name, so a store plugin can shadow a native plugin even when the marketplace entry names differ.

### 3. Plugin user config and `${CLAUDE_PLUGIN_DATA}` are per profile

- **verified**: `claude plugin configure cfgdemo@inline --values-stdin` in `profB` wrote `{"pluginConfigs":{"cfgdemo@inline":{"options":{...}}}}` into **the profile's** `settings.json`, and `configure --json` read it back. Options are keyed by the `@inline` id, so values a profile saved under `name@marketplace` for a native install don't carry over. Values set in the store don't reach profiles either.
- **docs**: `sensitive` options go to the platform's secure credential store instead of `settings.json`. **inferred**: on Linux that is the profile's own credential storage, so it is also per profile. Not exercised, to keep credential files untouched.
- **docs + source-inferred**: `${CLAUDE_PLUGIN_DATA}` is `<plugins root>/data/<id>/`, where `<id>` is the plugin id with every character other than a letter, digit, `_` or `-` replaced by `-`. The binary computes it as `join(<session plugins root>, "data", sanitize(source))`. For an injected plugin that is `$CLAUDE_CONFIG_DIR/plugins/data/<name>-inline/` in **the profile**, created on first use. The store's data directories are never used by sessions.
- Consequences:
  - Plugin state and configuration stay per account, which matches Roost's profile model.
  - A plugin that keeps state in `CLAUDE_PLUGIN_DATA` writes into the profile's config directory, upstream profiles included. Claude writes there, not Roost, so Roost's never-write-borrowed-data rule still holds, but the spec text should say so.
  - Moving a profile from a native install to the store starts that plugin with a fresh data directory and no saved options (`<name>-<marketplace>` becomes `<name>-inline`).
  - `roost plugin remove` leaves each profile's `plugins/data/<name>-inline/` and `pluginConfigs` entry in place. Claude's uninstall only cleans up the store's own data. This is harmless, and Roost must not clean it from upstream profiles.

### 4. Which directory to inject, and how it survives updates and the orphan sweep

- **verified layout** of a credential-free store after `marketplace add` + `install`:
  - `plugins/known_marketplaces.json` maps each marketplace to `installLocation` = `<store>/plugins/marketplaces/<mkt>` (the git checkout).
  - `plugins/installed_plugins.json` is `{"version":2,"plugins":{"<name>@<mkt>":[{"scope":"user","installPath":"<store>/plugins/cache/<mkt>/<name>/<version>","version":...,"gitCommitSha":...}]}}`.
  - `plugins/cache/<mkt>/<name>/<version>/` holds the plugin root (`.claude-plugin/plugin.json`, components, `node_modules` when Claude installed dependencies).
- **Inject the version directory, `installPath`, never the marketplace checkout.** The checkout is the marketplace root, not the plugin root. For plannotator the plugin lives at `./apps/hook` inside it. Claude also refreshes the checkout in place with git, and it never gets Claude's dependency install. The version directory is what `${CLAUDE_PLUGIN_ROOT}` points at for a native install.
- **verified update behavior** (`localmkt`, commit v1 → v2, `claude plugin update upd@localmkt` in the store):
  - Claude created a new directory `cache/localmkt/upd/d1d808d07351/` beside the old `ab23fe17c5e6/`.
  - It rewrote `installPath` and `version` in `installed_plugins.json`.
  - It wrote `.orphaned_at` (a millisecond timestamp) into the old directory.
  - The path therefore changes on every update.
- **docs**: orphaned version directories are removed by a background cleanup 14 days after `.orphaned_at`, and the sweep only runs while `installed_plugins.json` records at least one install.
- **verified**: setting an old `.orphaned_at` in the store and then running `plugin list` or `plugin update` did not delete the directory. `plugin update` did recreate `.last_inuse_sweep`. **source-inferred**: the age-based cleanup ("Plugin cache cleanup") runs from a different path, presumably interactive session start, which the store never runs. Expect orphaned versions to pile up in the store until Roost prunes them, or until a Claude version that sweeps from the CLI does.
- **source-inferred**: a session writes `.in_use/<pid>` liveness markers only into version directories under **its own** plugins cache root. The loader skips paths that resolve outside it, so profile sessions write nothing into the store. **verified**: `plugin list` in a profile with the variable set created no files in the store. The flip side is that the store cannot see live profile sessions. If a store sweep ever runs, it can delete a version that a profile session started more than 14 days earlier is still using, because that session would have loaded the pre-update path.
- **verified**: an orphaned directory (with `.orphaned_at`) still loads when injected. Claude does not check, so Roost must.

**Recommended launch computation (per subscribed plugin id `<name>@<mkt>`):**

1. Read `<store>/plugins/installed_plugins.json` read-only, with no lock and no Claude process. That keeps launch cheap: `claude plugin list --json` against the store took about 0.5 s. Require `version == 2`. On any other format, fall back to `claude plugin list --json` with `CLAUDE_CONFIG_DIR=<store>`, which prints the same `id`/`installPath` (verified), or warn and skip store plugins.
2. Take the single entry with `scope == "user"`. Roost installs only at user scope, so zero or several entries means skip and warn.
3. Validate `installPath`:
   - absolute
   - equal to `<store>/plugins/cache/<mkt>/<name>/<version>` after normalization
   - a real directory reached without following links (Roost's `Directory` handles)
   - no `.orphaned_at` inside
   - contains `.claude-plugin/plugin.json` or a recognizable plugin layout
   - no `:` in the path
4. Inject the **dependency closure**. Plugins that `claude plugin install` auto-installed as dependencies are separate entries in `installed_plugins.json` (the dependency graph is in each manifest's `dependencies`). **docs**: a `--plugin-dir` copy satisfies a dependency, but with the dependency absent the dependent plugin fails as "not installed" unless the profile has it natively. Roost should inject dependencies along with the subscribed plugin, or record them in the set when it adds the plugin.
5. Refuse duplicate manifest names across the injected set, skip missing directories, and join the rest with `:` into `CLAUDE_CODE_PLUGIN_DIRS`. Compute the value fresh at every launch. Never persist a version path into profiles or launchers, so updates take effect at the next launch.
6. Orphan handling: after `roost plugin update` or `remove`, Roost may prune store version directories whose `.orphaned_at` is older than 14 days and that `installed_plugins.json` no longer references. This is the documented grace period. Only prune directories inside `<store>/plugins/cache`, and only through descriptor-based no-follow removal. Alternatively, leave them and report their size in `doctor`. This is an implementation choice for the plugin-store ticket.

**Without parsing undocumented state.** The amended spec asks for this. The [loading reference](https://code.claude.com/docs/en/plugins/loading) documents `installed_plugins.json` as recording "each install with its `scope`, `installPath`, and `version`", but in prose, not as a schema. A fully public route is `claude plugin list --json` run against the store (`id`, `scope`, `version`, `installPath`, verified), but it costs about 0.5 s. The suggested split:

- After every Roost-driven store mutation (`add`, `update`, `remove`, and the opt-in daily update), still under the store lock, run `claude plugin list --json` in the store. Record each id's `installPath` and `version` in Roost's own store record.
- At launch, read only that record, then validate as in step 3.
- Use step 1's direct read of `installed_plugins.json` only as a `doctor` cross-check.

This keeps launch fast and puts Claude's file format outside Roost's contract.

Store mutations come only from Roost (`marketplace add`, `install`, `update`, `uninstall`) under the Roost lock. Claude's background auto-update runs only inside interactive sessions (**docs**), and the store never runs one, so `installed_plugins.json` changes only when Roost changes it.

### 5. claude.ai organization marketplaces don't work in an account-less store

- **verified**: `claude plugin marketplace add --claudeai claudeai-organization-library` in the store fails with `claude.ai marketplaces are not available here — sign in to claude.ai (claude /login) with plugin sync on`. `plugin marketplace list` shows no "From claude.ai" section.
- **docs**: claude.ai-hosted marketplaces and `@synced` plugins need a session signed in with a claude.ai account. Token env vars and `apiKeyHelper` don't count. Git-based marketplaces that claude.ai lists can be added by their git source, which works in the store with the user's git credentials, as already verified for GitHub.
- Consequence: organization-library plugins stay per account, synced into each signed-in profile as `@synced`. An injected plugin of the same name takes precedence over the `@synced` copy (docs). That is the right outcome only if the user meant it, so `doctor` could flag the overlap.

### 6. Other checks

- **verified**: a store built by `plugin marketplace add` / `install` writes `settings.json` (`extraKnownMarketplaces`, `enabledPlugins`), `.claude.json` and `backups/`, besides `plugins/`. These belong to the store and are harmless. The store's `enabledPlugins` doesn't affect profiles.
- **docs**: Node dependency install happens when Claude copies a plugin into the cache, so it ran at store install and update time. Injected directories already contain `node_modules`. Claude never installs dependencies into an in-place (`--plugin-dir`) directory, which is fine because the store's copy already has them.
- **docs**: hooks and MCP servers get `CLAUDE_PLUGIN_ROOT` = the injected store path, so plugin code runs from the store and must be readable by the profile's user. That is the same user.

## Addendum: user-level rules and `@path` imports (orchestrator question)

- **docs**: personal rules in `~/.claude/rules/` apply to every project. All `.md` files are found recursively. Rules without `paths` frontmatter load at launch, and rules with `paths` load when Claude reads or edits a matching file. User rules load before project rules. **docs**: user-scope memory files (`~/.claude/CLAUDE.md` and `~/.claude/rules/`) are trusted: their imports load without the external-import dialog. Only Cowork desktop sessions skip a symlinked or hard-linked `~/.claude/CLAUDE.md`, a symlinked `rules/` directory or rule file pointing outside the working directory, and outside-working-directory imports. So outside Cowork, symlinked rule files and directories are followed. **docs**: circular symlinks are detected. Links to network paths (`\\server\share`, `/net`, `/Network`) are not followed.
- **source-inferred**: the binary computes the user CLAUDE.md as `join(<config dir>, "CLAUDE.md")` and the user rules directory as `join(<config dir>, "rules")` from the same config-dir function. **verified**: that function follows `CLAUDE_CONFIG_DIR`, since the plugin cache under `CLAUDE_CONFIG_DIR` is used. So `$CLAUDE_CONFIG_DIR/rules/**/*.md` is the per-profile equivalent of `~/.claude/rules/`, loaded just like `$CLAUDE_CONFIG_DIR/CLAUDE.md`. This was not observed in a live session, which would need an account.
- **docs**: `@path` imports accept relative paths (relative to the importing file) and absolute paths. `@~/...` home paths are shown in the docs' own example. Imports recurse to at most four hops. Paths with spaces need backslash escapes. Imports inside code spans or fences are ignored.
- **docs + source-inferred** on deduplication: the docs only promise this for `AGENTS.md` ("skips an AGENTS.md it has already loaded… isn't read twice"). The loader threads one `processedPaths` set through CLAUDE.md files, rules and imports. It skips a file whose path, or resolved symlink target, is already in the set. So the same file imported twice, or imported and also linked into `rules/`, loads once per session. This is source-inferred, not observed.
- Implication for later "rules" sharing: linking rule files per item into `<profile>/rules/` works like skills (user scope, symlinks followed, no approval dialog). A config-dir `CLAUDE.md` can also `@`-import absolute or `~` paths. Both reach only owned profiles, since Roost won't write upstream ones.

## Verdict for ADR 0001

The open questions are settled in favor of the decision. Injection gives a stable, documented identity (`name@inline`) with a per-profile opt-out. It wins over same-named native installs. It keeps configuration and data per profile. It writes nothing into the store from sessions. Updates are picked up by recomputing `installPath` at each launch. The limitations (no claude.ai organization marketplaces in the store, orphan versions not swept by the CLI, shadowing of native installs, dependency closure) are implementation rules for the plugin-store ticket, not reasons to change direction.
