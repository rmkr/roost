# Roost naming snapshot

Checked **2026-10-06**, bounded to the proposed display **Roost**, executable `roost`, launchers `roost-NAME`, and Cargo package candidates. This is public software/namespace research, not a trademark opinion, reservation, or final naming decision.

## Observed public command collisions

| Original project source | Observed spelling and overlap |
| --- | --- |
| [cefege/roost](https://github.com/cefege/roost), [project CLI reference](https://roosttt.com/docs/cli/) | Its official README installs `$HOME/.local/bin/roost`; official CLI reference uses `roost <subcommand>`. It is an AI terminal fleet control plane with Claude Code/Codex integration: direct command and nearby subject collision. |
| [AvesAlight/roost](https://github.com/AvesAlight/roost) | Official README says Homebrew installation puts `roost` on PATH and gives `roost init`, `roost spawn`, `roost list`, and `roost --version`. It manages Claude Code agent teams: direct command and nearby subject collision. |
| [charliek/roost installation guide](https://charliek.github.io/roost/getting-started/installation/) | Official docs install Linux GUI `/usr/bin/roost` and build Cargo package `roost-cli`, whose executable is `roostctl`. This establishes existing public source-package spelling, **not crates.io registration**. |
| [RubixDev/Roost](https://github.com/RubixDev/roost) | Official README copies `target/release/roost-cli` into `/usr/local/bin/roost` or `~/.local/bin/roost`. It is a programming-language interpreter: another exact executable collision. |

These establish that `roost` is already used publicly. They do **not** mean a new project cannot choose `roost`, or that a local user necessarily has a conflict. Retaining it is a human policy choice with disclosed PATH/discoverability ambiguity. Do not replace an existing foreign executable.

## Registry checks and limits

| Candidate Cargo package | What was observed |
| --- | --- |
| `roost` | Public crates.io API browser lookup failed; direct API request returned **HTTP 403**, without a package record. Sparse index and docs.rs latest lookup also failed. Occupancy/availability **unverified**. |
| `roost-cli` | crates.io API, sparse index, and docs.rs lookup failed. Existing official charliek source/build documentation uses this exact Cargo package spelling, but published-registry occupancy **unverified**. Original manifest fetch attempts failed with cache misses. |
| `claude-roost` | crates.io API/index/docs.rs lookup failed; exact-name search found no obvious relevant CLI result. Registry availability **unverified**. Also overlaps upstream `claude-NAME` for an upstream profile named `roost`; package distinctness alone does not fix launcher collision. |
| `roost-claude` | Exact-name search found no obvious relevant CLI result; public crates.io API/index lookup was attempted. Treat registry availability as **unverified**, not available. |

Primary registry endpoints attempted: [roost API](https://crates.io/api/v1/crates/roost), [roost-cli API](https://crates.io/api/v1/crates/roost-cli), [claude-roost API](https://crates.io/api/v1/crates/claude-roost), [roost-claude API](https://crates.io/api/v1/crates/roost-claude). Failed retrieval is not a registry 404, and search absence is not a namespace guarantee. No package reservation, registration, publication, authentication, or local PATH change occurred.

## Smallest recommendation

Keep display **Roost**. If the user wants a less ambiguous executable, `roost-claude` is a candidate package/binary identity preserving the branding and avoiding upstream `claude-NAME`; its registry availability still needs confirmation before publication. Retaining `roost` is also an explicit human choice after reviewing the collisions. `roost-cli` does not solve command ambiguity by itself and already has public Rust source-package use. `roost-NAME` remains the proposed launcher family; collision refusal and generated spelling must follow the approved ownership/command contracts. If `roost-claude` is the binary and a profile can be named `claude`, reserve that launcher filename or choose a separate prefix: that concrete local collision needs a naming decision, not another package search.

Source-only testing can use a repository-local build without declaring any Cargo registry name available. Confirm final package availability immediately before any future publication. Exact software usage examples above were read from primary project sources; registry access remains the factual limitation.
