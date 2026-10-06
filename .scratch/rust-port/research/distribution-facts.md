# Distribution facts for ticket 05

Checked **2026-10-06** against current primary sources. This note supports the distribution decision; it does not choose names, promise package availability, establish a Claude version floor, or prove cross-platform execution.

## Claude platform and installation facts

**Documented:** macOS 13+, Windows 10 1809+/Server 2019+, Ubuntu 20.04+, Debian 10+, Alpine 3.19+; x64/ARM64, 4 GB+ RAM, internet, supported country. Shells: Bash, Zsh, PowerShell, CMD. Native Windows and WSL 1/2 are supported. Git for Windows is optional; without it Claude uses PowerShell. Install and launch the Linux Claude inside WSL.

Native-platform npm packages cover macOS x64/ARM64, Linux x64/ARM64 with glibc/musl variants, and Windows x64/ARM64. Native installation needs no Node; npm installation requires Node 22+ but installs the same native executable.

Official channels: native Bash/PowerShell/CMD installers, Homebrew, WinGet, apt/dnf/apk, npm. Native installs auto-update; package-manager installs normally use their manager's upgrades. Windows upgrades can fail while the executable is running. Alpine needs Bash/curl for installation and libgcc/libstdc++/ripgrep at runtime, with `USE_BUILTIN_RIPGREP=0`.

Verify installed Claude with `claude --version`; absence usually means PATH needs repair. `claude doctor` gives read-only installation/settings diagnostics. The native Unix launcher is `~/.local/bin/claude`. These facts support discovering existing Claude through PATH without installing, replacing, or moving it. [Official advanced setup](https://code.claude.com/docs/en/setup).

## Browser login and native credential prerequisites

**Documented:** Claude opens the browser for login; if it does not open, copy the supplied URL into a browser. If the browser cannot reach the local callback, Claude accepts the displayed login code in its terminal prompt; WSL2, SSH, and containers commonly need this fallback. A usable browser can therefore be elsewhere, but unattended authorization is not implied.

Native credentials belong to Claude: macOS Keychain, Linux `.credentials.json` mode `0600`, Windows `.credentials.json` inheriting user-profile ACLs. `CLAUDE_CONFIG_DIR` relocates the file and keys the macOS Keychain entry. Locked/rejected macOS Keychain login writes fall back to a `0600` file; Console login creating an API key requires writable Keychain. Native login needs an eligible account and network access.

Separate configuration directories isolate claude.ai logins/API keys; two keyless Console sign-ins remain outside this guarantee. `setup-token` also needs browser authorization and its token lacks Remote Control/connector capabilities. [Official authentication](https://code.claude.com/docs/en/authentication#log-in-to-claude-code), [credential management](https://code.claude.com/docs/en/authentication#credential-management).

## Proposed `ccm-rs` identity: availability unresolved

The public [crates.io package page](https://crates.io/crates/ccm-rs), [API endpoint](https://crates.io/api/v1/crates/ccm-rs), and [sparse index entry](https://index.crates.io/cc/m-/ccm-rs) were attempted. Browser retrieval returned tool errors; a direct public API request outside the restricted DNS sandbox returned **HTTP 403**, without a package record. This is an access failure, **not evidence that the package is absent or available**.

Exact-name web searches for `ccm-rs`, including GitHub/crates.io and `ccm_rs` variants, found no obvious public Rust CLI collision. Search absence does not establish registry availability, reservations, local PATH collisions, or trademark rights. Recheck the registry before publishing; retain `ccm-rs` only as a proposed name until the human naming decision. No package was reserved or registered.

## Pinned upstream attribution

**Source-verified:** upstream commit `12909e6fc76cfc3b36861c2b33f998585f2690f8` contains MIT License, copyright **2026 Carlos Aponte**. Its notice says copies or substantial portions include the copyright and permission notice. This agrees with the existing [upstream research](upstream-behavior.md#attribution-and-remaining-limits). [Pinned official LICENSE](https://raw.githubusercontent.com/CarlosTheory/claude-multi-account/12909e6fc76cfc3b36861c2b33f998585f2690f8/LICENSE).

**Recommendation:** ship the full upstream notice with reused source/templates/docs and identify the upstream author, repository, and pinned revision in attribution. This records the existing notice and a reuse expectation; it is not a legal interpretation or a claim of Anthropic affiliation.

## Remaining implementation evidence

**Recommendations, not settled decisions:** treat native Windows and WSL as separate environments with independent manager installs/storage/PATH and platform-native Claude; use the user's already accepted prebuilt archives plus Cargo installation; make browser/manual-code login instructions platform-aware and leave callbacks, credential storage, and refresh to Claude.

Ticket 06 still needs the chosen Claude version floor and executable-resolution mechanics, plus real acceptance evidence for every promised target/shell: archive/Cargo install and upgrade, existing Claude discovery, argument/TTY/exit handling, native browser/manual-code login, macOS Keychain behavior, Windows ACLs, WSL environment separation, concurrent starts, and unchanged borrowed profile paths. Documentation of ARM64 support is not evidence that this Rust manager or every launcher works there. No account login, credential inspection, installation, update, package publication, or platform runtime test was performed here. Release automation mechanics remain outside ticket 05.

## 2026-10-06 decision update and Fish facts

The human subsequently deferred prebuilt distribution until after a testing period; earlier references here to accepted prebuilt archives are historical, not the current delivery choice. They explicitly require Fish as their caller shell. Other proposed architecture/shell targets and the replacement name remain unsettled.

Fish can invoke external programs through PATH. It reads user `conf.d/*.fish` snippets before the main user configuration, using its configured Fish config directory (normally `~/.config/fish`, with `XDG_CONFIG_HOME` support). `fish_add_path --path` modifies the process PATH rather than persistent universal variables; configuration can apply it on shell startup. These facts support a scoped Fish PATH recipe, not a completed or accepted implementation. [Official Fish configuration](https://fishshell.com/docs/current/language.html#configuration-files), [official fish_add_path](https://fishshell.com/docs/current/cmds/fish_add_path.html). Checked 2026-10-06; no shell configuration was changed or runtime test performed.
