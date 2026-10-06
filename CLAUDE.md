# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Roost (crate `roost-claude`, binary `roost`) is a Rust CLI that manages Claude Code configuration profiles alongside an existing Claude installation: it creates **owned** profiles under `~/.roost` (or `ROOST_DIR`), **registers upstream** profiles at their existing paths without taking ownership, and launches the real `claude` with the right `CLAUDE_CONFIG_DIR`/token environment. Source-only development build; Linux and macOS are verified (`roost desktop` is Linux-only; macOS temp dirs sit under the `/var` → `/private/var` symlink, so tests canonicalize `temp_dir()`), and native Windows is unsupported (a `compile_error!`; use WSL 2).

Authoritative behavior lives in `.scratch/rust-port/spec.md` (accepted spec, including the workflow UX amendments), with the implementation record and remaining acceptance gates in `.scratch/rust-port/implementation.md`. Use the terms defined in `GLOSSARY.md` (profile vs. account, owned vs. registered upstream, retained profile, manager token, default alias, selected profile, shared set, plugin store, instruction and settings fragments) — the code and docs rely on these distinctions.

## Commands

Toolchain is pinned to Rust 1.97.0 (`rust-toolchain.toml`). If rustup tries to sync the exact-named toolchain, `RUSTUP_TOOLCHAIN=stable` (also 1.97.0 here) avoids it; `--offline` works from the existing cache.

```sh
cargo test --locked                                    # all unit + integration tests
cargo test --locked --test cli                         # integration tests only (tests/cli.rs)
cargo test --locked <test_name>                        # single test by name substring
cargo clippy --locked --all-targets -- -D warnings     # must be clean
cargo fmt --all -- --check
```

CI (`.github/workflows/ci.yml`) runs these three on Linux and macOS for every PR and push to `main`.

`tests/cli.rs` is `#![cfg(unix)]`. Each test builds a disposable fixture (temp dir with private perms, fake `HOME`, `ROOST_DIR`, and a fake `claude` written in Python at `bin/claude`), so `/usr/bin/python3` is required. The fake is steered by `FAKE_CLAUDE_*` env vars (version, mode, exit code). Never test against a real Claude binary, account, credentials, or the user's real PATH/startup files. Some ownership checks require real uid metadata and fail inside sandboxes that remap owners (e.g. to uid 65534); don't weaken those checks to make tests pass.

## Architecture

- `src/main.rs` — crate-root `Error { code, message, next_step, exit_code }` / `Result`, and `dispatch` over `cli::Action`. Every command can emit a `--json` envelope `{schema_version: 1, data, warnings, error}`; error `code`s are stable public strings from the spec. Exit codes: 2 usage, 130 cancelled, 1 otherwise (or the child's exit code for `run`).
- `src/cli.rs` — clap parsing into `Action`. Manager options precede the profile name; everything after NAME is passed opaquely to Claude (one leading `--` consumed). There is a hidden internal action invoked by generated launchers, carrying the bound root, root ID and registration ID.
- `src/store.rs` (largest module) — the manager storage: root marker (`.roost-root.json`), `registry.json`, per-profile marker, token files, and the `.roost-lock` lock (10s timeout, held only while a `Store` lives). Mutations are journaled through `.roost-operation.json` with exact before/staged/after file identities; the registry is written last. `Store::open` with `OpenMode::Read` must never recover or mutate; `Mutate` classifies/recovers pending intents; `RetryPurge` only admits an explicit purge retry. Purge never deletes foreign/unrecorded objects or follows links.
- `src/store/` — submodules of `store.rs`. `side.rs`: the unjournaled side files `state.json` (selections, last use, `desktop_launched`, recorded links and settings entries, Desktop links, plugin auto-update) and `sets.json` (set definitions and subscriptions), replaced atomically under the root lock and keyed by registration ID. `desktop.rs`: per-registration Desktop data folders `<root>/desktop/<ID>/` (journaled `desktop_create` / `desktop_data`). `plugin_store.rs`: the account-less `<root>/plugin-store/` config directory (journaled `store_create`).
- `src/launch.rs` — launcher templates (POSIX sh) binding executable, root, root ID and registration ID; `prepare` builds the child `Command` (rejects a fixed list of inherited auth/provider env vars unless `--allow-auth-env`, which also suppresses manager-token injection); `execute` (Unix `exec`); bounded (10s / 1 MiB) `claude --version` and `auth status` probes with minimum version 2.1.280. Status output is allowlisted — never surface raw child output or secrets.
- `src/select.rs` — selected profile per project (key: the git common directory, else the cwd), bare `roost`, `switch`, the shared arrow-key picker (`choose_with`, also used by `desktop`), last-use recording and the `ls` `@` marker. Launch paths open the store with `OpenMode::Launch`: like `Read` (never recovers) but may replace side files, plus the one journaled `desktop_create`.
- `src/sets.rs` — `roost set ...`, default/copied subscriptions on `add`, and launch-time link reconciliation for owned profiles (`PLACEMENTS` maps item kinds to `skills/`, `rules/`, `agents/`, `commands/`, `output-styles/`). Only links Roost recorded are ever removed; existing profile content wins; `skills/synced`/`plugins/synced` are never touched.
- `src/settings.rs` — settings fragments (`hooks`, `statusLine`, `outputStyle`; ADR 0003) merged at launch into an owned profile's `settings.json` via an order- and number-text-preserving `Json` document, write-ahead records in `state.json`, and an atomic compare-and-replace that skips with a warning if the file changed.
- `src/plugins.rs` — `roost plugin ...` (runs Claude's own `claude plugin` CLI with `CLAUDE_CONFIG_DIR=<store>` under the separate store lock, never the root lock), Roost's store record from `claude plugin list --json`, `CLAUDE_CODE_PLUGIN_DIRS` injection at launch (ADR 0001), the opt-in daily auto-update, orphan pruning and doctor's `plugin_shadowed`.
- `src/desktop.rs` — `roost desktop` (experimental, Linux only; ADR 0002): `claude-desktop --user-data-dir=<folder>` with the profile's isolated environment, detached by default, the Desktop picker, `SingletonLock` running checks, pidfd-verified `--close`, and `--link`/`--unlink` borrowing of the conventional folder, which Roost never writes.
- `src/table.rs` — human table renderer (display-width alignment, marker column, terminal-only color honoring `NO_COLOR`, width fitting, summary line) for `ls` and the pickers.
- `src/platform/` — `mod.rs` holds shared helpers (path normalization, `ROOST_DIR`, token validation, safe profile copy with exclusions such as `.credentials.json`/`.claude.json`). `unix.rs` implements a descriptor-backed `Directory` (no-follow opens, identity checks via device/inode, private-permission enforcement only for managed dirs), hidden terminal token input, confirmation and SIGINT handling; `unix/path_setup.rs` writes the delimited PATH block / Fish snippet. Native Windows is refused at compile time in `mod.rs`; use WSL 2.

Key invariants that cut across modules:
- File operations go through `Directory` handles and compare `FileIdentity` before acting, to defeat swap/symlink races; don't replace these with path-only checks.
- `main` drops the `Store` (releasing the lock) before prompting, reading tokens, or running a child; after reacquiring, re-verify root ID and registration evidence before mutating.
- Upstream (registered) profiles are borrowed: never write/clear their tokens, repair their permissions, alter their launchers, or purge their data. Default aliases pass the caller's environment through unchanged.
- Token values never go on the command line, into errors, or into logs.
- Launch-time side effects (selection/last use, link and settings reconciliation, plugin auto-update and injection) warn and never block the launch.

## Planning archive

`.scratch/` is a read-only archive of the completed rust-port and workflow-ux efforts (Markdown tracker conventions in `.scratch/README.md`); workflow-ux decisions are in `.scratch/workflow-ux/design.md` and ADRs in `docs/adr/`.

## Agent skills

### Issue tracker

GitHub issues on `rmkr/roost`: a spec is a parent issue, its tickets are sub-issues, order uses GitHub's "blocked by" relations, and PRs close their issues. See `docs/agents/issue-tracker.md`.

### Triage labels

The five default triage labels (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `GLOSSARY.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.
