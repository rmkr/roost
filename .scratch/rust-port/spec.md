# Roost implementation specification

Approved by the user on 2026-10-06. Amended the same day by [Workflow UX amendments](#workflow-ux-amendments-2026-10-06) through [Amend the specification for workflow UX](../workflow-ux/issues/01-amend-spec.md), accepted by the user on 2026-10-06 with two changes (separate plugin store lock; upstream remove deletes Roost's Desktop folder). Decision [06](issues/06-specification-readiness.md) and the [planning map](map.md) are resolved. Design Q1–Q9 and final confirmation Q10 are accepted; independent review is clean after one verified correction. No Rust application or release has been built or tested.

## Purpose and authority

Build a small synchronous Rust CLI for managing Claude configuration contexts alongside the existing shared Claude installation and upstream `ccm`. Source repository: [rmkr/roost](https://github.com/rmkr/roost). This specification combines the canonical [ownership](issues/03-profile-compatibility.md#answer), [command](issues/04-command-contract.md#answer), and [distribution](issues/05-distribution.md#answer) decisions with design Q1–Q9. Those decisions govern any accidental discrepancy; resolve a material conflict explicitly before implementation. Use the [glossary](../../GLOSSARY.md).

A profile is a context, an account is an external identity, and a session is a Claude conversation. Directory selection, status, and successful login do not establish effective account identity or service access. Support directory-scoped claude.ai login, explicit API keys, and subscription OAuth tokens under native Claude precedence. Keyless Console sign-ins are outside the isolation promise. Project settings, auth helpers, host/gateway state, and forwarded Claude options remain native behavior.

Keep upstream paths, credentials, tokens, launchers, and installation ownership intact. Explicit registration permits launch and supported inspection at the original path; Claude can write its own state there. Roost must never extract native credential files or Keychain entries, repair borrowed permissions, or purge borrowed/default data.

This artifact settles planning. Application implementation, installation, auth tests, commits, publication, release automation, and user shell edits are subsequent work. Other-system testing is deferred at the user's request; the test matrix below describes obligations, not passed checks.

## Product identity and delivery

Display name **Roost**; executable `roost` (`roost.exe` on Windows); Cargo package `roost-claude`; profile launchers `roost-NAME`, retaining stored spelling. The short command's known public-name collision is accepted. Refuse foreign executable/launcher collisions; do not recommend forced overwrites or silently select another Roost through PATH. Registry-name availability is unverified and must be rechecked before future publication.

During testing distribute source at an explicitly selected revision. Do not generate prebuilt archives, publish to crates.io, add package-manager packaging, a custom installer, or release automation. Generated profile scripts are required application behavior and are distinct from distributed prebuilt binaries.

The initial crate lives at repository root: one package, one binary, no workspace/sidecar/service. Use `main.rs` for dispatch and `cli`, `store`, `launch`, `platform` modules. CLI handles strict public parsing and safe formatting; store owns records, validation, locking and transactions; launch owns environment selection and process handling; platform contains only actual Unix/Windows differences. Exercise shared behavior through the CLI and a native fake-Claude probe; do not add a plugin interface or abstract platform framework.

Dependencies: `clap`, `serde`, `serde_json`; `getrandom` for 128-bit IDs; target-specific `rustix` and `windows-sys` bindings where std cannot satisfy required handles, permissions, terminal or signal behavior. Prefer std locking/process/filesystem APIs. No TOML config, database, async runtime, UUID framework, password framework or supervisor is required. Choose released versions compatible with Rust 1.97.0, check their real dependency MSRVs, and commit the generated Cargo.lock during implementation. Development-branch manifests in research are not release pins.

Roost uses MIT. Extra upstream credits/revision text is not required in README/help. Retain existing MIT copyright/permission notices when reusing upstream material; do not make an affiliation claim. Internal research citations remain provenance.

## Version and platform targets

Initial source-testing toolchain: Rust/Cargo **1.97.0**. This is a selected pin, not a proven application MSRV. Claude Code **2.1.280+** is the selected minimum, raised from 2.1.268 by the [workflow UX amendments](#workflow-ux-amendments-2026-10-06) because injected plugins need `CLAUDE_CODE_PLUGIN_DIRS`; it also covers status `configDirectory`. Prove floor/current required features before claiming support, and explicitly raise the floor if needed. Fish **3.2+** supplies `fish_add_path --path`. PowerShell **5.1** and **7+** remain required. Use portable POSIX syntax for sh/Bash/Zsh; record actual tested versions rather than invent historical floors.

| Environment | Required Rust target | Intended combined runtime baseline |
| --- | --- | --- |
| Linux x64 glibc | `x86_64-unknown-linux-gnu` | Claude-compatible Ubuntu 20.04+/Debian 10+ |
| Linux ARM64 glibc | `aarch64-unknown-linux-gnu` | Same, with native architecture evidence |
| Linux x64 musl | `x86_64-unknown-linux-musl` | Claude-compatible Alpine 3.19+ |
| Linux ARM64 musl | `aarch64-unknown-linux-musl` | Same, with native architecture evidence |
| macOS x64 | `x86_64-apple-darwin` | macOS 13+ |
| macOS ARM64 | `aarch64-apple-darwin` | macOS 13+ |
| Windows x64 | `x86_64-pc-windows-msvc` | Windows 10 1809+/Server 2019+ |
| Windows ARM64 | `aarch64-pc-windows-msvc` | Same, on a supported native system |

These are intended source/runtime targets, subject to locked-build and runtime checks. They are not proven Roost compatibility or a binary delivery promise. Linux needs the target's C linker/toolchain; musl/static linkage and dependency requirements must be verified on the stated baseline. macOS needs Clang/SDK with `MACOSX_DEPLOYMENT_TARGET=13.0`; MSVC needs the architecture's C++ build tools and Windows SDK (VS 2022 recommended). Non-Windows-to-MSVC builds are not the source-install recipe. Claude's own current runtime/account/network prerequisites also apply; Roost installs none of them.

Linux/macOS require Bash, Zsh, Fish and POSIX sh launcher execution. Native Windows requires Windows PowerShell 5.1, PowerShell 7+, CMD and Git Bash. Test PS 7.0–7.2 and 7.3+ argument modes separately. WSL 2 uses a Linux Roost/Claude installation, home, storage and PATH independent of native Windows; WSL 1 and Fish on native Windows are outside required coverage.

On native Windows the PATH-selected Claude must be native `claude.exe`. A selected `.cmd`/`.bat` shim is explicitly unsupported initially: refuse with native-install guidance without trying a later PATH installation. Roost's own CMD/PowerShell/Git Bash wrappers remain required. A future batch adapter needs a separate decision and native evidence.

## Public CLI

```text
roost [--allow-auth-env] [-- [CLAUDE_ARGS...]]
roost add NAME [--no-sets]
roost add NAME --link-default
roost add NAME --copy-default [--source DIR] [--yes] [--no-sets]
roost register NAME --path DIR
roost reuse NAME
roost list [--retained] [--full] [--json]
roost where NAME
roost run [--allow-auth-env] NAME [--] [CLAUDE_ARGS...]
roost switch [--allow-auth-env] [NAME] [--] [CLAUDE_ARGS...]
roost switch --no-launch [NAME]
roost switch --forget
roost status [--allow-auth-env] NAME [--json]
roost token NAME [--stdin | --clear]
roost remove NAME [--purge [--yes]]
roost update
roost setup-path [--shell SHELL] [--apply]
roost doctor [--json]
roost set list [--json]
roost set create SET [--default]
roost set delete SET
roost set default SET [--off]
roost set add SET ITEM
roost set drop SET ITEM
  ITEM: --skill DIR | --skills-from DIR | --instruction FILE | --instructions-from DIR | --plugin PLUGIN@MARKETPLACE
roost set subscribe NAME SET...
roost set unsubscribe NAME SET...
roost plugin list [--json]
roost plugin marketplace add SOURCE
roost plugin add PLUGIN[@MARKETPLACE] [--set SET... | --no-set]
roost plugin update [PLUGIN]
roost plugin auto-update (--on | --off)
roost plugin remove PLUGIN
roost desktop [--foreground] [NAME]
roost help [COMMAND]
roost --help | -h
roost --version | -v
```

Aliases: `ls`, `path`, `rm`. No args launches the [selected profile](#selected-profile-and-switching) (amended 2026-10-06; previously successful help). Version: bare version plus newline. Parse before storage mutation, prompts or secret input. Reject unknown switches/commands, extra positionals and invalid combinations. Copy and link conflict; source requires copy; no-sets conflicts with link; stdin/clear conflict; yes requires copy/purge; JSON exists only for list/status/doctor/set list/plugin list; full only for list. Switch's no-launch and forget conflict with each other, with allow-auth-env and with a Claude tail (including a lone `--`); forget takes no NAME. Switch NAME is optional (amended 2026-10-06; previously required): without it the picker chooses. Desktop NAME is optional too (amended 2026-10-06): without it the [Desktop picker](#desktop) chooses; desktop takes no argument tail, not even a lone `--`. Set add/drop take exactly one item switch; subscribe/unsubscribe take at least one SET; plugin add's repeatable `--set` conflicts with `--no-set`. Help/version do not inspect/create storage or require Claude.

Run and switch manager options precede NAME. Consume exactly one optional initial separator after NAME and preserve the remaining `OsString` tail, including `--help`, order, empty arguments and boundaries. `run work -- --` forwards a literal initial `--`. Switch without NAME takes its tail only after `--`: `switch -- --resume` forwards `--resume`, and that `--` is the one consumed separator. Bare `roost` accepts only an optional leading `--allow-auth-env` followed by nothing or by `--` and an opaque tail; `roost -- --help` forwards `--help`, while `roost --help`/`-h`/`--version`/`-v` keep their manager meaning. Any other first token is a command or switch; an unknown one is a usage error, never a profile name. Other commands allow their switches around positionals. Launchers consume no caller switches or separators.

Set names follow profile-name rules in a separate case-insensitive namespace. PLUGIN and MARKETPLACE are opaque Claude identifiers: 1–128 characters, valid Unicode, no whitespace, control characters, `/`, `\` or a second `@`.

Names: 1–32 ASCII characters, first alphanumeric then alphanumeric/underscore/hyphen; reject case-insensitive CON/PRN/AUX/NUL/COM1–9/LPT1–9. Apply validation on reads too. All manager lookups and uniqueness checks are ASCII case-insensitive. Stored spelling, directories and regenerated launcher spelling do not change on a casing variant. Native launcher filename casing follows the OS. Session names/UUIDs are opaque Claude arguments.

## Paths and persistent state

Default root is actual user home plus `.roost` on every platform (normally `%USERPROFILE%\.roost` on Windows). Unset `ROOST_DIR` uses it; empty explicit ROOST_DIR is an error for storage commands; relative root resolves from cwd. The override never changes home, Cargo root, conventional default Claude data or copy-source priority. Internal launch transport uses its bound root without editing or interpreting caller ROOST_DIR.

**Accepted Q8:** manager roots, executable/startup-file bindings, registered/source paths and selected copied names must be valid Unicode without CR/LF. Resolve to absolute paths while preserving original component spelling; do not relocate an upstream directory or use lossy text conversion. Unsupported path representation fails before publication. This restriction does not apply to the opaque Claude argument tail.

Check paths as physical objects as well as strings. Do not authorize ownership by canonicalization or path-prefix comparison alone. Reject redirected required roots/ancestors/destinations, overlaps and duplicate physical directory registration. Relative source/alias paths resolve against cwd. `where` prints one absolute directory plus newline; for aliases use caller's nonempty CLAUDE_CONFIG_DIR or conventional default (empty means unset for this query only). Alias target may not exist; do not create it.

```text
<root>/
  .roost-root.json               root ownership marker
  registry.json                 generated versioned records
  .roost-lock                   stable OS-lock object; never recreated normally
  .roost-operation.json         at most one pending mutation; nonsecret
  .roost-stage/<operation-id>/   private proven staging; not runnable
  profiles/<stored-NAME>/
    .roost-profile.json         owned isolated-profile marker
    .roost-token                optional owned manager token
    ...                         Claude-owned configuration/state
  state.json                    launch-written state: selections, last use, recorded links (amended)
  sets.json                     shared sets, subscriptions, plugin settings (amended)
  .roost-state-<ID>.tmp          transient private replacement of state.json/sets.json (amended)
  plugin-store/                 plugin store: Roost-owned Claude config dir, no account (amended)
    .roost-store.json           store marker
    .roost-store-lock           store lock, separate from the root lock
  desktop/<REGISTRATION_ID>/     Desktop data folder for one registration (amended)
    .roost-desktop.json         Desktop folder marker
  bin/roost-NAME                 POSIX launcher (all required systems)
  bin/roost-NAME.cmd             native Windows only
  bin/roost-NAME.ps1             native Windows only
```

Never place a Roost marker/token in registered upstream or default data. A local default alias has a registration and launchers, no owned data directory. Borrowed manager token name stays `.ccm-oauth-token`; linked-default marker stays `.ccm-linked-default`. Validate such borrowed objects without alteration.

IDs are independently generated 16-byte cryptographic random values, serialized as 32 lowercase hex digits; IDs contain no identity/credential content and are not security capabilities. File identity is `{ "platform":"unix", "device":"decimal-u64", "inode":"decimal-u64" }` or `{ "platform":"windows", "volume":"decimal-u64", "index":"decimal-u64" }`, obtained from open handles. Unsupported identity/security semantics cause safe refusal. IDs plus protected markers and current object identity must agree; a filename alone proves nothing. Same-user hostile modification/administrator privilege and universal power-loss durability are outside the guarantee.

The root marker is `{schema_version:1, root_id:ID, root_identity:FileIdentity}`. Owned profile marker is `{schema_version:1, root_id:ID, profile_id:ID, directory_identity:FileIdentity}`. Registry:

```json
{
  "schema_version": 1,
  "root_id": "ROOT_ID",
  "generation": 1,
  "registrations": [
    {
      "registration_id": "REGISTRATION_ID",
      "name": "Work",
      "kind": "owned",
      "state": "active",
      "directory": "/absolute/root/profiles/Work",
      "directory_identity": {"platform":"unix","device":"1","inode":"2"},
      "profile_id": "PROFILE_ID",
      "upstream_linked_default": false,
      "launcher_binding": {"format_version":1,"executable":"/absolute/cargo/bin/roost"}
    }
  ]
}
```

This is a Roost schema example, not native Claude output. `kind` is `owned`, `upstream`, or `default_alias`; `state` is `active` or `retained`. Owned records require directory/identity/profile_id and forbid linked-default. Upstream isolated records require unchanged directory/identity, null profile_id and false linked-default. Registered upstream aliases use kind default_alias, their unchanged upstream marker-directory/identity, null profile_id and true linked-default. Local aliases use null directory/identity/profile_id and false linked-default. Only owned records may be retained. Bindings are required for active and retained owned records; retained bindings are historical until reuse refreshes them. Records contain no native account, credential, token value or cached login verdict.

Retaining an owned profile retains its registration ID/name/kind/path/evidence as an inactive record; removing borrowed/local aliases removes that record. This implements retained ownership despite ordinary registration removal. Reuse preserves retained IDs/spelling, revalidates evidence and binds current executable. Reject unsupported schema/template versions, malformed records, duplicate names/IDs/physical mappings, impossible combinations, root-ID mismatch and unmanaged residue rather than resetting/migrating them. Unknown additive fields are not permission to rewrite/drop potentially incompatible state: refuse version-1 records outside this exact schema. No hand-edit/config feature or automatic migration.

If root is absent, list yields an empty list; doctor reports not initialized and PATH/Claude findings without creating it. Commands needing an existing registration fail not_found. Setup-path may print/apply the selected launcher path without initializing the store. Mutating creation initializes only an absent root or an empty real private directory. A nonempty unmarked root is foreign. Bootstrap exclusively creates the root marker; interrupted bootstrap may be completed only under the stable lock with exact root ID/identity and no foreign artifacts. An existing marker without valid registry is recovery, never an empty store to overwrite.

## Filesystem and token protection

Managed root/profiles/bin/staging are current-user owned and private. Unix creates private directories 0700 and token/registry/marker/operation/lock files 0600; launchers are owner-executable 0700. Verify effective owner/type/mode/link count on opened objects, including replacements. Do not chmod foreign or borrowed files. Owner restrictions stricter than requested are acceptable only if required read/write/traverse access works. Source/upstream directories need safe real-path identity but are not made private by Roost.

Unix: hold directory descriptors; walk components with no-follow/directory opens; inspect fstat; create/rename/unlink relative to verified parents using native bindings where needed. A symlink_metadata-check then unrelated path-open is insufficient. Check private namespaces and never follow a contained copy/purge link. Reject hardlinked secret/ownership/launcher files (link count other than one).

Windows: walk/open and retain verified ancestors with directory and open-reparse-point flags; reject every reparse family at required boundaries. Hold parent handles without delete-sharing during sensitive checks/publication, reopen/recheck final object identity and operate through verified parents/native handles. Create protected current-user DACL before secret bytes are written and verify owner/DACL/link count on the handle. Reject null, permissive, unverifiable ACLs or inadequate filesystems. Readonly flags are not confidentiality. SYSTEM/admin privilege and malicious same-user writers are not defeated by a marker. Validate the exact handle-walk/replace algorithm with native fixtures before support claims.

Read only Roost token or safe borrowed `.ccm-oauth-token`, never native credentials. Set/clear require active owned isolated profile. Preflight before input; release lock for hidden terminal input, reacquire and repeat validation before committing. Read at most 64 KiB raw, bound allocation while reading, trim outer whitespace, reject empty/embedded whitespace/NUL. Explicit `--stdin` permits nonterminal input; otherwise use an actual terminal without echo and restore its mode on every exit. No online validation, token argv/environment/file-input option or output. Cancellation leaves old token intact. Borrowed tokens satisfy equivalent confidentiality/link checks and are never repaired, even with allow-auth-env. Presence is not login validity.

Token replacement privately stages and flushes bytes, establishes protection before publish, then replaces only the verified owned destination. Old token remains until replacement publication. Clear deletes only a verified owned token, not native login; missing token clear is successful. Do not record bytes in registry/journal/errors/probe fixtures. Drop the temporary in-memory value after preparing the child; no claim of guaranteed memory erasure.

## Locking, transactions and recovery

One stable `.roost-lock` open object per initialized root. `try_lock` polling plus cancellation waits at most 10 seconds, then reports retry guidance (operational exit 1); no automatic lock-file deletion. Exclusive lock protects mutation and brief launch preparation. Read commands may acquire the same brief lock for a coherent view, but never recover/mutate. No handle inherits into Claude. Release before waiting for a session, native diagnostic probe, secret input, picker or confirmation; reacquire and revalidate after prompts. Distinct roots do not coordinate upstream/external writers. Concurrent Claude launches are permitted.

Each mutation writes a private complete intent before changing published artifacts, stages complete replacements and commits registry last. Intent shape:

```text
schema_version: 1
root_id, operation_id: ID
operation: initialize | add | register | reuse | remove | purge | token_set | token_clear | store_create | desktop_create
registration_id: ID or null for initialize
prior_generation, next_generation: unsigned integers (next = prior + 1)
phase: prepared | publishing | committed | cleanup
artifacts: [{ role, destination, staged, before, after, action, completed }]
```

Role is profile/launcher/token/registry/marker/staging_directory/store/desktop_data; action is create/replace/delete. Each artifact has an absolute destination, a nullable staged object `{path:string, object_identity:FileIdentity}`, and distinct nullable before/after destination states. Null before means verified absent at preflight; null after means intended absent after deletion. Create requires null before/non-null after; replace requires both; delete requires non-null before/null after. Each non-null state is exactly:

```text
object_identity: FileIdentity
profile_id: ID | null
launcher_binding: {format_version:unsigned-integer, executable:absolute-path} | null
launcher_form: sh | cmd | ps1 | null
registry_generation: unsigned-integer | null
```

Profile/marker states carry applicable profile ID; launcher states require binding and form (template version is binding.format_version); registry states require generation; other inapplicable fields are null. Fixed root/registration IDs and destination name supply the remaining canonical template operands. Never store template/token contents. Token states identify the file, never secret bytes. An owned private staging directory itself is journaled; record its identity immediately after exclusive creation and persist before creating child stages. A staged replacement's after identity is the same opened object as staged.object_identity across same-filesystem rename; before identifies the prior destination, never the stage.

Persist each complete before/staged/after record before publishing its artifact. Reopen/revalidate destination against before immediately before action, then against after before marking completed. Interrupted completed/phase flags are hints: recovery reopens actual objects and classifies before versus after identities, exact launcher templates/bindings, and committed registry generation. A missing or foreign object that matches neither expected state yields safe changed-state guidance, not invented ownership. If a new stage exists before its identity can be recorded, only its already-journaled protected staging parent and matching ownership evidence can authorize cleanup; otherwise retain/report it. Stage the complete next registry, flush files, perform verified same-filesystem publication, atomically replace registry, then clean journal/stage. Flush parent directory entries where supported; do not promise a cross-file atomic transaction or universal crash/power-loss durability.

All public/private launch preparation checks pending intent and rejects recovery state, even when the registry generation already advanced. An early visible launcher cannot run an incomplete profile. Read-only list/doctor report pending state; status/where refuse affected uncertain mappings. Mutating commands first classify and safely recover prior work under lock; unrelated uncertain residue blocks rather than authorizing broad cleanup.

| Mutation | Publication and recovery rule |
| --- | --- |
| Add/copy/register | Stage owned profile/marker where needed and every launcher. Publish profile and launchers while record is absent, then registry last. Uncommitted proven owned artifacts may be removed; foreign replacements block. No usable half-created profile. |
| Reuse/identical register refresh | Stage required templates/binding; preflight every destination; replace only exact owned templates for this registration, then registry. Pending state blocks launch. Proven new wrappers can be rolled back to recorded owned binding or left blocked with explicit guidance; never overwrite foreign replacement. |
| Ordinary remove | Preflight all launchers before deleting any; retain registry/evidence if a later deletion fails, report changed/remaining artifacts. Successful owned removal commits retained state; borrowed/alias removal drops record. Recovery of interrupted removal restores the previous active registry bookkeeping only after reporting missing launchers; explicit reuse repairs them, or explicit remove retries. |
| Token set/clear | Stage/protect token if setting. Record intent before replace/delete, then increment registry generation. Recovery determines actual token presence/identity, completes committed bookkeeping or reports whether old/new/absent token remains; do not invent rollback of secret bytes. Explicit token command can retry. |
| Store/Desktop folder creation | Exclusively create the private directory, record its identity, write its marker, then increment registry generation; registrations are unchanged. Recovery removes a proven new directory that is still empty or holds only its partial marker, or completes bookkeeping for a marked one. |
| Purge | Validate active/retained owned marker/registry/root agreement, all launchers and directory identity before confirmation. The registration's Desktop folder, if present, is a separate journaled `desktop_data` artifact deleted after profile content with the same marker-last rule. Preserve root marker and profile marker throughout content deletion. Journal remains after partial failure; no automatic continuation of deletion. Explicit remove --purge must revalidate and reconfirm/reacknowledge before retry. Commit removal only after deletion finishes. |

For purge, enumerate immediate entries through the validated owned directory, retaining `.roost-profile.json` until last; unlink contained links themselves, and use std's non-following `remove_dir_all` for each real child directory rather than writing a recursive deletion engine. Revalidate anchored ownership before each step. Remove marker and empty profile directory at the final boundary; a crash in that final empty interval may only finish empty-directory/bookkeeping cleanup with matching journal/root/physical identity, never authorize deletion of newly introduced content without marker evidence. Partial deletion is irreversible and must be reported. Native reparse/hardlink/symlink-race fixtures are required. Std traversal guarantees do not replace the initial ownership decision.

Completion/abort cleanup touches only journal-listed objects with matching root/profile/operation and physical evidence. Never clean by filename prefix alone. If registry is committed, finish proven bookkeeping; if uncommitted, remove/retain proven staged work and keep the last committed registry. Purge journal is an explicit retry requirement, not a request to auto-delete. Doctor identifies pending operation/staging and safe next command without repairing. Unsupported schema or unverifiable replacement requires manual inspection, not a generated rm recipe for foreign data.

## Command behavior

Add creates isolated owned data or a local pass-through alias with launchers; active/retained name blocks add. Empty/alias creation has no confirmation. Register accepts only a real `<upstream-home>/.ccm/profiles/<upstream-name>` directory including custom upstream homes; preserve path. Reject conventional default, Roost data, overlaps/duplicates/redirection. Safely recognize linked-default marker; same name/path/physical identity/kind refreshes launchers, changed mapping/marker fails. Reuse restores retained owned data or refreshes active registrations; missing unsafe isolated/borrowed roots fail, never reconstruct data.

List is metadata-only, active by default; retained flag includes retained owned records; full adds bounded probes ([`roost ls`](#roost-ls)). Run/status/token need active registration; where also accepts safe retained owned. Ordinary remove retains owned data/token/evidence and reports that native login is unchanged; already-retained safe owned remove is a no-op. Purge only active/retained owned isolated data, plus Roost-owned per-registration data under [remove and purge](#remove-and-purge); reject borrowed/default data before prompting even with yes. No automatic native logout/revocation.

Copy source: explicit source, nonempty inherited CLAUDE_CONFIG_DIR, conventional default. Reject absent/non-directory/redirected root and overlap; require quiet-source acknowledgement. Never interpret ROOST_DIR as source/home. Copy ordinary settings/plugin/history files independently, never hardlink. Exclude top-level cache/daemon/ide/paste-cache/shell-snapshots/telemetry/backups. At every depth exclude `.credentials.json`, `.claude.json` and their basename-plus-dot variants, `.ccm-oauth-token`, `.ccm-linked-default`, `.roost-root.json`, `.roost-profile.json`, `.roost-token`, `.roost-operation.json`, `.roost-lock`, `.roost-stage` and Roost-created temporary ownership/token names. Do not copy a separate companion `.claude.json` or parse global native state. Skip/report contained links/reparse points/multiply-linked regular/special files. Unreadable selected ordinary file fails staging. Copied settings/history may still contain secrets/helpers/machine references; require native login/auth verification afterward. Report omission paths/reasons/counts safely without file contents.

Copy/purge need quiet writers: show exact scope on stderr and terminal confirmation; yes skips it as acknowledgement; without terminal require yes. Decline/terminal EOF cancel before mutation. No confirmation for normal remove, token replacement/clear, registration/reuse or manual update. No reliable external-session detection or coordination promise.

## Launchers, environment and children

Every wrapper binds absolute manager executable, absolute root, root ID and registration ID; it never invokes Roost through PATH or edits ROOST_DIR. Exact private transport:

```text
ABSOLUTE_ROOST __roost_launch_v1 ABSOLUTE_ROOT ROOT_ID REGISTRATION_ID -- [literal caller arguments]
```

Private dispatch validates the fixed header and consumes its own fixed transport boundary only. It then forwards the entire caller tail, including an initial --, --help or --allow-auth-env as Claude arguments. Omit private transport from public help; it is no privilege bypass. Validate root/registry/IDs/active state, safety and matching binding/current executable before launch. A missing/relocated binding refuses with explicit reuse/identical-registration refresh guidance. Same path upgrades retain compatible transport/template formats; incompatibility refuses. No fallback to another executable.

Launcher ownership requires a protected regular single-link object whose complete bytes match the canonical template for its recorded format/root/IDs/binding; a comment/name alone is insufficient. No signature/authenticity promise against malicious same-user edits. Stage/validate every required form before publication; native Windows requires all three, Unix the sh form. Refresh accepts missing destination or an exact owned old template; any other content is foreign collision. Startup-file PATH blocks are separate ownership evidence, never launcher evidence.

POSIX wrappers use a sh shebang and exec the quoted absolute executable/fixed operands plus "$@". Quote fixed values with POSIX single-quote escaping; never eval caller text. Fish callers execute this file normally. Native Windows .cmd disables delayed expansion within its local scope, emits a single direct native EXE invocation using fixed batch-safe literals and raw %* (no CALL/extra cmd /c reparse), and returns the native status. Preserve the environment inherited at wrapper entry; do not create transport environment variables. Test local-scope/endlocal behavior and !/%/quotes explicitly. The initial caller shell's own parsing cannot be undone.

PowerShell .ps1 treats the received string-array tail as data, uses .NET ProcessStartInfo with UseShellExecute=false, inherited streams/cwd/environment and no shell command construction. Use ArgumentList when available; for 5.1/legacy .NET, implement the standard Windows runtime quoting algorithm (quote empty/whitespace/quote-bearing arguments; double backslashes before quotes and before a closing quote) into Arguments. Do not use legacy `& exe @args` as proof of empty/embedded-quote preservation. Wait and return ExitCode, retaining console cancellation. Git Bash uses the sh wrapper/native executable and normal MSYS conversion; do not silently edit MSYS2_ARG_CONV_EXCL or caller environment. Document native shell quoting/conversion controls and measure the values at the received-argument boundary.

Resolve the shared Claude through caller platform PATH. Unix accepts executable/shebang installations; Windows follows selected PATH/PATHEXT result and requires native claude.exe. No shell interpolation of Claude argument values. Validate supported version for launches/status/update, using bounded version probe; missing/unsupported result refuses with guidance. Version probes inherit ordinary caller environment without profile token injection and never read native credentials. Doctor captures version once, never runs auth status for every profile.

For isolated run/status set only CLAUDE_CONFIG_DIR=stored directory, DISABLE_AUTOUPDATER=1, FORCE_AUTOUPDATE_PLUGINS=1 plus a safe manager CLAUDE_CODE_OAUTH_TOKEN when eligible; isolated launches (owned and upstream; not status) also set CLAUDE_CODE_PLUGIN_DIRS when the registration's sets provide store plugins ([plugin store](#plugin-store)). Keep cwd/stdio/other environment. Refuse nonempty (including whitespace/0/false) inherited names from the exact finite [authentication table](issues/04-command-contract.md#launch-authentication-and-update-environment); no prefix heuristics or cloud-SDK reimplementation. Check before token injection; empty/absent is allowed. Isolated directory overrides inherited CLAUDE_CONFIG_DIR. Allow-auth-env preserves caller auth/provider variables and suppresses all manager token injection, but still checks root/token path/permissions. Aliases preserve entire caller environment and bypass token injection/auth conflicts, including empty/other-root ROOST_DIR and CLAUDE_CONFIG_DIR. Scope is pass_through, never isolated.

Run/update use uncaptured native execution and inherit stdin/stdout/stderr and console/cwd. On Unix use CommandExt::exec after closing store/lock handles and restoring manager-owned terminal/signal changes: Claude replaces Roost in the same PID/foreground group, so native signals and exit status reach the caller directly. On Windows spawn the native child and wait in the shared console. Update invokes exactly `claude update` with unchanged caller environment; it is shared-Claude updating, not Roost self-update, and never injects profile/token/plugin/update variables. Claude enforces caller restrictions; Roost neither installs nor package-manager-falls-back.

Before handoff cancellation restores terminal and leaves mutations uncommitted where still staged; manager returns 130. Unix interactive exec eliminates an extra parent/signal-forwarding heuristic: normal terminal and direct PID signals act on Claude itself after handoff. Exec failure is a safe operational error followed by immediate process exit, with no further mutation or state reuse: failed exec can already have changed process environment/stdio. [Rust exec semantics](https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html#tymethod.exec). Windows uses console handlers to survive normal CTRL_C/CTRL_BREAK long enough to wait for the shared-console child and wrapper; do not double-send an event already delivered or pretend CTRL_C can target arbitrary groups. Forced termination is distinct from graceful console cancellation; there is no automatic deadline on interactive children and no recursive task-tree supervisor. Return child numeric result; Unix signal death is observed by the caller as 128+signal; Windows preserves native exit/cancellation code. Captured probes use a managed child and are terminated/reaped on their explicit bounded/cancel path.

## Supported status, diagnostics and JSON

Status invokes `claude auth status` (JSON default) under the same profile/env/token safety as run. **Accepted Q9:** captured auth/version probes have a 10-second monotonic deadline and 1 MiB cap per stream, drain both concurrently, and terminate/reap on timeout/overflow/cancellation. Interactive run/login/session/update have no deadline. Never emit captured raw stdout/stderr in manager diagnostics.

Recognize status only when stdout parses as a JSON object with at least one allowlisted correctly typed documented authMethod or configDirectory, and exit is 0 or 1. Derive login boolean from documented exit (0 true, 1 false) only after that recognition. Parse authMethod only from `none`, `claude.ai`, `oauth_token`, `api_key`, `api_key_helper`, `third_party`; configDirectory only from a nonempty valid absolute path string. Missing/wrong/unknown optional fields become null with safe warning. No recognized field, contradictory recognized method/login verdict (none with exit 0 or non-none with exit 1), malformed JSON, unexpected exit, spawn/timeout/size failure => unknown auth_status error and exit 1. Ignore all native identity/credential/undocumented fields. Actual sanitized floor/current fixtures must validate this recognizer; do not describe this as Claude's complete native schema. Observed directory mismatch is a safe warning, not account evidence.

Human list/status/doctor results use stdout; warnings/prompts/progress/errors use stderr. Where/version are bare data; run/update streams belong to Claude, manager notices stay on stderr. Successfully parsed JSON list/status/doctor print exactly one object on stdout, including operational errors; malformed manager syntax prints normal stderr usage and exit 2.

All JSON commands use `{ "schema_version":1, "data":..., "warnings":[], "error":null }`. Warnings are an array of safe strings. Error is null or `{ "code":string, "message":string, "next_step":string|null }`. Emit every documented key, explicit null for unavailable values, no raw captured native data or token contents.

| Command | Exact data object |
| --- | --- |
| list | `{ "project":string|null, "profiles": [ProfileRecord] }`; project (amended) is the current project key, null when unavailable |
| status | `{ "name":string|null, "kind":Kind|null, "reported_logged_in":boolean|null, "auth_method":AuthMethod|null, "config_directory":string|null, "scope":"isolated"|"pass_through"|null }` |
| doctor | `{ "claude_path":string|null, "claude_version":string|null, "storage_directory":string|null, "launcher_directory":string|null, "path_member":boolean|null, "profiles":[ProfileRecord], "findings":[Finding] }` |
| set list | `{ "sets":[{ "name":string, "default":boolean, "items":[{ "kind":ItemKind, "value":string }], "subscribers":[string] }] }`; ItemKind is `skill`, `skill_source`, `instruction`, `instruction_source` or `plugin`; value is the absolute path or plugin ID |
| plugin list | `{ "auto_update":boolean, "last_auto_update":integer|null, "plugins":[{ "id":string, "sets":[string] }] }` |

ProfileRecord is exactly `{ "name":string, "kind":Kind, "state":"active"|"retained", "directory":string|null, "token_present":boolean|null, "launchers":[{ "path":string, "condition":"ready"|"missing"|"unsafe"|"collision"|"stale"|"not_required" }], "sets":[string], "last_used":integer|null, "selected":boolean, "most_recent":boolean, "probe":{ "reported_logged_in":boolean|null, "auth_method":AuthMethod|null, "config_directory":string|null }|null }` (last five keys amended 2026-10-06). Sets are subscribed set names sorted by folded name (empty for aliases). Last_used is Unix seconds UTC of the last recorded launch. Selected is true only for the current project's valid selection; most_recent only for the registration with the greatest last_used. Probe is null without `--full`, for aliases and for retained records; with `--full` its fields follow status recognition and are null on probe failure with a safe warning. Kind uses owned/upstream/default_alias. Directory is stored isolated/borrowed root, or expected caller directory for an alias; list computes this without creating it. A registered linked-default alias's upstream marker-directory is not its effective Claude directory; persisted evidence remains internal. Token_present is safe manager/borrowed token presence only (null when unsafe/unavailable, false for aliases), not auth validity. Retained launchers are not_required when absent; stale/foreign leftovers are reported. Finding is `{ "code":string, "severity":"warning"|"error", "message":string, "path":string|null, "next_step":string|null }`. Findings serialize storage/launcher/Claude/PATH/recovery checks, not account identity.

Sort profile records by folded name then stored spelling, launcher records by path and findings by code/path for deterministic CLI fixtures. Errors may retain safe partial data. Doctor remains read-only: [Desktop](#desktop) and [state](#state-files) findings are warnings; PATH absence alone warning, missing Claude/required active launchers/unsafe paths/collisions/recovery requiring action fail. Retained profiles need no working launchers. Never chmod/fix/remove while diagnosing.

Stable error categories: usage, not_found, collision, ownership, unsafe_path, auth_conflict, invalid_token, claude_unavailable, claude_unsupported, auth_status, io, diagnostics, cancelled, plugin_store, desktop_running (last two amended 2026-10-06). Do not add secret/native-field details to explain them. Include safe operation/profile/path and concrete next step, actual changed/remaining state on partial failure.

| Result | Exit |
| --- | --- |
| Success/help/version/warning-only doctor | 0 |
| Operational/safety/failed doctor/unknown status | 1 |
| Invalid manager syntax; picker or set prompt needed without a terminal | 2 |
| Recognized native status logged out | 3 |
| Declined confirmation/terminal EOF/cancel before child handoff, including picker and set prompt | 130 |
| Run/update/switch/bare-launch/desktop child | Numeric child result, Unix 128+signal, Windows native result |

Empty explicit token stdin is invalid_token, not terminal cancellation. A pending purge is reported as partial deletion, never as successfully retained intact data.

## PATH setup

Default is instructions; apply edits only the selected one user destination. Supported Unix shell values bash/zsh/posix/fish; Windows powershell/cmd. Infer current shell for printed recipe where recognized; if apply cannot choose one unambiguously require explicit shell with safe guidance. Reject unsupported platform combinations. Show target before apply without another confirmation.

Bash: actual-home .bashrc. Zsh: nonempty ZDOTDIR/.zshrc else actual-home .zshrc. POSIX: actual-home .profile. Use one `# >>> roost PATH >>>` / `# <<< roost PATH <<<` block with a safely quoted resolved launcher path and idempotent PATH membership logic. Require no linebreak in fixed literal; preserve all other bytes/line endings. Refuse malformed/multiple conflicting blocks, redirection/nonregular/unowned/multiply-linked destination. Replace only a verified manager block through private same-directory staging and revalidation. Missing file may be created privately; do not otherwise rewrite shell config permissions/content.

Fish: one complete owned `roost-path.fish` in configured user conf.d, with the same Roost markers and `fish_add_path --path 'resolved-launcher-directory'`. Obtain configured user directory from the selected Fish's supported `__fish_config_dir` query, with native XDG/home fallback only for conventional configuration; fail when discovery is ambiguous. Respect nonempty XDG_CONFIG_HOME; do not modify universal vars. Foreign/malformed snippet refuses. Shell startup discovery can execute user shell setup, so do not treat captured output as authority without validating the single path; no source parsing of arbitrary scripts.

Native Windows: read/write only HKCU Environment Path, preserve existing order/spelling/value kind, append resolved launcher directory once using case-insensitive membership, notify environment change and advise new terminal. Preserve expandable entries; compare a safely expanded view for membership without rewriting their stored spelling. No system PATH/current-parent environment mutation/automatic undo. Cargo's executable bin PATH is a separate manual installation concern.

## Workflow UX amendments (2026-10-06)

Source: [workflow UX design](../workflow-ux/design.md), [ADR 0001](../../docs/adr/0001-shared-plugins-injected-at-launch.md), [ADR 0002](../../docs/adr/0002-desktop-per-profile-via-user-data-dir.md). Ownership invariants are unchanged: nothing below writes into registered upstream or default data. Upstream and alias state lives only in Roost's own files, keyed by registration ID.

### State files

`state.json` and `sets.json` are side files, not journaled mutations. Write them only under the root lock by creating `.roost-state-<ID>.tmp` exclusively and privately in the root, writing and flushing the complete document, then renaming over the destination with the same protections as the registry. Validate the destination's owner/mode/link count before replacement. A leftover temp file is admitted by root validation and removed by the next side-file write only when it is a private regular single-link current-user file of that pattern. Absent files mean empty state; read commands never create them.

```text
state.json: { schema_version:1, root_id:ID,
  selections:  [{ project:absolute-path, registration_id:ID }],
  last_used:   [{ registration_id:ID, at:unix-seconds }],
  links:       [{ registration_id:ID, path:relative-path, target:absolute-path, link_identity:FileIdentity }],
  desktop_launched: [{ registration_id:ID, at:unix-seconds }],
  plugin_auto_update_at: unix-seconds | null }
sets.json: { schema_version:1, root_id:ID,
  sets: [{ name:string, default:boolean, items:[{ kind:ItemKind, value:string }] }],
  subscriptions: [{ registration_id:ID, set:string }],
  plugin_auto_update: boolean }
```

Link `path` is relative to the profile directory (`skills/NAME` or `rules/NAME.md`). Validation follows the registry rules (exact schema, root-ID match, no duplicate project/registration/name/path entries). Entries naming a registration ID absent from the registry are ignored and dropped by the next write. At launch a missing, malformed or unsupported side file is a safe warning and the launch proceeds without that state and without rewriting it; explicit `switch`, `set`, `plugin` and `add` writes fail with the registry's validation category instead. Side files never hold tokens, credentials or Claude output.

### Selected profile and switching

Project key: walk from the current directory toward the filesystem root to the first `.git` entry. A directory is the git directory; a regular file of the form `gitdir: PATH` names it (relative to the file's directory). If the git directory contains `commondir`, resolve its single-line path relative to the git directory. The key is that common directory as an absolute path, so all worktrees share one key; without `.git` it is the current directory. Do not run git or read `GIT_DIR`/`GIT_COMMON_DIR`. A malformed `.git` file or `commondir` falls back to the current directory with a safe warning. A key that is not valid Unicode without CR/LF is unsafe_path for selection commands and makes list's `project` null.

Bare `roost` launches the registration selected for the current project with the tail after `--`, exactly as `run` would. A selection is valid only when its registration is active. With no valid selection (a stale one first warns naming the forgotten profile), Roost shows the picker when stdin and stderr are both terminals; otherwise it fails usage (exit 2) with next steps `roost switch NAME` and `roost run NAME`. With no active registrations it fails not_found naming `roost add NAME`. The picker shows even for one profile.

Picker: single-choice list on stderr of active registrations sorted as list, each row the `ls` columns with the `ls` styles (bold names, colored launcher state, dim placeholders) when stderr is a terminal and `NO_COLOR` is unset or empty; the highlighted row is reverse video across its styled cells (amended 2026-10-06). Initial highlight is this project's valid selection, else the most recently launched registration, else the first row. Up/Down (and k/j) move, Enter chooses; Ctrl-C, Esc, `q` or EOF cancel with exit 130 and no state change. Raw mode is restored on every exit. The lock is released while the picker waits; afterwards reacquire, revalidate the choice and record the selection before launching.

`switch NAME` records NAME as this project's selection and launches it like `run`; `--no-launch` only records; `--forget` removes this project's selection (success when none). `switch` without NAME always shows the picker, even when the project already has a valid selection, then records and launches (or with `--no-launch` only records) the choice exactly as `switch NAME` would (amended 2026-10-06). It needs stdin and stderr to be terminals, checked before storage is read; otherwise it fails usage (exit 2) with next steps `roost switch NAME` and `roost run NAME`. Cancel is exit 130 with no state change. NAME must be active; default aliases may be selected. Selection is recorded before launch preparation, so a later launch failure leaves it in place.

Last use: `run`, `switch`, bare `roost` and profile launchers set `last_used` for the launched owned or upstream registration. Default-alias launches never record last use; picking or switching to an alias records only the selection. The most recent registration is the one with the greatest `at`.

### `roost ls`

Human `list` prints one table on stdout: marker column (`@` selected for this project, `^` most recent, space otherwise; `@` wins), then Profile, Kind, Token (`✓`, `–` absent, `?` unknown), Launchers (`ready` when every required launcher is ready, else the worst condition), Sets (comma-joined, `—` when none), Last used (relative: `now`, `Nm`, `Nh`, `Nd`, `never`). Columns align by display width. A summary line follows: profile count, upstream count, marker legend and the names of columns hidden to fit the terminal width (Path is always hidden in the default view). Color only when stdout is a terminal and `NO_COLOR` is unset or empty; piped output is plain, same columns. `--retained` keeps its meaning. `--full` adds Login, Auth and Claude dir columns from status probes for active owned/upstream rows, run in parallel under the existing 10-second/1 MiB bounds and allowlist; the lock is not held during probes and a failure shows `?` with a warning. Default list runs no probes.

### Shared sets

A set has a name, a default flag and items. Item kinds (v1): `skill` (an absolute skill directory, linked under `skills/` as its basename), `skill_source` (a directory; every immediate child directory is a skill), `instruction` (an absolute `.md` instruction fragment, linked under `rules/` as its basename), `instruction_source` (a directory; every immediate child regular `.md` file is a fragment) and `plugin` (a store plugin ID). Paths are stored absolute per Q8 and need not exist at definition time. Source expansion skips entries whose names start with `.`, are not valid Unicode, contain CR/LF, or are named `synced`, and skips links/non-matching types. Roost never writes into source directories.

Commands: `set create` refuses an existing name (collision). `set delete` drops the set and its subscriptions; recorded links leave at each profile's next launch. `set default` marks or (`--off`) unmarks. `set add`/`set drop` add or remove one item; adding a duplicate or dropping an absent item is a no-op success; `--plugin` requires the plugin to be installed in the store (not_found). `set subscribe NAME SET...` and `unsubscribe` accept active or retained owned registrations and active upstream registrations; aliases fail usage ("default aliases receive no shared items"). Subscribing is refused (collision) when, after the change, two different subscribed items would produce the same link path or the same plugin name from different marketplaces; this is checked against current source contents. A subscription on an upstream registration delivers only plugin items; skills and instructions are ignored for it.

`add` of an owned profile subscribes it to every default set after the registry commits, unless `--no-sets`. `add --copy-default` whose source resolves to the physical directory of an active or retained owned registration copies that registration's subscriptions instead of the default sets. Copy already skips contained links, so recorded links are not copied; the next launch creates them. A subscription write failure after a committed add warns with a `roost set subscribe` next step.

Reconciliation (owned registrations only, at every launch, under the lock): desired links are the union of subscribed skill and instruction items after expansion. A path desired by two items is a launch-time conflict: warn and skip that path. For each recorded link no longer desired or whose target changed, remove it only when the object at its path is still the recorded symlink (same identity, same target text); a missing or replaced object drops the record with a warning and is never touched. For each desired path: if absent, create the `skills/` or `rules/` directory if needed (private) and a symlink with the absolute target through the verified profile directory handle, then record it; if anything else exists there, existing content wins: warn and skip. Never create, remove or replace `skills/synced` or `plugins/synced`. Targets are not followed or validated beyond their link text. Links are Unix symlinks; Windows link support is deferred with the rest of the Windows backend. A profile's `CLAUDE.md` is never touched.

Settled by [ticket 06](../workflow-ux/issues/06-injected-plugin-behavior.md) ([research addendum](../workflow-ux/research/injected-plugins.md#addendum-user-level-rules-and-path-imports-orchestrator-question)): `rules/` in a config directory loads like `~/.claude/rules/` (the same config-dir function yields the user `CLAUDE.md` and rules directory), so instruction items link into `rules/` as above. The considered fallback, a Roost-managed `@ABSOLUTE-PATH` import block in the profile's `CLAUDE.md`, is not part of v1.

### Launch-time mutation

Launch preparation (`run`, `switch`, bare `roost`, profile launchers, `desktop`) keeps its existing checks, including refusing a pending intent; it never recovers journaled operations. After those checks pass and while still holding the brief lock it: records selection (switch/picker) and last use; for owned registrations reconciles links; and, when auto-update is due, claims it by writing its timestamp. Then it releases the lock, runs a claimed auto-update under the store lock only (see Auto-update), injects store plugins and hands off. Each of these steps that fails emits a safe stderr warning and the launch continues. Lock contention keeps the existing 10-second failure. `status`, `where` and `list` do none of this.

### Plugin store

`<root>/plugin-store/` is a Roost-owned Claude config directory with no account that never runs sessions. Its first use creates it through the `store_create` journal operation with marker `.roost-store.json` `{schema_version:1, root_id:ID, directory_identity:FileIdentity}`. It is never a registration, copy source or `where` result.

Store commands run the PATH-selected Claude (version-checked) as `claude plugin marketplace add SOURCE`, `claude plugin install ID`, `claude plugin update ID` (once per plugin: Claude's CLI requires exactly one plugin per update, verified on 2.1.291; amended 2026-10-06) and `claude plugin uninstall ID` with the caller's environment plus `CLAUDE_CONFIG_DIR=<store>` and `DISABLE_AUTOUPDATER=1`, minus `CLAUDE_CODE_OAUTH_TOKEN` and `CLAUDE_CODE_PLUGIN_DIRS`; no manager token is injected and the auth-conflict table does not apply. Store commands take the root lock only briefly, to validate the store and update Roost's records, never while the child runs. The child runs under the separate store lock `plugin-store/.roost-store-lock` (handle not inheritable; same 10-second wait and retry guidance), so launches, which only read the store, never wait on an install; the child is spawned with inherited stdio and waited for, with no deadline. Store commands and auto-update serialize on the store lock. A launch that cannot read or parse the store's state skips plugin injection with a warning and still launches. A nonzero child exit is plugin_store (exit 1) naming the retry command; Roost's own records change only after success.

`plugin add` resolves `PLUGIN` without `@MARKETPLACE` through Claude's store listing (fields fixed by ticket 06); ambiguity or absence is not_found. Without `--set`/`--no-set`, a terminal shows the sets and reads a comma-separated list (empty for none) before taking the lock; without a terminal it fails usage naming both flags; with no sets it proceeds with none. Named sets must exist (not_found) and pass the subscribe conflict rule (collision) before install. After a successful install the plugin ID is added to the sets. `plugin update` updates one or all store plugins, running `claude plugin update ID` once per plugin recorded in Roost's store record. `plugin remove` first drops the ID from every set, then uninstalls; an uninstall failure is plugin_store and may be retried. `plugin list` reports store plugins known to Roost's sets plus the auto-update state. `plugin auto-update` sets `plugin_auto_update`.

Injection: for an isolated launch, collect the plugin items of every subscribed set, deduplicate by ID, and resolve each to its installed directory inside the store (which directory, and how Roost learns it without parsing undocumented state, is fixed by ticket 06). Missing plugins are skipped with a warning. If any remain, set `CLAUDE_CODE_PLUGIN_DIRS` to the caller's nonempty inherited value (if any) followed by the store directories, joined with the platform path-list separator; otherwise leave the variable as inherited. Allow-auth-env does not affect injection. Aliases, status probes and `update` never receive it. Injected plugins appear as `name@inline`; Claude does not auto-update them.

Auto-update: when `plugin_auto_update` is true and `plugin_auto_update_at` is null or at least 24 hours old, a launch first writes the new timestamp under the root lock (so concurrent launches skip) and releases the root lock; then, only if the store lock is free (non-blocking try; otherwise skip silently), it runs `claude plugin update ID` in the store once per recorded plugin, refreshes Roost's store record from `claude plugin list --json` and prunes orphaned versions (Q49), all under the store lock only, never the root lock (amended 2026-10-06). Children get stdin null, output captured and discarded under the 1 MiB cap, and one 8-second deadline for the whole update. Timeout or failure terminates/reaps the child, warns and launches. Injection then uses the refreshed record, so the launch already receives updated versions.

Orphaned versions (accepted 2026-10-06, Q49): Claude's 14-day sweep never runs on the store. After a successful `plugin update`, still under the store lock, Roost deletes store version directories under `plugins/cache/` whose `.orphaned_at` marker is more than 14 days old and that no Roost store record references, opening each without following links and verifying it lies inside the store cache. Failures warn. Native duplicates (Q50): a store plugin also installed natively in an owned profile is silently overridden by the injected copy, so `doctor` (never launch) runs `claude plugin list --json` per owned profile under the bounded-probe rules and reports `plugin_shadowed` (warning) with next step `claude plugin uninstall NAME` in that profile. Injection also follows the [injected plugin research](../workflow-ux/research/injected-plugins.md): inject each plugin's dependency closure, refuse duplicate manifest names, skip missing or orphaned directories with a warning, and leave a profile's `"<name>@inline": false` opt-out to Claude.

### Desktop

`roost desktop NAME` (experimental, Linux only; other platforms fail claude_unavailable) needs an active registration and resolves `claude-desktop` through PATH (missing: claude_unavailable). There is no forwarded argument tail.

- Picker (amended 2026-10-06, Q56/Q57): `roost desktop [--foreground]` without NAME always shows the [picker](#selected-profile-and-switching) (same keys, styles, cancel with exit 130 and no state change, lock released while it waits, then reacquire and revalidate the root and choice) titled for Claude Desktop, then launches the choice exactly as `roost desktop NAME` would, including `--foreground`. Desktop is machine-wide, so the initial highlight is the registration with the most recent `desktop_launched`, else the most recently launched registration, else the first row, and the choice is never recorded as a project selection. Rows are the `ls` columns plus Desktop: `running` (its Desktop folder holds a live `SingletonLock`), `signed in` (its Desktop folder exists), `—` (no folder yet); a default alias row shows `plain` (plain Desktop, no Roost folder). Choosing a running profile fails desktop_running as for NAME. Without stdin and stderr terminals, checked before storage is read, it fails usage (exit 2) with next step `roost desktop NAME`; with no active registrations, not_found naming `roost add NAME`.
- Owned/upstream: the data folder is `<root>/desktop/<REGISTRATION_ID>/`, created on first use by the `desktop_create` journal operation with marker `.roost-desktop.json` `{schema_version:1, root_id:ID, registration_id:ID, directory_identity:FileIdentity}`. Launch preparation is the isolated `run` preparation (auth checks, token, plugin injection, reconciliation, last use), then record `desktop_launched` and start `claude-desktop --user-data-dir=<folder>` [detached](#desktop).
- Default alias: start plain `claude-desktop` [detached](#desktop) with the caller's environment; no folder, nothing recorded.
- Detached by default (amended 2026-10-06 at the user's request): the child gets a new session (`setsid`), stdin/stdout/stderr on the null device and no inherited Roost handles, and Roost does not wait for it. Roost then watches it for up to 2 seconds: an exit in that window is reported as claude_unavailable with the exit status and a `--foreground` next step; otherwise Roost prints `Started Claude Desktop for NAME` to stderr and exits 0. Chromium's own console noise is never shown. `--foreground` instead execs (Unix) with inherited stdio, as for `run`, for debugging.
- Running check: if the folder's Electron `SingletonLock` link names this host and a live PID, fail desktop_running ("this profile's Desktop is already running"). If any other Roost Desktop folder, or the conventional `~/.config/Claude`, holds such a live lock, warn that Cowork is untested with a second concurrent Desktop, and launch.
- Doctor: for each folder with a `desktop_launched` record but no Electron data besides the marker, finding `desktop_data_not_isolated` (warning) suggesting the flag may have stopped working.

### Remove and purge

| State | Ordinary remove (owned: retained) | Remove of upstream/alias (record dropped) | Purge (owned) |
| --- | --- | --- | --- |
| Selections | Cleared | Cleared | Cleared |
| Last use, `desktop_launched` | Kept | Dropped | Dropped |
| Subscriptions | Kept, so reuse restores them | Dropped | Dropped |
| Recorded links | Links stay in retained data; records kept | n/a | Links unlinked as contained links, never followed; records dropped |
| Desktop folder | Kept | Upstream: deleted after confirmation (see below); alias: none | Deleted (journaled `desktop_data`) |
| Plugin store | Untouched | Untouched | Untouched |

Side-file changes happen after the registry commits; failure warns with a `roost switch --forget` or `roost set unsubscribe` next step, and dropped registration IDs are ignored by readers anyway. Ordinary remove of an upstream registration that has a Desktop folder also deletes that Roost-owned folder (its Desktop sign-in), after confirmation naming the folder (`--yes` skips; no terminal without `--yes` is usage), as a journaled `desktop_data` artifact with the marker-last rule, then drops the record; borrowed data is never touched. Purge stays owned-only: upstream and alias registrations still reject `--purge`. Purge, and upstream remove, refuse with desktop_running (next step: quit Claude Desktop for NAME) while that registration's Desktop folder holds a live `SingletonLock`, checked before confirmation (accepted 2026-10-06, Q51). `remove NAME --yes` without `--purge` is accepted only for upstream registrations, where it skips the Desktop-folder confirmation; otherwise it is usage (Q52). The plugin store has no removal command; uninstall plugins with `plugin remove`.

## Source install, upgrade and practical recipes

Once implementation supplies Cargo.toml/Cargo.lock, use a checkout at the selected tested revision:

```text
cargo +1.97.0 install --path . --locked --bin roost
roost --version
roost doctor
roost setup-path --shell fish
roost setup-path --shell fish --apply
```

These are future instructions, not commands run during planning. Check selected Cargo installation root/bin and existing executable ownership first; retain Cargo's normal collision protection, never force a foreign binary. Cargo native custom-root/config precedence stays authoritative. Add its bin to PATH independently. Upgrades explicitly select the next tested revision and repeat same-package source install; end Roost operations first (Windows executable sharing matters), verify selected path/version/doctor, and use reuse/identical registration refresh for relocated owned wrappers. Compatible schema/template/transport state remains usable; incompatible state refuses with tested-version/manual-recovery guidance, never automatic migration.

Uninstall with Cargo's package uninstall for roost-claude in the same chosen Cargo root. It removes the Cargo-managed executable only; data/tokens/launchers/PATH remain. Remove registrations or explicitly purge owned profiles beforehand only when deliberately requested. There is no logout/revocation/uninstall cleanup shortcut.

```text
roost add work
roost run work auth login
roost status work
roost run work --name client-a
roost run work --name client-b
roost run work --resume
roost run work --resume client-a
roost run work --continue
roost run work --resume client-a --fork-session
```

Use separate terminal tabs/windows; same/different profiles can run concurrently. Native picker defaults to current project/worktree; Claude owns name/UUID selection, widening options and errors. Shared conversation continuation can interleave transcript; fork makes an independent native continuation. Native background dispatch/worker attachment has no Roost authentication/result guarantee.

Browser sign-in and native /login use Claude's own browser/manual URL/code flow and credential storage/refresh; no Rust OAuth/callback/client. Show owned token transition `roost token work --clear` before browser login reliance. Borrowed tokens are changed upstream, or explicit direct run/status allow-auth-env suppresses token injection without bypassing borrowed-path safety. Successful login is not service/account proof under all settings. Switch managers by command: ccm/claude-NAME versus roost/roost-NAME; register an unchanged upstream real path explicitly, never migrate automatically.

## Implementation acceptance recipes

Implement checks as code exists, using disposable roots and fake secrets; no production credential inspection. CLI/fixture checks should exercise behavior, not mirror private functions. Record command, source revision, lockfile, toolchain, OS/architecture/filesystem, shell/native Claude version, safe result and unmet limits. Inject failures at real publication boundaries; a checklist is not evidence.

| Gate | Runnable/manual recipe and pass criterion |
| --- | --- |
| A1 Parsing/output | Native fake-Claude records argv/cwd/env/TTY. Exercise every grammar conflict/help/version/name/device/casing case, run initial -- and -- --, launcher literal leading --/help/allow-auth-env. Invalid manager syntax has no mutation/secret read; JSON keys/types/exits match exactly. |
| A2 Binding/aliases | Two roots with Work, different ROOST_DIR absent/empty/other-root, custom Cargo root and foreign PATH roost. Each wrapper chooses bound registration and retains caller env; alias env equals direct native probe. Missing/relocated/replaced binding refuses and explicit reuse refresh works. |
| A3 Arguments/console | In every required shell probe empty values, Unicode/spaces, embedded quotes, trailing backslashes, &|<>^%!, newlines and path-looking Git Bash values with shell-appropriate quoting. Include Unicode installation/root paths and native Windows OEM/UTF-8 console code pages. Compare received argv boundaries and inherited TTY/streams/cwd; document initial-shell parsing. Native result/signal/cancellation preserved. |
| A4 Ownership/coexistence | Register real upstream/custom-home/linked-default; reject wrong roots/duplicate physical mapping/overlap/changed marker. Borrowed artifacts byte/permission unchanged by manager operations. Case-variant lookup preserves path/spelling. Local alias never owns default data. |
| A5 Filesystem/security | Disposable symlink/dangling-link, hardlink, junction/all representative reparse, permissive owner/mode/DACL, unsupported security/identity filesystem and namespace replacement fixtures. Open-object checks refuse redirected roots/tokens/launchers; new/replaced secrets private before bytes; borrowed unsafe token refuses without chmod, including override. |
| A6 Tokens/auth env | Bound terminal/stdin tests at 0/64KiB/>64KiB, trim/embedded whitespace/NUL, cancellation and replacement. No argv/output/journal secret. Test each finite env name with absent/empty/0/false/whitespace; override suppresses injection but not unsafe-path checks; alias env remains exact. |
| A7 Copy | Quiet-source confirm/yes/EOF/nonterminal rules; explicit/env/default source, overlaps and redirection. Seed all exclusion classes at depth, links/reparse/hardlinks/special/unreadable ordinary files. Omission report correct, no copied token/marker/global companion/no linked target access/no runnable residue after failure. |
| A8 Lock/recovery | Simultaneous same-root mutations serialize, 10-second contention/cancel works; long children do not hold lock. Terminate manager before/after every stage/rename/registry/delete/journal cleanup, including executable relocation followed by launcher replacement before registry commit; old/new binding and physical identities must classify the actual destination correctly. Pending launcher refuses; read-only doctor unchanged; next mutation only recovers proven artifacts. Foreign replacements refuse; separate roots have no false shared-lock claim. |
| A9 Retain/purge | Remove/reuse active/retained, token/native state retention, partial launcher deletion and purge failure. Marker persists through content deletion; contained links never traversed. Crash at final marker/empty-dir boundary cannot delete introduced foreign data. Purge recovery never auto-continues; explicit acknowledged retry only. |
| A10 Status/doctor | Sanitized native floor/current auth-status fixtures plus fake malformed/unknown/missing/wrong-type/contradictory/unexpected-exit cases. Ignore secret/undocumented fields. Slow/oversized/flood-both-streams/cancel probes are reaped; no raw output disclosure. Doctor metadata only, PATH warning versus unsafe/missing-required failures, no auth sweep/repairs. |
| A11 PATH | Temporary homes/ZDOTDIR/XDG/custom Fish discovery; foreign/malformed/multiple/redirection/hardlinked startup targets, repeated apply, quoting and new-shell membership. Only one owned block/snippet changes, no Fish universal/system mutations. Windows only user PATH preserves existing/expandable entries and is idempotent. |
| A12 Source lifecycle | Known-revision locked install/uninstall/reinstall/compatible upgrade in default/custom Cargo roots. Foreign bin refusal, separate two PATH concerns, stale wrapper explicit refresh, incompatible schema refusal, data retention on uninstall. Missing/old/Windows-shim Claude diagnostics and no automatic install/fallback. |
| A13 Native OAuth/storage | Consenting eligible nonproduction accounts: two claude.ai contexts, first browser/manual-code flow, cancellation/failure, observed directory, owned token clear then browser reliance; borrowed token read-only/override. Linux native private files, macOS Keychain success/denied-write native fallback, Windows native ACL/browser, WSL separate native/manual flow. Record safe observations, never extracted credentials. |
| A15 Selection | Fake git repos with linked worktrees, nested repos, `.git` files, malformed `commondir` and non-git directories. Picker without terminal, Ctrl-C/EOF, one profile, stale selection warning, alias pick, last-used highlight. Switch/forget/no-launch. Switch without NAME under a PTY: picker shown despite an existing selection, highlighting it over the last-used profile (last-used without one), keys move, Enter launches with the tail after `--`, `--no-launch` only records, Esc/Ctrl-C exit 130 keeping the selection; without a terminal usage naming `roost switch NAME`, nothing written. Picker rows carry `ls` styles on a terminal and none under `NO_COLOR`, the highlight re-applied after each styled cell. Remove/purge then same-name add inherits nothing. State write failure warns and launches. |
| A16 Sets/links | Explicit and source skill and instruction items, default sets on add/no-sets/copy from owned, subscribe-time conflict refusal, launch-time conflict skip, existing content wins, `synced` never linked, foreign replacement of a recorded link untouched, upstream/alias receive no links, purge unlinks without following. |
| A17 Plugin store | Fake Claude records store `plugin` subcommands, environment and lock holding; set prompt/flags/no terminal; remove order; CLAUDE_CODE_PLUGIN_DIRS composition with an inherited value; daily auto-update window, timeout and failure warning; alias and update environments unchanged. |
| A18 Desktop | Fake `claude-desktop`: data-folder argument and environment per kind, alias plain launch, same-profile singleton refusal, concurrent-instance warning, remove/purge/upstream lifecycle, doctor finding. `desktop` without NAME under a PTY: highlight on the last Desktop launch over a later `run`, else last used; Desktop column `running`/`signed in`/`—`/`plain`; keys move; Enter launches detached (or `--foreground`) exactly as `desktop NAME`; a running choice fails desktop_running; Esc/`q`/Ctrl-C exit 130 with state unchanged; no selection recorded; without a terminal usage naming `roost desktop NAME`, nothing written. Never launch the real app in tests. |
| A14 Native sessions/update | Same/different-profile named starts, picker/UUID/name resume, current-directory continue, fork versus shared continuation, login/native options quoted forwarding. Shared explicit Claude updater with caller restrictions/env and native result. No eventual-background-result or auth-switch assertion. |

Development starts locally on x64 glibc Linux with Bash/Fish and focused fake-Claude checks. Other OS/architecture/shell/auth evidence is deferred. Later supported first-release claims require relevant A1–A14 evidence on all eight native target environments and WSL 2, required shells and floor/current Claude versions. Container/emulation may support specifically stated Linux fixtures; compilation alone cannot pass native launch/browser/Keychain/ACL gates. Mark unavailable cases unmet rather than assuming parity. No prebuilt/release automation gate exists in this planning scope.

## Readiness and evidence

Specification readiness requires final human agreement, every accepted behavior having the approach/recipe above, all material independent-review findings resolved, and explicit remaining runtime evidence gaps. No application/runtime proof is required now or to begin local development. A failed future adapter/permission/version fixture returns the precise affected support choice to the human before claiming that case supported.

Primary-source API/version evidence: [Rust/native design facts](research/rust-native-design-facts.md), [version/platform facts](research/version-acceptance-facts.md), [Claude contracts](research/claude-contracts.md), [upstream baseline](research/upstream-behavior.md), [distribution facts](research/distribution-facts.md) and [name check](research/roost-name-check.md). Their proposals are superseded where the human decided differently. Local research observed tools only; it did not run real Claude auth/version, install/build Roost, or prove other systems.

Out of scope: rename, global active-profile switching (per-project selection is not global), automatic discovery/migration/ownership transfer, import/export, per-profile Claude binaries, selectable copy classes, completions, global manager config (the plugin auto-update flag is the only setting), PATH uninstall cleanup, Rust OAuth/session supervisor, GUI/TUI beyond the single-choice pickers, prebuilt delivery/publication/packaging/installer/release automation. Forwarded native Claude capabilities remain native behavior.

Independent review completed in two rounds with SPEC-RECOVERY-01 corrected and no actionable findings remaining; the fingerprints and disposition are recorded in decision 06. The user accepted the completed specification at Q10 on 2026-10-06. This completes the planning map; application implementation and deferred runtime/platform evidence remain subsequent work.
