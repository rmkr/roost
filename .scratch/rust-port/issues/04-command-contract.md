# Choose the Rust command contract and targeted improvements

Type: grilling
Labels: wayfinder:grilling
Status: resolved
Assignee: Codex (chat 01a10c8f-27ab-7672-8383-5120cf0007c6)
Parent: [Find the way to a Rust Claude profile manager](../map.md)
Blocked by: 01, 02, 03

## Question

Which CLI commands, options, launch behavior, and errors deliver equivalent upstream capabilities under the chosen coexistence contract, and which targeted improvements belong in the first release?

Use the upstream inventory to account for every capability. Settle argument forwarding, interactive prompts and noninteractive use, exit behavior, diagnostic output, naming validation, PATH setup, and updates where they affect observable behavior. List retained, changed, or deliberately excluded behavior with reasons. Ask the user about trade-offs rather than treating recommendations as decisions.

## Comments

### 2026-10-05: behavior choices exposed by the baseline

The [upstream baseline](../research/upstream-behavior.md) records permissive flag parsing, heuristic login display, inherited update environment, and incomplete signal/argument edge coverage. The [Claude contract research](../research/claude-contracts.md) identifies an official authentication status command as a possible alternative to credential-file and Keychain heuristics. These are candidates for targeted improvements, pending the user's decisions.

### 2026-10-05: ownership contract resolved; this ticket is now unblocked

Work within [Define safe profile coexistence and ownership](03-profile-compatibility.md#answer). Settle exact registration/reuse/removal/purge commands, environment-conflict variables and explicit override syntax, configuration-copy selection/link handling, authentication/status presentation, token input/noninteractive behavior, error/recovery output, and process-local/manual update behavior. Preserve its approved ownership and safety boundaries; CLI details remain human decisions. This ticket remains open and unassigned.

### 2026-10-05: claimed for the command-contract conversation

Claimed in chat `01a10c8f-27ab-7672-8383-5120cf0007c6` after rechecking that all three blockers are resolved. The canonical ownership Answer and glossary remain binding. No command decision is accepted yet. Native subagents are checking capability coverage and current authentication/update documentation; this is planning only.

### 2026-10-05: human decision round 1 pending

The independent policy choices put to the user are:

1. Command shape: familiar flat upstream commands/aliases, including default-alias/copy creation and explicit purge; add explicit registration, retained-data reuse, and supported auth status. Recommend strict manager parsing and command-specific help, with no rename, active-profile switching, import/export, per-profile binaries, or completions in the first release. Exact option/forwarding boundaries follow after this choice.
2. Lifecycle: recommend explicit single-path upstream registration without automatic discovery; new creation refuses registered names or retained data; explicit reuse restores retained owned data. Provide idempotent refresh of Rust-owned launchers for an unchanged existing registration; do not reinterpret data or replace a mapping/kind silently. Exact refresh syntax follows after command shape.
3. Copy breadth: recommend preserving upstream's broad settings/plugin/history data seeding, excluding known credentials/manager tokens and transient data. No selectable copy categories in the first release. Exact source/exclusion/link behavior follows after breadth is answered.
4. Listing/diagnostics: recommend fast metadata-only listing (ownership/kind, token presence, paths, launcher state), a separate explicit supported Claude auth-status operation, and JSON for list/status/doctor. Do not label token/file existence as logged-in or an effective-account proof. Exact fields and statuses follow after this split.
5. Names: recommend upstream's 1-32 ASCII characters, alphanumeric first, then alphanumeric/underscore/hyphen; preserve spelling, require case-insensitive uniqueness on every platform, reject Windows reserved device names, and validate read commands too.
6. Updates: recommend retaining process-local main-updater disable plus plugin auto-update enable for isolated profiles, leaving default aliases pass-through. Manual update delegates to the shared Claude updater and respects an inherited explicit manual-update prohibition. Exact update environment/recovery output follows after this policy.
7. PATH: recommend printing setup instructions by default and writing only on explicit apply, scoped to one chosen Unix shell file or Windows user PATH. No mass shell-file edits or automatic uninstall cleanup; concrete syntax and idempotent edit behavior follow after this policy.
8. Custom roots: recommend configurable Rust storage independent from Claude's default data, with default copy resolving the caller's effective Claude config directory and allowing an explicit source directory. Never reinterpret a manager-storage override as a different user home. Exact variable names/locations remain with naming/distribution; source selection syntax remains here.

These are recommendations, not decisions. Await the user's answers before dependent choices. Capability audit confirmed the upstream inventory and identified existing-registration launcher refresh, default-alias path semantics, and custom-root/source behavior as details to cover. Current first-party authentication-variable research is available for the subsequent conflict-coverage round; native credential/settings/provider chains remain outside an exhaustive environment-only guarantee.

### 2026-10-05: human decision round 1 accepted

The user answered "Accept all" to Q1-Q8. All eight recommendations above are accepted: flat commands and strict parsing; explicit registration/reuse and launcher refresh; broad copy without selectable categories; metadata listing plus explicit supported auth status and JSON; portable name validation; process-local updater/plugin policy; instructions-first PATH setup with explicit apply; independent configurable storage and default-copy source.

These are policy decisions, not a completed command contract. Exact command/option grammar, forward boundaries, conflict coverage/override behavior, copy selection/link treatment, prompts/noninteractive use, JSON/exit behavior, PATH edit details, manual-update environment, and recovery output remain open. Keep the ticket claimed until these are answered and the consolidated contract is confirmed.

### 2026-10-05: human decision round 2 pending

`mgr` below is a placeholder; executable/package naming remains with the distribution decision. Q9-Q16 are recommendations pending the user's answers.

**Q9 — Lifecycle grammar.** Recommend:

```text
mgr add NAME
mgr add NAME --link-default
mgr add NAME --copy-default [--source DIR]
mgr register NAME --path DIR
mgr reuse NAME
mgr list [--retained] [--json]
mgr remove NAME [--purge]
```

`reuse` restores a retained owned profile or refreshes launchers for an unchanged active registration of any kind. Create missing launchers; replace only verifiably Rust-owned launchers for that same registration. Missing/unsafe isolated profile roots or registered upstream marker-directory roots fail; never recreate their data. A default alias's caller-selected effective Claude directory need not exist and is not subject to this profile-root requirement. Repeated `register` with unchanged name/path/kind is an allowed refresh; any differing mapping or changed linked-default marker fails. Removed upstream entries require explicit registration again; removed Rust default aliases can be recreated with `add --link-default`. Ordinary `remove` on already-retained safe owned data succeeds with an already-retained message and no mutation; `remove --purge` can target active or retained owned data after safety checks. Ordinary `list` shows active registrations; `--retained` also shows retained owned data as inactive. `run` and `status` require active registration. Link/copy are mutually exclusive; `--source` requires `--copy-default`.

**Q10 — Registration eligibility and duplicate paths.** Recommend accepting real directories in the known upstream `<upstream-home>/.ccm/profiles/<upstream-name>` layout, including a custom upstream home, while recognizing its linked-default marker. Reject redirected roots, conventional default data, Rust storage/owned data, overlaps with already managed data, and duplicate registration of the same physical directory under another name. Registration preserves the supplied directory identity/path; it is not an ownership assertion or proof of account isolation. A changed kind requires removal and deliberate re-registration, not silent refresh.

**Q11 — Forwarding and exits.** Recommend `mgr run [--allow-auth-env] NAME [--] [CLAUDE_ARGS...]`; manager launch options are parsed only before NAME. After NAME, preserve each argument exactly, consuming only one optional initial delimiter `--`. To pass a literal leading `--`, use `-- --`. All generated-launcher arguments go unchanged to Claude; launchers accept no manager switches. Use direct run for an explicit authentication override. Preserve working directory, stdin/stdout/stderr, interactivity, and numeric child exit status. Propagate cancellation to the child and wait for it; Unix signal termination reports `128 + signal` rather than success, while Windows retains native exit/cancellation behavior. Exact platform mechanics/evidence remain with the design ticket.

**Q12 — Authentication conflict coverage and override.** Recommend rejecting every inherited, nonempty variable in the finite list below, even values `0`, `false`, or whitespace. Exactly empty/absent values do not conflict. This intentionally asks for acknowledgment of controls without reproducing Claude's evolving parsers. Report names only. Do not reject ambient AWS/Google/Azure credential variables alone when none of the listed Claude provider controls selects them. Isolated profile selection replaces inherited `CLAUDE_CONFIG_DIR`; that variable is not itself an authentication conflict.

`--allow-auth-env` on `run` and `status` preserves caller auth/provider variables and suppresses manager-token injection entirely, preventing the stored token from outranking a caller-selected Anthropic profile. It still selects the isolated profile directory and never bypasses path/permission checks: an unsafe borrowed manager token still blocks launch even when injection would be suppressed. Without the override, check conflicts first, then inject a safe stored manager token if present. Default aliases preserve all caller environment and bypass manager-token injection/conflict checking; accepting the flag for an alias is a harmless no-op. Generated launchers use the guarded default and point to direct run when an override is needed. Never silently remove inherited authentication or parse native credential storage.

<a id="proposed-auth-conflict-list"></a>

The proposed finite list is exact (no prefix matching):

| Category | Names |
| --- | --- |
| Direct credentials/login provisioning | `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `CLAUDE_CODE_OAUTH_TOKEN`, `CLAUDE_CODE_OAUTH_REFRESH_TOKEN`, `CLAUDE_CODE_OAUTH_SCOPES` |
| Provider selection | `CLAUDE_CODE_USE_ANTHROPIC_AWS`, `CLAUDE_CODE_USE_BEDROCK`, `CLAUDE_CODE_USE_FOUNDRY`, `CLAUDE_CODE_USE_MANTLE`, `CLAUDE_CODE_USE_VERTEX` |
| Provider authentication bypass | `CLAUDE_CODE_SKIP_ANTHROPIC_AWS_AUTH`, `CLAUDE_CODE_SKIP_BEDROCK_AUTH`, `CLAUDE_CODE_SKIP_FOUNDRY_AUTH`, `CLAUDE_CODE_SKIP_MANTLE_AUTH`, `CLAUDE_CODE_SKIP_VERTEX_AUTH` |
| Provider credentials | `ANTHROPIC_AWS_API_KEY`, `ANTHROPIC_FOUNDRY_API_KEY`, `ANTHROPIC_FOUNDRY_AUTH_TOKEN`, `AWS_BEARER_TOKEN_BEDROCK` |
| Endpoint/account routing | `ANTHROPIC_BASE_URL`, `ANTHROPIC_AWS_BASE_URL`, `ANTHROPIC_AWS_WORKSPACE_ID`, `ANTHROPIC_BEDROCK_BASE_URL`, `ANTHROPIC_BEDROCK_MANTLE_BASE_URL`, `ANTHROPIC_FOUNDRY_BASE_URL`, `ANTHROPIC_FOUNDRY_RESOURCE`, `ANTHROPIC_VERTEX_BASE_URL`, `ANTHROPIC_VERTEX_PROJECT_ID`, `ANTHROPIC_CUSTOM_HEADERS` |
| Named profiles/federation | `ANTHROPIC_PROFILE`, `ANTHROPIC_CONFIG_DIR`, `ANTHROPIC_FEDERATION_RULE_ID`, `ANTHROPIC_ORGANIZATION_ID`, `ANTHROPIC_SERVICE_ACCOUNT_ID`, `ANTHROPIC_WORKSPACE_ID`, `ANTHROPIC_IDENTITY_TOKEN`, `ANTHROPIC_IDENTITY_TOKEN_FILE` |
| Host/auth mode | `CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST`, `CLAUDE_CODE_SIMPLE` |
| Gateway client credentials | `CLAUDE_CODE_CLIENT_CERT`, `CLAUDE_CODE_CLIENT_KEY`, `CLAUDE_CODE_CLIENT_KEY_PASSPHRASE` |

Evidence: current [environment reference](https://code.claude.com/docs/en/env-vars), [authentication precedence](https://code.claude.com/docs/en/authentication#authentication-precedence), [WIF reference](https://platform.claude.com/docs/en/manage-claude/wif-reference#environment-variables), and [network configuration](https://code.claude.com/docs/en/network-config), checked by the native research subagent on 2026-10-05. Native gateway/active-federation state, settings/env/helpers, host behavior, and forwarded Claude options can still affect selection; this finite environment check never proves an effective account or complete native isolation.

**Q13 — Confirmation and scripts.** Recommend no prompt for ordinary removal, explicit registration/reuse, empty/default-alias creation, or explicit token replacement/clear. Purge prompts on a terminal, showing exact scope, retained/native credentials, and the requirement to stop sessions/external writers; `--yes` acknowledges and skips the prompt. Without a terminal, purge requires `--yes`. Copy similarly requires acknowledgment that the selected source is quiet (terminal prompt or `--yes`). Missing acknowledgment refuses without mutation. `--yes` never bypasses ownership/path/token protection and is invalid on commands that have no safety confirmation. Rejected borrowed/default purge checks occur before prompting. Declining or EOF cancels without mutation; exit details remain for the diagnostic/output round.

**Q14 — Token input.** Recommend `mgr token NAME` for hidden terminal input, `mgr token NAME --stdin` for explicit piped input, and `mgr token NAME --clear`. Validate profile registration/ownership/path eligibility before reading a secret. Without terminal input and without `--stdin`, fail without reading. Trim surrounding whitespace, require one nonempty token, reject embedded whitespace/NUL, and cap input at 64 KiB; do not validate the token online. Input cancellation leaves the existing token unchanged. Reject `--stdin` with `--clear`; no token values as arguments, environment-input feature, token file-input feature, or token output. File protection and native-login consequences remain as fixed by the ownership Answer.

**Q15 — Copy selection and links.** Recommend recursively seeding all ordinary files/directories from `--source DIR`, else nonempty caller `CLAUDE_CONFIG_DIR`, else the conventional default directory. Resolve a relative source from the current working directory; refuse missing/redirected/non-directory roots and source/destination overlap. Exclude top-level `cache`, `daemon`, `ide`, `paste-cache`, `shell-snapshots`, `telemetry`, and `backups`. At every depth, exclude `.credentials.json` and `.claude.json` plus names beginning those basenames followed by `.`, upstream `.ccm-oauth-token` and `.ccm-linked-default`, and known Rust manager-token/ownership artifacts (concrete artifact names remain for later design/distribution). Do not copy a companion global `.claude.json` outside the selected source. This deliberately omits global MCP/UI/trust/session state held there; reconfigure it in the new profile rather than parse credential-bearing global state. Other selected settings/history/plugin data can contain secrets, helpers, or machine-specific references; no secret-free or fully portable claim.

Reject linked source roots; skip contained symlinks, junctions/reparse points, multiply linked regular files, and special files without following them. Report omissions by relative path/reason and counts; an unreadable selected ordinary file is a failed staged copy, not a silently incomplete success. Ordinary files are independently copied, never hardlinked. Preserve the fixed staging/no-usable-partial-profile rule and require login/authentication verification afterward. First-party [directory documentation](https://code.claude.com/docs/en/claude-directory) identifies `.claude.json` as app/OAuth/global-MCP state and `backups` as its earlier copies; upstream's broad `.claude.json` copy is therefore deliberately changed.

**Q16 — Path query.** Recommend `mgr where NAME` / `path NAME` emit only one absolute directory path and newline. Accept active registrations and safely referenced retained owned profiles. For isolated profiles, return the stored profile directory; for a default alias, return the caller's nonempty environment `CLAUDE_CONFIG_DIR` resolved relative to the current directory, else the conventional default directory. Do not create it or claim that it exists. This is the directory selected by the manager/caller environment; native settings can still affect Claude. Actual observed configuration directory belongs to supported auth-status output. Empty `CLAUDE_CONFIG_DIR` is treated as unset for this query; the alias launch itself still preserves it exactly.

Await Q9-Q16 before dependent refinements or resolution. Observable diagnostic/JSON/error output, final confirmation/cancellation exit policy, PATH edits, and manual update/recovery details remain for the following round; design and acceptance evidence remain outside this ticket.

### 2026-10-05: human decision round 2 accepted; session-management clarification pending

The user answered "yes this looks good" to Q9-Q16. All eight proposals in round 2 are accepted, including the exact lifecycle/forwarding grammar, registration eligibility, authentication refusal list and override behavior, confirmation/input rules, copy exclusions/link handling, and path-query semantics.

The user additionally requested easy management of multiple sessions. The glossary currently defines profiles, accounts, launchers, and default aliases, but not sessions. Clarify whether this means convenient concurrent profile launches, resuming saved Claude conversations, or discovering/switching/stopping live sessions before choosing commands or broadening scope. This is not the consolidated shared-understanding confirmation: the remaining diagnostic/PATH/update/recovery choices and session request are still open.

### 2026-10-05: session-management scope accepted

At Q17 the user chose "Start concurrent sessions and easily resume saved conversations (Recommended)." First-release session ergonomics therefore cover starting concurrent Claude conversations and convenient resumption, using native Claude capabilities. Live/background session inventory, attach/stop controls, or a manager-owned supervisor are not requested by this answer. Exact exposure/shortcuts remain to be decided after native command research. The glossary now distinguishes a session from a profile/account; multiple sessions may belong to one profile. Preserve the approved launch/environment/ownership boundaries and leave session history/identity Claude-owned.

### 2026-10-06: human decision round 3 pending

Q18-Q23 are recommendations, not accepted decisions. Current first-party [CLI reference](https://code.claude.com/docs/en/cli-reference) and [session documentation](https://code.claude.com/docs/en/sessions) were checked on 2026-10-06; native subagent research on 2026-10-05 supplied the broader session inventory. No real sessions or credentials were accessed.

**Q18 — Session ergonomics.** Recommend using the existing guarded `run` command and profile launchers with native Claude session flags:

```text
mgr run work --name client-a
mgr run work --name client-b
mgr run work --resume
mgr run work --resume client-a
mgr run work --continue
mgr run work --resume client-a --fork-session
```

Start concurrent foreground sessions in separate terminal tabs/windows. `--resume` without an identifier opens Claude's picker; `--continue` selects its latest conversation in the current directory. Native names/IDs/picker/history stay Claude-owned, with the selected profile's configuration and existing authentication rules. Use `--fork-session` for an independent continuation; resuming one conversation in two terminals can interleave its transcript. No additional Rust session commands, registry, terminal automation, or supervisor in the first release; the CLI help includes these recipes. Native forwarded background/control options remain usable at the user's explicit request, subject to Claude's behavior; returning from background dispatch is not the eventual task result, and attaching an existing worker does not establish newly selected authentication. Concrete Claude-version support and platform evidence remain with the final design/acceptance ticket.

**Q19 — Diagnostic and JSON output.** Recommend `list [--retained] [--json]`, `status [--allow-auth-env] NAME [--json]`, and `doctor [--json]`. List is metadata-only and identifies name, kind, active/retained state, paths, manager-token presence, and launcher condition. Status invokes supported Claude auth status under the same launch/environment safety rules, showing reported login, method, observed config directory when supported, and isolated/pass-through scope; do not print raw native JSON, credentials, or undocumented account identity fields. Unsupported/uninterpretable status is unknown, never a credential-file fallback. Doctor checks Claude resolution/version, manager paths, PATH membership, launcher/profile safety, and recovery findings; it does not run auth status for every profile or repair files. PATH absence alone is a warning; missing Claude, missing required launchers, unsafe paths, and collisions are failures.

JSON for these three commands is one object on stdout with `schema_version: 1`, `data`, `warnings`, and nullable `error`. An error has stable `code`, safe `message`, and nullable `next_step`; useful partial diagnostic data may remain present. List data contains `profiles`; status data contains `name`, `kind`, `reported_logged_in`, `auth_method`, `config_directory`, and `scope`; doctor data contains Claude path/version, storage/launcher directories, PATH membership, profiles, and findings. Optional unavailable status fields are null with a warning. Only supported allowlisted native status fields are exposed. Human read/mutation results use stdout; prompts, progress, warnings, and ordinary errors use stderr. `where` and version remain bare stdout data. `run`/`update` retain native child streams; manager notices never contaminate their stdout. No JSON mode for mutation or forwarding commands in this release.

**Q20 — Manager exit and help behavior.** Recommend 0 for successful manager operations, 1 for operational/safety failures, 2 for invalid syntax, 3 for supported status reporting not logged in, and 130 for user cancellation (declined confirmation, terminal EOF/cancel, Ctrl+C). Empty explicit stdin token is invalid input, not terminal cancellation. Doctor exits 1 on failed checks, 0 with warnings alone. `run` and `update` keep child exit/signal behavior accepted at Q11; their codes may overlap manager codes. Help/no arguments succeeds; support `help [COMMAND]`, `-h`/`--help`, per-command `--help`, and top-level `-v`/`--version`. Under `run`, `--help` after NAME goes to Claude. Unknown flags/extra manager arguments fail before mutation or secret input.

**Q21 — PATH syntax and writes.** Recommend `setup-path [--shell SHELL] [--apply]`. No arguments prints platform-appropriate instructions; `--shell` selects one recipe. Unix apply requires an explicit supported shell: bash's home `.bashrc`, zsh's `.zshrc` under the caller's nonempty `ZDOTDIR` or home, or posix's home `.profile`. Windows apply edits only user PATH; PowerShell/CMD select the printed recipe. Reject unsupported platform/shell combinations. Show the destination before apply. Unix edits only the manager's clearly delimited block in that one file, preserving other content; refuse redirected/non-regular destination files or malformed/multiple conflicting manager blocks. Windows adds the launcher directory only if absent. Compare paths case-sensitively on Unix and case-insensitively on Windows. Report already-configured state and required terminal restart; no system PATH edits, PATH undo, or uninstall cleanup. Exact manager identity/launcher location remains with naming/distribution, and safe write mechanics with the design ticket.

**Q22 — Manual update environment.** Recommend `update` resolve the existing shared `claude` from PATH and invoke exactly `claude update`, with caller environment/working directory/streams preserved and no manager-injected config directory, token, updater, or plugin variables. Preserve caller update restrictions and let Claude enforce them; do not clear environment, force installation, or fall back to package-manager commands. State that the shared installation is affected. Unsupported installation methods or running-file/update failures retain Claude's result and add safe guidance when recognizable. Do not promise detection or coordination of sessions started by other managers. No extra manager confirmation beyond explicitly invoking update.

**Q23 — Failure and recovery output.** Recommend stable error categories (`usage`, `not_found`, `collision`, `ownership`, `unsafe_path`, `auth_conflict`, `invalid_token`, `claude_unavailable`, `claude_unsupported`, `auth_status`, `io`, `diagnostics`, `cancelled`) with operation/profile, safe affected paths, actual changed/retained state, and a concrete next step. Authentication conflicts print variable names only. No secret values, credential contents, raw auth-status stderr, or automatic extraction/repair of borrowed data. Preflight removal destinations before deleting owned launchers; if deletion fails after some changes, retain registration/ownership evidence, exit failure, report exactly which launchers remain, and permit a safe retry after inspection. Failed creation/copy publishes no usable partial registration/launcher/profile; report any owned staging residue and safe cleanup/retry guidance. Do not claim atomic rollback of every filesystem failure or suggest deleting foreign/uncertain artifacts. Ordinary removal explicitly reports retained data/token/native-login consequences; purge states that deletion is not logout or revocation. Final implementation/recovery representation and evidence remain with the design ticket.

Await Q18-Q23. Then prepare the consolidated contract, independently inspect it for omissions/contradictions, and obtain the human's final shared-understanding confirmation before resolving this ticket. No application implementation, commit, or publication is authorized by these planning choices.

### 2026-10-06: human decision round 3 accepted

The user answered "accept all" to Q18-Q23. All six recommendations are accepted: native session recipes through run/launchers; diagnostic/JSON/stdout-stderr rules; manager exit/help behavior; instructions-first scoped PATH application; manual update in the caller's environment; and explicit recovery reporting. Together with Q1-Q17, this settles the proposed command policies. A consolidated contract and independent review follow before final shared-understanding confirmation. This acceptance is not itself that final confirmation and does not authorize application implementation.

## Answer

### 2026-10-06: approved command contract, sessions, and OAuth login

The human accepted Q1-Q23 and confirmed the consolidated contract plus lookup clarification at Q24. The same response explicitly requested OAuth login, captured through the already approved native Claude sign-in/forwarding capability and independently reviewed before resolution. This Answer is the canonical command decision, constrained by [Define safe profile coexistence and ownership](03-profile-compatibility.md#answer); comments above preserve the conversation history. `mgr` is a placeholder; names, storage locations, and launch prefixes remain with [Choose command naming and cross-platform distribution](05-distribution.md). Use the [glossary](../../../GLOSSARY.md); a profile is a context, an account is an identity, and a session is a Claude conversation.

### Command surface and parsing

```text
mgr add NAME
mgr add NAME --link-default
mgr add NAME --copy-default [--source DIR] [--yes]
mgr register NAME --path DIR
mgr reuse NAME
mgr list [--retained] [--json]
mgr where NAME
mgr run [--allow-auth-env] NAME [--] [CLAUDE_ARGS...]
mgr status [--allow-auth-env] NAME [--json]
mgr token NAME [--stdin | --clear]
mgr remove NAME [--purge [--yes]]
mgr update
mgr setup-path [--shell SHELL] [--apply]
mgr doctor [--json]
mgr help [COMMAND]
mgr --help | -h
mgr --version | -v
```

Retain aliases `ls` for list, `path` for where, and `rm` for remove. No arguments show help successfully; per-command `--help` is supported. Version is a bare version string plus newline. Command-specific options may appear around their positionals, except run's manager options must precede NAME. After run NAME, only one optional initial `--` is consumed; all remaining arguments preserve order, boundaries, and values for Claude. `-- --` forwards a literal leading `--`. After run NAME, `--help` belongs to Claude. Generated launchers forward every argument and consume no manager switches; override guidance points to direct run.

Reject unknown manager commands/options, extra positionals, invalid combinations, and missing required arguments before mutation or secret input. `--link-default` and `--copy-default` conflict; `--source` requires copy; `--stdin` and `--clear` conflict; `--yes` is valid only for copy or purge; JSON is valid only for list/status/doctor. Help performs no mutation or secret acquisition.

Names have 1-32 ASCII characters: first alphanumeric, remaining alphanumeric/underscore/hyphen. Preserve spelling, enforce case-insensitive uniqueness on every platform, and reject Windows device names `CON`, `PRN`, `AUX`, `NUL`, `COM1`-`COM9`, `LPT1`-`LPT9`, regardless of case. Apply validation to read commands too. Claude session names/IDs/arguments are opaque forwarded values, not manager profile names.

Every manager command resolves active and retained profile names case-insensitively. Thus `run work` selects stored `Work`; output and regenerated launchers retain the original `Work` spelling and stored directory. A casing variant never creates another registration, silently renames one, or changes an upstream path. Launcher invocation itself follows the operating system's normal filename/command casing rules; use the printed launcher spelling. The human confirmed this lookup clarification at Q24.

### Creation, registration, reuse, and removal

- `add` creates a new owned isolated profile or default alias and its Rust launchers. Existing registrations or retained owned data block creation; do not silently reuse or change kind. Empty/default-alias creation needs no confirmation.
- `register` accepts a real upstream directory in `<upstream-home>/.ccm/profiles/<upstream-name>`, including custom upstream homes, at its unchanged upstream path. Recognize linked-default entries as pass-through aliases. Reject redirected roots, conventional default data, Rust storage/owned data, overlapping managed data, and a second name for the same physical directory. Registration does not transfer ownership or authorize token mutation, borrowed permission repair, purge, or changing upstream launchers.
- Repeating register with identical name/path/kind refreshes Rust launchers; a differing mapping or changed linked-default marker fails. A changed kind needs removal and deliberate registration again.
- `reuse` restores retained owned data or refreshes Rust launchers for an unchanged active registration of any kind. Create missing launchers; replace only verifiably Rust-owned launchers for that same registration. Missing/unsafe isolated roots or registered upstream marker-directory roots fail; never recreate their data. Default aliases' caller-selected effective Claude directories need not already exist.
- `list` enumerates active Rust registrations, without automatic upstream discovery. `--retained` additionally includes retained owned profiles as inactive. Run/status/token input require active registration; where also accepts safely referenced retained owned data.
- Ordinary `remove` deletes only Rust-owned launchers/registration, retaining owned data, tokens, and ownership evidence for reuse. On already-retained safe owned data it succeeds with an already-retained message and no mutation. Removing an upstream entry/default alias preserves upstream/default artifacts; restore them via explicit registration or alias creation, respectively.
- `remove --purge` may target active or retained verified owned isolated data only. Reject upstream/default purge before confirmation, even with `--yes`. Deletion is not native logout or revocation; do not traverse contained links into other data. Missing/unsafe paths or uncertain ownership never authorize deletion/recreation of foreign artifacts.

Rust mutations serialize; launches may run concurrently under Claude's native behavior. No shared coordination protocol with upstream/other managers is promised. Purge and copy require quiet sessions/external writers. On a terminal, show scope and ask confirmation; `--yes` acknowledges and skips it. Without a terminal, require `--yes`. Refusal occurs before mutation; declining/terminal EOF cancels. No extra confirmation for ordinary removal, registration/reuse, explicit token replacement/clear, or manual update.

### Launch, authentication, and update environment

Invoke the existing shared Claude resolved through PATH; preserve working directory, inherited stdio, and interactivity. The subsequent [design decision](06-specification-readiness.md) explicitly narrows initial native Windows support to a PATH-selected `claude.exe`; a selected `.cmd`/`.bat` Claude shim is refused with guidance, while Roost's own required launcher forms remain. That choice was accepted at design Q7; other environment/forwarding rules below remain authoritative. Isolated profiles select their stored `CLAUDE_CONFIG_DIR`, set process-local `DISABLE_AUTOUPDATER=1` and `FORCE_AUTOUPDATE_PLUGINS=1`, and otherwise retain caller environment subject to the authentication rules below. Default aliases preserve the caller's entire environment and bypass manager-token injection/auth-conflict checking, consistently in run and launchers. Label aliases pass-through; profile selection does not prove the effective account or isolate project files/settings.

Isolated run/status reject inherited nonempty values in the finite authentication list below, including `0`, `false`, and whitespace. Exactly empty/absent is not a conflict. Report names only. Ambient AWS/Google/Azure credential-chain variables alone do not trigger refusal. Inherited `CLAUDE_CONFIG_DIR` is replaced by isolated profile selection rather than treated as an authentication conflict.

Without override, check conflicts before injecting a safe stored manager token. Owned and safe borrowed upstream manager tokens can be used for launch; native Claude credentials/Keychain are never extracted. `--allow-auth-env` on run/status preserves caller auth/provider values and suppresses manager-token injection entirely, accepting Claude's native precedence while still selecting the profile directory. It never bypasses root/token-path/permission protection: unsafe borrowed tokens still block launch. On default aliases the flag is harmless. Native gateway/federation state, settings/helpers, host behavior, and forwarded Claude flags remain outside a complete environment-only/account-identity guarantee.

The exact finite refusal list is:

| Category | Names |
| --- | --- |
| Direct credentials/login provisioning | `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `CLAUDE_CODE_OAUTH_TOKEN`, `CLAUDE_CODE_OAUTH_REFRESH_TOKEN`, `CLAUDE_CODE_OAUTH_SCOPES` |
| Provider selection | `CLAUDE_CODE_USE_ANTHROPIC_AWS`, `CLAUDE_CODE_USE_BEDROCK`, `CLAUDE_CODE_USE_FOUNDRY`, `CLAUDE_CODE_USE_MANTLE`, `CLAUDE_CODE_USE_VERTEX` |
| Provider authentication bypass | `CLAUDE_CODE_SKIP_ANTHROPIC_AWS_AUTH`, `CLAUDE_CODE_SKIP_BEDROCK_AUTH`, `CLAUDE_CODE_SKIP_FOUNDRY_AUTH`, `CLAUDE_CODE_SKIP_MANTLE_AUTH`, `CLAUDE_CODE_SKIP_VERTEX_AUTH` |
| Provider credentials | `ANTHROPIC_AWS_API_KEY`, `ANTHROPIC_FOUNDRY_API_KEY`, `ANTHROPIC_FOUNDRY_AUTH_TOKEN`, `AWS_BEARER_TOKEN_BEDROCK` |
| Endpoint/account routing | `ANTHROPIC_BASE_URL`, `ANTHROPIC_AWS_BASE_URL`, `ANTHROPIC_AWS_WORKSPACE_ID`, `ANTHROPIC_BEDROCK_BASE_URL`, `ANTHROPIC_BEDROCK_MANTLE_BASE_URL`, `ANTHROPIC_FOUNDRY_BASE_URL`, `ANTHROPIC_FOUNDRY_RESOURCE`, `ANTHROPIC_VERTEX_BASE_URL`, `ANTHROPIC_VERTEX_PROJECT_ID`, `ANTHROPIC_CUSTOM_HEADERS` |
| Named profiles/federation | `ANTHROPIC_PROFILE`, `ANTHROPIC_CONFIG_DIR`, `ANTHROPIC_FEDERATION_RULE_ID`, `ANTHROPIC_ORGANIZATION_ID`, `ANTHROPIC_SERVICE_ACCOUNT_ID`, `ANTHROPIC_WORKSPACE_ID`, `ANTHROPIC_IDENTITY_TOKEN`, `ANTHROPIC_IDENTITY_TOKEN_FILE` |
| Host/auth mode | `CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST`, `CLAUDE_CODE_SIMPLE` |
| Gateway client credentials | `CLAUDE_CODE_CLIENT_CERT`, `CLAUDE_CODE_CLIENT_KEY`, `CLAUDE_CODE_CLIENT_KEY_PASSPHRASE` |

Use no arbitrary prefix matching or native cloud-SDK parser reimplementation. Sources and verification limits are in the research/conversation above; implementation must check these contracts against its supported Claude versions.

`update` invokes exactly `claude update` for the existing shared installation, preserving caller environment/working directory/stdio and injecting no profile/token/updater/plugin variables. Preserve caller restrictions and let Claude enforce them. Identify shared-installation scope, propagate Claude's result, and offer safe recognizable failure guidance. Do not install/replace Claude automatically, globally rewrite update policy, fall back to package-manager mutations, or claim detection/coordination of externally launched sessions.

### OAuth browser login

The user's OAuth-login requirement uses the already approved native Claude sign-in capability through run; it does not add a Rust OAuth client or change the authentication/ownership contract. Include a first-login recipe in help and creation guidance:

```text
mgr add work
mgr run work auth login
mgr status work
```

For supported directory-scoped claude.ai/subscription OAuth, select that account type in Claude's native browser sign-in flow. Claude owns browser authorization, callbacks, native credential storage, refresh, and login/logout. Preserve the selected configuration directory, launch safety/conflict checks, terminal/streams, and native child result. The native interactive `/login` remains available in launched sessions; documented auth-login options such as `--email` and `--sso` are forwarded unchanged. Keyless Console OAuth remains outside the approved profile-isolation promise; this recipe does not widen it. [Supported auth login](https://code.claude.com/docs/en/cli-reference#cli-commands), [authentication contract](https://code.claude.com/docs/en/authentication).

Do not silently clear or replace a manager token when logging in. For an owned profile switching from a manager token to native browser login, explicitly run `mgr token work --clear`, then native auth login and status. Otherwise the stored manager token can become effective again on the next ordinary launch, despite successful native sign-in. Registered upstream tokens remain read-only: change them through upstream, or deliberately use `mgr run --allow-auth-env work auth login` and the corresponding status/run override to suppress Rust token injection while accepting caller precedence. The override still refuses unsafe borrowed token paths/permissions, and inherited native authentication controls may still affect the flow. Do not claim that a successful login exit proves model/service access or a specific effective account under every configuration.

Long-lived subscription tokens remain the separate token-input feature; `claude setup-token` may be explicitly forwarded via run, with its output native-owned. Browser sign-in does not involve manager token scraping, storing an extracted access/refresh token, or logging credentials. OAuth first-login, explicit token-to-browser transition, cancellation/failure, unchanged-path upstream native login, and Linux/macOS/Windows browser/native-credential behavior are required acceptance topics for the final design ticket; they have not been runtime tested here.

### Manager tokens and copying

Token set/clear is limited to active owned isolated profiles. Validate registration, ownership, and path eligibility before secret acquisition. Hidden terminal input is the default; `--stdin` explicitly permits piped input. Without terminal input or `--stdin`, fail without reading. Trim surrounding whitespace; reject empty input, embedded whitespace/NUL, and raw input over 64 KiB. Do not validate tokens online. Cancel leaves the old token intact. Accept no token argument, environment/file-input feature, or token output. Protect new/replacement token files with private Unix permissions/current-user Windows protection; refuse if protection cannot be established. Borrowed tokens are never repaired. Clear removes only the manager token and can expose existing native credentials next launch.

Copy source priority: explicit `--source DIR`, nonempty inherited `CLAUDE_CONFIG_DIR`, then the conventional default Claude directory. Resolve relative sources from current working directory. Rust storage overrides never change the user's home/default source. Reject missing/redirected/non-directory roots and source/destination overlap. Require quiet-source acknowledgment. Seed broad ordinary settings/plugin/history data, independently copying regular files rather than hardlinking them.

Exclude top-level `cache`, `daemon`, `ide`, `paste-cache`, `shell-snapshots`, `telemetry`, `backups`. At every depth exclude `.credentials.json`, `.claude.json`, their basename-plus-dot variants, `.ccm-oauth-token`, `.ccm-linked-default`, and known Rust token/ownership artifacts (names settled later). Do not copy a separate companion `.claude.json`. Global MCP/UI/trust/sign-in state there must be reconfigured, instead of parsing credential-bearing global state. Arbitrary copied settings/history/plugins can still contain secrets, helpers, or machine-specific references; copying is data seeding, not account migration or a portability/secret-free guarantee.

Skip contained symlinks, junctions/reparse points, multiply linked regular files, and special files without following their targets; report omitted relative paths/reasons/counts. Unreadable selected ordinary files fail the staged copy. Publish no usable half-created profile, registration, or launcher on failure; report owned staging residue with safe cleanup/retry guidance. Require login/authentication verification afterward; never extract Keychain secrets.

### Concurrent sessions and resume

Use native Claude flags through guarded run/profile launchers, and include these recipes in help:

```text
# Separate terminal tabs/windows; same or different profiles are permitted
mgr run work --name client-a
mgr run work --name client-b
mgr run personal --name notes
# Native picker, explicit conversation, latest in current directory, independent fork
mgr run work --resume
mgr run work --resume client-a
mgr run work --continue
mgr run work --resume client-a --fork-session
```

Claude owns session names/IDs, saved history, picker/name ambiguity, and resume errors. Native default picker scope is the current project/worktree; its own widening controls apply. Independent continuations use `--fork-session`; resuming one conversation in two terminals can interleave a transcript. Names/IDs are not accounts and resume does not establish authentication. No manager-owned session registry, terminal automation, extra resume/continue commands, or supervisor in this release. Explicit forwarded native background commands remain native behavior: a dispatch result is not an eventual task result, and attaching a worker does not promise changed authentication. Version/platform evidence and supervisor token behavior remain for the design/acceptance ticket, without expanding the selected first-release session promise.

### Paths, diagnostics, JSON, and exits

Where/path prints exactly one absolute directory path plus newline. Return the stored isolated directory for active or safely referenced retained owned profiles. For a default alias, return the caller's nonempty `CLAUDE_CONFIG_DIR` resolved relative to cwd, else conventional default; treat empty as unset for the query but preserve it unchanged during alias launch. Do not create the alias target or claim it exists. This is the expected manager/caller environment path; supported status reports Claude's observed directory when available.

List is fast metadata-only output: name, kind, active/retained state, paths, token presence, launcher condition. Presence is never called a working login. Status invokes supported `claude auth status` under the same safety/environment selection, captures only supported allowlisted fields, and reports login/method/observed config directory/isolated or pass-through scope. Unsupported/uninterpretable results are unknown; missing optional fields are null with warning. Do not print raw native JSON, credential contents, or undocumented identity fields, and never fall back to credential-file/Keychain heuristics. Config-directory/status comparisons do not override the approved account-isolation caveats.

Doctor checks Claude resolution/version, storage/launcher paths, PATH membership, known profile/launcher safety, and recovery findings. Do not run auth status for every profile or repair files. PATH absence alone is a warning; missing Claude, missing required launchers for active registrations, unsafe paths, and collisions fail checks. Retained profiles require no usable launcher.

JSON is one object on stdout for list/status/doctor:

```json
{"schema_version":1,"data":{},"warnings":[],"error":null}
```

An error contains stable `code`, safe `message`, and nullable `next_step`; useful partial diagnostic data may remain. List data contains `profiles`; status data contains `name`, `kind`, `reported_logged_in`, `auth_method`, `config_directory`, `scope`; doctor data contains Claude path/version, storage/launcher directories, PATH membership, profiles, findings. The final specification records exact record keys/types as a faithful serialization of these accepted fields; no native secret/undocumented fields can be added. Malformed command syntax uses ordinary stderr usage diagnostics; successfully parsed JSON operations keep operational errors inside their single JSON object. Human results use stdout; prompts/progress/warnings/ordinary errors use stderr. Where/version are bare data. Run/update inherit native child streams; manager notices stay off child stdout. No mutation/forwarding JSON mode.

| Manager result | Exit |
| --- | --- |
| Success, including help and warning-only doctor | 0 |
| Operational/safety failure; failed doctor; unsupported/uninterpretable auth status | 1 |
| Invalid syntax | 2 |
| Supported status reports not logged in | 3 |
| Declined confirmation, terminal EOF/cancel, or manager Ctrl+C before child handoff | 130 |

Empty explicit stdin token is invalid input, not terminal cancellation. Run/update preserve child numeric status; propagate cancellation to the child and wait. Unix signals yield `128 + signal`, Windows keeps native exit/cancellation behavior. Manager and child codes may overlap. Cross-platform process mechanics belong to the design ticket.

### PATH application and recovery

Setup-path defaults to platform-appropriate instructions; `--shell` chooses one recipe. Unix apply requires bash, zsh, or posix and edits only one file: home `.bashrc`, `.zshrc` under nonempty `ZDOTDIR` else home, or home `.profile`. Windows apply changes only user PATH; powershell/cmd choose its printed recipe. Reject unsupported shell/platform combinations. Show the destination before apply; no further confirmation or `--yes` is involved. Unix changes only a clearly delimited manager block, preserving other content; refuse redirected/non-regular files or malformed/multiple conflicting blocks. Windows adds the launcher directory only when absent. Compare paths case-sensitively on Unix and case-insensitively on Windows. Report already-configured state and terminal restart guidance. No system PATH mutations, undo, or automatic uninstall cleanup. Actual launcher location/manager marker identity remain with distribution/design.

The subsequently confirmed [naming/distribution decision](05-distribution.md#fish-and-path-setup) explicitly extends Unix shell recipes/apply to Fish and selects the concrete Roost identity and locations. That accepted extension governs Fish support; the other PATH safety and command rules above remain authoritative.

Stable error categories: `usage`, `not_found`, `collision`, `ownership`, `unsafe_path`, `auth_conflict`, `invalid_token`, `claude_unavailable`, `claude_unsupported`, `auth_status`, `io`, `diagnostics`, `cancelled`. Include operation/profile where relevant, safe affected paths, actual changed/retained state, and a concrete next step. Secrets, native credential contents, and raw captured auth-status stderr are excluded from manager error output. Forwarded Claude streams remain native-owned.

Preflight removal destinations before deleting owned launchers. If a later deletion fails, retain registration/ownership evidence, exit failure, and report changed/remaining launchers for safe inspection/retry. Failed create/copy publishes no usable partial profile; report any owned staging residue and safe cleanup/retry steps. Do not claim rollback of every filesystem failure or prescribe deletion of foreign/unverifiably owned artifacts. Removal reports retained-data/token/native-login consequences; purge is not logout/revocation. Preserve the canonical ownership/concurrency/filesystem requirements throughout; representation, crash recovery mechanisms, and acceptance evidence are still design decisions.

### Upstream capability accounting and handoff

| Upstream capability | Rust disposition |
| --- | --- |
| Empty creation; linked-default alias; launcher generation | Retained with separate ownership, portable names, collision checks, consistent alias pass-through |
| Repeating add regenerates launchers; removed data stays reusable | Changed to explicit reuse and identical registration refresh; inactive retained data is explicit |
| Broad default copy including credentials/global `.claude.json` | Data seeding retained; source override added; known credential/global-state/backup/link exclusions and staged failure behavior deliberately changed |
| List/ls with credential/Keychain login heuristic | List/ls retained as metadata; supported explicit status and JSON replace inferred login |
| Where/path | Bare-path output retained; retained-profile lookup and caller-selected alias semantics made explicit |
| Run and Unix/Windows launchers | Retained with exact argument boundaries, consistent environment/safety, and faithful child/signal handling; native session recipes added |
| Native login after profile creation | Explicit OAuth/browser-login recipes through run/native auth login; Claude owns authorization/storage/refresh and manager-token switching remains explicit |
| Token set/clear | Retained for owned profiles; hidden/explicit-stdin input, bounded validation, replacement protection, and borrowed-token launch rules added |
| Remove/rm and explicit purge | Retained with ownership guards, confirmation/noninteractive acknowledgment, retained evidence, and truthful partial-failure reporting |
| Update | Retained as explicit shared native updater delegation; no injected profile/update environment or automatic installation |
| Setup-path | Retained with instructions default, explicit scoped apply, idempotence, and guarded edits |
| Doctor | Retained with meaningful failure exit, safe diagnostics, and JSON; no native-credential heuristics/repair |
| Help/version | Retained; strict manager parsing and command help added |
| Custom upstream CCM_HOME behavior | Configurable Rust storage retained independently from effective default-copy source; env spelling/location deferred |

Not selected for this release: profile rename, global active-profile switching, automatic discovery/migration/ownership transfer, import/export, selectable copy categories, per-profile Claude binaries, shell completions, manager config-file feature, PATH uninstall cleanup, terminal automation, or a Rust session registry/supervisor. These exclusions do not remove Claude's explicit native capabilities forwarded through run.

No additional decision ticket is required by the accepted choices. Hand off executable/package/prefix/storage identity and install/platform/switching policy to [Choose command naming and cross-platform distribution](05-distribution.md). Hand off ownership evidence, exact JSON record serialization, locking/publication/recovery/permission/process implementation, supported Claude-version floor, and Linux/macOS/Windows evidence (including concurrent start/resume/fork, quoted args, cancellation, tokens/copy/purge) to [Set the Rust design and specification acceptance criteria](06-specification-readiness.md). These are future implementation/release checks, not tests performed in this planning chat. The final implementation-ready `spec.md` belongs to that last ticket.

### Independent review and final confirmation

The Forge inspection used a fresh native reviewer context with Mid/Top/Reviewer inheriting this chat's model and effort. The first snapshot (ticket SHA256 `33b779dac183857076d9749f63b14df0ec975619409d7befa25577e53c153e7f`, glossary SHA256 `9ac8c854bd58584f86cdaa9161217570066cc19564b1f145a0dbfca569de5151`) had no verified actionable defects. The reviewer identified one unsettled observable choice, lookup casing after creating `Work`; its explicit proposal passed focused re-review (ticket SHA256 `20e0c13ae531a2ca93bb439bafc5ec77504b3f26af49e6b994a8b09e205c2d5c`) and the human confirmed it at Q24. The OAuth addition passed a further focused re-review (ticket SHA256 `5e5d4b5c487066cc30f503b9d0c763f3c4b786990d78613653651f5015b8d9d3`). All three inspections returned no actionable defects, and the last found no essential unanswered policy. Whitespace/local-link checks and tracked diff checks passed. No application/runtime/authentication/platform tests were performed.

### 2026-10-06: final shared understanding confirmed; OAuth requirement captured

The user answered "Yes" to Q24, confirming the consolidated contract and case-insensitive lookup clarification, and added "We will also need the ability to login with oauth." This OAuth requirement is explicitly captured above using the already approved native-authentication scope and unchanged `run` forwarding; no new manager command, credential ownership, Console-isolation promise, or Rust OAuth implementation is introduced. The native first-login and explicit manager-token transition recipes make the capability visible. Focused independent review of that addition passed. This command decision is resolved; the map and existing naming/design handoffs are updated, while those later decisions remain open. No application implementation, commit, or publication occurred.
