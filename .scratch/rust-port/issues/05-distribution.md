# Choose command naming and cross-platform distribution

Type: grilling
Labels: wayfinder:grilling
Status: resolved
Assignee: Codex (chat 01a11119-87ad-7390-a614-c412170f0d1a)
Parent: [Find the way to a Rust Claude profile manager](../map.md)
Blocked by: 03, 04

## Question

What executable identity, install method, and platform support policy make this Rust CLI usable on Linux, macOS, and Windows while coexisting with the Node version?

Settle the command and package names, required architectures and shell environments, install and upgrade expectations, Claude prerequisite handling, source-reuse attribution, and how users switch between managers. Keep release automation implementation outside this decision; identify any external facts that need their own research ticket.

## Comments

### 2026-10-05: ownership constraints for naming and locations

[Define safe profile coexistence and ownership](03-profile-compatibility.md#answer) requires separate Rust manager storage/launchers and a distinct launcher command prefix, unchanged paths for registered upstream profiles, and preservation of the shared Claude installation. Select concrete names/locations and manager-switching instructions here; none were selected by the ownership decision. This ticket remains blocked by Choose the Rust command contract and targeted improvements.

### 2026-10-06: command contract resolved; naming/distribution is the next frontier

[Choose the Rust command contract and targeted improvements](04-command-contract.md#answer) is resolved. This ticket is now unblocked but remains open and unassigned; it is not being resolved in that chat.

Choose concrete executable/package identity, distinct launcher prefix/location, Rust storage override spelling/default locations, and setup-path identity under the approved flat command contract. Storage overrides do not reinterpret the user home or default copy source. Names are portable/case-insensitively unique and manager lookup is case-insensitive, while printed/generated spelling and registered upstream paths remain unchanged; launcher invocation follows native OS casing. First release includes native OAuth login plus named concurrent starts/resume/continue/fork through run/launchers. Preserve the existing Claude installation, document prerequisite/browser/native-credential expectations and switching between managers, and keep custom OAuth/session infrastructure outside scope. Exact Claude-version floor and platform evidence belong to the final design/acceptance ticket.

### 2026-10-06: claimed for naming and distribution decisions

Rechecked the local tracker: both ownership and command blockers are resolved. Claimed in chat `01a11119-87ad-7390-a614-c412170f0d1a`. The canonical Answers of both decisions govern this conversation. The final design/specification ticket remains blocked by this decision. Preserve existing uncommitted work; no application implementation, commit, or publication. Human decision rounds and final shared-understanding confirmation are still pending.

### 2026-10-06: human decision round 1 pending

Q1 asks which first-release delivery channels are required. Recommendation: prebuilt archives for every supported OS/architecture plus Cargo installation from source; no Rust/Node runtime for prebuilt manager users; publish version information, checksums, attribution, and manual installation instructions. Defer Homebrew, WinGet, distro packages, and custom installer scripts. Release automation implementation stays outside this decision. Await the human answer.

A bounded primary-source investigation is checking candidate name availability, Claude platform/architecture/shell prerequisites, browser/native-credential expectations, and pinned upstream attribution. Findings will be saved at `../research/distribution-facts.md`. Dependent naming/platform choices await those facts; no name or support matrix has been accepted.

### 2026-10-06: human decision Q1 accepted

The user answered "Accept the recommendation." First release requires prebuilt archives for every supported OS/architecture and Cargo installation from source. Prebuilt manager users need no Rust/Node runtime. Include version information, checksums, attribution, and manual installation instructions; Homebrew, WinGet, distro packages, and custom installer scripts are deferred. This does not select targets/names yet or authorize publication. Release automation implementation remains outside this decision.

### 2026-10-06: human decision round 2 pending

- Q2 recommendation: x86_64 and ARM64 on Linux, macOS, and native Windows; glibc and musl Linux archives, eight builds total. WSL uses separate Linux installation/storage. Actual build/runtime evidence belongs to the final acceptance ticket.
- Q3 recommendation: Bash/Zsh on Linux/macOS, POSIX sh launcher execution; Windows PowerShell 5.1, PowerShell 7+, CMD, and Git Bash, keeping Windows sh/CMD/PowerShell launcher forms for upstream parity. Require WSL 2 as a separate Linux environment. Approved PATH apply scope remains Bash/Zsh/POSIX startup files or Windows user PATH.

Await the human answers before treating these support targets as settled. The [distribution fact report](../research/distribution-facts.md) verifies current first-party Claude platform and browser/credential prerequisites and pinned upstream attribution. Candidate `ccm-rs` registry availability could not be verified (HTTP 403/tool errors, not an absence response); no obvious public CLI collision appeared in searches. Neither observation reserves or guarantees the name.

Q4 (independent of Q2/Q3) recommends executable/Cargo package `ccm-rs`, display name "CCM Rust", and launcher family `ccm-rs-NAME`, preserving stored name spelling (example `ccm-rs-Work`). This distinguishes upstream `ccm` and `claude-NAME`. Approval would select an intended identity, not verified registry availability; recheck before publication and return a conflicting package-name choice to the human instead of silently renaming. Await the answer.

### 2026-10-06: distribution correction, Fish requirement, and naming reset

The user clarified: "Lets not generate the bins for the user. We can set that up after we are done testing it for a while." In the preceding archive-distribution context, this supersedes Q1's prebuilt-archive requirement: begin with source builds/Cargo installation during the testing period and defer prebuilt binary distribution until a later explicit release decision. This does not remove the already approved generated profile launchers. The eventual platform support goal remains Linux/macOS/Windows; Q2's architecture scope has not been accepted, and the eight-archive proposal is no longer the immediate delivery plan.

The user explicitly requires Fish because it is their shell. Add Fish as a required caller shell alongside the remaining proposed shells; do not treat Q3's other shell/version selections as accepted. Fish requires a usable PATH setup recipe and meaningful launcher/argument acceptance evidence. A narrowly scoped Fish `setup-path --apply` extension is proposed next; its exact mechanism belongs to the design ticket after the human choice. This records the user-requested extension to the earlier shell scope without rewriting the canonical command Answer silently.

The user rejected `ccm-rs` and requested other name ideas. Q4 is superseded; executable/package/display/prefix identity remains unsettled. Next round will brainstorm candidate naming families without claiming package or command availability. Screen the human's shortlist against primary sources before settling names; previous `ccm-rs` availability failures do not establish anything about new candidates.

### 2026-10-06: revised naming and Fish round pending

Q5 offers unscreened naming candidates: Clade (`clade`), Claudeck (`claudeck`), Roost (`roost`), Cove (`cove`), Folio (`folio`), and Clasp (`clasp`). Recommendation: shortlist Clade for a short general name or Claudeck for an explicit Claude association. Example proposed naming family: `clade run work` and `clade-work`. None has verified package/PATH/public-project availability; research the selected shortlist before making the canonical identity decision. The user may suggest another direction.

Q6 recommends Fish support including `setup-path --shell fish` instructions and explicit `--apply` for one clearly owned PATH snippet in the user's Fish `conf.d` directory, respecting their configured Fish config location and the existing collision/redirection/idempotence guards. Do not edit unrelated Fish configuration or universal variables. Exact file basename follows the selected manager name; implementation details/version floor belong to the design ticket. This extends the prior PATH shell scope only if accepted. Await the human answers.

### 2026-10-06: Roost preference and remaining recommendations accepted

The user answered "I like roost, and accept the rest." Record Roost as the preferred display name and naming direction, subject to the promised collision screening before concrete executable/package identity is settled. Fish PATH instructions and explicit scoped apply are accepted: one manager-owned user Fish `conf.d` snippet, configured directory respected, existing path/collision/idempotence guards, no unrelated config/universal-variable edits. This is an explicit extension of the earlier command decision's shell list.

The remaining support recommendations are accepted under the corrected source-only delivery policy: x86_64/ARM64 for Linux/macOS/native Windows, glibc/musl Linux targets, Bash/Zsh/POSIX launcher execution, Windows PowerShell 5.1/7+, CMD/Git Bash, and WSL 2 as a separate Linux installation/storage environment. Fish is additionally required on Linux/macOS and in the Linux WSL environment. These are support/acceptance targets, not promised prebuilt artifacts or performed tests. The final consolidated confirmation will make this interpretation of "the rest" explicit. Concrete storage paths/override, installation and upgrade instructions, attribution/switching, and final confirmation remain pending.

### 2026-10-06: storage, source delivery, attribution, and naming round pending

- Q7 recommendation: home `.roost` on Linux/macOS and `%USERPROFILE%\.roost` on Windows; owned profiles in `profiles/`, launchers in `bin/`. `ROOST_DIR` denotes the complete manager root, not a substituted home; relative values resolve from cwd, an explicitly empty value fails. Registered upstream paths and default-copy source are unaffected.
- Q8 recommendation: testing-period Cargo installation from source checkout at a known revision; explicitly install the next tested revision to upgrade, preserving compatible profile/registration/launcher state. Refuse incompatible storage with guidance, no automatic migration or manager self-update. Native `update` retains shared-Claude meaning. Defer crates.io publication, prebuilt release artifacts, and package-manager packaging until a later release decision.
- Q9 recommendation: MIT for the Rust project, complete upstream 2026 Carlos Aponte notice for reused source/templates/docs, plus upstream author/repository/inspected-revision attribution; no Anthropic affiliation claim.
- Q10 recommendation after [Roost naming research](../research/roost-name-check.md): display Roost, executable `roost`, Cargo source package `roost-claude`, launchers `roost-NAME`. Existing public `roost` commands are a disclosed ambiguity; installation must not replace a foreign executable. Alternative: executable/package `roost-claude` and prefix `roost-profile-NAME`, avoiding the specific binary/profile-name overlap of `roost-claude` with `roost-NAME` for profile `claude`. Registry availability remains unverified and needs recheck before publication.

All four choices await the human answers. The naming research found real command collisions; it did not find a registry availability response or prohibit retaining the brand. No source installation, profile/launcher creation, publication, or application implementation occurred.

### 2026-10-06: concrete Roost identity accepted

The user answered Q10: "Keep short roost command (Recommended)." Settled: display name Roost, executable `roost` (`roost.exe` on Windows), Cargo source-package name `roost-claude`, generated launchers `roost-NAME` preserving profile spelling. Existing public `roost` command collisions are explicitly disclosed and accepted; installation must refuse to overwrite a foreign executable. Package registry availability remains unverified, with publication deferred and a registry recheck required before any future publication. If occupied then, return the package-name choice to the human without silently renaming the brand/command or claiming a reservation.

Q7 storage/layout/override, Q8 source install/upgrade compatibility policy, and Q9 license/attribution are still pending. The specific Q10 reply does not answer those questions. This decision remains claimed; final shared-understanding confirmation and resolution have not occurred.

### 2026-10-06: storage, upgrades, and attribution accepted

The user answered "Accpe al." (accept all) to Q7-Q9. Settled: home `.roost` manager layout with `ROOST_DIR` as the entire-root override, source-checkout Cargo installation and explicit tested-revision upgrades preserving compatible state/refusing incompatible storage, and MIT with the full upstream notice and attribution. Previously accepted Roost identity, source-only testing distribution, platform/shell targets, Fish integration, and ownership/command contracts remain authoritative.

The consolidated proposal below is pending independent review and the final shared-understanding confirmation. It is not yet the canonical Answer or a resolution. No application implementation, installation, commit, or publication is authorized by this planning conversation.


### 2026-10-06: independent review clean; final shared understanding pending

The Forge review used a fresh native inspection-only reviewer, inheriting this session's model/effort for Mid/Top/Reviewer. It verified the proposed ticket SHA256 `c72fb1f97ed0e2702c766d5c4cadbfb1fb376aca3d73b6343ab8e94a6030ee34` and glossary SHA256 `ee97bacd20bd8bde1424eb993dc5f0c5993f51710daaed3e57b737832419821e`. Result: no actionable findings and no essential silently assumed human policy. Public executable collisions, Cargo local-path reinstalls/custom roots, stale launcher binding, borrowed-token OAuth, and WSL separation are covered by explicit guards/design handoffs. Ordinary instructed inspection boundary; no strict runtime enforcement was requested or claimed.

Whitespace/local-file-link checks passed on the distribution ticket, glossary, and both distribution/name research reports; tracked `git diff --check` passed. Distribution remains claimed and design remains open/blocked; no final `spec.md`, application implementation, platform/authentication tests, installation, commit, or publication occurred.

Q11 requests final confirmation of the consolidated proposal before recording it as the canonical Answer, resolving this one ticket, updating the map, and handing off the existing design/specification ticket. Await the human answer; neither the review nor prior per-choice approvals substitutes for this requested final confirmation.


### 2026-10-06: final shared understanding confirmed

The user answered "accept" to Q11's consolidated contract after the independent review returned no actionable findings. This confirms the final Roost identity, source-only testing distribution, storage/override, required architecture/shell matrix including Fish, existing Claude/native OAuth prerequisites, upgrade protection, attribution, and switching rules. Resolve this one decision; the existing design/specification ticket becomes the next open, unclaimed frontier. The Answer below is canonical; earlier comments retain superseded proposals and per-round history.

## Answer

### 2026-10-06: approved Roost naming and distribution contract

The human confirmed the consolidated contract at Q11. This is the canonical decision, constrained by the accepted ownership and command Answers, with the explicitly requested Fish PATH extension recorded here.

### Identity and locations

| Item | Selected spelling/location |
| --- | --- |
| Display name | Roost |
| Executable | `roost`; `roost.exe` on native Windows |
| Cargo source package | `roost-claude` |
| Profile launcher command | `roost-NAME`, preserving stored profile spelling; e.g. `roost-Work` |
| Linux/macOS default manager root | Actual user home plus `.roost` |
| Native Windows default manager root | Actual user home plus `.roost`, conventionally `%USERPROFILE%\.roost` |
| Owned profile directories | `<root>/profiles/<stored-NAME>` |
| Generated launcher directory | `<root>/bin` |
| Storage override | `ROOST_DIR`, the entire manager root |
| Fish PATH snippet | `roost-path.fish` in the configured user's Fish `conf.d` directory |
| Other Unix PATH block identity | Clearly delimited `roost` manager block, with start/end markers settled in the design ticket |

Unset `ROOST_DIR` uses the home default. For commands that require storage, an explicitly empty value fails; a relative value resolves from cwd. The override does not redefine user home, Cargo installation root, native Claude ownership, the conventional Claude directory, or default-copy source selection. Registered upstream profile paths are retained exactly as registered. Exact root/profile metadata, ownership/token artifact names, and safe path resolution are design/acceptance work, not selected file formats here. Foreign or redirected storage/launcher artifacts remain protected by the approved ownership rules; `.roost` is not a claim on any existing other Roost tool's data.

Linux/macOS generate an executable POSIX sh launcher; native Windows retains extensionless sh, `.cmd`, and `.ps1` forms for Git Bash/CMD/PowerShell. Their command stem is `roost-NAME`; use the printed spelling. Manager lookup is case-insensitive, while paths/generated spelling and native launcher filename casing follow the approved command contract. Every launcher must use the same safety/auth/forwarding behavior as direct run, including native session/OAuth forwarding and default-alias pass-through. The design ticket must settle launcher executable/storage binding and custom install-root behavior without allowing foreign executable selection, wrong-profile selection, or changing the approved caller environment/argument semantics.

Existing public tools also use `roost`, including Claude agent tooling. The human explicitly accepted this ambiguity after the [naming check](../research/roost-name-check.md). Source install instructions must identify and refuse a foreign executable at the destination and explain checking PATH resolution; do not tell users to force-overwrite it. The chosen package name is an intended local-source identity, not a crates.io availability/reservation claim. Registry access failed; publication is deferred. Recheck before any future publication, returning an occupied package-name decision to the human without silently renaming the brand/command.

### Source installation, upgrades, and eventual distribution

During the testing period, distribute source instructions and install from a checkout at a known revision using Cargo's local-path installation. A representative recipe is `cargo install --path <crate-directory> --locked --bin roost`; the design/specification supplies the actual crate directory and required toolchain/target-linker setup. Include the dependency lockfile with source instructions. Do not supply prebuilt archives, publish a registry package, add Homebrew/WinGet/distro packaging, or implement installer/release automation as part of this decision. Revisit shipping binaries only after testing and a later explicit release decision; this does not remove generated profile launchers.

Let Cargo own the executable installation root and its `bin` directory: ordinarily the user's `.cargo/bin`, while preserving Cargo's native custom-root/configuration precedence. Manager storage is independently `.roost` or `ROOST_DIR`. Document adding the selected Cargo bin directory to PATH separately from `roost setup-path`, whose scope remains the generated-launcher directory. Use normal Cargo collision protection; no automatic `--force` over foreign binaries. A same-package local-path reinstall is the upgrade mechanism. [Cargo install](https://doc.rust-lang.org/cargo/commands/cargo-install.html).

Upgrades explicitly install the next tested revision, keep the stable command/storage layout, and preserve compatible profile/registration/launcher state. Refuse incompatible storage with clear next steps; no automatic migration or self-updater is selected. Document ending Roost operations before replacing its executable, particularly on Windows, and verify installation with the selected executable's version and `doctor`. Missing/stale owned launchers use approved `reuse` or identical registration refresh; relocation/custom-root executable binding is a required design/acceptance case. `roost update` still delegates exactly to the existing shared `claude update`; it is not a Roost upgrade command. Package uninstall removes the Cargo-managed executable; it does not authorize profile purge, native logout, token revocation, or automatic PATH cleanup. Exact uninstall/reinstall instructions are validated in the design ticket.

### Required platforms and caller shells

| Platform | Required architectures/environments | Required caller shells |
| --- | --- | --- |
| Linux | x86_64 and ARM64; glibc and musl | Bash, Zsh, Fish; POSIX sh launcher execution |
| macOS | x86_64 and ARM64 | Bash, Zsh, Fish; POSIX sh launcher execution |
| Native Windows | x86_64 and ARM64 | Windows PowerShell 5.1, PowerShell 7+, CMD, Git Bash |
| WSL 2 | Linux build/Claude/storage/PATH inside WSL, separate from native Windows | Linux caller-shell requirements |

These are support/acceptance targets for the first release, not an eight-archive delivery promise or completed tests. WSL 1, Fish on native Windows, and additional shell-specific integrations are not required by this matrix. Exact Rust triples, toolchain/OS compatibility floors, meaningful coverage per target/environment, and runtime evidence belong to the design/acceptance decision. Cross-compilation alone does not establish native execution/credential/browser/launcher support.

### Fish and PATH setup

Add `fish` to supported Unix `setup-path --shell` recipes and explicit apply. Default remains instructions; `--apply` manages one user-owned `roost-path.fish` snippet in the configured Fish `conf.d` directory (normally `~/.config/fish/conf.d`, respecting `XDG_CONFIG_HOME` and supported Fish configuration discovery). Add only the resolved generated-launcher directory. Preserve unrelated configuration; do not change universal variables or system paths. Retain existing path/ownership/collision/redirection checks, idempotence, destination reporting, and restart guidance. A foreign or malformed snippet cannot be overwritten. Fish can run external executables/POSIX shebang launchers; a separate Fish profile-function generator is not required. [Fish configuration](https://fishshell.com/docs/current/language.html#configuration-files), [fish_add_path](https://fishshell.com/docs/current/cmds/fish_add_path.html).

The rest of the approved PATH contract remains: bash/zsh/posix apply edits one selected startup file/manager block; native Windows apply changes user PATH only, with PowerShell/CMD recipes. This Fish extension was explicitly requested and accepted here; the prior command Answer remains otherwise unchanged and should link this decision after resolution.

### Existing Claude, browser, and credential prerequisites

Use the existing shared Claude executable through PATH in the same native platform environment. Users install/configure Claude themselves using its supported method; Roost must not automatically install, replace, relocate, scrape, or take ownership of it. Source-installing Roost needs Rust/Cargo plus the target's build/linker prerequisites; Roost itself needs no Node runtime. A user's chosen Claude installation can have its own prerequisites. Missing/unsupported Claude uses approved diagnostics and guidance, with the exact Claude version floor settled in the design ticket.

The [current platform research](../research/distribution-facts.md) records Claude's then-current prerequisites: macOS 13+, Windows 10 1809+/Server 2019+, Ubuntu 20.04+/Debian 10+/Alpine 3.19+, supported processor/account/region, network, and native environment dependencies. These are documented Claude requirements, not measured Roost compatibility or an OS/version-floor substitute. Git for Windows is optional for Claude's native PowerShell tool but required for this matrix's Git Bash invocation tests. Alpine's Claude runtime dependencies and configuration must be documented; Roost does not install or silently set them. Native Windows and WSL use their corresponding Claude installation and independent manager context.

Include native browser-login guidance: `roost add work`, `roost run work auth login`, `roost status work`. A usable browser/account/network and writable native credential facilities are prerequisites. If automatic opening/callback fails, use Claude's supplied URL/manual-code flow, including from WSL/headless environments; no unattended-login guarantee or custom Rust OAuth/callback/refresh service is selected. Native Claude owns macOS Keychain/file fallback, Linux credential-file protection, and Windows profile ACL behavior; Roost owns only its separate manager token under the approved safety rules. Keyless Console separation remains outside the promise. [Authentication prerequisites](https://code.claude.com/docs/en/authentication).

Keep explicit token transitions visible: owned token-to-browser login uses `roost token work --clear` before native login/status; borrowed tokens stay read-only, with the approved deliberate direct-run/status `--allow-auth-env` path suppressing manager token injection. It never bypasses unsafe borrowed-token refusal. Native authorization and a successful login do not prove the effective account/service access under all configuration sources.

### Attribution and switching managers

Use MIT for the Rust project. **Amended during the design continuation on 2026-10-06 at the user's request:** no extra upstream credits or inspected-revision attribution is required in the planned README/help. If upstream source/templates/documentation are reused, preserve their MIT copyright/permission notice (2026 Carlos Aponte) as the license requires. Internal research citations and the pinned upstream behavior baseline remain evidence; they are not required product credits. Make no official Anthropic or unrelated Roost-project affiliation claim. [Pinned upstream notice](https://raw.githubusercontent.com/CarlosTheory/claude-multi-account/12909e6fc76cfc3b36861c2b33f998585f2690f8/LICENSE).

Switch by choosing the command: upstream uses `ccm` and its existing `claude-NAME` launchers; Roost uses `roost` and `roost-NAME`. There is no global active-manager/profile switch. To launch a borrowed upstream profile, explicitly register its existing real path with `roost register work --path <unchanged-upstream-path>` and use `roost run work`/its generated launcher. Returning to upstream uses its original command/launcher; registration never migrates ownership or moves its profile. Claude itself still writes native state during launch/login. Roost-owned profiles are not automatically registered with, exported to, or mutation-managed by upstream. Users stop sessions/external writers before copy/purge and coordinate manager mutations as required by the ownership contract.

### Evidence and handoff

Research checked first-party documentation/pinned upstream attribution and discovered public name collisions; it did not authenticate, inspect credentials, install/update either program, build the Rust application, reserve/publish packages, or run real platform/shell tests. Registry-name availability remains a publication-time prerequisite and does not block local-source testing. No new research ticket is required by the selected source-only policy. Implementation and release automation remain outside this ticket.

Hand off to [Set the Rust design and specification acceptance criteria](06-specification-readiness.md): exact Claude/Rust/OS floors and target triples; faithful JSON/ownership/token record serialization; secure paths, permissions, locking/staged recovery; launcher root/executable binding including overrides/relocation and unchanged caller environment; Cargo install/uninstall/upgrade collision handling and compatible/incompatible storage; native Windows/Git Bash/Fish/WSL argument/TTY/exit/cancellation and PATH evidence; browser/manual-code/Keychain/file/ACL acceptance plus all earlier profile/session/token/copy/purge scenarios. It creates the final implementation-ready `spec.md`; this chat resolves at most the distribution decision.
