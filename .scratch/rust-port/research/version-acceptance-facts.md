# Version, platform, and acceptance facts for ticket 06

Checked **2026-10-06**. Read the canonical Answers in 03/04/05, the 06 handoff, and existing Claude/distribution research. This report supplies evidence and proposals; it does not settle the remaining human decisions or demonstrate Roost runtime support.

Source instructions target the user-selected Rust repository [rmkr/roost](https://github.com/rmkr/roost); the local directory remains `claude-multi-account`. CarlosTheory/claude-multi-account remains the separate upstream attribution source. This identity does not imply the new remote repository was inspected or published in this investigation.

## Claude version floor and observable contracts

**Proposed floor: Claude Code 2.1.268.** This is the documented introduction of the required observed `configDirectory` status field, independently confirmed by the official changelog. It is a defensible lower bound, not proof that every current documented behavior works on that revision. Test the floor and the current selected revision before claiming support; move the floor upward if required features fail rather than inventing a compatibility shim. [CLI reference](https://code.claude.com/docs/en/cli-reference#cli-commands), [official changelog, 2.1.268](https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md#21268).

| Required capability | Current primary-source evidence | Historical introduction verified here |
| --- | --- | --- |
| `--name` / `-n` | Names session for resume/title; current live-name collisions may produce a variant | Exact introduction **not established** |
| `--resume <name>` | Name or native picker/identifier forwarding | Named resume added **2.0.64** |
| `--continue` | Latest conversation in current directory | Exact introduction **not established** |
| `--fork-session` | New session ID with resume/continue | Exact introduction **not established** |
| `auth login`, `auth status`, `auth logout` | Supported native auth commands | Added **2.1.41** |
| Status `configDirectory` | Observed configuration-directory field | Added **2.1.268** |

The first four current contracts are in the [CLI flags reference](https://code.claude.com/docs/en/cli-reference#cli-flags); introduction evidence is in the [2.0.64](https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md#2064) and [2.1.41](https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md#2141) changelog sections. No precise minimum for directory isolation, `FORCE_AUTOUPDATE_PLUGINS`, or `--name` was established in this bounded investigation. Latest docs cannot establish their historical introduction.

### Safe status interpretation

The current official command table explicitly documents JSON by default, `--text` for human output, exit **0 logged in / 1 not logged in**, `configDirectory`, and these `authMethod` strings: `none`, `claude.ai`, `oauth_token`, `api_key`, `api_key_helper`, `third_party`. The enum is therefore documented today; earlier notes treating all method values as undescribed should be qualified. The changelog later fixes misclassification of a Console stored API key, so method reporting is version-sensitive. [CLI reference](https://code.claude.com/docs/en/cli-reference#cli-commands), [official changelog](https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md).

**No complete public JSON schema or official complete JSON example was found on the checked pages.** Required-versus-optional keys, nullability, a native `loggedIn` field, identity/name/email keys, and every enum's availability at 2.1.268 remain unverified. Do not label an invented object a native schema. Recommended Roost interpretation: derive `reported_logged_in` from the documented exit only after recognizing valid status JSON; accept only typed `authMethod` and `configDirectory`; missing/wrong-type/unknown values become null with a safe warning or unknown under the approved command contract. Ignore every other native field. Never copy raw JSON/stderr into manager output or read native credential stores. A complete schema is unnecessary for this allowlist, but floor/current sanitized fixtures are a release prerequisite. JSON parse/shape failure must not turn an operational exit 1 into a claimed logged-out result.

### Isolation and updater controls

Separate `CLAUDE_CONFIG_DIR` contexts support claude.ai sign-ins/API keys and select separate native credential files/macOS Keychain entries. Keyless Console profiles live elsewhere and remain outside this isolation promise. Host gateway/profile/federation state, project settings, helpers, and credential precedence can still change the effective account. Directory equality and login status do not establish account identity/service access. [Authentication and multiple accounts](https://code.claude.com/docs/en/authentication#log-in-with-multiple-accounts), [credential management](https://code.claude.com/docs/en/authentication#credential-management).

Current docs support `DISABLE_AUTOUPDATER=1` for background updater suppression and `FORCE_AUTOUPDATE_PLUGINS=1` for plugin updates despite that suppression. `DISABLE_UPDATES` also restricts manual installation/update. These are per-child environment controls when Roost sets them, not a global update lock; manual `roost update` retains caller restrictions as already approved. Historical minimums remain unknown. [Environment variables](https://code.claude.com/docs/en/env-vars), [update controls](https://code.claude.com/docs/en/setup#disable-auto-updates).

## Eight Rust compilation targets

| Required variant | Rust triple | Rust target baseline documented today |
| --- | --- | --- |
| Linux x64 glibc | `x86_64-unknown-linux-gnu` | Kernel 3.2+, glibc 2.17+ |
| Linux ARM64 glibc | `aarch64-unknown-linux-gnu` | Kernel 4.1+, glibc 2.17+ |
| Linux x64 musl | `x86_64-unknown-linux-musl` | Rust lists musl 1.2.5 |
| Linux ARM64 musl | `aarch64-unknown-linux-musl` | Rust lists musl 1.2.5 |
| macOS x64 | `x86_64-apple-darwin` | macOS 10.12+ |
| macOS ARM64 | `aarch64-apple-darwin` | macOS 11+ |
| Native Windows x64 MSVC | `x86_64-pc-windows-msvc` | Windows 10+ / Server 2016+ |
| Native Windows ARM64 MSVC | `aarch64-pc-windows-msvc` | Windows 10+ / Server 2016+ |

These are Rust compiler targets, not Roost's supported operating-system promise. [Rust platform support](https://doc.rust-lang.org/rustc/platform-support.html), [Darwin targets](https://doc.rust-lang.org/rustc/platform-support/apple-darwin.html), [MSVC targets](https://doc.rust-lang.org/rustc/platform-support/windows-msvc.html).

**Combined runtime proposal:** retain Claude's stricter macOS **13+**, native Windows **10 1809+ / Server 2019+**, and Linux distribution baselines **Ubuntu 20.04+ / Debian 10+ / Alpine 3.19+**, subject to actual locked-build/runtime evidence. Claude lists x64/ARM64, 4 GB RAM and network requirements. The musl version used by Rust's target is not automatically the minimum installed host musl for a statically linked application; verify static/dynamic linkage and the locked dependency set on the declared Alpine baseline. Do not silently raise Alpine to a newer release solely from the Rust target table, or promise an untested older runtime. [Claude system requirements](https://code.claude.com/docs/en/setup#system-requirements).

**Build prerequisites:** Linux native builds need the matching C linker/toolchain and any dependency headers; cross builds need a target-compatible linker/sysroot if the chosen dependencies/link mode require them. Darwin cross builds may require Clang, Xcode/macOS SDK; use `MACOSX_DEPLOYMENT_TARGET=13.0` for the selected application floor, with real floor execution. MSVC needs the architecture's Visual Studio C++ build tools/Windows SDK; Rust documents VS 2017 minimum, recommends VS 2022, and supports Windows-host architectural cross builds with appropriate components. Non-Windows-to-MSVC cross compilation is not supported by that target's documented toolchain contract. ARM64 musl is distributed through rustup and can be cross compiled; its compiler-build example names `aarch64-linux-musl-gcc`, not a requirement to rebuild rustc. [Darwin build/cross details](https://doc.rust-lang.org/rustc/platform-support/apple-darwin.html), [MSVC build details](https://doc.rust-lang.org/rustc/platform-support/windows-msvc.html), [ARM64 musl](https://doc.rust-lang.org/rustc/platform-support/aarch64-unknown-linux-musl.html).

**Rust floor is a design choice, not inferable from target names.** Lowest Rust/Cargo version satisfying the chosen APIs and locked dependency MSRVs requires the eventual crate/lockfile. A simple proposal is pinning **1.97.0** as the initial source-testing toolchain (locally present) rather than promising older compatibility without a build. This is not a verified minimum for nonexistent Rust application code; confirm dependency compatibility before adopting it and run `cargo install --locked` on every declared source platform. Publish the exact tested toolchain/revision in instructions.

## Caller shell floors

Keep the already accepted Windows PowerShell **5.1** and PowerShell **7+** promises; newer native argument handling cannot replace 5.1 coverage. Microsoft documents a breaking native argument-passing change in **7.3**, so separate 5.1, 7.0–7.2 legacy behavior, and 7.3+ Windows/Standard/Legacy behavior in launcher acceptance. `.cmd` still selects legacy behavior in Windows mode. Empty arguments and embedded quotes are essential cases. [PowerShell native argument preference](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_preference_variables#psnativecommandargumentpassing), [Windows PowerShell distinction](https://learn.microsoft.com/powershell/scripting/windows-powershell/install/installing-windows-powershell).

**Fish proposal: 3.2+** for a scoped startup snippet using `fish_add_path --path`, without universal-variable writes. The official release notes introduce the helper in **3.2.0** and its versioned documentation includes `--path`. This gives a documented feature floor, subject to actual 3.2/current execution. No special Fish profile launcher is required for executable POSIX sh scripts. Bash/Zsh/POSIX sh minimum numbered versions depend on the eventual recipe syntax; choose portable syntax and record the actual tested versions. [Fish 3.2 release notes](https://fishshell.com/docs/3.2/relnotes.html#fish-3-2-0-released-march-1-2021), [3.2 fish_add_path](https://fishshell.com/docs/3.2/cmds/fish_add_path.html).

## Specification readiness versus release evidence

**Specification can become ready now:** user agrees on floors, precise status allowlist/failure interpretation, module/filesystem/process design, exact schemas and acceptance obligations; independent review finds no unresolved policy/implementation ambiguity; `spec.md` faithfully references 03/04/05. A clearly enumerated future platform test is an acceptance obligation, not an unsettled design question. Readiness does not claim code exists, a supported release exists, or any runtime/authentication test has passed.

**Before a supported source release:** record tested source/lockfile/toolchain and native Claude versions for all eight target environments plus separate WSL 2. Show native execution, real PATH/launcher/custom-root/upgrade behavior, faithful arguments/stdio/TTY/exits/cancellation, filesystem redirection/ownership/ACL and token replacement protections, locking/staging/crash/partial-removal recovery, copy exclusions, retained/reuse/purge behavior, and all required shells. Cross compilation proves compilation only; QEMU/container runs can establish stated Linux process/filesystem cases but do not prove native browser/Keychain/Windows ACL behavior.

**Consenting-account evidence:** use eligible nonproduction test accounts to verify two distinct claude.ai contexts, native browser/manual-code first login and cancellation/failure, observed directories, same/different-profile named sessions/picker/UUID/name resume/continue/fork, explicit owned-token clearing before browser reliance, borrowed-token read-only override safety, and inherited-auth refusal. Include macOS Keychain success/rejected-write fallback, Linux private credential files, native Windows profile ACL/native browser behavior, and WSL manual-code/native-environment separation. Keep keyless Console exclusion explicit; do not scrape credentials. Record safe outcomes instead of tokens/raw status. This investigation did not perform these checks.

## Local capacity observed, not acceptance performed

Read-only probes on 2026-10-06 observed Ubuntu **24.04.5**, Linux **7.0.0-38**, **x86_64**, glibc **2.39**, rustc/Cargo **1.97.0**, installed Rust std targets `x86_64-unknown-linux-gnu` and `x86_64-unknown-linux-musl`, Bash **5.2.21**, Fish **4.9.3**, native GCC/ld, and Podman on PATH. Zsh, PowerShell, Windows CMD, Wine, QEMU ARM64 and ARM64/musl C cross-linkers were not found by the PATH probe. No container capability/image, musl linker/build, other OS/architecture runner, or consenting-account availability was established. This host can support later native x64-glibc checks; merely having the musl Rust target installed proves no musl runtime/build result.

No real auth/status/version command against Claude, credential read, application implementation/install, shell configuration mutation, compilation, update, commit, publication, or new ticket occurred. Direct shell retrieval of the public changelog failed DNS; browser/web retrieval supplied the cited official evidence. Full native JSON schema, several exact historical feature introductions, and the final dependency-driven Rust minimum remain explicitly unverified.
