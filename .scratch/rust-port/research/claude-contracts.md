# Verify Claude isolation and authentication contracts

Researched 2026-10-05 for a Rust CLI on Linux, macOS, and Windows, with equivalent upstream capabilities, targeted improvements, and safe coexistence. This is research for decisions, not an implementation specification or a completed migration.

Evidence labels: **documented** means current first-party documentation; **source-observed** means implementation inspected at an identified revision; **unverified** means no supported guarantee or runtime evidence was obtained. Recommendations below are proposals, not settled decisions.

## Supported account boundary

**Documented:** `CLAUDE_CONFIG_DIR` supports concurrent work/personal accounts with separate settings, history, and claude.ai login/API key. **Exception:** two Console sign-ins without API keys are not isolated: their Anthropic profiles live outside that directory. Keyless Console login can sign out a stored claude.ai login on the machine. [Multiple accounts](https://code.claude.com/docs/en/authentication#log-in-with-multiple-accounts), [Keyless Console login](https://code.claude.com/docs/en/authentication#sign-in-without-an-api-key).

**Documented credential storage:** macOS uses Keychain keyed to the configuration directory, with a `0600` `.credentials.json` fallback for rejected login writes; Console API-key creation requires writable Keychain. Linux uses `.credentials.json` mode `0600`; Windows uses profile-directory ACLs. The override relocates that credentials file. [Credential management](https://code.claude.com/docs/en/authentication#credential-management).

**Unverified:** Keychain service-name/hash algorithm, path normalization, portability of copied credentials, cleanup after deleting a profile directory, and historical minimum version for directory isolation. The documented per-directory boundary does not specify these internals. Keep an existing profile's path unchanged; let Claude own login/logout and credential access rather than deriving a hash or extracting tokens.

## Authentication environment

**Documented:** priority is provider selection, `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_API_KEY`, `apiKeyHelper`, `CLAUDE_CODE_OAUTH_TOKEN`, named/federation credentials, then `/login`; active user-OAuth profiles rank below working `/login`. Gateway sessions have separate precedence. `setup-token` produces a one-year subscription token for model requests, excluding Remote Control/connectors; `--bare` ignores it. [Precedence](https://code.claude.com/docs/en/authentication#authentication-precedence), [Long-lived tokens](https://code.claude.com/docs/en/authentication#generate-a-long-lived-token).

**Documented:** the OAuth environment token outranks Keychain credentials and persists for the session unless `/login` replaces it; replacing an expired token requires generating another and restarting. The configuration override includes settings, history, and plugins and is ignored in project/local settings. [Environment variables](https://code.claude.com/docs/en/env-vars).

**Proposed decision:** ordinary profile launching should set the directory and leave native authentication to Claude. Decide whether conflicting inherited authentication/provider variables trigger an error, require an explicit override, or pass through visibly. Silently inserting an extracted access token risks changing the effective account and capabilities. Settings-file `env` values also matter: a shell-only filter is not evidence that all credential sources disappeared.

## Configuration is not project isolation

**Documented:** user settings are distinct from shared `.claude/settings.json` and personal `.claude/settings.local.json` in the project. Managed settings, CLI flags, project-local settings, and shared project settings can override user settings. Claude also maintains `.claude.json` for sign-in/MCP/per-project state; do not treat it as an editable application registry. [Settings files and precedence](https://code.claude.com/docs/en/settings).

**Documented:** transcripts, prompt history, and other application data are plaintext; project files and some temp-directory data sit outside the profile directory. [Directory and application data](https://code.claude.com/docs/en/claude-directory).

**Unverified:** full filesystem or process isolation from selecting a profile, or exhaustive relocatable state layouts across supported versions. Two accounts in the same project still share project configuration and files. Safe coexistence must therefore distinguish manager metadata, Claude profile state, project state, and the shared installation.

## Launching, installation, and updates

**Documented:** Linux/macOS/WSL have a Bash native installer; Windows has PowerShell/CMD installers and native or WSL launch options. Git for Windows is optional today; without it Claude uses PowerShell. Native installs auto-update; Homebrew, WinGet, apt/dnf/apk normally use package-manager updates. macOS/Linux native launchers point from `~/.local/bin/claude` into versioned storage. Custom launchers are preserved from v2.1.207 onward, but retain all versions. Windows upgrades can fail while the executable is running. Shared/network storage and in-place npm upgrades can remove a binary still needed by another session; old versions must remain readable. [Advanced setup](https://code.claude.com/docs/en/setup).

**Documented controls:** `DISABLE_AUTOUPDATER=1` stops background updates but permits manual `claude update`/`claude install`; `DISABLE_UPDATES=1` blocks those too. `FORCE_AUTOUPDATE_PLUGINS=1` allows plugin auto-updates when the main updater is disabled. [Environment variables](https://code.claude.com/docs/en/env-vars).

**Documented plugin caveat:** updater-disable variables suppress the marketplace update pass unless forced; command-source plugins re-run once per session independently of `DISABLE_AUTOUPDATER`. Changing disk copies does not automatically switch all running components. [Plugin update loading](https://code.claude.com/docs/en/plugins/loading#which-marketplaces-and-plugins-auto-update), [Command sources](https://code.claude.com/docs/en/plugins/loading#when-a-command-source-re-runs).

**Unverified:** a general updater concurrency guarantee for all installation methods, versions, and profile combinations. Per-profile directories do not establish independent binary installations. A wrapper should not claim that disabling background updates makes all updating impossible or serializes other managers.

## Supported inspection surface

**Documented:** `claude auth login`, `auth logout`, and `auth status` exist. Status provides JSON, returns 0 when logged in and 1 otherwise, and reports `authMethod`; its `configDirectory` field requires v2.1.268+. `claude doctor` is read-only terminal diagnostics. [CLI reference](https://code.claude.com/docs/en/cli-reference#cli-commands).

**Proposed decision:** use the supported CLI for auth diagnostics instead of opening credential files or Keychain. Decide the supported Claude version floor and behavior when status fields are missing. An authenticated-status result proves credential selection, not actual service access or account separation under every configuration.

## Upstream assumptions to avoid promoting into guarantees

**Source-observed by the coordinating upstream investigation:** baseline is upstream v0.1.2, commit `12909e6fc76cfc3b36861c2b33f998585f2690f8`. The wrapper and direct launch set configuration/updater/plugin/token variables; macOS auth detection queries Keychain, while file-backed detection checks existence. The Keychain suffix derives from the first eight SHA-256 hex characters of the raw directory string. These are upstream behaviors, not Anthropic's supported contract. [Shim environment](https://github.com/CarlosTheory/claude-multi-account/blob/12909e6fc76cfc3b36861c2b33f998585f2690f8/src/shims.js#L20), [Launch and auth detection](https://github.com/CarlosTheory/claude-multi-account/blob/12909e6fc76cfc3b36861c2b33f998585f2690f8/src/profiles.js#L123), [Keychain naming](https://github.com/CarlosTheory/claude-multi-account/blob/12909e6fc76cfc3b36861c2b33f998585f2690f8/src/paths.js#L39).

Targeted improvements can replace storage sniffing with official status inspection and avoid token injection in ordinary native-login profiles. Preserve directory identity when adopting profiles; file existence and a successful Keychain lookup alone do not demonstrate effective authentication under inherited environment precedence.

## Decisions still open and acceptance implications

1. Which authentication types does first-release isolation promise? Treat subscription logins/API keys separately from keyless Console profiles; do not promise two isolated keyless Console accounts using the directory override alone.
2. How are inherited auth variables, provider selection, gateway state, and settings-file environment conflicts surfaced? Verify effective authentication before presenting a profile as active; never print secrets.
3. Does importing/adopting an upstream profile preserve its exact path? Moving or renaming a directory needs explicit credential/login and rollback semantics, especially on macOS.
4. Who owns the installed binary and update policy? Prefer invoking the existing executable without rewriting its launcher, shell startup files, or shared update settings. If manager-controlled updates are needed, specify treatment of running sessions and externally initiated updates.
5. Which shells and Claude versions form the acceptance matrix? Cover Bash/Zsh, PowerShell/CMD, Windows-native versus WSL, paths with spaces, argument preservation, stdin/TTY and exit/signal behavior.
6. What does removal mean for adopted profiles? Separate forgetting the manager's association, deleting Claude data, and revoking native credentials; directory deletion is not proven equivalent to logout.

Coexistence acceptance should include two differently authenticated profiles, inherited-token conflict, project-local settings, unchanged adopted profile paths, and a shared binary with sessions already running. Actual account tests require consenting test accounts; this research did not authenticate or inspect real credentials.

## Checks and limits

Read current official authentication, environment, setup, settings, directory, CLI, and plugin-loading pages. Followed section references and searched the pages for the named variables and storage boundaries. No Anthropic implementation source was inspected: source-observed evidence above is explicitly upstream wrapper behavior provided by the coordinating investigation. Direct raw-source fetches for that pinned upstream revision returned cache misses; the upstream ticket owns the source inspection. No real user credentials, login flows, installation, shell changes, updater execution, or cross-platform runtime tests were performed. The local worktree starts as a planning repository with only `README.md`. Verify this documentation against the chosen Claude version during implementation; docs evolve independently of that compatibility floor.
