# Roost

![Roost: friendly birds sharing one perch, each with its own profile](docs/banner.png)

Roost is a Rust CLI for selecting Claude configuration profiles while keeping your existing Claude installation. It can create owned profiles, register upstream profiles at their unchanged paths, and launch native browser login and saved conversations. It also remembers which profile each project uses, shares skills, instructions, settings and plugins across profiles, and can start Claude Desktop per profile.

This is a source-only development build. Local Linux automated tests and disposable Bash/Fish checks pass, and the automated tests pass on macOS (Apple silicon), where `roost desktop` is unavailable. Other architectures, native Windows, WSL and real-account acceptance remain unverified; the Windows storage backend currently refuses operations until its handle/ACL protection is implemented. No prebuilt downloads or package publication are provided.

Terms such as profile, account, owned, registered upstream, selected profile and shared set are defined in the [glossary](GLOSSARY.md).

## Build from source

Use Rust/Cargo 1.97.0 and the native C linker/toolchain. From a selected tested checkout:

```sh
cargo test --locked
cargo install --path . --locked --bin roost
roost --version
roost doctor
```

Keep Cargo's normal collision protection: do not force-overwrite an unrelated `roost` executable. Cargo owns its installation root/bin; Roost's profile storage and generated-launcher PATH are separate. Check which executable your shell resolves before use.

Roost requires an existing supported Claude installation on PATH, with Claude Code 2.1.280 or later. Roost does not install Claude. Native Windows targets require the selected `claude.exe`; batch shims are unsupported initially.

## First profile and login

```sh
roost add work
roost run work auth login
roost status work
roost run work --name client-a
```

Claude owns browser authorization, credentials and refresh. Use its manual URL/code flow when automatic browser opening is unavailable. Profile selection does not prove account identity; keyless Console sign-ins are outside the isolation promise. Clear an owned manager token with `roost token work --clear` before relying on native browser login.

Start concurrent conversations in separate terminals and use native resume/continue/fork:

```sh
roost run work --name client-b
roost run work --resume client-a
roost run work --continue
roost run work --resume client-a --fork-session
```

Run's manager options precede the profile name. Arguments after NAME belong to Claude; one optional initial `--` is consumed. Generated launchers forward every caller argument literally.

## A profile per project

Each project remembers its selected profile. Inside a git repository the project is the repository, so all its worktrees share one selection; elsewhere it is the current directory.

```sh
roost                    # launch this project's selected profile
roost -- --continue      # same, passing arguments to Claude
roost switch             # pick a profile, remember it here, launch it
roost switch work        # select work here and launch it
roost switch --no-launch work
roost switch --forget
```

Bare `roost` opens an arrow-key picker when the project has no selection, or its selected profile was removed. Up/Down or k/j move, Enter chooses, and Ctrl-C, Esc or `q` cancel (exit 130). The choice is remembered. Without a terminal there is no picker: bare `roost` fails and names `roost switch NAME` and `roost run NAME`. `roost run NAME` launches without changing the selection. A default alias can be selected too.

## Listing profiles

```sh
roost ls
roost ls --full
roost ls --retained
roost list --json
```

`roost ls` (an alias of `list`) prints an aligned table: `@` marks this project's selection, followed by Profile, Kind, Token, Launchers, Sets and Last used, and a summary line naming any columns hidden to fit the terminal. It is colored on a terminal (unless `NO_COLOR` is set) and plain when piped. Default `ls` reads metadata only. `--full` adds Login, Auth and Claude dir columns from bounded `claude auth status` probes run in parallel. Token presence is not login validity.

## Shared sets

A shared set is a named list of items that owned profiles subscribe to. Roost links each item into the subscribed profile at every launch; it never copies, owns or edits the sources, which stay in directories you manage.

| Item | Placed in an owned profile as |
| --- | --- |
| `--skill DIR`, `--skills-from DIR` | link under `skills/` |
| `--instruction FILE`, `--instructions-from DIR` | link under `rules/` (one topic per `.md` file) |
| `--agent FILE`, `--agents-from DIR` | link under `agents/` |
| `--command FILE`, `--commands-from DIR` | link under `commands/` |
| `--output-style FILE`, `--output-styles-from DIR` | link under `output-styles/` |
| `--setting FILE`, `--settings-from DIR` | merged into the profile's `settings.json` |
| `--plugin PLUGIN@MARKETPLACE` | injected from the [plugin store](#plugin-store) |

A `*-from DIR` item covers every child of that directory (skill directories, or `.md`/`.json` files), so new files are picked up at the next launch. A worked layout:

```text
~/.agents/
  skills/           one directory per skill
  instructions/     commits.md, subagents.md, ...
  agents/           reviewer.md, ...
  settings/         statusline.json, atuin.json, ...
  statusline-command.sh
```

```sh
roost set create core --default
roost set add core --skills-from ~/.agents/skills
roost set add core --instructions-from ~/.agents/instructions
roost set add core --agents-from ~/.agents/agents
roost set add core --settings-from ~/.agents/settings
roost set subscribe personal core
roost set list
```

A default set is subscribed to every owned profile created later by `roost add` (`--no-sets` skips it); `roost add NAME --copy-default --source DIR` from an owned profile copies that profile's subscriptions instead. Use `roost set default core --off`, `set drop`, `set unsubscribe` and `set delete` to undo; links Roost created leave at the profile's next launch.

A settings fragment is a small JSON file for one concern, containing only `hooks`, `statusLine` or `outputStyle`. For example, `~/.agents/settings/statusline.json`:

```json
{
  "statusLine": {
    "type": "command",
    "command": "~/.agents/statusline-command.sh"
  }
}
```

Hooks from all fragments are combined with the profile's own hooks. If a profile sets its own, different `statusLine` or `outputStyle` (for example with `/config`), that value wins and Roost stops managing the key there. Roost writes `settings.json` only when the result changes, keeps every other key, and skips with a warning if the file changed underneath it.

Rules worth knowing:

- Content already in a profile always wins; Roost only adds and removes links and entries it recorded.
- Two subscribed sets providing the same name, or two fragments setting the same `statusLine`/`outputStyle`, are refused at subscribe time; a conflict found at launch warns and skips that item.
- `set add` refuses a missing source. A source that disappears later is skipped with a warning at launch, and its link is removed.
- A problem during reconciliation warns and the launch continues.
- Only owned profiles receive linked items and settings. Registered upstream profiles receive plugins only, and default aliases receive nothing. A profile's `CLAUDE.md` and Claude's `skills/synced` and `plugins/synced` are never touched.

If you also run plain `claude`, you can keep `~/.claude/CLAUDE.md` as a list of `@~/.agents/instructions/...` imports of the same fragments; Claude loads a fragment imported there and linked into `rules/` only once.

## Plugin store

Shared plugins are installed once into a Roost-owned plugin store (`<root>/plugin-store/`, a Claude config directory with no account) using Claude's own installer, then injected into subscribed profiles at launch through `CLAUDE_CODE_PLUGIN_DIRS`. Nothing is written into the profiles. See [ADR 0001](docs/adr/0001-shared-plugins-injected-at-launch.md).

```sh
roost plugin marketplace add OWNER/REPO
roost plugin add PLUGIN@MARKETPLACE --set core
roost plugin add PLUGIN --no-set
roost plugin list
roost plugin update            # all store plugins, or name one
roost plugin auto-update --on
roost plugin remove PLUGIN
```

`marketplace add` passes SOURCE to `claude plugin marketplace add` unchanged. `plugin add` without `--set` or `--no-set` asks which sets on a terminal and fails without one. Injected plugins appear in Claude as `name@inline`, and Claude does not update them itself: use `roost plugin update`, or `auto-update --on` to update at most once a day at launch (short timeout; failure warns and still launches). A profile can turn one off with `"<name>@inline": false` in its own settings. `roost plugin remove` drops the plugin from every set and uninstalls it; profiles lose it at their next launch. `roost doctor` reports a store plugin that is also installed natively in an owned profile.

Marketplace and install use your own git credentials; marketplaces hosted on claude.ai (organization libraries) cannot be added to the store. `roost update` still only runs `claude update`.

## Claude Desktop (experimental, Linux)

`roost desktop` starts Claude Desktop with a profile's configuration and its own Desktop data folder, so each profile has its own Desktop sign-in. It relies on Electron's undocumented `--user-data-dir`, which a Desktop update may break; `roost doctor` checks that the data actually landed in the profile's folder. See [ADR 0002](docs/adr/0002-desktop-per-profile-via-user-data-dir.md).

```sh
roost desktop                  # picker, then launch
roost desktop work             # launch work's Desktop, detached
roost desktop --foreground work
roost desktop --close work
roost desktop --link work      # work borrows the existing Desktop folder
roost desktop --link --replace work
roost desktop --unlink work
```

- Desktop starts detached; Roost waits up to 2 seconds for early failure, then exits. `--foreground` stays attached with Desktop's output, for debugging.
- The picker shows each profile's Desktop state (`running`, `signed in`, `—`). In it, `x` closes the highlighted profile's running Desktop.
- `--close` asks the profile's running Desktop to quit (SIGTERM, up to 10 seconds) after verifying the process; it never force-kills.
- Launching a profile whose Desktop is already running is refused. Different profiles' Desktops can run at once, but only the first to use Cowork gets it; the other cannot use Cowork until both are closed. Roost warns when another Desktop is running.
- `--link NAME` lets one owned or upstream profile use the existing, already signed-in folder (`~/.config/Claude`) in place. Roost never copies, moves or deletes it, and the default alias then shares the same instance. If NAME already has its own Roost Desktop folder, `--link --replace` deletes that folder after confirmation (`--yes` skips it).
- The default alias starts plain `claude-desktop` with its usual folder.

Desktop folders live in Roost's storage, not inside profiles. `remove` keeps an owned profile's folder; `remove --purge` deletes it; removing an upstream registration deletes its Roost Desktop folder after confirmation. A borrowed folder is never touched.

## Fish and other shells

```sh
roost setup-path --shell fish
roost setup-path --shell fish --apply
roost-work
```

Default PATH setup prints instructions. Explicit apply edits one user startup block/snippet; Fish uses `roost-path.fish` and `fish_add_path --path` without universal-variable changes. Restart the shell afterward. Bash, Zsh and POSIX sh recipes are also available on Unix. Adding Cargo's executable bin directory to PATH remains a separate installation step.

## Storage and existing profiles

Storage defaults to `~/.roost`; `ROOST_DIR` selects the entire manager root. It does not change your home, default-copy source or Cargo root. Names are ASCII, case-insensitive for manager lookup, and keep their stored spelling.

```sh
roost register work --path /absolute/upstream-home/.ccm/profiles/work
roost list --json
roost where work
roost remove work
roost list --retained
roost reuse work
```

Registration preserves upstream ownership: Roost does not write/clear borrowed tokens, repair borrowed permissions, change upstream launchers or purge upstream data. Claude still writes its own state when launched. Selections, last-use times and set subscriptions are kept in Roost's own storage, never in upstream data. Ordinary removal retains owned profile data/tokens and subscriptions (so `reuse` restores them) and clears project selections naming the profile; explicit `remove NAME --purge` deletes verified owned data only, after acknowledging quiet writers. Purge is not logout or credential revocation.

Set owned manager tokens with hidden terminal input or explicit `roost token work --stdin`; never put token values on the command line. Isolated launches reject known inherited auth/provider overrides by default. Deliberate `roost run --allow-auth-env work` preserves caller auth and suppresses manager-token injection, while retaining filesystem/token safety checks. Default aliases are pass-through.

`roost update` delegates to the shared `claude update`; upgrade Roost by explicitly installing the next tested source revision. Compatible state stays intact; unsupported schemas are refused. End Roost operations before replacement, then verify version/doctor and refresh relocated bindings with reuse. `cargo uninstall roost-claude` removes the Cargo executable and retains profile data, launchers and PATH edits.

## Further reading

- [Specification](.scratch/rust-port/spec.md): exact command grammar and behavior, including the workflow UX amendments.
- [Implementation record](.scratch/rust-port/implementation.md): evidence and remaining acceptance gates.
- ADRs: [plugin store](docs/adr/0001-shared-plugins-injected-at-launch.md), [Desktop per profile](docs/adr/0002-desktop-per-profile-via-user-data-dir.md), [shared settings](docs/adr/0003-shared-settings-written-into-owned-profiles.md).
- [Glossary](GLOSSARY.md), [workflow UX design](.scratch/workflow-ux/design.md) and the completed planning maps for [the Rust port](.scratch/rust-port/map.md) and [workflow UX](.scratch/workflow-ux/map.md).
