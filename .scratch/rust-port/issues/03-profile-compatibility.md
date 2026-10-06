# Define safe profile coexistence and ownership

Type: grilling
Labels: wayfinder:grilling
Status: resolved
Assignee: Codex (chat 01a10c57-1955-7213-9022-d7751e4c397a)
Parent: [Find the way to a Rust Claude profile manager](../map.md)
Blocked by: 01, 02

## Question

What compatibility and ownership contract lets the Rust CLI safely coexist with upstream profiles and the original Claude installation?

Using the two research answers, settle what a profile, account, and launcher mean; supported authentication modes and inherited environment precedence; which existing files may be reused or changed; launcher collisions; copying or linking the default installation; concurrent managers; and what remove, purge, and token operations may affect. Record supported cases and explicit rejection behavior. Resolve through the user's answers, with concrete data-loss and cross-account scenarios.

## Comments

### 2026-10-05: authentication boundary surfaced during research

Claude's [multiple-account documentation](https://code.claude.com/docs/en/authentication#log-in-with-multiple-accounts) states that separate configuration directories do not isolate two Console sign-ins without API keys. The [authentication precedence](https://code.claude.com/docs/en/authentication#authentication-precedence) also allows other credential sources to override a stored profile login. Include these cases in this decision's isolation guarantee; the supported authentication scope remains for the user to decide.

The [upstream baseline](../research/upstream-behavior.md) also exposes linked launcher/run environment differences, incomplete recovery after failed copies, and token replacement permissions that retain an existing permissive mode. Use these observed cases when settling default-profile semantics, safe mutations, and token ownership.

### 2026-10-05: human decision round 1 pending

Claimed in chat `01a10c57-1955-7213-9022-d7751e4c397a`; both research blockers are resolved. No compatibility decision is settled yet.

The first independent decisions put to the user are:

1. Vocabulary: a profile is a Claude configuration/state context, an account is the external authenticated identity, and a launcher selects a profile and starts the shared Claude installation. Profiles need not correspond one-to-one with accounts. Recommendation: adopt these definitions.
2. Ownership boundary: separate Rust-owned manager metadata/new profiles/launchers from upstream-owned artifacts. Recommendation: explicitly register existing upstream profiles at their unchanged paths; allow launch and inspection, but refuse Rust-manager token writes, purge, or replacement/deletion of upstream launchers. Claude itself still writes state when launched; this is not a filesystem read-only guarantee. No automatic ownership transfer.
3. Authentication scope: first-release separation supports directory-scoped claude.ai logins, explicit API keys, and upstream-style subscription OAuth tokens, subject to Claude's documented credential precedence. Recommendation: exclude keyless Console sign-ins from the supported isolation promise and never present them as isolated. Environment conflict policy and precise enforcement follow after this boundary is answered.

Await the user's answers before descending into launcher collisions, adoption mutation, default alias/copy semantics, authentication conflicts, and removal/concurrency behavior. Resolution remains pending.

### 2026-10-05: human decision round 1 accepted

The user answered "Accept all" to the three recommendations. Settled:

- Use profile, account, and launcher as defined in the [glossary](../../../GLOSSARY.md); there is no one-to-one profile/account assumption.
- Keep Rust-owned metadata/new profiles/launchers separate from upstream artifacts. Explicitly register existing profiles at their unchanged paths. Registration allows launch and inspection but does not authorize Rust token writes, data purge, or replacement/deletion of upstream launchers. Claude still writes its own state during execution. No automatic ownership transfer.
- Support directory-scoped claude.ai logins, explicit API keys, and subscription OAuth tokens. Exclude keyless Console sign-ins from the isolation promise. Credential precedence remains a constraint; conflict policy is still open.

### 2026-10-05: human decision round 2 pending

The next independent choices are inherited authentication/provider conflicts, default alias semantics, default-copy credential handling, and launcher collision behavior. Recommendations put to the user:

- Isolated profiles reject inherited authentication/provider overrides by default; an explicit launch override permits native Claude precedence and forfeits the profile-account expectation. Do not silently clear credentials or rewrite settings.
- Default aliases behave exactly like ordinary Claude with the caller's environment, consistently through launcher and direct run. Clearly label them as pass-through and outside profile isolation.
- Copy default state into a new owned profile, exclude known Claude credential files and manager token files, never extract Keychain secrets, and require authentication verification/login. Preserve other selected configuration without a guarantee that arbitrary copied settings contain no secrets or auth helpers. Copy is data seeding, not account migration.
- Use a separate launcher directory and distinct command prefix; fail on any existing destination unless it is verifiably this manager's launcher for the same profile. Exact names belong to the CLI decision.

Await answers before treating these recommendations as settled. Removal, token protection, filesystem rejection/recovery, and concurrency remain open.

### 2026-10-05: human decision round 2 accepted

The user answered "Accept all" to Q4-Q7. Settled:

- Isolated launches refuse inherited authentication/provider overrides by default, naming conflicting variables without printing values. An explicit launch override accepts Claude's native credential precedence. Do not silently clear credentials or rewrite settings. Profile/project settings can affect authentication, so environment checking is not an account-identity guarantee.
- Default aliases preserve the caller's environment exactly like ordinary Claude, with identical launcher/direct-run behavior. Label them as pass-through, outside profile isolation. The [glossary](../../../GLOSSARY.md) now defines default alias.
- Default copying seeds a new owned profile with selected configuration/data, excludes known Claude credential files and manager tokens, never extracts Keychain secrets, and requires authentication verification/login afterward. Copied settings can still contain secrets/helpers; copying does not establish a different account.
- Launchers use a separate directory and distinct command prefix. Refuse collisions unless the existing launcher is verifiably Rust-owned for the same profile. Exact names remain for Choose command naming and cross-platform distribution; no executable name is selected here.

### 2026-10-05: human decision round 3 pending

Recommendations put to the user for Q8-Q11:

- Ordinary removal deletes only Rust-owned launchers and registration metadata, retaining profile data; retained owned data keeps its ownership evidence so explicit reuse is possible. Explicit purge can delete a verified Rust-owned profile only. Registered upstream profiles and default aliases never authorize deletion of upstream/default data. Purge is not native logout or account-token revocation; Claude owns native login/logout.
- Manager tokens may be written/cleared only for Rust-owned isolated profiles. Use hidden terminal input or explicit stdin, never command-line token values or secret output. Secure both new and replacement files with private Unix permissions/current-user Windows protection; refuse if that protection cannot be established. Clearing a token removes only the manager token and may expose native login credentials at the next launch.
- Serialize Rust-manager mutations. Permit concurrent launches subject to Claude's native behavior, without promising safety against external managers/direct Claude mutations. Require users to stop sessions and external writers before purge or default copying; do not add cross-manager coordination or pretend external sessions are detectable.
- Reject redirected/symlink/junction profile roots and secret/launcher destinations inside managed storage; register upstream real directories only. Copy must not follow linked targets and must stage before publication, leaving no usable half-created profile on failure. Purge must never traverse contained links into other data. Missing, redirected, or unverifiably owned destinations fail closed.

Await answers. This ticket remains claimed; no decision ticket has been resolved in this session.

### 2026-10-05: human decision round 3 accepted

The user answered "Accept all" to Q8-Q11. Settled:

- Ordinary removal deletes only Rust-owned launchers and registration metadata, retaining profile data and ownership evidence. Explicit purge may delete verified Rust-owned profile data only, never upstream/default data. Purge is not native logout or credential revocation; Claude owns native login/logout.
- Token writes/clears are limited to owned isolated profiles. Hidden terminal input or explicit stdin only; no command-line token values or secret output. Protect both new and replacement files with private Unix permissions/current-user Windows protection; refuse writes if protection fails. Clearing removes only the manager token and may reveal native login credentials at the next launch.
- Serialize Rust mutations; allow concurrent launches subject to Claude's behavior. Users must stop sessions/external writers before purge or default copying. Cross-manager concurrent mutation safety is not promised; no shared upstream locking protocol or reliable detection of external sessions is assumed.
- Register real upstream directories only. Reject symlink/junction redirection of managed roots, tokens, and launcher destinations; copy does not follow linked targets and is staged before publication. Failed copy leaves no usable half-created profile. Purge never follows contained links into other data; uncertain ownership blocks deletion.

### 2026-10-05: human decision round 4 pending

Two remaining ownership choices are put to the user:

- Registered upstream profiles can use an existing upstream manager-token file read-only for launch, if its path and permissions satisfy the token safety contract. Do not change permissions on borrowed artifacts; unsafe token files block launch with remediation guidance. Do not inspect/extract Claude-native credential files or Keychain secrets; authentication diagnostics use Claude's supported interface. Recognize upstream linked-default markers as default aliases, without rewriting their files.
- Invoke the existing shared Claude installation, leaving its executable, original launcher, global configuration, and installation ownership alone. Preserve an explicitly requested update capability by delegating to Claude's supported updater; never install, replace, or globally disable updates automatically. Process-local update settings and exact command behavior remain for Choose the Rust command contract and targeted improvements.

Await answers, then present the consolidated contract for the human's final shared-understanding confirmation before resolution.

### 2026-10-05: human decision round 4 accepted

The user answered "Accept all" to Q12-Q13. Settled:

- Read existing upstream manager-token files only to launch registered profiles, and only if paths/permissions meet the token safety contract. Unsafe tokens block launch with guidance; do not repair borrowed permissions. Never inspect/extract Claude-native credentials or Keychain secrets. Use Claude's supported interface for authentication diagnostics. Recognize linked-default upstream markers as pass-through aliases without rewriting upstream files.
- Use the existing shared Claude installation. Preserve its executable/original launcher/global configuration; do not automatically install, replace, or globally change update policy. Explicitly requested updates may delegate to Claude's supported updater. Process-local controls and precise update behavior belong to the command-contract ticket.

### 2026-10-05: final shared-understanding confirmation pending

All Q1-Q13 recommendations are accepted. The consolidated contract is being presented for confirmation before resolving this ticket. No application implementation is authorized by this planning decision.

Remaining detail is already covered by later tickets: command syntax and observable behavior (including configuration-copy selection, environment-conflict variable coverage, authentication/status presentation, update controls, recovery output, and removal/reuse commands); naming/distribution; Rust ownership representation, filesystem/process handling, and platform acceptance evidence. This compatibility ticket fixes the ownership and safety requirements those details must implement.

### 2026-10-05: final shared understanding confirmed

After accepting all recommendations in four rounds, the user confirmed the consolidated contract: "Yes this looks good." Resolve this decision only; the remaining decision tickets stay open. Application implementation remains outside this planning effort.

## Answer

### 2026-10-05: approved coexistence and ownership contract

The human accepted Q1-Q13 and confirmed the consolidated contract at Q14. This answer is the canonical decision; the comments above retain the conversation history. It uses the [upstream baseline](../research/upstream-behavior.md) at v0.1.2 and the [Claude contract research](../research/claude-contracts.md), subject to their recorded verification limits.

#### Vocabulary and ownership

Use the [glossary](../../../GLOSSARY.md): a profile is a Claude configuration/state context, an account is an external authenticated identity, and a launcher selects a profile and starts Claude. Profiles and accounts need not correspond one-to-one. A default alias passes through to ordinary Claude in the caller's environment and does not provide an isolated context.

Rust manager metadata, newly created profiles, and generated launchers use storage separate from upstream. Existing upstream profiles require explicit registration at their unchanged paths; registration is not ownership transfer. Do not automatically migrate, relocate, or take ownership of upstream profiles.

| Artifact | Rust may do | Rust must refuse |
| --- | --- | --- |
| Rust-owned isolated profile | Launch, inspect via Claude, manage its manager token, remove its registration/owned launchers, explicitly purge verified owned data | Native credential extraction; deletion when ownership/path safety cannot be established |
| Registered upstream profile | Launch at unchanged path, inspect via Claude, read a safe existing upstream manager token for launch, remove its Rust registration/owned launchers | Token writes/clears, permission repair, data purge, replacement/deletion of upstream launchers |
| Default alias, including registered upstream linked-default entries | Pass-through launch; manage only Rust registration/owned launchers | Tokens or deletion/modification of default Claude data or upstream artifacts |
| Existing shared Claude installation | Invoke; delegate an explicitly requested update to Claude's supported updater | Automatic installation/replacement, rewriting the original launcher/global configuration, globally changing update policy |

Claude itself writes its state when launched. Registration limits Rust-manager mutations; it does not make the directory read-only. Native login/logout and credential storage remain Claude-owned; diagnostics use Claude's supported interface rather than reading native credential contents or extracting Keychain secrets.

#### Launching and authentication

Support directory-scoped claude.ai logins, explicit API keys, and subscription OAuth tokens within Claude's documented capabilities. Keyless Console sign-ins are outside the isolation promise and must not be presented as isolated. Profile selection does not provide filesystem/project isolation or prove the effective account.

Isolated profiles select their own configuration directory. Inherited authentication/provider overrides cause refusal by default; diagnostics identify conflicting variable names without exposing values. An explicit launch override accepts Claude's native authentication precedence. Do not silently clear credentials or rewrite configuration. Profile/project settings and helpers can also affect authentication, so the environment check is not an account-identity guarantee.

Default aliases preserve the caller's environment exactly like ordinary Claude, consistently through generated launchers and direct run. Label them as pass-through and outside isolation. Recognize upstream linked-default markers without rewriting upstream files.

Launchers use a separate directory and distinct command prefix. An existing destination is a collision and must not be overwritten unless it is verifiably Rust-owned for the same profile. Exact names remain for the naming/distribution decision.

#### Copying, removal, and tokens

Default copying seeds a new owned profile with selected configuration/data. Exclude known Claude credential files and manager tokens; never extract Keychain secrets. Require authentication verification/login afterward. Copied settings may still contain secrets or authentication helpers; copying is data seeding, not account migration, and does not establish a different account.

Ordinary removal deletes only Rust-owned launchers and registration metadata, retaining profile data and ownership evidence for explicit reuse. Explicit purge can delete verified Rust-owned profile data only. It cannot delete upstream/default data, revoke tokens, or substitute for native logout. Clearing a manager token removes only that token; native login credentials may become effective on the next launch.

Manager token writes/clears are limited to Rust-owned isolated profiles. Accept secrets through hidden terminal input or explicit stdin, never command-line token values or secret output. Establish private Unix permissions/current-user Windows protection for both new and replacement token files; refuse writes if protection cannot be established. Registered upstream manager tokens may be read only for launch when paths and permissions meet the same safety contract. Unsafe borrowed tokens block launch with repair guidance; Rust does not change borrowed permissions.

#### Filesystem, failure, and concurrency boundaries

Register real upstream directories only. Reject redirected/symlink/junction managed roots, token paths, and launcher destinations. Copy must not follow linked targets and must stage creation before publication so failure leaves no usable half-created profile. Purge must never follow contained links into other data. Missing or unsafe referenced paths and unverifiable ownership fail closed; they do not authorize recreating or deleting foreign artifacts.

Serialize Rust-manager mutations. Concurrent launches are permitted subject to Claude's native behavior. Users must stop sessions and external writers before purge or default copying. Rust does not promise safe simultaneous mutations by upstream/other managers/direct Claude, nor reliable detection of those external sessions; upstream has no shared locking protocol.

#### Required acceptance scenarios

- Register a real upstream profile: its path and upstream launcher stay unchanged; Rust launch is allowed, while Rust token mutation and purge are rejected. Claude-written state is not mistaken for a manager ownership violation.
- Launch an isolated work profile with inherited personal credentials: reject by default without printing secret values; explicit override follows native precedence and does not claim the stored work account is effective.
- Register an upstream token profile: a safe token is usable read-only; unsafe permissions or redirection block launch without repairing the borrowed file.
- Launch a default alias with a caller-supplied configuration directory: launcher and direct run preserve the same environment and display pass-through semantics.
- Copy default state: excluded credential/token files are absent, no Keychain secret is extracted, and no copied-account claim is made. Failed copy leaves no usable partial profile.
- Encounter a foreign launcher: refuse replacement. Encounter uncertain profile ownership or redirected paths: refuse the unsafe operation. Purge cannot traverse a contained link to default/upstream/other data.
- Remove an owned profile: retain data and ownership evidence. Remove an upstream registration/default alias: preserve upstream/default data. Purge does not imply logout or revocation.
- Replace a token: protection applies to existing as well as new files, with no echoed/logged secret. Clearing can expose native credentials and does not erase them.
- Concurrent Rust mutations serialize; documentation requires quiet source/destination state for copy/purge and does not claim coordination with external writers.
- Shared installation and original launcher/global settings remain unchanged except when an explicit update delegates to Claude's supported updater.

These are future implementation/release checks, not tests performed in this planning session. Real authentication and Windows/macOS runtime evidence remain subject to the research limits and later acceptance decision.

#### Handoff to existing decisions

- [Choose the Rust command contract and targeted improvements](04-command-contract.md): registration/reuse/removal commands, exact environment-conflict coverage and override syntax, copy selection and link handling, authentication/status presentation, secret input/noninteractive behavior, error/recovery output, and process-local/manual update behavior.
- [Choose command naming and cross-platform distribution](05-distribution.md): executable/package identity, distinct launcher prefix, concrete storage/install locations, and manager-switching instructions under this ownership boundary.
- [Set the Rust design and specification acceptance criteria](06-specification-readiness.md): ownership evidence and retained-data representation, locking, atomic publication/recovery, filesystem/permission implementation, supported Claude versions, and Linux/macOS/Windows evidence for the scenarios above.

No additional ticket is needed: the remaining precise details fit these existing decisions. This session resolves only this ownership ticket.
