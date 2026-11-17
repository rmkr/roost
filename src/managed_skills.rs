//! Stage portable skills in a disposable directory. Use the checkout's Git
//! directory to serialize promotion and retain ownership across interrupted writes.
use crate::{
    Error, Result, launch,
    platform::{self, Directory, FileIdentity},
};
use rustix::fs::{self as sys, Mode, OFlags, RenameFlags};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs::{self, DirBuilder, File},
    io::{BufWriter, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

const INSTALLER: &str = "skills@1.9.0";
const LOCK_FILE: &str = "skills-lock.json";
const METADATA: &str = "roost-managed-skills";
const RECORD: &str = "transaction.json";
const JSON_LIMIT: usize = 4 * 1024 * 1024;
const TREE_LIMIT: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct Receipt {
    pub revision: String,
    scope: String,
    checkout: FileIdentity,
    source: String,
    files: Vec<Owned>,
    lock_version: Value,
    lock_entries: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Owned {
    placement: Placement,
    digest: String,
    identity: FileIdentity,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct Placement {
    agent: Agent,
    name: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
enum Agent {
    Canonical,
    Claude,
}

impl Placement {
    fn parent(&self, root: &Directory, create: bool) -> Result<Option<Directory>> {
        let agent_dir = match self.agent {
            Agent::Canonical => ".agents",
            Agent::Claude => ".claude",
        };
        let Some(agent_dir) = child(root, agent_dir, create)? else {
            return Ok(None);
        };
        child(&agent_dir, "skills", create)
    }

    fn claude_link(&self) -> PathBuf {
        Path::new("../../.agents/skills").join(&self.name)
    }
}

pub(crate) struct Staged {
    revision: String,
    scope: String,
    source: String,
    files: Vec<(Placement, Content, String)>,
    lock_version: Value,
    lock_entries: BTreeMap<String, Value>,
}

impl Staged {
    pub(crate) fn revision(&self) -> &str {
        &self.revision
    }
}

#[derive(Serialize)]
enum Content {
    Tree {
        mode: u32,
        children: BTreeMap<String, Content>,
    },
    File {
        mode: u32,
        bytes: Vec<u8>,
    },
    Link(PathBuf),
}

/// A checkout may have several disjoint managed bundles. Each bundle retains
/// its latest transition so the caller can retry after losing the returned
/// receipt. The before and after identities distinguish staged objects from
/// unrelated files whose contents happen to match.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Transaction {
    version: u32,
    before: Option<Receipt>,
    after: Option<Receipt>,
    workspace: String,
    complete: bool,
}

struct Checkout {
    root: Directory,
    state: Directory,
    _lock: File,
}

struct Temp(PathBuf);

impl Temp {
    fn new() -> Result<Self> {
        let base = env::temp_dir()
            .canonicalize()
            .map_err(|e| Error::io("resolve staging directory", &env::temp_dir(), e))?;
        let path = base.join(format!("roost-skills-{}", platform::random_id()?));
        DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(|e| Error::io("create staging directory", &path, e))?;
        Ok(Self(path))
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn git() -> Command {
    let mut command = Command::new("git");
    // Git's checkout and config overrides must not affect identity checks or
    // the stable, repository-independent content digest.
    for (key, _) in env::vars_os() {
        if key.as_encoded_bytes().starts_with(b"GIT_") {
            command.env_remove(key);
        }
    }
    command
}

fn digest(content: &impl Serialize) -> Result<String> {
    // Git is already required for checkout validation. A blob hash outside
    // any repository is stable across processes, unlike Rust's map hashers.
    let mut hash = git()
        .current_dir("/")
        .args(["hash-object", "--stdin", "--no-filters"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| Error::new("skills_unavailable", format!("Cannot start git: {e}")))?;
    let written = {
        let mut stdin = BufWriter::new(hash.stdin.take().expect("piped stdin"));
        serde_json::to_writer(&mut stdin, content).is_ok() && stdin.flush().is_ok()
    };
    let output = hash
        .wait_with_output()
        .map_err(|e| Error::new("io", format!("Cannot read git content hash: {e}")))?;
    let hash = std::str::from_utf8(&output.stdout)
        .unwrap_or_default()
        .trim();
    if !written || !output.status.success() || !valid_hash(hash) {
        return Err(Error::new(
            "io",
            "Git could not hash the staged skill content",
        ));
    }
    Ok(hash.to_owned())
}

fn valid_hash(hash: &str) -> bool {
    hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_hexdigit())
}

fn valid_skill(name: &str) -> bool {
    name.len() <= 255
        && name
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn conflict(path: &Path) -> Error {
    Error::new(
        "conflict",
        format!(
            "Existing or locally changed skill data at {}",
            path.display()
        ),
    )
}

fn invalid_stage(message: impl Into<String>) -> Error {
    Error::new("skills_install_failed", message)
}

fn child(parent: &Directory, name: &str, create: bool) -> Result<Option<Directory>> {
    match parent.entry(name)? {
        Some(entry) if entry.is_dir => parent.child(name, false).map(Some),
        Some(_) => Err(conflict(&parent.path.join(name))),
        None if create => match parent.create_dir(name) {
            Ok(dir) => {
                parent.sync()?;
                Ok(Some(dir))
            }
            Err(e) if e.code == "collision" => parent.child(name, false).map(Some),
            Err(e) => Err(e),
        },
        None => Ok(None),
    }
}

fn read_content(
    parent: &Directory,
    name: &str,
    remaining: &mut usize,
    depth: usize,
) -> Result<Content> {
    let path = parent.path.join(name);
    if depth > 64 || *remaining < name.len() + 1 {
        return Err(Error::new(
            "unsafe_path",
            "Skill tree exceeds supported size or depth",
        ));
    }
    *remaining -= name.len() + 1;
    match parent.entry(name)? {
        Some(entry) if entry.is_dir => {
            let dir = parent.child(name, false)?;
            let mut children = BTreeMap::new();
            for name in dir.entries()? {
                let content = read_content(&dir, &name, remaining, depth + 1)?;
                children.insert(name, content);
            }
            let mode = platform::file_mode(&directory_handle(&dir)?)?;
            Ok(Content::Tree { mode, children })
        }
        Some(entry) if entry.is_file && entry.nlink == 1 => {
            let file = parent.open_file(name, false, false)?;
            let mode = platform::file_mode(&file)?;
            use std::io::Read;
            let mut bytes = Vec::new();
            file.take(*remaining as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| Error::io("read skill file", &path, e))?;
            if bytes.len() > *remaining {
                return Err(Error::new(
                    "unsafe_path",
                    "Skill tree exceeds supported size",
                ));
            }
            *remaining -= bytes.len();
            Ok(Content::File { mode, bytes })
        }
        _ => Err(conflict(&path)),
    }
}

struct LockDocument {
    bytes: Vec<u8>,
    fields: Map<String, Value>,
}

struct LockChange {
    original: Option<Vec<u8>>,
    replacement: Vec<u8>,
}

fn lock_document(root: &Directory) -> Result<Option<LockDocument>> {
    let path = root.path.join(LOCK_FILE);
    if root
        .entry(LOCK_FILE)?
        .is_some_and(|entry| !entry.is_file || entry.nlink != 1)
    {
        return Err(conflict(&path));
    }
    root.read(LOCK_FILE, false, JSON_LIMIT)?
        .map(|bytes| {
            let document: Map<String, Value> =
                serde_json::from_slice(&bytes).map_err(|_| conflict(&path))?;
            if !document.get("version").is_some_and(Value::is_u64)
                || !document.get("skills").is_some_and(Value::is_object)
            {
                return Err(conflict(&path));
            }
            Ok(LockDocument {
                bytes,
                fields: document,
            })
        })
        .transpose()
}

fn run_installer(mut command: Command, timeout: Duration) -> Result<ExitStatus> {
    let mut child = command
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| invalid_stage(format!("Cannot start pinned skills installer: {e}")))?;
    let deadline = Instant::now() + timeout;
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => (),
            Err(e) => {
                break Err(invalid_stage(format!(
                    "Cannot wait for the skills installer: {e}"
                )));
            }
        }
        if platform::cancelled() {
            break Err(Error::cancelled());
        }
        if Instant::now() >= deadline {
            break Err(invalid_stage("The skills installer exceeded its deadline"));
        }
        thread::sleep(Duration::from_millis(25));
    };
    // The installer can run Git. Stop its process group before deleting the
    // staged checkout so a child process cannot continue writing to it.
    // SAFETY: process_group(0) created a group with the spawned PID as its ID.
    unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
    let _ = child.wait();
    result
}

pub(crate) fn stage(source: &str, skills: &[String], agents: &[String]) -> Result<Staged> {
    let skills: BTreeSet<&str> = skills.iter().map(String::as_str).collect();
    let agents: BTreeSet<&str> = agents
        .iter()
        .map(|agent| match agent.as_str() {
            "claude" => "claude-code",
            agent => agent,
        })
        .collect();
    if source.is_empty()
        || source.starts_with('-')
        || source.contains(['\0', '\r', '\n'])
        || skills.is_empty()
        || skills.iter().any(|name| !valid_skill(name))
        || agents.is_empty()
        || agents
            .iter()
            .any(|agent| !matches!(*agent, "claude-code" | "codex"))
    {
        return Err(Error::new(
            "usage",
            "Select a skills source, explicit skill names and supported agents",
        ));
    }

    let local = Path::new(source);
    let source = if local.is_absolute()
        || local.exists()
        || source.starts_with("./")
        || source.starts_with("../")
    {
        let local = local
            .canonicalize()
            .map_err(|e| Error::io("resolve local skills source", local, e))?;
        platform::absolute(&local)?.to_string_lossy().into_owned()
    } else {
        source.to_owned()
    };

    let bunx =
        launch::resolve_program("bunx").map_err(|e| Error::new("skills_unavailable", e.message))?;
    let temp = Temp::new()?;
    let project = temp.0.join("project");
    let home = temp.0.join("home");
    for path in [&project, &home] {
        fs::create_dir(path).map_err(|e| Error::io("create staging directory", path, e))?;
    }
    let mut init = git();
    init.current_dir(&project)
        .args(["init", "-q", "--template="]);
    let (.., status) = launch::probe(init, "skills_unavailable", Duration::from_secs(5))?;
    if !status.success() {
        return Err(Error::new(
            "skills_unavailable",
            "Cannot initialize the staging checkout",
        ));
    }
    // Keep project-only writes and agent detection away from real profiles.
    // Bun may reuse its normal package cache.
    let mut install = Command::new(bunx);
    for (key, _) in env::vars_os() {
        if key.as_encoded_bytes().starts_with(b"GIT_")
            && !matches!(
                key.to_str(),
                Some("GIT_SSH" | "GIT_SSH_COMMAND" | "GIT_ASKPASS")
            )
        {
            install.env_remove(key);
        }
    }
    for key in [
        "CLAUDE_CONFIG_DIR",
        "CODEX_HOME",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
    ] {
        install.env_remove(key);
    }
    if env::var_os("BUN_INSTALL_CACHE_DIR").is_none()
        && let Some(home) = env::var_os("HOME")
    {
        install.env(
            "BUN_INSTALL_CACHE_DIR",
            Path::new(&home).join(".bun/install/cache"),
        );
    }
    let claude_only = !agents.contains("codex");
    install
        .current_dir(&project)
        .env("HOME", &home)
        .env("CI", "1")
        .env("DISABLE_TELEMETRY", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(["--bun", INSTALLER, "add", &source, "--skill"])
        .args(&skills)
        .arg("-a")
        .args(&agents)
        .arg("-y");
    if claude_only {
        install.arg("--copy");
    }
    let status = run_installer(install, Duration::from_secs(60))?;
    if !status.success() {
        return Err(invalid_stage(format!(
            "Pinned {INSTALLER} exited with {status}"
        )));
    }

    let project = Directory::open(&project, false)?;
    let lock = lock_document(&project)?
        .ok_or_else(|| invalid_stage("The skills installer did not produce skills-lock.json"))?
        .fields;
    let staged_entries = lock["skills"].as_object().expect("validated lock document");
    if staged_entries
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
        != skills
    {
        return Err(invalid_stage(
            "The skills installer did not stage exactly the requested skills",
        ));
    }
    let mut lock_entries = BTreeMap::new();
    let mut files = Vec::new();
    let mut remaining = TREE_LIMIT;
    for name in &skills {
        let name = *name;
        let entry = staged_entries[name]
            .as_object()
            .ok_or_else(|| invalid_stage("The skills installer produced an invalid lock entry"))?;
        let mut entry = entry.clone();
        if entry.get("sourceType").and_then(Value::as_str) == Some("local") {
            // skills resolves relative local sources against its working
            // directory. Store the original absolute source, not a temp path.
            entry.insert("source".into(), json!(source));
        }
        let copy = entry.get("installMode").and_then(Value::as_str) == Some("copy");
        lock_entries.insert(name.to_owned(), Value::Object(entry));
        let placement = Placement {
            agent: if claude_only {
                Agent::Claude
            } else {
                Agent::Canonical
            },
            name: name.to_owned(),
        };
        let parent = placement
            .parent(&project, false)?
            .ok_or_else(|| invalid_stage("Missing staged skills directory"))?;
        let content = read_content(&parent, name, &mut remaining, 0)?;
        if !matches!(&content, Content::Tree { children, .. } if matches!(children.get("SKILL.md"), Some(Content::File { .. })))
        {
            return Err(invalid_stage(format!(
                "Staged skill {name} is missing SKILL.md"
            )));
        }
        let hash = digest(&content)?;
        files.push((placement, content, hash.clone()));

        if !claude_only && agents.contains("claude-code") {
            let claude = Placement {
                agent: Agent::Claude,
                name: name.to_owned(),
            };
            let parent = claude
                .parent(&project, false)?
                .ok_or_else(|| invalid_stage("Missing staged Claude skills directory"))?;
            let link = claude.claude_link();
            let (content, hash) = if copy {
                let content = read_content(&parent, name, &mut remaining, 0)?;
                if digest(&content)? != hash {
                    return Err(invalid_stage(format!(
                        "Staged Claude copy differs for skill {name}"
                    )));
                }
                (content, hash)
            } else {
                if parent.read_link(name)?.as_ref() != Some(&link) {
                    return Err(invalid_stage(format!(
                        "Unexpected Claude link for staged skill {name}"
                    )));
                }
                let content = Content::Link(link);
                let hash = digest(&content)?;
                (content, hash)
            };
            files.push((claude, content, hash));
        }
    }
    files.sort_by(|(left, _, _), (right, _, _)| left.cmp(right));
    let scope = digest(&(&source, &skills, &agents))?;
    let fingerprints: Vec<_> = files.iter().map(|(path, _, hash)| (path, hash)).collect();
    let lock_version = lock["version"].clone();
    let revision = digest(&(&scope, &fingerprints, &lock_version, &lock_entries))?;
    Ok(Staged {
        revision,
        scope,
        source,
        files,
        lock_version,
        lock_entries,
    })
}

impl Checkout {
    fn open(root: &Path) -> Result<Self> {
        let root = Directory::open(&platform::absolute(root)?, false)?;
        let dot_git = root.path.join(".git");
        let invalid = || {
            Error::new(
                "invalid_project",
                format!("Expected a Git worktree root at {}", root.path.display()),
            )
        };
        let marker = root.entry(".git")?.ok_or_else(invalid)?;
        if !marker.is_dir && !marker.is_file {
            return Err(invalid());
        }
        let marker_contents = if marker.is_file {
            root.read(".git", false, 4096)?
        } else {
            None
        };
        let mut command = git();
        command.current_dir(&root.path).args([
            "rev-parse",
            "--show-toplevel",
            "--absolute-git-dir",
            "--is-inside-work-tree",
        ]);
        let (bytes, status) = launch::probe(command, "invalid_project", Duration::from_secs(5))?;
        let mut lines = std::str::from_utf8(&bytes).map_err(|_| invalid())?.lines();
        let Some((top, gitdir, "true")) = lines
            .next()
            .zip(lines.next())
            .zip(lines.next())
            .map(|((top, gitdir), inside)| (top, gitdir, inside))
        else {
            return Err(invalid());
        };
        if !status.success() || lines.next().is_some() || Path::new(top) != root.path {
            return Err(invalid());
        }
        let gitdir = Directory::open(Path::new(gitdir), false)?;
        if marker.is_dir {
            if gitdir.path != dot_git || gitdir.identity()? != marker.identity {
                return Err(invalid());
            }
        } else if gitdir.entry("commondir")?.is_some() {
            let backlink = gitdir.read("gitdir", false, 4096)?.ok_or_else(invalid)?;
            let backlink = std::str::from_utf8(&backlink)
                .map_err(|_| invalid())?
                .trim_end_matches(['\r', '\n']);
            if platform::absolute(&gitdir.path.join(backlink))? != dot_git {
                return Err(invalid());
            }
        }
        if root.entry(".git")?.map(|entry| entry.identity) != Some(marker.identity.clone()) {
            return Err(invalid());
        }
        let state = private_child(&gitdir, METADATA)?;
        let lock = match state.create_file("lock", 0o600) {
            Err(e) if e.code == "collision" => state.open_file("lock", true, true),
            result => result,
        }?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                    if platform::cancelled() {
                        return Err(Error::cancelled());
                    }
                    thread::sleep(Duration::from_millis(25));
                }
                Err(e) => {
                    return Err(Error::new(
                        "busy",
                        format!("Cannot lock {}: {e}", state.path.display()),
                    ));
                }
            }
        }
        if root.entry(".git")?.map(|entry| entry.identity) != Some(marker.identity)
            || (marker.is_file && root.read(".git", false, 4096)? != marker_contents)
            || state.entry("lock")?.map(|entry| entry.identity)
                != Some(platform::file_identity(&lock)?)
        {
            return Err(invalid());
        }
        let device = |dir: &Directory| -> Result<_> {
            directory_handle(dir)?
                .metadata()
                .map(|metadata| metadata.dev())
                .map_err(|e| Error::io("inspect directory filesystem", &dir.path, e))
        };
        if device(&root)? != device(&state)? {
            return Err(Error::new(
                "unsafe_path",
                "Skill checkout and Git metadata must share a filesystem",
            ));
        }
        Ok(Self {
            root,
            state,
            _lock: lock,
        })
    }

    fn scope(&self, key: &str) -> Result<Directory> {
        if !valid_hash(key) {
            return Err(Error::new(
                "ownership",
                "Invalid managed skills receipt scope",
            ));
        }
        private_child(&self.state, key)
    }

    fn verify_receipt(&self, receipt: &Receipt, key: &str) -> Result<()> {
        let mut paths = BTreeSet::new();
        if receipt.checkout != self.root.identity()?
            || receipt.scope != key
            || !valid_hash(&receipt.revision)
            || !receipt.lock_version.is_u64()
            || receipt.files.is_empty()
            || receipt.files.iter().any(|owned| {
                !valid_skill(&owned.placement.name)
                    || !receipt.lock_entries.contains_key(&owned.placement.name)
                    || !valid_hash(&owned.digest)
                    || !paths.insert(&owned.placement)
            })
            || receipt.lock_entries.iter().any(|(name, entry)| {
                !entry.is_object()
                    || !receipt
                        .files
                        .iter()
                        .any(|owned| owned.placement.name == *name)
            })
        {
            return Err(Error::new(
                "ownership",
                "Invalid or foreign managed skills receipt",
            ));
        }
        Ok(())
    }
}

fn private_child(parent: &Directory, name: &str) -> Result<Directory> {
    if parent.entry(name)?.is_none() {
        match parent.create_dir(name) {
            Ok(dir) => {
                parent.sync()?;
                return Ok(dir);
            }
            Err(e) if e.code == "collision" => (),
            Err(e) => return Err(e),
        }
    }
    parent.child(name, true)
}

fn directory_handle(dir: &Directory) -> Result<File> {
    let file = sys::open(
        &dir.path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(|e| Error::io("open verified directory", &dir.path, e.into()))?;
    if platform::file_identity(&file)? != dir.identity()? {
        return Err(conflict(&dir.path));
    }
    Ok(file)
}

fn rename(
    source: &Directory,
    name: &str,
    target: &Directory,
    new_name: &str,
    flags: RenameFlags,
) -> Result<()> {
    let source_fd = directory_handle(source)?;
    let target_fd = directory_handle(target)?;
    sys::renameat_with(&source_fd, name, &target_fd, new_name, flags).map_err(|e| {
        let path = target.path.join(new_name);
        if e == rustix::io::Errno::EXIST {
            conflict(&path)
        } else {
            Error::io("move skill data", &path, e.into())
        }
    })?;
    target.sync()?;
    source.sync()
}

fn matches_owned(parent: &Directory, name: &str, owned: &Owned) -> Result<bool> {
    let Some(entry) = parent.entry(name)? else {
        return Ok(false);
    };
    if entry.identity != owned.identity {
        return Ok(false);
    }
    let mut remaining = TREE_LIMIT;
    let content = match owned.placement.agent {
        _ if entry.is_dir => read_content(parent, name, &mut remaining, 0)?,
        Agent::Claude if entry.is_link => {
            let Some(target) = parent.read_link(name)? else {
                return Ok(false);
            };
            Content::Link(target)
        }
        _ => return Ok(false),
    };
    Ok(digest(&content)? == owned.digest)
}

fn write_content(parent: &Directory, name: &str, content: &Content) -> Result<()> {
    match content {
        Content::Tree { mode, children } => {
            let dir = parent.create_dir(name)?;
            for (name, content) in children {
                write_content(&dir, name, content)?;
            }
            platform::set_file_mode(&directory_handle(&dir)?, *mode)?;
            dir.sync()
        }
        Content::File { mode, bytes } => {
            let mut file = parent.create_file(name, 0o600)?;
            file.write_all(bytes)
                .map_err(|e| Error::io("write staged skill", &parent.path.join(name), e))?;
            platform::set_file_mode(&file, *mode)?;
            file.sync_all()
                .map_err(|e| Error::io("flush staged skill", &parent.path.join(name), e))
        }
        Content::Link(target) => sys::symlinkat(target, &directory_handle(parent)?, name)
            .map_err(|e| Error::io("stage Claude skill link", &parent.path.join(name), e.into())),
    }
}

fn get_transaction(
    scope: &Directory,
    checkout: &Checkout,
    key: &str,
) -> Result<Option<Transaction>> {
    let Some(bytes) = scope.read(RECORD, true, JSON_LIMIT)? else {
        return Ok(None);
    };
    let record: Transaction = serde_json::from_slice(&bytes)
        .map_err(|_| Error::new("ownership", "Invalid managed skills transaction"))?;
    if record.version != 1
        || !record
            .workspace
            .strip_prefix("work-")
            .is_some_and(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
        || (record.before.is_none() && record.after.is_none())
    {
        return Err(Error::new(
            "ownership",
            "Invalid managed skills transaction",
        ));
    }
    for receipt in record.before.iter().chain(&record.after) {
        checkout.verify_receipt(receipt, key)?;
    }
    Ok(Some(record))
}

fn save_transaction(scope: &Directory, record: &Transaction) -> Result<()> {
    let bytes = serde_json::to_vec(record)
        .map_err(|e| Error::new("io", format!("Cannot encode skills transaction: {e}")))?;
    if bytes.len() > JSON_LIMIT {
        return Err(Error::new(
            "io",
            "Skills transaction exceeds supported size",
        ));
    }
    let temp = format!("record-{}", platform::random_id()?);
    scope.write_new(&temp, &bytes, 0o600)?;
    scope.rename(&temp, scope, RECORD)?;
    scope.sync()
}

fn clean_workspace(scope: &Directory, name: &str) -> Result<()> {
    if scope.entry(name)?.is_some() {
        let dir = scope.child(name, true)?;
        dir.purge_children(".roost-unused")?;
        scope.remove(name, true)?;
        scope.sync()?;
    }
    Ok(())
}

fn clean_transaction_workspace(scope: &Directory, record: &Transaction) -> Result<()> {
    if scope.entry(&record.workspace)?.is_none() {
        return Ok(());
    }
    let work = scope.child(&record.workspace, true)?;
    for (index, placement) in placements(record.before.as_ref(), record.after.as_ref())
        .into_iter()
        .enumerate()
    {
        let saved = format!("old-{index}");
        if let Some(old) = opposite(record.before.as_ref(), &placement)
            && work.entry(&saved)?.is_some()
            && !matches_owned(&work, &saved, old)?
        {
            return Err(conflict(&work.path.join(saved)));
        }
    }
    for name in work.entries()? {
        if let Some(hashes) = name.strip_prefix("lock-") {
            let bytes = work
                .read(&name, false, JSON_LIMIT)
                .ok()
                .flatten()
                .ok_or_else(|| conflict(&work.path.join(&name)))?;
            let hash = digest(&bytes)?;
            if !hashes.split('-').take(2).any(|expected| expected == hash) {
                return Err(conflict(&work.path.join(&name)));
            }
        }
    }
    clean_workspace(scope, &record.workspace)
}

fn opposite<'a>(receipt: Option<&'a Receipt>, placement: &Placement) -> Option<&'a Owned> {
    receipt?
        .files
        .iter()
        .find(|owned| owned.placement == *placement)
}

fn placements(before: Option<&Receipt>, after: Option<&Receipt>) -> BTreeSet<Placement> {
    before
        .into_iter()
        .chain(after)
        .flat_map(|receipt| receipt.files.iter().map(|owned| owned.placement.clone()))
        .collect()
}

fn check_owned(root: &Directory, receipt: Option<&Receipt>, allow_missing: bool) -> Result<()> {
    for owned in receipt.into_iter().flat_map(|receipt| &receipt.files) {
        let placement = &owned.placement;
        let Some(parent) = placement.parent(root, false)? else {
            if allow_missing {
                continue;
            }
            return Err(conflict(&root.path.join(&placement.name)));
        };
        if !(matches_owned(&parent, &placement.name, owned)?
            || allow_missing && parent.entry(&placement.name)?.is_none())
        {
            return Err(conflict(&parent.path.join(&placement.name)));
        }
    }
    Ok(())
}

fn lock_changes(
    root: &Directory,
    before: Option<&Receipt>,
    after: Option<&Receipt>,
    retry: bool,
) -> Result<Option<LockChange>> {
    let path = root.path.join(LOCK_FILE);
    let Some(version) = after.or(before).map(|receipt| &receipt.lock_version) else {
        return Ok(None);
    };
    let existing = lock_document(root)?;
    let (original, mut document) = match existing {
        Some(LockDocument { bytes, fields }) => (Some(bytes), fields),
        None => (
            None,
            Map::from_iter([
                ("version".into(), version.clone()),
                ("skills".into(), json!({})),
            ]),
        ),
    };
    if document["version"] != *version {
        return Err(conflict(&path));
    }
    let entries = document
        .get_mut("skills")
        .and_then(Value::as_object_mut)
        .expect("validated lock document");
    let expected = before.map(|receipt| &receipt.lock_entries);
    let desired = after.map(|receipt| &receipt.lock_entries);
    let names: BTreeSet<_> = expected
        .into_iter()
        .chain(desired)
        .flat_map(BTreeMap::keys)
        .collect();
    let mut differs = false;
    for name in names {
        let old = expected.and_then(|entries| entries.get(name));
        let new = desired.and_then(|entries| entries.get(name));
        let current = entries.get(name);
        if current != old && !(retry && current == new) {
            return Err(conflict(&path));
        }
        if current != new {
            differs = true;
            if let Some(new) = new {
                entries.insert(name.clone(), new.clone());
            } else {
                entries.remove(name);
            }
        }
    }
    if differs {
        let mut bytes = serde_json::to_vec_pretty(&document)
            .map_err(|e| Error::new("io", format!("Cannot encode skills lock: {e}")))?;
        bytes.push(b'\n');
        if bytes.len() > JSON_LIMIT {
            return Err(Error::new("io", "Skills lock exceeds supported size"));
        }
        Ok(Some(LockChange {
            original,
            replacement: bytes,
        }))
    } else {
        Ok(None)
    }
}

fn publish_lock(root: &Directory, work: &Directory, change: LockChange) -> Result<()> {
    let LockChange {
        original,
        replacement: bytes,
    } = change;
    let before_digest = original.as_ref().map(digest).transpose()?;
    let temp = format!(
        "lock-{}-{}-{}",
        before_digest.as_deref().unwrap_or("absent"),
        digest(&bytes)?,
        platform::random_id()?
    );
    let mode = match &original {
        Some(_) => platform::file_mode(&root.open_file(LOCK_FILE, false, false)?)?,
        None => 0o644,
    };
    write_content(
        work,
        &temp,
        &Content::File {
            mode,
            bytes: bytes.clone(),
        },
    )?;
    work.sync()?;
    if let Some(original) = original {
        rename(work, &temp, root, LOCK_FILE, RenameFlags::EXCHANGE)?;
        if work.read(&temp, false, JSON_LIMIT).ok().flatten().as_ref() != Some(&original) {
            // A writer outside Roost replaced the lock while we were preparing
            // the merge. Put it back if our replacement is still in place.
            if root
                .read(LOCK_FILE, false, JSON_LIMIT)
                .ok()
                .flatten()
                .as_ref()
                == Some(&bytes)
            {
                rename(work, &temp, root, LOCK_FILE, RenameFlags::EXCHANGE)?;
            }
            return Err(conflict(&root.path.join(LOCK_FILE)));
        }
    } else {
        rename(work, &temp, root, LOCK_FILE, RenameFlags::NOREPLACE)?;
    }
    Ok(())
}

fn finish(root: &Directory, scope: &Directory, mut record: Transaction) -> Result<Option<Receipt>> {
    let before = record.before.as_ref();
    let after = record.after.as_ref();
    let work = scope.child(&record.workspace, true)?;

    // Recheck the lock before changing any skill paths. Other disjoint
    // installs can add entries between an interrupted write and its retry.
    lock_changes(root, before, after, true)?;
    for (index, placement) in placements(before, after).into_iter().enumerate() {
        let old = opposite(before, &placement);
        let new = opposite(after, &placement);
        let Some(parent) = placement.parent(root, new.is_some())? else {
            continue;
        };
        let name = &placement.name;
        let saved = format!("old-{index}");
        let staged = format!("new-{index}");
        if let Some(new) = new
            && matches_owned(&parent, name, new)?
        {
            continue;
        }
        if parent.entry(name)?.is_some() {
            let old = old.ok_or_else(|| conflict(&parent.path.join(name)))?;
            if !matches_owned(&parent, name, old)? {
                return Err(conflict(&parent.path.join(name)));
            }
            rename(&parent, name, &work, &saved, RenameFlags::NOREPLACE)?;
            if !matches_owned(&work, &saved, old).unwrap_or(false) {
                rename(&work, &saved, &parent, name, RenameFlags::NOREPLACE)?;
                return Err(conflict(&parent.path.join(name)));
            }
        } else if let Some(old) = old
            && new.is_some()
            && !matches_owned(&work, &saved, old)?
        {
            return Err(conflict(&parent.path.join(name)));
        }
        if let Some(new) = new {
            if !matches_owned(&work, &staged, new)? {
                return Err(conflict(&parent.path.join(name)));
            }
            rename(&work, &staged, &parent, name, RenameFlags::NOREPLACE)?;
        }
    }
    if let Some(change) = lock_changes(root, before, after, true)? {
        publish_lock(root, &work, change)?;
    }

    // Retain the completed transition until the caller can commit its receipt.
    // A retry can then recognize exactly what was promoted.
    record.complete = true;
    save_transaction(scope, &record)?;
    clean_transaction_workspace(scope, &record)?;
    Ok(record.after)
}

pub(crate) fn apply(root: &Path, stage: &Staged, prior: Option<&Receipt>) -> Result<Receipt> {
    let checkout = Checkout::open(root)?;
    let root = &checkout.root;
    let scope = checkout.scope(&stage.scope)?;
    if let Some(prior) = prior {
        checkout.verify_receipt(prior, &stage.scope)?;
    }
    if let Some(record) = get_transaction(&scope, &checkout, &stage.scope)? {
        if record
            .after
            .as_ref()
            .is_some_and(|after| after.revision == stage.revision)
            && (prior == record.before.as_ref() || prior == record.after.as_ref())
        {
            if record.complete {
                check_owned(root, record.after.as_ref(), false)?;
                lock_changes(root, record.after.as_ref(), record.after.as_ref(), false)?;
                clean_transaction_workspace(&scope, &record)?;
                return Ok(record.after.expect("matching after receipt"));
            }
            return finish(root, &scope, record).map(|receipt| receipt.expect("apply transaction"));
        }
        if !record.complete || prior != record.after.as_ref() {
            return Err(conflict(&scope.path));
        }
        clean_transaction_workspace(&scope, &record)?;
    }
    check_owned(root, prior, false)?;

    let record = prepare(root, &scope, stage, prior)?;
    finish(root, &scope, record).map(|receipt| receipt.expect("apply transaction"))
}

fn prepare(
    root: &Directory,
    scope: &Directory,
    stage: &Staged,
    prior: Option<&Receipt>,
) -> Result<Transaction> {
    let workspace = format!("work-{}", platform::random_id()?);
    let work = scope.create_dir(&workspace)?;
    let prepared = (|| {
        let mut after = Receipt {
            revision: stage.revision.clone(),
            scope: stage.scope.clone(),
            checkout: root.identity()?,
            source: stage.source.clone(),
            files: Vec::new(),
            lock_version: stage.lock_version.clone(),
            lock_entries: stage.lock_entries.clone(),
        };
        for (index, (placement, content, digest)) in stage.files.iter().enumerate() {
            let old = opposite(prior, placement);
            if old.is_none()
                && let Some(parent) = placement.parent(root, false)?
                && parent.entry(&placement.name)?.is_some()
            {
                return Err(conflict(&parent.path.join(&placement.name)));
            }
            if let Some(old) = old
                && old.digest == *digest
            {
                after.files.push(old.clone());
                continue;
            }
            let name = format!("new-{index}");
            write_content(&work, &name, content)?;
            let identity = work
                .entry(&name)?
                .ok_or_else(|| conflict(&work.path.join(&name)))?
                .identity;
            after.files.push(Owned {
                placement: placement.clone(),
                digest: digest.clone(),
                identity,
            });
        }
        lock_changes(root, prior, Some(&after), false)?;
        work.sync()?;
        Ok(Transaction {
            version: 1,
            before: prior.cloned(),
            after: Some(after),
            workspace: workspace.clone(),
            complete: false,
        })
    })();
    let record = match prepared {
        Ok(record) => record,
        Err(error) => {
            let _ = clean_workspace(scope, &workspace);
            return Err(error);
        }
    };
    save_transaction(scope, &record)?;
    Ok(record)
}

pub(crate) fn remove(root: &Path, receipt: &Receipt) -> Result<()> {
    let checkout = Checkout::open(root)?;
    let root = &checkout.root;
    checkout.verify_receipt(receipt, &receipt.scope)?;
    let scope = checkout.scope(&receipt.scope)?;
    if let Some(record) = get_transaction(&scope, &checkout, &receipt.scope)? {
        if record.before.as_ref() == Some(receipt) && record.after.is_none() {
            if record.complete {
                return clean_transaction_workspace(&scope, &record);
            }
            return finish(root, &scope, record).map(|_| ());
        }
        if !record.complete || record.after.as_ref() != Some(receipt) {
            return Err(conflict(&scope.path));
        }
        clean_transaction_workspace(&scope, &record)?;
    }
    check_owned(root, Some(receipt), true)?;
    lock_changes(root, Some(receipt), None, true)?;
    let workspace = format!("work-{}", platform::random_id()?);
    scope.create_dir(&workspace)?;
    let record = Transaction {
        version: 1,
        before: Some(receipt.clone()),
        after: None,
        workspace,
        complete: false,
    };
    save_transaction(&scope, &record)?;
    finish(root, &scope, record).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn repo() -> Temp {
        let repo = Temp::new().unwrap();
        assert!(
            git()
                .current_dir(&repo.0)
                .args(["init", "-q", "--template="])
                .status()
                .unwrap()
                .success()
        );
        repo
    }

    fn bundle(body: &str) -> Staged {
        let mut files = Vec::new();
        let mut lock_entries = BTreeMap::new();
        for name in ["alpha", "beta"] {
            let content = Content::Tree {
                mode: 0o755,
                children: BTreeMap::from([(
                    "SKILL.md".into(),
                    Content::File {
                        mode: 0o644,
                        bytes: format!("{body} {name}").into_bytes(),
                    },
                )]),
            };
            let hash = digest(&content).unwrap();
            files.push((
                Placement {
                    agent: Agent::Canonical,
                    name: name.into(),
                },
                content,
                hash,
            ));
            lock_entries.insert(name.into(), json!({"source":"owner/repo","ref":body}));
            let location = Placement {
                agent: Agent::Claude,
                name: name.into(),
            };
            let content = Content::Link(location.claude_link());
            let hash = digest(&content).unwrap();
            files.push((location, content, hash));
        }
        files.sort_by(|(left, _, _), (right, _, _)| left.cmp(right));
        Staged {
            revision: digest(&body).unwrap(),
            scope: digest(&"scope").unwrap(),
            source: "owner/repo".into(),
            files,
            lock_entries,
            lock_version: json!(2),
        }
    }

    fn installed(root: &Path, skill: &str) -> PathBuf {
        root.join(".agents/skills").join(skill).join("SKILL.md")
    }

    fn lock(root: &Path) -> Value {
        serde_json::from_slice(&fs::read(root.join(LOCK_FILE)).unwrap()).unwrap()
    }

    #[test]
    fn apply_update_remove_and_retry_preserve_unrelated_data() {
        let repo = repo();
        let root = &repo.0;
        let foreign = root.join(".agents/skills/unrelated");
        fs::create_dir_all(&foreign).unwrap();
        fs::write(foreign.join("SKILL.md"), "unrelated").unwrap();
        fs::write(root.join(LOCK_FILE), r#"{"version":2,"extension":{"flag":true},"skills":{"foreign":{"source":"elsewhere"}}}"#).unwrap();
        let v1 = bundle("v1");
        let first = apply(root, &v1, None).unwrap();
        assert_eq!(first.revision, v1.revision());
        assert_eq!(first, apply(root, &v1, None).unwrap());
        assert_eq!(first, apply(root, &v1, Some(&first)).unwrap());
        assert_eq!(
            fs::read_link(root.join(".claude/skills/alpha")).unwrap(),
            Path::new("../../.agents/skills/alpha")
        );
        let v2 = bundle("v2");
        let second = apply(root, &v2, Some(&first)).unwrap();
        assert_eq!(second, apply(root, &v2, Some(&first)).unwrap());
        assert_eq!(
            fs::read_to_string(installed(root, "alpha")).unwrap(),
            "v2 alpha"
        );
        assert_eq!(lock(root)["skills"]["alpha"]["ref"], "v2");
        remove(root, &second).unwrap();
        remove(root, &second).unwrap();
        assert!(!installed(root, "alpha").exists());
        assert!(!root.join(".claude/skills/alpha").is_symlink());
        assert!(foreign.join("SKILL.md").is_file());
        assert_eq!(
            lock(root),
            json!({"version":2,"extension":{"flag":true},"skills":{"foreign":{"source":"elsewhere"}}})
        );
    }

    #[test]
    fn protects_preexisting_and_edited_skills_and_lock_entries() {
        let repo = repo();
        let root = &repo.0;
        let v1 = bundle("v1");
        let target = installed(root, "alpha");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "v1 alpha").unwrap();
        assert_eq!(apply(root, &v1, None).unwrap_err().code, "conflict");
        fs::remove_dir_all(target.parent().unwrap()).unwrap();

        let first = apply(root, &v1, None).unwrap();
        fs::write(&target, "local edit").unwrap();
        assert_eq!(
            apply(root, &bundle("v2"), Some(&first)).unwrap_err().code,
            "conflict"
        );
        assert_eq!(remove(root, &first).unwrap_err().code, "conflict");
        assert_eq!(fs::read_to_string(&target).unwrap(), "local edit");
        fs::write(&target, "v1 alpha").unwrap();

        let lock_path = root.join(LOCK_FILE);
        let mut local = lock(root);
        local["skills"]["beta"]["extension"] = json!(true);
        fs::write(&lock_path, serde_json::to_vec(&local).unwrap()).unwrap();
        assert_eq!(
            apply(root, &bundle("v2"), Some(&first)).unwrap_err().code,
            "conflict"
        );
        assert_eq!(remove(root, &first).unwrap_err().code, "conflict");
        assert_eq!(lock(root), local);

        fs::remove_file(&target).unwrap();
        symlink(&lock_path, &target).unwrap();
        assert_eq!(remove(root, &first).unwrap_err().code, "conflict");
        assert_eq!(fs::read_link(&target).unwrap(), lock_path);
    }

    #[test]
    fn resumes_after_only_one_skill_has_been_published() {
        let repo = repo();
        let root = &repo.0;
        let staged = bundle("v1");
        {
            let checkout = Checkout::open(root).unwrap();
            let scope = checkout.scope(&staged.scope).unwrap();
            let record = prepare(&checkout.root, &scope, &staged, None).unwrap();
            let work = scope.child(&record.workspace, true).unwrap();
            let placement = &staged.files[0].0;
            let parent = placement.parent(&checkout.root, true).unwrap().unwrap();
            rename(
                &work,
                "new-0",
                &parent,
                &placement.name,
                RenameFlags::NOREPLACE,
            )
            .unwrap();
        }
        let receipt = apply(root, &staged, None).unwrap();
        assert_eq!(
            fs::read_to_string(installed(root, "beta")).unwrap(),
            "v1 beta"
        );
        assert_eq!(receipt, apply(root, &staged, None).unwrap());
    }

    #[test]
    fn verifies_linked_worktree_and_rejects_a_copied_git_pointer() {
        let repo = repo();
        assert!(
            git()
                .current_dir(&repo.0)
                .args([
                    "-c",
                    "user.email=fixture@example.test",
                    "-c",
                    "user.name=Fixture",
                    "commit",
                    "--allow-empty",
                    "-qm",
                    "initial"
                ])
                .status()
                .unwrap()
                .success()
        );
        let targets = Temp::new().unwrap();
        let linked = targets.0.join("linked");
        assert!(
            git()
                .current_dir(&repo.0)
                .args(["worktree", "add", "--detach", "-q"])
                .arg(&linked)
                .status()
                .unwrap()
                .success()
        );
        let copy = targets.0.join("copied");
        fs::create_dir(&copy).unwrap();
        fs::copy(linked.join(".git"), copy.join(".git")).unwrap();
        let staged = bundle("v1");
        let first = apply(&linked, &staged, None).unwrap();
        assert_eq!(
            apply(&copy, &staged, None).unwrap_err().code,
            "invalid_project"
        );
        assert_eq!(remove(&repo.0, &first).unwrap_err().code, "ownership");
        assert!(!installed(&repo.0, "alpha").exists());
        remove(&linked, &first).unwrap();
    }

    #[test]
    fn installer_times_out() {
        let mut installer = Command::new("sh");
        installer.args(["-c", "sleep 10 & wait"]);
        let error = run_installer(installer, Duration::from_millis(30)).unwrap_err();
        assert!(error.message.contains("exceeded its deadline"));
    }

    #[test]
    #[ignore = "requires the pinned bunx skills package"]
    fn pinned_installer_stages_both_layouts_and_stable_revisions() {
        let source = Temp::new().unwrap();
        for name in ["alpha", "beta"] {
            let skill = source.0.join("skills").join(name);
            fs::create_dir_all(&skill).unwrap();
            fs::write(
                skill.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: Fixture skill.\n---\n{name}"),
            )
            .unwrap();
        }
        let names = ["alpha".into(), "beta".into()];
        let repo = repo();
        let source = source.0.to_str().unwrap();
        let both = ["claude".into(), "codex".into()];
        let first = stage(source, &names, &both).unwrap();
        assert_eq!(
            first.revision(),
            stage(source, &names, &both).unwrap().revision()
        );
        let receipt = apply(&repo.0, &first, None).unwrap();
        assert!(installed(&repo.0, "beta").is_file());
        assert!(repo.0.join(".claude/skills/beta").is_symlink());
        assert_eq!(lock(&repo.0)["skills"]["beta"]["source"], source);
        remove(&repo.0, &receipt).unwrap();

        let claude = stage(source, &names, &["claude".into()]).unwrap();
        let receipt = apply(&repo.0, &claude, None).unwrap();
        assert!(!installed(&repo.0, "beta").exists());
        assert!(repo.0.join(".claude/skills/beta/SKILL.md").is_file());
        assert_eq!(lock(&repo.0)["skills"]["beta"]["installMode"], "copy");
        remove(&repo.0, &receipt).unwrap();
    }
}
