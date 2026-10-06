# Workflow UX design

Agreed with the user on 2026-10-06 in a grilling session. Vocabulary follows the [glossary](../../GLOSSARY.md): **selected profile**, **shared set**, **plugin store**. Decisions that change the accepted [Roost specification](../rust-port/spec.md) are listed under [Spec changes](#spec-changes) and land through [Amend the specification for workflow UX](issues/01-amend-spec.md).

## Selected profile and switching

- A project's selected profile is remembered per project. The project key is the git common directory, so all worktrees of a repo share one selection; outside git it is the current directory.
- Bare `roost` launches the selected profile. `roost -- <args>` passes Claude arguments; `roost --help` still prints help.
- A project with no selection, or whose selection names a profile that no longer exists (warning first), shows an arrow-key picker. Rows reuse the `ls` columns; the most recently launched profile is highlighted. The choice is remembered. Ctrl-C exits 130.
- Picking happens even when only one profile exists.
- With no terminal, the picker is never shown: fail with a usage error naming `roost switch NAME` or `roost run NAME`.
- `roost switch NAME` selects for this project and launches; `--no-launch` only selects; `--forget` clears the selection.
- The default alias may be picked or switched to, and is remembered. Launching the alias directly records nothing.
- `run`, `switch` and profile launchers record the most recently launched profile. Registered upstream profiles are recorded too; the record lives in manager storage, never in their data.
- `remove` and `purge` clear selections naming that profile, so a later profile with the same name inherits nothing.
- Selection state is small and frequently written; it should not require the registry's journaled mutation path.

## `roost ls`

Worktrunk-style table, colored on a terminal and plain when piped; `--json` unchanged in shape apart from added fields.

```
  Profile   Kind      Token  Launchers  Sets          Last used
@ personal  owned     ✓      ready      core,writing  2m
  work      owned     ✓      ready      core          3d
  ccm-old   upstream  –      ready      —             never

○ 3 profiles · 1 upstream · @ selected here · hidden: Path
```

- Default `ls` stays metadata-only and fast.
- `ls --full` adds probed columns: login state, auth method, observed Claude directory. Probes run in parallel and stay within the existing bounded, allowlisted status rules.
- `--retained` keeps its meaning.

## Shared sets

- A shared set is named and holds skills and plugins. Items are explicit names, or "everything from this source" entries over a source directory the user manages (for example `~/.claude/skills`). Roost links to skill sources; it never owns or installs skills.
- Profiles subscribe to sets. Only owned profiles receive linked items (skills). Registered upstream profiles may receive injected items (plugins), since injection writes nothing into their data. The default alias receives nothing.
- A set may be marked default. `roost add` subscribes the new owned profile to every default set; `--no-sets` overrides. `add --copy` from an owned profile copies its subscriptions.
- Links are reconciled at every launch: Roost adds and removes only the links it recorded in that profile. Reconcile failure warns and still launches.
- Conflicts: content already in the profile always wins and is never replaced. Two subscribed sets providing the same name are refused at subscribe time; a conflict found at launch warns and skips the item.
- Never link `skills/synced` or `plugins/synced`; they are per account and Claude-managed.
- v1 item kinds: skills (linked), instruction fragments (linked) and plugins (injected). Later: agents, commands and output-styles (linked), and hooks/settings through a shared `--settings` file.

## Shared instructions

Added 2026-10-06, reversing the earlier "`CLAUDE.md` is not shared" decision.

- Instructions are shared as **instruction fragments**: one topic per `.md` file, kept in a directory the user manages (for example `~/.agents/instructions/`). Roost never owns or edits them.
- A set's instruction items are linked into an owned profile's `rules/` folder and reconciled at launch exactly like skills (same recording, conflict and purge rules). A profile's own `CLAUDE.md` is never touched.
- Fallback if `rules/` in the config directory does not load like `CLAUDE.md`: a Roost-managed delimited import block of `@path` lines in the profile's `CLAUDE.md`, leaving content outside the block to the user. See [Investigate injected plugin behavior](issues/06-injected-plugin-behavior.md) for the check.
- Settled 2026-10-06: the import-block fallback is not needed and was not built. The [injected plugin research addendum](research/injected-plugins.md#addendum-user-level-rules-and-path-imports-orchestrator-question) found that Claude derives the user rules directory from the same config-dir function as the user `CLAUDE.md`, so `$CLAUDE_CONFIG_DIR/rules/` is the per-profile equivalent of `~/.claude/rules/` and follows symlinked rule files. Instruction items are linked into `rules/` ([Share instruction fragments through sets](issues/11-shared-instructions.md)).
- No section-level selection inside one file: split large files into fragments instead.
- Owned profiles only; registered upstream profiles and the default alias receive no instructions.
- The user's `~/.claude/CLAUDE.md` is split into fragments and placed in a default set (for example `core`); the original becomes a thin list of imports of the same fragments so projects under `$HOME` do not load them twice.

## Plugin store

See [ADR 0001](../../docs/adr/0001-shared-plugins-injected-at-launch.md) (accepted 2026-10-06 after the [injected plugin research](research/injected-plugins.md)).

- The plugin store is a Roost-owned Claude config directory that never runs sessions and has no account. Roost writes it only through Claude's own CLI, under the Roost lock.
- `roost plugin marketplace add SOURCE` passes through to `claude plugin marketplace add` in the store.
- `roost plugin add PLUGIN[@MARKETPLACE] [--set NAME... | --no-set]` installs through `claude plugin install` and adds it to the named sets. With neither flag it asks which sets (including none); with no terminal it fails naming both flags.
- Subscribed profiles receive store plugins at launch through `CLAUDE_CODE_PLUGIN_DIRS`; they appear as `name@inline`.
- `roost plugin update [PLUGIN]` runs Claude's updater against the store. An opt-in setting runs it at launch at most once per 24 hours, before Claude starts, with a short timeout; failure warns and still launches. `roost update` remains exactly `claude update`.
- `roost plugin remove PLUGIN` removes it from every set and uninstalls it from the store. Profiles lose it at their next launch; no profile cleanup is needed.
- Verified 2026-10-06: marketplace add and install from GitHub succeed in a credential-free config directory, using the user's git credentials. claude.ai organization marketplaces are untested.
- Roost's minimum supported Claude version rises to 2.1.280 (`CLAUDE_CODE_PLUGIN_DIRS`).

## Desktop

See [ADR 0002](../../docs/adr/0002-desktop-per-profile-via-user-data-dir.md).

- `roost desktop NAME` (experimental) launches `claude-desktop --user-data-dir=<dir>` with `CLAUDE_CONFIG_DIR` set to the profile, so the Desktop account, Code-tab settings, history, MCP config and shared sets all match the profile. The user signs in once per profile.
- The Desktop data folder lives in Roost's own tree per registration, not inside the profile, so upstream profiles are supported and Electron's cache stays out of `add --copy`. `remove` keeps it with the retained profile; `purge` deletes it.
- The default alias launches plain `claude-desktop` with its usual data folder.
- Different profiles' Desktops may run at once (verified). If the same profile's Desktop is already running, refuse with a message instead of letting it silently focus. Warn that Cowork is untested when another Desktop is running: its helper socket path is fixed and owned by the first instance.
- `doctor` checks that a profile's Desktop data actually landed in its folder.

## Spec changes

- Bare `roost` launches the selected profile instead of printing help; `roost -- <args>` is new syntax.
- New commands: `switch`, `set ...`, `plugin ...`, `desktop`.
- Launch becomes a mutation for skill reconciliation and selection recording; reconcile failures do not block launch.
- Isolated runs inject `CLAUDE_CODE_PLUGIN_DIRS` in addition to the variables the spec allows today; upstream launches may receive it too.
- Minimum Claude version 2.1.280.
- New manager storage: selection state, set definitions and subscriptions, plugin store, Desktop data folders; purge and remove rules for each.
- `list` JSON ProfileRecord gains sets, last-used and selection fields.

## Migration

The `personal` profile currently has 60 hand-made, unrecorded links in `skills/` pointing at `~/.claude/skills/*`. Roost treats them as foreign. Once skill sets ship, remove them by hand and subscribe `personal` to a set covering `~/.claude/skills`.

## Desktop: borrowing the existing data folder

Added 2026-10-06. A profile can borrow the conventional Claude Desktop data folder in place (`roost desktop --link NAME`, undone with `--unlink`), so `roost desktop NAME` opens the already signed-in Desktop with that profile's configuration. Roost never copies, moves, repairs or deletes the borrowed folder; only one profile may borrow it, and the default alias then shares it. See [Let a profile borrow the existing Claude Desktop data folder](issues/16-desktop-link-existing.md).

## Shared settings

Agreed 2026-10-06 (Q65–Q77); see [ADR 0003](../../docs/adr/0003-shared-settings-written-into-owned-profiles.md).

- **Settings fragments** are small JSON files, one per concern (for example `atuin.json`, `cbm.json`, `herdr.json`, `statusline.json`), in a directory the user manages such as `~/.agents/settings/`, carried by sets like instruction fragments. Allowed keys: `hooks`, `statusLine`, `outputStyle`; anything else is refused at `set add`. Hook scripts stay where they are.
- At launch of an owned profile, Roost merges the subscribed fragments and reconciles them into the profile's own `settings.json`, writing only when the result differs from what it recorded. Owned profiles only; upstream and the default alias get nothing.
- Hooks combine across fragments and with the profile's own hooks; an identical handler is not duplicated, and a Roost-added hook removed by hand is re-added. `statusLine`/`outputStyle`: two subscribed fragments setting the same key are refused at subscribe time; a profile's own, different value wins and Roost stops managing that key there, while a recorded value deleted by hand is re-added.
- Writes are atomic compare-and-replace that preserve all other keys and their order in Claude's 2-space JSON style; a concurrent change skips the update with a warning until the next launch. An invalid fragment is skipped with a warning.

