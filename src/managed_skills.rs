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
    cell::Cell,
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
const BUNDLE: &str = "bundle.json";
const JSON_LIMIT: usize = 4 * 1024 * 1024;
const TREE_LIMIT: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct Receipt {
    pub revision: String,
    scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner: Option<String>,
    checkout: FileIdentity,
    source: String,
    files: Vec<Owned>,
    lock_version: Value,
    lock_entries: BTreeMap<String, Value>,
}

impl Receipt {
    pub(crate) fn verify_owner(&self, root_id: &str) -> Result<()> {
        if !valid_root_id(root_id) || self.owner.as_deref() != Some(root_id) {
            return Err(Error::new(
                "ownership",
                "Managed skills receipt belongs to a different Roost root",
            ));
        }
        Ok(())
    }
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
        self.agent.skills_dir(root, create)
    }

    fn claude_link(&self) -> PathBuf {
        Path::new("../../.agents/skills").join(&self.name)
    }
}

impl Agent {
    fn skills_dir(self, root: &Directory, create: bool) -> Result<Option<Directory>> {
        let dir = match self {
            Self::Canonical => ".agents",
            Self::Claude => ".claude",
        };
        let Some(dir) = child(root, dir, create)? else {
            return Ok(None);
        };
        child(&dir, "skills", create)
    }
}

pub(crate) struct Staged {
    revision: String,
    bundle: Bundle,
    files: Vec<(Placement, Content, String)>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    version: u32,
    source: String,
    skills: BTreeSet<String>,
    agents: BTreeSet<String>,
    owner: Option<String>,
    lock_version: Value,
    lock_entries: BTreeMap<String, Value>,
}

impl Staged {
    pub(crate) fn revision(&self) -> &str {
        &self.revision
    }

    pub(crate) fn bind_owner(&mut self, root_id: &str) -> Result<()> {
        if !valid_root_id(root_id) || self.bundle.owner.as_deref().is_some_and(|id| id != root_id) {
            return Err(Error::new(
                "ownership",
                "Invalid or foreign staged skills owner",
            ));
        }
        self.bundle.owner = Some(root_id.to_owned());
        Ok(())
    }

    fn scope(&self) -> Result<String> {
        let scope = self.bundle.scope()?;
        match &self.bundle.owner {
            Some(owner) => digest(&(owner, scope)),
            None => Ok(scope),
        }
    }

    pub(crate) fn save(&self, cache: &Directory) -> Result<()> {
        verify_cache(cache)?;
        if self.bundle.owner.is_none() {
            return Err(Error::new(
                "ownership",
                "Staged skills have no Roost root owner",
            ));
        }
        let reuse = || {
            if load(cache, self.revision())?.bundle.owner != self.bundle.owner {
                return Err(Error::new(
                    "ownership",
                    "Cached skills belong to a different Roost root",
                ));
            }
            cache.sync()
        };
        if cache.entry(self.revision())?.is_some() {
            return reuse();
        }
        let manifest = serde_json::to_vec(&self.bundle)
            .map_err(|e| Error::new("io", format!("Cannot encode staged skills bundle: {e}")))?;
        if manifest.len() > JSON_LIMIT {
            return Err(invalid_stage(
                "Staged skills bundle metadata exceeds supported size",
            ));
        }
        // Keep the manifest and portable tree under one private directory.
        // Publishing that directory makes the complete revision visible at once.
        let name = format!("bundle-{}", platform::random_id()?);
        let prepared = cache.create_dir(&name)?;
        let result = (|| {
            prepared.write_new(BUNDLE, &manifest, 0o600)?;
            for (placement, content, _) in &self.files {
                let parent = placement
                    .parent(&prepared, true)?
                    .expect("create skills directory");
                write_content(&parent, &placement.name, content)?;
                parent.sync()?;
            }
            prepared.sync()?;
            match cache.publish_new_entry(&name, prepared.identity()?, self.revision()) {
                Ok(()) => cache.sync(),
                Err(e) if e.code == "collision" => reuse(),
                Err(e) => Err(e),
            }
        })();
        let cleanup = clean_workspace(cache, &name);
        result.and(cleanup)
    }
}

impl Bundle {
    fn scope(&self) -> Result<String> {
        digest(&(&self.source, &self.skills, &self.agents))
    }
}

fn verify_cache(cache: &Directory) -> Result<()> {
    let current = Directory::open(&cache.path, true)?;
    if current.identity()? != cache.identity()? {
        return Err(conflict(&cache.path));
    }
    Ok(())
}

pub(crate) fn load(cache: &Directory, revision: &str) -> Result<Staged> {
    if !valid_hash(revision) {
        return Err(invalid_stage("Invalid staged skills revision"));
    }
    verify_cache(cache)?;
    if cache.entry(revision)?.is_none() {
        return Err(Error::new(
            "skills_unavailable",
            "Selected staged skills revision is not cached",
        ));
    }
    let directory = cache.child(revision, true)?;
    let bytes = directory
        .read(BUNDLE, true, JSON_LIMIT)?
        .ok_or_else(|| invalid_stage("Cached skills bundle has no manifest"))?;
    let bundle: Bundle = serde_json::from_slice(&bytes)
        .map_err(|_| invalid_stage("Invalid cached skills bundle manifest"))?;
    if !bundle.owner.as_deref().is_some_and(valid_root_id) {
        return Err(invalid_stage("Invalid cached skills bundle owner"));
    }
    let stage = read_bundle(&directory, bundle)?;
    if stage.revision() != revision {
        return Err(invalid_stage(
            "Cached skills bundle does not match the selected revision",
        ));
    }
    Ok(stage)
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

fn valid_root_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
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
            let budget = Cell::new(*remaining);
            let names = dir.matching_entries(|name| {
                let name = name
                    .ok_or_else(|| Error::new("unsafe_path", "Skill file name is not Unicode"))?;
                platform::valid_name(name)?;
                let left = budget.get().checked_sub(name.len() + 1).ok_or_else(|| {
                    Error::new("unsafe_path", "Skill tree exceeds supported size")
                })?;
                budget.set(left);
                Ok(true)
            })?;
            for name in names {
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

fn valid_source(source: &str) -> bool {
    !source.is_empty() && !source.starts_with('-') && !source.contains(['\0', '\r', '\n'])
}

fn valid_selection(skills: &BTreeSet<String>, agents: &BTreeSet<String>) -> bool {
    !skills.is_empty()
        && (skills.len() == 1 && skills.contains("*")
            || skills.iter().all(|name| valid_skill(name)))
        && !agents.is_empty()
        && agents
            .iter()
            .all(|agent| matches!(agent.as_str(), "claude-code" | "codex"))
}

fn normalize_source(source: &str) -> Result<String> {
    let local = Path::new(source);
    if local.is_absolute() || source.starts_with("./") || source.starts_with("../") {
        let local = local
            .canonicalize()
            .map_err(|e| Error::io("resolve local skills source", local, e))?;
        Ok(platform::absolute(&local)?.to_string_lossy().into_owned())
    } else {
        Ok(source.to_owned())
    }
}

pub(crate) fn stage(source: &str, skills: &[String], agents: &[String]) -> Result<Staged> {
    let wildcard = skills.len() == 1 && skills[0] == "*";
    let skills: BTreeSet<String> = skills.iter().cloned().collect();
    let agents: BTreeSet<String> = agents
        .iter()
        .map(|agent| match agent.as_str() {
            "claude" => "claude-code".to_owned(),
            _ => agent.clone(),
        })
        .collect();
    if !valid_source(source)
        || !valid_selection(&skills, &agents)
        || skills.contains("*") && !wildcard
    {
        return Err(Error::new(
            "usage",
            "Select a skills source, explicit skill names or '*' alone, and supported agents",
        ));
    }

    let source = normalize_source(source)?;

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
    let mut lock_entries = BTreeMap::new();
    for (name, entry) in lock["skills"].as_object().expect("validated lock document") {
        let mut entry = entry
            .as_object()
            .cloned()
            .ok_or_else(|| invalid_stage("The skills installer produced an invalid lock entry"))?;
        if entry.get("sourceType").and_then(Value::as_str) == Some("local") {
            // skills resolves relative local sources against its working
            // directory. Store the original absolute source, not a temp path.
            entry.insert("source".into(), json!(source));
        }
        lock_entries.insert(name.clone(), Value::Object(entry));
    }
    read_bundle(
        &project,
        Bundle {
            version: 1,
            source,
            skills,
            agents,
            owner: None,
            lock_version: lock["version"].clone(),
            lock_entries,
        },
    )
}

fn read_bundle(project: &Directory, bundle: Bundle) -> Result<Staged> {
    let names: BTreeSet<_> = bundle.lock_entries.keys().cloned().collect();
    if bundle.version != 1
        || !valid_source(&bundle.source)
        || !valid_selection(&bundle.skills, &bundle.agents)
        || !bundle.lock_version.is_u64()
        || names.is_empty()
        || names.iter().any(|name| !valid_skill(name))
        || !bundle.skills.contains("*") && names != bundle.skills
        || bundle.lock_entries.values().any(|entry| !entry.is_object())
    {
        return Err(invalid_stage(
            "Invalid staged skills bundle metadata or selection",
        ));
    }
    let canonical = bundle.agents.contains("codex");
    let claude = bundle.agents.contains("claude-code");
    let locate = |agent: Agent, selected: bool| -> Result<Option<Directory>> {
        let mismatch = || {
            invalid_stage("Staged skills directory does not match the selected skills and agents")
        };
        if let Some(parent) = agent.skills_dir(project, false)? {
            let entries = parent.matching_entries(|name| {
                if !selected || !name.is_some_and(|name| names.contains(name)) {
                    return Err(mismatch());
                }
                Ok(true)
            })?;
            if !selected || entries.into_iter().collect::<BTreeSet<_>>() == names {
                return Ok(Some(parent));
            }
        } else if !selected {
            return Ok(None);
        }
        Err(mismatch())
    };
    let canonical_parent = locate(Agent::Canonical, canonical)?;
    let claude_parent = locate(Agent::Claude, claude)?;
    let primary = if canonical {
        Agent::Canonical
    } else {
        Agent::Claude
    };
    let primary_parent = if canonical {
        &canonical_parent
    } else {
        &claude_parent
    };
    let primary_parent = primary_parent.as_ref().expect("selected skills directory");
    let mut files = Vec::new();
    let mut remaining = TREE_LIMIT;
    for (name, entry) in &bundle.lock_entries {
        let copy = entry.get("installMode").and_then(Value::as_str) == Some("copy");
        let placement = Placement {
            agent: primary,
            name: name.clone(),
        };
        let content = read_content(primary_parent, name, &mut remaining, 0)?;
        if !matches!(&content, Content::Tree { children, .. } if matches!(children.get("SKILL.md"), Some(Content::File { .. })))
        {
            return Err(invalid_stage(format!(
                "Staged skill {name} is missing SKILL.md"
            )));
        }
        let hash = digest(&content)?;
        files.push((placement, content, hash.clone()));

        if canonical && claude {
            let claude = Placement {
                agent: Agent::Claude,
                name: name.to_owned(),
            };
            let parent = claude_parent.as_ref().expect("selected Claude directory");
            let link = claude.claude_link();
            let (content, hash) = if copy {
                let content = read_content(parent, name, &mut remaining, 0)?;
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
    let revision = bundle_revision(&bundle, &files)?;
    Ok(Staged {
        revision,
        bundle,
        files,
    })
}

fn bundle_revision(bundle: &Bundle, files: &[(Placement, Content, String)]) -> Result<String> {
    let fingerprints: Vec<_> = files.iter().map(|(path, _, hash)| (path, hash)).collect();
    digest(&(
        &bundle.scope()?,
        &fingerprints,
        &bundle.lock_version,
        &bundle.lock_entries,
    ))
}

impl Checkout {
    fn open(root: &Path, expected: Option<(&FileIdentity, &FileIdentity)>) -> Result<Self> {
        let root = Directory::open(&platform::absolute(root)?, false)?;
        let changed = || {
            Error::new(
                "ownership",
                format!(
                    "Repository checkout or Git directory changed at {}",
                    root.path.display()
                ),
            )
        };
        if let Some((checkout, _)) = expected
            && root.identity()? != *checkout
        {
            return Err(changed());
        }
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
            "--path-format=absolute",
            "--show-toplevel",
            "--absolute-git-dir",
            "--git-common-dir",
            "--is-inside-work-tree",
        ]);
        let (bytes, status) = launch::probe(command, "invalid_project", Duration::from_secs(5))?;
        let mut lines = std::str::from_utf8(&bytes).map_err(|_| invalid())?.lines();
        let (Some(top), Some(gitdir), Some(common), Some("true")) =
            (lines.next(), lines.next(), lines.next(), lines.next())
        else {
            return Err(invalid());
        };
        if !status.success() || lines.next().is_some() || Path::new(top) != root.path {
            return Err(invalid());
        }
        let gitdir = Directory::open(Path::new(gitdir), false)?;
        let common = Directory::open(Path::new(common), false)?;
        if let Some((_, common_identity)) = expected
            && common.identity()? != *common_identity
        {
            return Err(changed());
        }
        let common_pointer = gitdir.read("commondir", false, 4096)?;
        let mut backlink = None;
        if marker.is_dir {
            if gitdir.path != dot_git || gitdir.identity()? != marker.identity {
                return Err(invalid());
            }
        } else {
            let pointer = marker_contents.as_deref().ok_or_else(invalid)?;
            let pointer = std::str::from_utf8(pointer)
                .map_err(|_| invalid())?
                .trim_end_matches(['\r', '\n'])
                .strip_prefix("gitdir:")
                .ok_or_else(invalid)?
                .trim_start();
            if Directory::open(&root.path.join(pointer), false)?.identity()? != gitdir.identity()? {
                return Err(invalid());
            }
            if common_pointer.is_some() {
                let bytes = gitdir.read("gitdir", false, 4096)?.ok_or_else(invalid)?;
                let target = std::str::from_utf8(&bytes)
                    .map_err(|_| invalid())?
                    .trim_end_matches(['\r', '\n']);
                if platform::absolute(&gitdir.path.join(target))? != dot_git {
                    return Err(invalid());
                }
                backlink = Some(bytes);
            }
        }
        let configured_common = match &common_pointer {
            Some(pointer) => {
                let pointer = std::str::from_utf8(pointer)
                    .map_err(|_| invalid())?
                    .trim_end_matches(['\r', '\n']);
                Directory::open(&gitdir.path.join(pointer), false)?.identity()?
            }
            None => gitdir.identity()?,
        };
        if configured_common != common.identity()? {
            return Err(invalid());
        }
        let verify = || -> Result<()> {
            for dir in [&root, &gitdir, &common] {
                if Directory::open(&dir.path, false)?.identity()? != dir.identity()? {
                    return Err(changed());
                }
            }
            if root.entry(".git")?.map(|entry| entry.identity).as_ref() != Some(&marker.identity)
                || (marker.is_file && root.read(".git", false, 4096)? != marker_contents)
                || gitdir.read("commondir", false, 4096)? != common_pointer
                || (backlink.is_some() && gitdir.read("gitdir", false, 4096)? != backlink)
            {
                return Err(invalid());
            }
            Ok(())
        };
        verify()?;
        let device = |dir: &Directory| -> Result<_> {
            directory_handle(dir)?
                .metadata()
                .map(|metadata| metadata.dev())
                .map_err(|e| Error::io("inspect directory filesystem", &dir.path, e))
        };
        if device(&root)? != device(&gitdir)? {
            return Err(Error::new(
                "unsafe_path",
                "Skill checkout and Git metadata must share a filesystem",
            ));
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
        verify()?;
        if gitdir.entry(METADATA)?.map(|entry| entry.identity) != Some(state.identity()?)
            || state.entry("lock")?.map(|entry| entry.identity)
                != Some(platform::file_identity(&lock)?)
        {
            return Err(invalid());
        }
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
}

fn verify_receipt(root: &Directory, receipt: &Receipt, key: &str) -> Result<()> {
    let mut paths = BTreeSet::new();
    if receipt.checkout != root.identity()?
        || receipt.scope != key
        || !valid_hash(key)
        || receipt
            .owner
            .as_deref()
            .is_some_and(|owner| !valid_root_id(owner))
        || !valid_hash(&receipt.revision)
        || !valid_source(&receipt.source)
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

pub(crate) fn inspect(root: &Path, receipt: &Receipt) -> Result<()> {
    let root = Directory::open(root, false)?;
    verify_receipt(&root, receipt, &receipt.scope)?;
    check_owned(&root, Some(receipt), false)?;
    lock_changes(&root, Some(receipt), Some(receipt), false)?;
    Ok(())
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
        verify_receipt(&checkout.root, receipt, key)?;
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

#[cfg(test)]
fn apply(root: &Path, stage: &Staged, prior: Option<&Receipt>) -> Result<Receipt> {
    apply_with_identities(root, None, stage, prior)
}

pub(crate) fn apply_verified(
    root: &Path,
    checkout: &FileIdentity,
    common: &FileIdentity,
    stage: &Staged,
    prior: Option<&Receipt>,
) -> Result<Receipt> {
    if stage.bundle.owner.is_none() {
        return Err(Error::new(
            "ownership",
            "Staged skills have no Roost root owner",
        ));
    }
    apply_with_identities(root, Some((checkout, common)), stage, prior)
}

fn apply_with_identities(
    root: &Path,
    expected: Option<(&FileIdentity, &FileIdentity)>,
    stage: &Staged,
    prior: Option<&Receipt>,
) -> Result<Receipt> {
    let key = stage.scope()?;
    if let Some(prior) = prior
        && (prior.owner != stage.bundle.owner || prior.scope != key)
    {
        return Err(Error::new(
            "ownership",
            "Invalid or foreign managed skills receipt",
        ));
    }
    let checkout = Checkout::open(root, expected)?;
    let root = &checkout.root;
    if let Some(prior) = prior {
        verify_receipt(root, prior, &key)?;
    }
    let scope = checkout.scope(&key)?;
    if let Some(record) = get_transaction(&scope, &checkout, &key)? {
        if record.after.as_ref().is_some_and(|after| {
            after.revision == stage.revision && after.owner == stage.bundle.owner
        }) && (prior == record.before.as_ref() || prior == record.after.as_ref())
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

    let record = prepare(root, &scope, &key, stage, prior)?;
    finish(root, &scope, record).map(|receipt| receipt.expect("apply transaction"))
}

fn prepare(
    root: &Directory,
    scope: &Directory,
    key: &str,
    stage: &Staged,
    prior: Option<&Receipt>,
) -> Result<Transaction> {
    let workspace = format!("work-{}", platform::random_id()?);
    let work = scope.create_dir(&workspace)?;
    let prepared = (|| {
        let mut after = Receipt {
            revision: stage.revision.clone(),
            scope: key.to_owned(),
            owner: stage.bundle.owner.clone(),
            checkout: root.identity()?,
            source: stage.bundle.source.clone(),
            files: Vec::new(),
            lock_version: stage.bundle.lock_version.clone(),
            lock_entries: stage.bundle.lock_entries.clone(),
        };
        // Recovery indexes the union of old and new placements, including
        // those that a wildcard update removes.
        let mut paths = placements(prior, None);
        paths.extend(
            stage
                .files
                .iter()
                .map(|(placement, _, _)| placement.clone()),
        );
        for (index, path) in paths.iter().enumerate() {
            let Ok(staged_index) = stage
                .files
                .binary_search_by(|(placement, _, _)| placement.cmp(path))
            else {
                continue;
            };
            let (placement, content, digest) = &stage.files[staged_index];
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

#[cfg(test)]
fn remove(root: &Path, receipt: &Receipt) -> Result<()> {
    remove_with_identities(root, None, receipt)
}

pub(crate) fn remove_verified(
    root: &Path,
    checkout: &FileIdentity,
    common: &FileIdentity,
    receipt: &Receipt,
) -> Result<()> {
    if receipt.owner.is_none() {
        return Err(Error::new(
            "ownership",
            "Managed skills receipt has no Roost root owner",
        ));
    }
    remove_with_identities(root, Some((checkout, common)), receipt)
}

fn remove_with_identities(
    root: &Path,
    expected: Option<(&FileIdentity, &FileIdentity)>,
    receipt: &Receipt,
) -> Result<()> {
    let checkout = Checkout::open(root, expected)?;
    let root = &checkout.root;
    verify_receipt(root, receipt, &receipt.scope)?;
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
    } else if receipt.owner.is_some() {
        return Err(conflict(&scope.path));
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

    const OWNER: &str = "0123456789abcdef0123456789abcdef";
    const OTHER_OWNER: &str = "fedcba9876543210fedcba9876543210";

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
        bundle_with_names(body, &["alpha", "beta"])
    }

    fn bundle_with_names(body: &str, names: &[&str]) -> Staged {
        let mut files = Vec::new();
        let mut lock_entries = BTreeMap::new();
        for &name in names {
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
        let bundle = Bundle {
            version: 1,
            source: "owner/repo".into(),
            skills: names.iter().map(|&name| name.to_owned()).collect(),
            agents: ["claude-code".into(), "codex".into()].into(),
            owner: None,
            lock_entries,
            lock_version: json!(2),
        };
        let revision = bundle_revision(&bundle, &files).unwrap();
        Staged {
            revision,
            bundle,
            files,
        }
    }

    fn installed(root: &Path, skill: &str) -> PathBuf {
        root.join(".agents/skills").join(skill).join("SKILL.md")
    }

    fn lock(root: &Path) -> Value {
        serde_json::from_slice(&fs::read(root.join(LOCK_FILE)).unwrap()).unwrap()
    }

    fn identity(path: &Path) -> FileIdentity {
        Directory::open(path, false).unwrap().identity().unwrap()
    }

    #[test]
    fn inspect_detects_edits_without_creating_checkout_metadata() {
        let repo = repo();
        let root = &repo.0;
        let receipt = apply(root, &bundle("v1"), None).unwrap();
        let metadata = root.join(".git").join(METADATA);
        fs::remove_dir_all(&metadata).unwrap();
        let lock_path = root.join(LOCK_FILE);
        let original = fs::read(&lock_path).unwrap();
        inspect(root, &receipt).unwrap();
        assert_eq!(fs::read(&lock_path).unwrap(), original);

        let mut edited = lock(root);
        edited["skills"]["beta"]["ref"] = json!("edited");
        fs::write(&lock_path, serde_json::to_vec(&edited).unwrap()).unwrap();
        assert_eq!(inspect(root, &receipt).unwrap_err().code, "conflict");
        fs::write(&lock_path, &original).unwrap();
        fs::write(installed(root, "alpha"), "edited").unwrap();
        assert_eq!(inspect(root, &receipt).unwrap_err().code, "conflict");

        let mut invalid = receipt;
        invalid.scope = "../foreign".into();
        assert_eq!(inspect(root, &invalid).unwrap_err().code, "ownership");
        assert!(!metadata.exists());
    }

    #[test]
    fn owner_binding_namespaces_recovery_without_changing_revision() {
        let repo = repo();
        let root = &repo.0;
        let checkout = identity(root);
        let common = identity(&root.join(".git"));
        let mut first = bundle("v1");
        let mut foreign = bundle("v1");
        first.bind_owner(OWNER).unwrap();
        first.bind_owner(OWNER).unwrap();
        foreign.bind_owner(OTHER_OWNER).unwrap();
        assert_eq!(first.revision(), foreign.revision());
        assert_ne!(first.scope().unwrap(), foreign.scope().unwrap());
        assert_eq!(first.bind_owner(OTHER_OWNER).unwrap_err().code, "ownership");

        let receipt = apply_verified(root, &checkout, &common, &first, None).unwrap();
        assert_eq!(
            apply_verified(root, &checkout, &common, &first, None).unwrap(),
            receipt
        );
        receipt.verify_owner(OWNER).unwrap();
        assert_eq!(
            receipt.verify_owner(OTHER_OWNER).unwrap_err().code,
            "ownership"
        );
        assert_eq!(
            apply_verified(root, &checkout, &common, &foreign, None)
                .unwrap_err()
                .code,
            "conflict"
        );
        assert_eq!(
            apply_verified(root, &checkout, &common, &foreign, Some(&receipt))
                .unwrap_err()
                .code,
            "ownership"
        );
        let mut relabeled = receipt.clone();
        relabeled.owner = Some(OTHER_OWNER.into());
        relabeled.scope = foreign.scope().unwrap();
        assert_eq!(
            remove_verified(root, &checkout, &common, &relabeled)
                .unwrap_err()
                .code,
            "conflict"
        );
        remove_verified(root, &checkout, &common, &receipt).unwrap();
    }

    #[test]
    fn cached_bundle_is_private_idempotent_and_checked_before_use() {
        let temp = Temp::new().unwrap();
        let cache = Directory::open(&temp.0, true).unwrap();
        let mut staged = bundle("v1");
        staged.bind_owner(OWNER).unwrap();
        staged.save(&cache).unwrap();
        staged.save(&cache).unwrap();
        assert_eq!(cache.entries().unwrap(), [staged.revision()]);

        let mut loaded = load(&cache, staged.revision()).unwrap();
        assert_eq!(
            loaded.bind_owner(OTHER_OWNER).unwrap_err().code,
            "ownership"
        );
        loaded.bind_owner(OWNER).unwrap();
        let repo = repo();
        let receipt = apply_verified(
            &repo.0,
            &identity(&repo.0),
            &identity(&repo.0.join(".git")),
            &loaded,
            None,
        )
        .unwrap();
        inspect(&repo.0, &receipt).unwrap();
        assert_eq!(
            fs::read_to_string(installed(&repo.0, "alpha")).unwrap(),
            "v1 alpha"
        );

        let saved = temp.0.join(staged.revision());
        assert_eq!(fs::metadata(saved.join(BUNDLE)).unwrap().mode() & 0o077, 0);
        let claude = saved.join(".claude/skills/alpha");
        fs::remove_file(&claude).unwrap();
        symlink("../../.agents/skills/beta", &claude).unwrap();
        assert!(load(&cache, staged.revision()).is_err());
        fs::remove_file(&claude).unwrap();
        symlink("../../.agents/skills/alpha", &claude).unwrap();
        fs::write(installed(&saved, "alpha"), "edited cache").unwrap();
        assert!(load(&cache, staged.revision()).is_err());
        assert!(staged.save(&cache).is_err());
        assert!(load(&cache, "../other").is_err());
    }

    #[test]
    fn verified_operations_reject_changed_identities_before_creating_metadata() {
        let repo = repo();
        let root = &repo.0;
        let checkout = identity(root);
        let common = identity(&root.join(".git"));
        let mut staged = bundle("v1");
        staged.bind_owner(OWNER).unwrap();
        let receipt = apply_verified(root, &checkout, &common, &staged, None).unwrap();

        fs::rename(root.join(".git"), root.join(".old-git")).unwrap();
        assert!(
            git()
                .current_dir(root)
                .args(["init", "-q", "--template="])
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(
            apply_verified(root, &checkout, &common, &staged, Some(&receipt))
                .unwrap_err()
                .code,
            "ownership"
        );
        let replacement_common = identity(&root.join(".git"));
        assert_eq!(
            apply_verified(root, &common, &replacement_common, &staged, None)
                .unwrap_err()
                .code,
            "ownership"
        );
        assert_eq!(
            remove_verified(root, &checkout, &common, &receipt)
                .unwrap_err()
                .code,
            "ownership"
        );
        assert!(!root.join(".git").join(METADATA).exists());
        inspect(root, &receipt).unwrap();
    }

    #[test]
    fn accepts_wildcard_only_when_it_is_the_only_selector() {
        let agents = ["codex".into()];
        for other in ["alpha", "*"] {
            let err = stage("owner/repo", &["*".into(), other.into()], &agents)
                .err()
                .unwrap();
            assert_eq!(err.code, "usage");
        }
    }

    #[test]
    fn wildcard_update_can_add_and_remove_skills() {
        let repo = repo();
        let checkout = identity(&repo.0);
        let common = identity(&repo.0.join(".git"));
        let wildcard = |body, names: &[&str]| {
            let mut stage = bundle_with_names(body, names);
            stage.bundle.skills = ["*".into()].into();
            stage.revision = bundle_revision(&stage.bundle, &stage.files).unwrap();
            stage.bind_owner(OWNER).unwrap();
            stage
        };
        let first = wildcard("v1", &["alpha", "beta"]);
        let before = apply_verified(&repo.0, &checkout, &common, &first, None).unwrap();
        let update = wildcard("v2", &["beta", "gamma"]);
        let after = apply_verified(&repo.0, &checkout, &common, &update, Some(&before)).unwrap();
        inspect(&repo.0, &after).unwrap();
        assert!(!installed(&repo.0, "alpha").exists());
        assert!(!repo.0.join(".claude/skills/alpha").is_symlink());
        assert_eq!(
            fs::read_to_string(installed(&repo.0, "beta")).unwrap(),
            "v2 beta"
        );
        assert_eq!(
            fs::read_to_string(installed(&repo.0, "gamma")).unwrap(),
            "v2 gamma"
        );
    }

    #[test]
    fn normalizes_only_explicit_local_sources() {
        assert!(Path::new("src/store").is_dir());
        assert_eq!(normalize_source("src/store").unwrap(), "src/store");
        assert_eq!(
            Path::new(&normalize_source("./src/store").unwrap()),
            Path::new("./src/store").canonicalize().unwrap(),
        );
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
            let checkout = Checkout::open(root, None).unwrap();
            let key = staged.scope().unwrap();
            let scope = checkout.scope(&key).unwrap();
            let record = prepare(&checkout.root, &scope, &key, &staged, None).unwrap();
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
        let mut staged = bundle("v1");
        staged.bind_owner(OWNER).unwrap();
        let checkout = identity(&linked);
        let common = identity(&repo.0.join(".git"));
        let gitdir = repo.0.join(".git/worktrees/linked");
        assert_eq!(
            apply_verified(&linked, &checkout, &identity(&gitdir), &staged, None)
                .unwrap_err()
                .code,
            "ownership"
        );
        assert!(!gitdir.join(METADATA).exists());
        let first = apply_verified(&linked, &checkout, &common, &staged, None).unwrap();
        assert_eq!(
            apply(&copy, &staged, None).unwrap_err().code,
            "invalid_project"
        );
        assert_eq!(remove(&repo.0, &first).unwrap_err().code, "ownership");
        assert!(!installed(&repo.0, "alpha").exists());
        remove_verified(&linked, &checkout, &common, &first).unwrap();
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

        let mut wildcard = stage(source, &["*".into()], &both).unwrap();
        wildcard.bind_owner(OWNER).unwrap();
        let cache = Temp::new().unwrap();
        let cache = Directory::open(&cache.0, true).unwrap();
        wildcard.save(&cache).unwrap();
        fs::remove_dir_all(Path::new(source).join("skills")).unwrap();
        let loaded = load(&cache, wildcard.revision()).unwrap();
        let receipt = apply(&repo.0, &loaded, None).unwrap();
        for name in ["alpha", "beta"] {
            assert!(installed(&repo.0, name).is_file());
            assert!(repo.0.join(".claude/skills").join(name).is_symlink());
        }
        assert_eq!(
            receipt.lock_entries.keys().collect::<Vec<_>>(),
            ["alpha", "beta"]
        );
        remove(&repo.0, &receipt).unwrap();
    }
}
