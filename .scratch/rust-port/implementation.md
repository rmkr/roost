# Roost implementation

Started 2026-10-06 after explicit user authorization. The accepted [specification](spec.md) governs. Preserve the existing planning documents/glossary; source-only development, no live credential/account tests, installation, publication or prebuilt distribution. Other-system tests remain deferred.

## Ownership and module interface

Root agent owns Cargo.toml/Cargo.lock, main.rs, cli.rs, integration tests, README and this implementation record. Native workers own only the assigned files/modules and their in-module tests; all share the workspace and must preserve others' edits. Interfaces below coordinate this first implementation, not a new public/plugin API.

Shared crate-root `Error {code: &'static str, message: String, next_step: Option<String>, exit_code: i32}` and `type Result<T> = std::result::Result<T, Error>`. Constructors `Error::new(code, message)`, `.next(step)`, `Error::cancelled()`, `Error::io(operation, path, error)`. Stable codes from spec. No secrets in errors.

### Platform worker — src/platform/**

Use descriptor/handle-backed `pub struct Directory` with `pub path: PathBuf`, open object retained. Required API:

```text
pub fn absolute(path: &Path) -> Result<PathBuf> // lexical absolute, Unicode/no CRLF, no lossy rewrites
pub fn home() -> Result<PathBuf>
pub fn root() -> Result<PathBuf> // ROOST_DIR semantics
pub fn default_directory() -> Result<PathBuf>
pub fn random_id() -> Result<String>
pub fn install_cancel_handler() -> Result<()>
pub fn cancelled() -> bool
pub fn restore_for_exec() -> Result<()>
pub fn token_input(stdin: bool) -> Result<String> // hidden/bounded, no token logging
pub fn confirm(scope: &str, yes: bool) -> Result<()>
pub fn setup_path(root: &Path, shell: Option<&str>, apply: bool) -> Result<Vec<String>>
pub fn path_member(path: &Path) -> bool
pub fn copy_profile(source: &Path, destination: &Directory) -> Result<Vec<String>> // omission messages, exclusions

FileIdentity: Clone+Debug+Serialize+Deserialize+PartialEq+Eq; tagged platform unix {device:String,inode:String} or windows {volume:String,index:String}, deny unknown fields.
Entry {pub identity: FileIdentity, pub is_dir: bool, pub is_file: bool, pub is_link: bool, pub nlink: u64}
Directory::open(path: &Path, private: bool) -> Result<Self>
Directory::create(path: &Path) -> Result<Self> // exclusive private final directory; parents must exist
Directory::identity(&self) -> Result<FileIdentity>
Directory::entries(&self) -> Result<Vec<String>> // safe Unicode names
Directory::entry(&self, name: &str) -> Result<Option<Entry>> // no-follow
Directory::child(&self, name: &str, private: bool) -> Result<Self>
Directory::create_dir(&self, name: &str) -> Result<Self>
Directory::open_file(&self, name: &str, private: bool, write: bool) -> Result<std::fs::File>
Directory::read(&self, name: &str, private: bool, limit: usize) -> Result<Option<Vec<u8>>>
Directory::write_new(&self, name: &str, bytes: &[u8], mode: u32) -> Result<FileIdentity> // exclusive, protected before bytes, sync
Directory::rename(&self, name: &str, destination: &Directory, new_name: &str) -> Result<()>
Directory::remove(&self, name: &str, directory: bool) -> Result<()>
Directory::sync(&self) -> Result<()>
Directory::purge_children(&self, retain: &str) -> Result<()> // no linked targets; retain ownership marker
```

Only managed directory opens use private=true; ancestors/source/upstream roots are real no-follow but not chmod-repaired. Native Windows must fail closed on security gaps, never use readonly as ACL. Report any unimplemented required backend honestly.

### Store worker — src/store.rs

Use serde exact records from spec. `pub enum Kind {Owned, Upstream, DefaultAlias}` and `State {Active, Retained}` serialize snake_case. Both Clone+Copy+Debug+Serialize+Deserialize+PartialEq+Eq. Public Registration fields match exact schema; optional paths are Option<PathBuf>, identities Option<FileIdentity>, binding Option<LauncherBinding {format_version:u32, executable:PathBuf}>; pub Registry {schema_version:u32, root_id:String, generation:u64, registrations:Vec<Registration>}.

```text
pub enum OpenMode {Read, Mutate, RetryPurge}
pub struct Store {pub root: PathBuf, pub root_id: String, pub registry: Registry, ...retained lock/dirs...}
Store::open(root: &Path, create: bool, mode: OpenMode) -> Result<Self>
Store::pending(&self) -> bool
Store::find(&self, name: &str, retained: bool) -> Result<&Registration>
Store::validate(&self, registration: &Registration) -> Result<()>
Store::token(&self, registration: &Registration) -> Result<Option<String>> // safe owned/borrowed; alias None
Store::token_present(&self, registration: &Registration) -> Result<bool> // metadata only, no token-content reads
Store::profiles(&self, retained: bool) -> Result<Vec<serde_json::Value>> // exact public ProfileRecord
Store::add(&mut self, name: &str, link_default: bool, copy: Option<&Path>) -> Result<Vec<String>>
Store::register(&mut self, name: &str, path: &Path) -> Result<Vec<String>>
Store::reuse(&mut self, name: &str) -> Result<Vec<String>>
Store::preflight_remove(&self, name: &str, purge: bool) -> Result<(String, Registration)> // validate eligibility/destinations before confirmation
Store::remove(&mut self, name: &str, purge: bool) -> Result<Vec<String>>
Store::set_token(&mut self, name: &str, value: Option<&str>) -> Result<Vec<String>> // None clear
pub fn validate_name(name: &str) -> Result<()>
```

Store calls launch::templates for canonical owned launcher bytes. Same-root lock10sec, held only while Store lives; main releases before prompt/child. Read open must not recover/mutate; absent root => not_found; list main treats genuine absent root as empty. Pending ordinary intents classified/recovered on Mutate; RetryPurge admits only explicit validated partial-purge retry. Bootstrap/journal/copy/retain/reuse/purge/no foreign deletion per spec. Do not silently substitute a path-only security check for platform anchors. No schema shortcuts; journal must include exact before/staged/after evidence and registry last. Meaningful focused recovery tests in module permitted.

### Launch worker — src/launch.rs

```text
pub fn templates(root: &Path, root_id: &str, registration: &Registration) -> Result<Vec<(PathBuf, Vec<u8>)>>
pub fn prepare(store: &Store, registration: &Registration, allow_auth_env: bool) -> Result<std::process::Command>
pub fn execute(command: std::process::Command, arguments: &[OsString]) -> Result<i32> // Unix exec, Windows spawn/wait
pub fn update() -> Result<i32>
pub fn claude_info() -> Result<(PathBuf, String)> // PATH resolution, bounded version probe/floor
pub struct Status {pub data: serde_json::Value, pub warnings: Vec<String>, pub exit_code:i32}
pub fn status(command: std::process::Command, registration: &Registration) -> Result<Status>
```

Exact finite inherited-auth list from command Answer; no prefix guesses, overrides suppress token injection but not token safety. prepare validates store/registration/pending and constructs env selection without running version probe under store lock. execute/status perform version probe after caller releases Store. Aliases unchanged env. Templates bind recorded executable/root/root_id/reg_id and opaque tail, support exact sh/.cmd/.ps1 forms. Bounded10sec/1MiB per stream captured probes, concurrent drain/reap, no raw output disclosure. Unit tests for quoting/status recognition allowed; fake-native CLI integration tests root-owned.

## Evidence and remaining work

Initial state: only README tracked, existing .scratch/GLOSSARY untracked; no Rust sources. Graph indexed and no reusable application modules found. Rust/Cargo1.97.0 present. Native workers/reviewer inherit session model/effort, explicit assignment boundaries; review is ordinary inspection-only, not strict enforcement.

### Local development evidence

Native environment: Linux 7.0.0-38-generic, x86_64-unknown-linux-gnu, glibc, /tmp filesystem reported ext2/ext3. rustc 1.97.0 (2d8144b78 2026-07-07), LLVM 22.1.6. Cargo.lock contains the resolved dependency set. Rust/Cargo were already installed; dependencies built offline from the existing cache.

Use `RUSTUP_TOOLCHAIN=stable` for these local checks: installed stable is 1.97.0. An initial unqualified worker Cargo call started rustup component synchronization for the exact-name toolchain and was interrupted; subsequent checks used the installed stable toolchain offline. No toolchain installation was needed for the application. The sandbox remaps native ancestor owners to uid65534; it cannot pass the intentional root/euid ownership checks. Approved native disposable execution supplied accurate metadata; the policy was not weakened.

- `RUSTUP_TOOLCHAIN=stable cargo test --locked --offline`: first review snapshot passed 32 unit tests and 15 CLI tests.
- `RUSTUP_TOOLCHAIN=stable cargo clippy --locked --offline --all-targets -- -D warnings`: passed.
- `RUSTUP_TOOLCHAIN=stable cargo fmt --all -- --check`: passed.
- `git diff --check`: passed. New source is untracked, so formatting and compiler checks cover it independently of tracked-diff whitespace.
- Native Bash/sh source-twice tests preserved literal quoted/Unicode PATH membership. The CRLF-existing startup fixture executes the generated LF Bash block without altering existing bytes.
- Native `/home/linuxbrew/.linuxbrew/bin/fish` disposable HOME/XDG fixture sourced the owned snippet twice and found the existing launcher directory once. PATH setup did not initialize storage or mutate universal variables.
- Native PTY tests observed fake Claude stdin as a terminal. Hidden fake token input did not echo; EOF and SIGINT returned 130 and retained the existing token; blank input failed; terminal attributes were restored on every path.
- Real lock/process fixture: held `.roost-lock`, observed timeout after 10.006 seconds with exit 1, SIGINT exit 130, and two concurrent adds serialized with both registrations intact. Timeout/cancel created no registrations. Long native fake-Claude execution separately released the root lock and received direct SIGTERM.

Only disposable roots and fake secrets/Claude were used. No real Claude binary, account, credential file, user installation/PATH, shared updater, commit, push, release or publication was invoked. Cargo lifecycle tests used temporary install roots only.

### Independent review

Forge correctness review and installed Ponytail complexity review used inherited session model/effort. Reviewers were assigned inspection only, with fresh reviewer context; this is an ordinary review, not a runtime-enforced strict sandbox.

First combined snapshot: manifest SHA256 `1756ece1a4102a9e97bc32bd99bdd5b9c6ff625ad61a536e4a883f3aff1331d0`, covering Cargo manifests/lock, toolchain, license, README, accepted spec and all Rust source/tests. Both reviewers confirmed all 16 target files unchanged during review.

| Finding | Disposition |
| --- | --- |
| ROOST-REVIEW-01: profile replacement during purge confirmation/token input | Accepted. Preserve the preflight root ID and exact registration/physical profile evidence; compare after reacquiring before mutation. A native PTY race reproduced both original failures; the CLI regression checks replacement preservation and no token write. |
| ROOST-REVIEW-02: incomplete Prepared purge missing Profile artifact | Accepted. Complete proven preparation before any publication during explicit acknowledged retry; test after begin, partial launchers and marker recording. |
| ROOST-REVIEW-03: add rollback interrupted after marker deletion | Accepted. Permit only the already-proven same-identity empty markerless cleanup boundary, then remove the empty directory. Cover staged and published data; introduced foreign contents still refuse. |
| PT-01: duplicate journal role/operation guard | Accepted. Remove only the redundant guard; earlier stricter action validation remains. |
| PT-02: duplicate token decoding/trimming | Accepted. Reuse the platform token validator after the protected bounded read. |
| PT-03: duplicate filtered launcher assertion | Accepted. Retain the exact byte comparison, including empty argument boundaries. |

Corrections are implemented. Round2 manifest SHA256 `c50fc02b9fe22d94bac4289cdf0e40495c6b73280b9811abac3051135d5bbd53`; changed files are README, main.rs, store.rs, launch.rs and tests/cli.rs. Native full validation passed 34 unit and 16 CLI tests, strict all-target Clippy and formatting. Round2 correctness re-review is clean for the scoped Linux development implementation; no actionable findings remain. Complexity re-review confirms all three accepted cuts and no new actionable findings. This does not certify full cross-platform specification acceptance.

A disposable Cargo-root recipe used `cargo install --path CHECKOUT --locked --offline --bin roost --root TEMP_ROOT`, repeated the same install, and verified `--version` returned 0.1.0. A separate root with an unrelated `bin/roost` sentinel was refused without changing its bytes. `cargo uninstall --root TEMP_ROOT roost-claude` removed only the Cargo executable and retained the temporary owned profile and launcher. Both installation roots and profile data were discarded with the temporary fixture. This checks the selected custom-root lifecycle; no compatible older-version upgrade or user installation was performed.

### Remaining acceptance

This is the first Linux development pass, not first-release acceptance. Windows handle/identity/DACL storage protection, hidden terminal input/confirmation and HKCU PATH editing are unimplemented and refuse safely. Windows launch templates/resolution/probe code does not establish support. macOS, other Linux architectures, WSL and other native shells remain untested; real-account OAuth, credential-storage and native floor/current status/session/update gates remain deferred as requested.

A1–A11 have focused local fixtures, not exhaustive gate completion. The full terminate-before/after-every-publication-boundary matrix, complete native schema fixtures, and A12 default/config-precedence/older-version upgrade evidence remain unmet. A12 isolated custom-root install/reinstall/uninstall/collision checks passed. A13/A14 real-account/native-session acceptance is deferred. Unrecorded staging objects are preserved for inspection instead of deleting by name. No prebuilt delivery/release automation is introduced.
