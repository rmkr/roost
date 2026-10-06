# Roost

Roost is a Rust CLI for selecting Claude configuration profiles while keeping your existing Claude installation. It can create owned profiles, register upstream profiles at their unchanged paths, and launch native browser login and saved conversations.

This is a source-only development build. Local Linux automated tests and disposable Bash/Fish checks pass. macOS, other architectures, native Windows, WSL and real-account acceptance remain unverified; the Windows storage backend currently refuses operations until its handle/ACL protection is implemented. No prebuilt downloads or package publication are provided.

## Build from source

Use Rust/Cargo 1.97.0 and the native C linker/toolchain. From a selected tested checkout:

```sh
cargo test --locked
cargo install --path . --locked --bin roost
roost --version
roost doctor
```

Keep Cargo's normal collision protection: do not force-overwrite an unrelated `roost` executable. Cargo owns its installation root/bin; Roost's profile storage and generated-launcher PATH are separate. Check which executable your shell resolves before use.

Roost requires an existing supported Claude installation on PATH, with Claude Code 2.1.268 or later. Roost does not install Claude. Native Windows targets require the selected `claude.exe`; batch shims are unsupported initially.

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

Registration preserves upstream ownership: Roost does not write/clear borrowed tokens, repair borrowed permissions, change upstream launchers or purge upstream data. Claude still writes its own state when launched. Ordinary removal retains owned profile data/tokens; explicit `remove NAME --purge` deletes verified owned data only, after acknowledging quiet writers. Purge is not logout or credential revocation.

Set owned manager tokens with hidden terminal input or explicit `roost token work --stdin`; never put token values on the command line. Isolated launches reject known inherited auth/provider overrides by default. Deliberate `roost run --allow-auth-env work` preserves caller auth and suppresses manager-token injection, while retaining filesystem/token safety checks. Default aliases are pass-through.

`roost update` delegates to the shared `claude update`; upgrade Roost by explicitly installing the next tested source revision. Compatible state stays intact; unsupported schemas are refused. End Roost operations before replacement, then verify version/doctor and refresh relocated bindings with reuse. `cargo uninstall roost-claude` removes the Cargo executable and retains profile data, launchers and PATH edits.

The accepted [specification](.scratch/rust-port/spec.md) and [implementation record](.scratch/rust-port/implementation.md) contain the behavior, evidence and remaining acceptance gates. Decisions and research remain in the completed [planning map](.scratch/rust-port/map.md).
