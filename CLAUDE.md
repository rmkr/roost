# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Roost (crate `roost-claude`, binary `roost`) is a Rust CLI that manages Claude Code configuration profiles alongside an existing Claude installation: it creates **owned** profiles under `~/.roost` (or `ROOST_DIR`), **registers upstream** profiles at their existing paths without taking ownership, and launches the real `claude` with the right `CLAUDE_CONFIG_DIR`/token environment. Source-only development build; Linux is the only verified platform, and the Windows storage backend deliberately refuses every operation.

Authoritative behavior lives in `.scratch/rust-port/spec.md` (accepted spec), with the implementation record and remaining acceptance gates in `.scratch/rust-port/implementation.md`. Use the terms defined in `GLOSSARY.md` (profile vs. account, owned vs. registered upstream, retained profile, manager token, default alias) — the code and docs rely on these distinctions.

## Commands

Toolchain is pinned to Rust 1.97.0 (`rust-toolchain.toml`). If rustup tries to sync the exact-named toolchain, `RUSTUP_TOOLCHAIN=stable` (also 1.97.0 here) avoids it; `--offline` works from the existing cache.

```sh
cargo test --locked                                    # all unit + integration tests
cargo test --locked --test cli                         # integration tests only (tests/cli.rs)
cargo test --locked <test_name>                        # single test by name substring
cargo clippy --locked --all-targets -- -D warnings     # must be clean
cargo fmt --all -- --check
```

`tests/cli.rs` is `#![cfg(unix)]`. Each test builds a disposable fixture (temp dir with private perms, fake `HOME`, `ROOST_DIR`, and a fake `claude` written in Python at `bin/claude`), so `/usr/bin/python3` is required. The fake is steered by `FAKE_CLAUDE_*` env vars (version, mode, exit code). Never test against a real Claude binary, account, credentials, or the user's real PATH/startup files. Some ownership checks require real uid metadata and fail inside sandboxes that remap owners (e.g. to uid 65534); don't weaken those checks to make tests pass.

## Architecture

- `src/main.rs` — crate-root `Error { code, message, next_step, exit_code }` / `Result`, and `dispatch` over `cli::Action`. Every command can emit a `--json` envelope `{schema_version: 1, data, warnings, error}`; error `code`s are stable public strings from the spec. Exit codes: 2 usage, 130 cancelled, 1 otherwise (or the child's exit code for `run`).
- `src/cli.rs` — clap parsing into `Action`. Manager options precede the profile name; everything after NAME is passed opaquely to Claude (one leading `--` consumed). There is a hidden internal action invoked by generated launchers, carrying the bound root, root ID and registration ID.
- `src/store.rs` (largest module) — the manager storage: root marker (`.roost-root.json`), `registry.json`, per-profile marker, token files, and the `.roost-lock` lock (10s timeout, held only while a `Store` lives). Mutations are journaled through `.roost-operation.json` with exact before/staged/after file identities; the registry is written last. `Store::open` with `OpenMode::Read` must never recover or mutate; `Mutate` classifies/recovers pending intents; `RetryPurge` only admits an explicit purge retry. Purge never deletes foreign/unrecorded objects or follows links.
- `src/launch.rs` — launcher templates (sh / `.cmd` / `.ps1`) binding executable, root, root ID and registration ID; `prepare` builds the child `Command` (rejects a fixed list of inherited auth/provider env vars unless `--allow-auth-env`, which also suppresses manager-token injection); `execute` (Unix `exec`, Windows spawn/wait); bounded (10s / 1 MiB) `claude --version` and `auth status` probes with minimum version 2.1.268. Status output is allowlisted — never surface raw child output or secrets.
- `src/platform/` — `mod.rs` holds cross-platform helpers (path normalization, `ROOST_DIR`, token validation, safe profile copy with exclusions such as `.credentials.json`/`.claude.json`). `unix.rs` implements a descriptor-backed `Directory` (no-follow opens, identity checks via device/inode, private-permission enforcement only for managed dirs), hidden terminal token input, confirmation and SIGINT handling; `unix/path_setup.rs` writes the delimited PATH block / Fish snippet. `windows.rs` is a fail-closed stub.

Key invariants that cut across modules:
- File operations go through `Directory` handles and compare `FileIdentity` before acting, to defeat swap/symlink races; don't replace these with path-only checks.
- `main` drops the `Store` (releasing the lock) before prompting, reading tokens, or running a child; after reacquiring, re-verify root ID and registration evidence before mutating.
- Upstream (registered) profiles are borrowed: never write/clear their tokens, repair their permissions, alter their launchers, or purge their data. Default aliases pass the caller's environment through unchanged.
- Token values never go on the command line, into errors, or into logs.

## Planning tracker

`.scratch/` is a local Markdown tracker (`map.md` + numbered `issues/` tickets with `Status`/`Assignee`/`Blocked by` metadata); see `.scratch/README.md` for the claim/resolve conventions before editing tickets. The rust-port map is complete.
