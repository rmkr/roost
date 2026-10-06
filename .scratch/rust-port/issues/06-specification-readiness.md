# Set the Rust design and specification acceptance criteria

Type: grilling
Labels: wayfinder:grilling
Status: resolved
Assignee: Codex (chat 01a11119-87ad-7390-a614-c412170f0d1a; design continuation)
Parent: [Find the way to a Rust Claude profile manager](../map.md)
Blocked by: 03, 04, 05

## Question

What is the smallest Rust design and validation plan that implements the settled behavior, and what evidence will make the specification ready to hand off?

Choose only required module boundaries, dependencies, process and filesystem handling, and cross-platform acceptance scenarios. Include compatibility, credential handling, command forwarding, interruption recovery, and safe deletion where earlier decisions require them. Identify unresolved questions as tickets before declaring readiness. Capture the agreed implementation-ready specification in `../spec.md` and link it from the resolution; resolving this ticket requires the user's agreement.

## Comments

### 2026-10-05: ownership requirements and acceptance handoff

[Define safe profile coexistence and ownership](03-profile-compatibility.md#answer) supplies approved ownership/safety requirements and required acceptance scenarios. Settle ownership evidence retained after removal, mutation locking, atomic creation/recovery, secure filesystem/permission handling, and a Claude-version/platform evidence matrix here. External-writer coordination and automatic installation ownership are not promised. The linked scenarios are future checks, not completed runtime validation. This ticket remains blocked by the command-contract and distribution decisions.

### 2026-10-06: approved command/session/OAuth behavior handed off

[Choose the Rust command contract and targeted improvements](04-command-contract.md#answer) is resolved and supplies exact command/option grammar, finite authentication refusal list/override semantics, copy/input/removal/PATH/update/diagnostic/error rules, and native session/OAuth recipes. This ticket remains open, unassigned, and blocked by Choose command naming and cross-platform distribution.

Produce the smallest implementation design and final `../spec.md` without reopening approved behavior. Settle ownership/retained-data representation, locking, staged publication/recovery, secret/filesystem protections, exact JSON record serialization, safe PATH edits, child process/argument/signal mechanics, and supported Claude-version floor. Define meaningful Linux/macOS/Windows evidence for the ownership scenarios plus native browser OAuth first login, explicit owned manager-token clearing before browser-login reliance, borrowed-token read-only/override safety, cancellation/login failures, unchanged registered paths, and native credential storage. Include concurrent named sessions in the same/different profiles, picker/name/UUID resume, continue/current-directory behavior, fork versus shared-transcript continuation, and quoted argument boundaries. Native background dispatch/worker authentication are outside a new Rust orchestration guarantee; do not claim instantaneous token switching or account proof. Copy exclusions/global-state loss, partial removal, failed staging, and noninteractive secret/confirmation paths need acceptance evidence. No real authentication/runtime/platform tests were performed while resolving the command ticket.

### 2026-10-06: distribution confirmed; next decision is unblocked and unclaimed

[Choose command naming and cross-platform distribution](05-distribution.md#answer) is resolved after final human confirmation and a clean independent review. All blockers are resolved. This ticket remains open and unassigned; claim it in the next Wayfinder session before work. No second decision is resolved in the distribution chat.

Read the canonical ownership, command, and distribution Answers plus the glossary before designing. Preserve the Roost executable/source-package/launcher family, home `.roost`/`ROOST_DIR` layout, source-checkout Cargo testing delivery and compatible-state upgrade/incompatible-state refusal, MIT attribution, existing shared Claude/native browser credential ownership, unchanged registered paths, and explicit command-based manager switching. Fish support/scoped `setup-path --apply` is an explicitly accepted extension of the earlier shell scope. Prebuilt binaries, registry publication, package-manager packaging, custom installers, and release automation are deferred to a later release decision; failed registry reads are not availability evidence and do not block local source testing.

Settle only remaining implementation/acceptance details: minimal module/dependency boundaries; exact Claude/Rust/OS floors and Rust triples; faithful JSON/registration/ownership/token serialization; permissions and redirection-safe paths; locking/staging/recovery; launcher root/executable binding preserving the selected profile and original caller environment/arguments despite ambient `ROOST_DIR`, foreign PATH commands, custom Cargo roots, upgrades, or relocation; and safe Cargo install/uninstall/upgrade guidance. Cargo's executable bin directory and Roost's generated-launcher bin directory are distinct PATH concerns.

Define meaningful x86_64/ARM64 Linux glibc/musl, macOS, native Windows, and separate WSL 2 acceptance evidence; Linux/macOS/WSL Bash/Zsh/Fish/POSIX launcher execution plus Windows PowerShell 5.1/7+, CMD/Git Bash. Include source-install/upgrade compatibility and foreign executable/storage/launcher refusal, Fish/configured-directory/user-PATH safety, quoted arguments/TTY/exit/cancellation, native browser/manual-code fallback, Keychain/file/ACL handling, borrowed-token read-only/explicit override behavior, and every earlier session/token/copy/purge scenario. Cross-compilation and documentation do not count as these runtime checks. Existing [distribution facts](../research/distribution-facts.md) and [Roost naming snapshot](../research/roost-name-check.md) record factual limits.

Use live human decision rounds and final agreement to produce the final implementation-ready `../spec.md` here. No application implementation, source install, real OAuth/credential test, cross-platform runtime validation, commit, publication, or final specification was performed/created while resolving the distribution decision.

### 2026-10-06: claimed for the final design/specification phase

After the distribution decision was resolved, the user explicitly instructed "Okay lets continue." This starts the next Wayfinder decision phase in the same chat; the earlier one-ticket distribution scope has been completed and the continuation authorizes work on this next ticket. Rechecked all blockers as resolved and this ticket as open/unassigned, then claimed it. Preserve all existing uncommitted files. Use live human decision rounds, current external/API evidence, and final shared-understanding agreement to write `../spec.md` and resolve this design decision. Application implementation, commits, installation, publication, and release automation remain outside scope.

### 2026-10-06: design round 1 pending

- Q1 recommends one versioned JSON registry per manager root, retaining owned-profile records after ordinary removal, plus per-owned-profile ownership markers whose root/profile IDs agree before reuse/purge. Tokens stay separate and copy excludes Roost ownership/token artifacts. Registered upstream records preserve their paths without receiving ownership markers. No database/native credential contents in registry. Await the human choice before selecting exact schema.
- Q2 recommends separate spec-readiness and implemented-release gates: complete implementation approach/acceptance recipes, explicit evidence gaps, and clean independent review make the spec ready; focused local checks plus fake-Claude CI on Linux/macOS/Windows, followed by real required platform/shell/source-install/filesystem/token/browser/session checks using consenting test accounts, are required before supported first-release claims. Missing platform evidence stays unmet; cross-compilation is not runtime proof. Await the human choice.

Bounded Research subagents are investigating native Rust/filesystem/locking/launcher APIs and Claude/Rust/platform version/acceptance facts, saved to `../research/rust-native-design-facts.md` and `../research/version-acceptance-facts.md`. They own only their reports; no application implementation or platform/authentication test is being performed. Version/dependency/launcher transport decisions await those facts. Early primary-source findings: std file locking starts at Rust 1.89; Windows readonly does not establish secret ACL protection; launcher root selection cannot overwrite caller `ROOST_DIR` when alias environment must be preserved, and public run's initial-separator handling differs from literal launcher forwarding; Claude auth-status configDirectory is documented since v2.1.268. Exact applicability and policy choices remain pending.

### 2026-10-06: target repository updated; registry-format question open

The user supplied the Rust project's renamed repository, `https://github.com/rmkr/roost`. GitHub metadata confirms repository `rmkr/roost`, private, default branch `main`. The local checkout directory remains `claude-multi-account`; origin is updated to `git@github-personal:rmkr/roost.git`, preserving the prior SSH host alias. No push or directory relocation occurred. The upstream CarlosTheory/claude-multi-account attribution/revision remains distinct and unchanged.

The user asked whether `config.toml` would be better than the proposed JSON state. Q1 remains open: this is generated registration/ownership/retained-state storage, not a newly requested human settings feature. Recommendation remains `registry.json`; if the human prefers TOML, use `registry.toml` with the same schema/version/ownership/locking/atomicity rules. Neither encoding provides locking/crash safety by itself. TOML supports human-readable configuration/comments, but introducing TOML state would add an encoding dependency while public diagnostic JSON already requires JSON support. No global config-file feature is proposed or silently added. [TOML objectives](https://toml.io/en/v1.0.0), [serde_json](https://docs.rs/serde_json/latest/serde_json/), [toml crate](https://docs.rs/toml/latest/toml/). Q2 acceptance-gate choice also remains pending.

### 2026-10-06: other-system testing deferred and extra attribution removed

The user answered Q2: "We will test on other systems later." Accept local-development-first validation and defer other operating-system/architecture/native-account evidence to later implementation/testing. The spec may be finalized with explicit runnable/manual test recipes and clearly unperformed platform evidence; no other-system proof is required during planning or to begin local development. Keep future support requirements separate from current verified status and never turn compilation or an unperformed checklist into a passed runtime test.

The user requested "We can rid of the atribution." Remove the extra user-facing upstream credits/revision-attribution requirement from the planned README/help. Roost's selected MIT license remains. Preserve the upstream copyright/permission notice if any upstream source/templates/documentation are reused, as the inspected upstream MIT text requires. Historical research source citations and the internal pinned upstream baseline remain evidence, not required product credits. This amendment is recorded in the distribution Answer and map. The eventual spec will reflect the revised policy.

Both bounded research reports are complete: [Rust/native design facts](../research/rust-native-design-facts.md) and [version/platform facts](../research/version-acceptance-facts.md). They identify concrete API/launcher constraints and version proposals while clearly distinguishing local capacity and future tests from completed runtime evidence. Q1 registry format remains unresolved after the human's TOML question; no new configuration feature is selected.

### 2026-10-06: clarified format, version, and launcher round pending

- Q1 clarified recommendation remains generated `registry.json`, reusing the serializer required for diagnostic JSON. `registry.toml` is equally feasible if preferred, with the same ownership/lifecycle invariants and an added TOML serializer. No `config.toml` preferences feature is requested. Await choice.
- Q3 recommends Claude Code 2.1.268+ as the documented status-directory lower bound, Rust/Cargo 1.97.0 as the initial pinned source-testing toolchain (observed locally, not proven app MSRV), and Fish 3.2+ for the selected native helper. Required PowerShell 5.1/7+ coverage remains. Validate floor/current named-session/auth/status features during implementation; explicitly revise the floor if it fails, without speculative compatibility workarounds. Await choice. Exact target triples/OS prerequisites are in the linked research and will be incorporated into the proposed spec.
- Q4 recommends a hidden same-binary internal launcher entrypoint with absolute manager executable/root/registration binding and a literal caller-argument tail. It performs identical ordinary safety/auth checks and preserves caller environment, including `ROOST_DIR`, while preventing foreign PATH/wrong-root selection. It is an internal protocol, not a new public manager command/flag, is omitted from public help, and does not strip a caller leading delimiter. No sidecar binary. Missing/relocated bindings need explicit approved reuse/registration refresh. Await choice; exact private wire shape, identity verification, native adapters, and process mechanics follow after this human decision.

Other-system/runtime/authentication tests remain deferred, as the user requested. The full specification has not been generated and this design ticket stays claimed.

### 2026-10-06: registry, versions, and internal launcher protocol accepted

The user answered "Accept." to the clarified Q1/Q3/Q4 recommendations. Settle one versioned JSON registry (including retained owned-profile records) with root/profile ownership markers and separate token storage; no TOML/global settings feature. Initial version requirements: Claude Code 2.1.268+, pinned Rust/Cargo 1.97.0 for source testing, Fish 3.2+, retaining required PowerShell 5.1/7+ coverage; actual floor/dependency/feature evidence remains later implementation work. Use a same-binary hidden internal launcher transport binding the absolute manager executable/root/registration and forwarding the literal caller tail without changing caller environment. Missing/relocated bindings need explicit reuse/registration refresh, not foreign-PATH fallback.

Q2's local-first/other-system-tests-later steering remains authoritative. Extra product attribution is removed, with reused material's existing MIT notice retained conditionally. Source repository is rmkr/roost, origin updated, local checkout path unchanged. Remaining frontier: minimal module/dependency plan; lock/contention and recoverable publication; native Windows Claude resolution/batch adapter policy; precise schema/path/file-identity handling; child cancellation/status and evidence/checklists. Planning only; no Rust source/application tests exist yet.

### 2026-10-06: modules, locking/recovery, and Windows installation round pending

- Q5 recommends one synchronous Cargo crate at repository root, one `roost` executable, and `cli`, `store`, `launch`, `platform` implementation modules. Store owns ownership/locking/recovery; launch owns shared auth/forwarding; platform holds actual Unix/Windows differences. Dependencies: established CLI parser and JSON serialization (`clap`, `serde`, `serde_json`), secure random identity generation, and only needed native Unix/Windows bindings. No async/database/plugin abstraction/separate launcher binary. CLI/shared interfaces are the test surface. Await choice; library versions and native helpers must satisfy the accepted pinned toolchain and lockfile.
- Q6 recommends one stable OS lock per root, cancellable contention wait up to 10 seconds then safe retry guidance. Locks cover filesystem transactions/brief launch preparation, not Claude lifetime or human secret input. Stage complete changes with a small nonsecret operation record and activate registry last. Next mutation may recover only proven Roost-owned incomplete work; never auto-continue partial purge. Read-only doctor reports pending recovery; uncertain ownership refuses mutation. Alternative user option is waiting until the other operation ends. Await choice; precise lock modes/file names/recovery records follow acceptance.
- Q7 recommends requiring the PATH-selected native `claude.exe` for initial Windows support, refusing a selected `.cmd`/`.bat` shim with native-install guidance rather than unsafe argument reparsing or choosing another installation silently. Roost's required CMD/PowerShell/Git Bash launcher forms remain. This is an explicit proposed narrowing of supported Claude installation forms; native-only versus first-release safe batch adapter is a human choice, not presumed capability parity. Await choice. Source argument constraints: [Rust Windows CommandExt](https://doc.rust-lang.org/std/os/windows/process/trait.CommandExt.html#method.raw_arg), [native design research](../research/rust-native-design-facts.md).

No application source, manifest/lockfile, runtime test, source install, or final specification has been produced. The design/specification ticket remains claimed pending these choices and later final confirmation.

### 2026-10-06: minimal structure, lock/recovery policy, and native Windows Claude accepted

The user answered "Accpet all" (accept all) to Q5-Q7. Settled: one synchronous Cargo crate at repository root with one Roost executable and cli/store/launch/platform implementation modules; established parser/JSON libraries, secure random IDs, and required target-native bindings only. One stable manager-root OS lock with cancellable 10-second contention wait, held for filesystem changes/brief launch preparation rather than sessions or secret prompts; staged registry-last activation with nonsecret operation records; recovery limited to proven Roost-owned incomplete work, no automatic continuation of partial purge, and read-only doctor reporting.

Native Windows initially requires the PATH-selected `claude.exe`. Selected `.cmd`/`.bat` Claude shims fail safely with native-install guidance; no silent PATH fallback/installation replacement. Required Roost CMD/PowerShell/Git Bash launchers remain in scope. This is an explicit human-approved narrowing of the earlier shared-Claude installation contract, and must be visible in the final spec/prerequisites/tests. Unix ordinary executable/shebang resolution remains as selected. Batch-adapter support is deferred until later native Windows evidence and a follow-up decision.

Remaining final-design details are concrete schema/file identities, recovery lifecycle transitions, status parsing and bounded captured output, path encoding, parent/child cancellation, exact private launcher transport/platform adapters, version gating/diagnostics, and test recipes. They must faithfully implement accepted policy; only newly exposed human tradeoffs need another live round. No app implementation/manifest/install/test/commit/publication occurred.

### 2026-10-06: final observable edge policies pending

- Q8 recommends valid-Unicode, no-line-break manager roots/registered/source paths and selected copied filenames for the initial version. Preserve spelling and reject before publication rather than lossy conversion; forwarded native Claude argument values remain unrestricted by this path-serialization rule. This is an explicit proposed restriction, not already approved. Supporting all native path encodings would require a lossless platform-specific storage/JSON/output representation. Await the human answer before finalizing affected schema/path clauses.
- Q9 recommends a 10-second deadline and 1 MiB per captured stdout/stderr stream for native version/auth-status probes, concurrent draining, and terminating/reaping timed-out or oversized probes with unknown/failed diagnostic results. Interactive run/login/session/update children have no manager deadline. Await the human answer before finalizing probe limits.

The specification is being drafted as a reviewable artifact. These clauses remain visibly pending, and the ticket stays claimed until final shared-understanding confirmation after independent review.

### 2026-10-06: path/probe limits accepted; concrete draft saved

The user accepted both Q8 and Q9 through the question replies: valid-Unicode/no-line-break manager paths with no lossy rewriting, and 10-second/1 MiB-per-stream captured diagnostic limits. Native Claude argument forwarding stays opaque; interactive children have no manager timeout. These are settled choices.

The reviewable [specification draft](../spec.md) now supplies exact state/public JSON schemas, artifact names and IDs, per-operation recovery/purge evidence, native filesystem/token protections, private launcher transport, platform adapters, source lifecycle, and A1–A14 acceptance recipes. Unix interactive run/update use native exec after closing locks/restoring manager-owned terminal/signal state, preserving native process signals/status without a parent signal-source heuristic; Windows retains child/console wait. This is an implementation mechanism for the approved child behavior, not a new supervisor promise. Independent review and final shared-understanding confirmation remain pending; no application implementation has begun.

### 2026-10-06: independent review round 1 and verified correction

The Forge native fresh-context inspection (Reviewer/Mid/Top inherit session model and effort; ordinary inspection-only assignment, not strict enforcement) reviewed draft SHA256 `450a0393f183e2796d0fdd18fb0248bdf19d526e2fece2057898cdfd51f31612` against canonical Answers, accepted Q1–Q9, glossary and research. One actionable delivery finding: SPEC-RECOVERY-01 (P2), journal replacement artifacts had one ambiguous physical identity and no serialized old/new binding/template evidence despite requiring both in prose. The reviewer found no verified premise defect or new essential human-policy question.

Accepted after checking the actual schema/prose. Corrected artifacts to explicit before/staged/after states with independent physical identities, binding/template version/form and applicable generation/profile evidence, null absence semantics, and required persistence/revalidation points. A8 now specifically covers relocated executable/launcher replacement interrupted before registry commit. No token/template bytes enter the operation record. Also cited std native exec failure semantics (no continued state reuse after failed exec) and made Unicode Windows console-code-page installation paths explicit in argument acceptance. These clarify implementation/recipes, not accepted product scope. Re-review of the combined draft is pending.

Local checks passed draft/ticket whitespace, local link targets, JSON examples and `git diff --check`; these are documentation checks only, not application/runtime evidence.

### 2026-10-06: clean independent re-review; final confirmation pending

Round 2 inspected spec SHA256 `6f129a19472fc11351e752e5bc5f1f31f03a38fe63baf10638678d8d8c9c2102` and ticket SHA256 `ac76928ca4002aea8f189301c9f2d0a6ad529c1fcde31a7c4610ffb0f917e3a3`, unchanged at completion. Explicit result: no actionable findings remain; SPEC-RECOVERY-01 resolved, combined token/launcher/registry recovery consistent, no verified scope conflict from exec/code-page clarifications. Native runtime/adapter/ACL/authentication evidence remains unproven and deferred. Spec editorial readiness text now reflects the clean review rather than pending review.

All human Q1–Q9 design recommendations are accepted. The concrete specification supplies the final implementation handoff while preserving source-only testing, Fish, selected Roost identity, unchanged upstream ownership, and removal of extra product attribution. Present the completed draft for final shared-understanding confirmation under Grilling before adding an Answer or resolving this ticket/map. That confirmation approves this planning specification only; it does not authorize application implementation, installation, publication, or deferred tests.

### 2026-10-06: final shared understanding accepted

The user answered "Accept" to Q10, the final confirmation of the completed Roost specification. This accepts the concrete reviewed handoff after Q1–Q9 and the clean independent review, and authorizes resolving this design decision and the planning map. Implementation, installation, publication and deferred runtime/platform/authentication checks remain subsequent work.

## Answer

### 2026-10-06: approved implementation-ready Roost specification

The human accepted design Q1–Q9 and final shared-understanding Q10. The [final specification](../spec.md) is the canonical design and acceptance handoff, constrained by the earlier ownership/command/distribution Answers and their explicitly accepted amendments. The planning destination is satisfied; all in-scope decisions are resolved and no planning fog remains.

- One synchronous root Cargo crate and Roost executable with cli/store/launch/platform modules, established parser/JSON dependencies and required native bindings; no added config/database/service framework.
- One versioned JSON registry, retained owned records, matching root/profile markers and physical identities, separate protected tokens, exact public JSON records and safe unchanged-path upstream registration.
- Stable per-root native locking with cancellable 10-second contention, complete staged publication with registry committed last, explicit before/staged/after recovery evidence, read-only diagnostics, and no automatic continuation of partial purge.
- Same-binary private launcher transport binds the absolute executable/root/registration, preserving caller environment and literal arguments; native Windows initially requires selected claude.exe while Roost's CMD/PowerShell/Git Bash forms remain required.
- Selected Rust/Cargo 1.97.0, Claude 2.1.268+, Fish 3.2+ and PowerShell 5.1/7+ requirements; intended eight-target/native-shell/WSL matrix with explicit later evidence obligations. Unicode/no-line-break manager paths avoid lossy serialization; captured native probes are bounded to 10 seconds and 1 MiB per stream without timing out interactive children.
- Source-only testing delivery, Roost identity, Fish PATH support, explicit native OAuth/session/update recipes and guarded installation/reuse/retain/purge behavior. Extra product attribution is removed; required notices on reused MIT material remain.
- A1–A14 executable/manual acceptance recipes distinguish local development from deferred platform/account proof. Cross-compilation/checklists are never runtime evidence.

The Forge fresh native review found SPEC-RECOVERY-01 (P2), accepted and corrected the ambiguous replacement evidence, and re-reviewed clean. Final editorial snapshot confirmation returned no actionable findings for spec SHA256 `308af2e149883e88c97c7c9f6d4e8ab2dcec930a11a5d6e136e42f462798d710` and ticket SHA256 `40927ff6a2a6c0ddc90c21257de76eb53e5198215e60acd42446507ef2447993`. Reviewer/Mid/Top inherited the session model and effort; inspection-only assignments were ordinary, not strict runtime enforcement. Human approval then changed only specification readiness/authority metadata, not accepted behavior.

Approved specification SHA256: `ca0fea216578c0e545e8247d98ce466d39e3b5f1f3ed0ed5929ad65979b192fc`. Documentation whitespace/local-link/JSON-example checks and tracked diff checks pass. No application implementation/build/install, native Claude auth/version/credential inspection, other-system runtime test, commit, push or publication was performed to close this planning decision. Future evidence gaps remain explicit in the specification; no further planning ticket is needed to close this map.
