//! Protected records and registry-last transactions. Every filesystem action uses
//! a retained directory handle; journal identities describe objects, not names.
use crate::{
    Error, Result, launch,
    platform::{self, Directory, FileIdentity},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    fs::File,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

const MARKER: &str = ".roost-root.json";
const PROFILE_MARKER: &str = ".roost-profile.json";
const REGISTRY: &str = "registry.json";
const JOURNAL: &str = ".roost-operation.json";
const LOCK: &str = ".roost-lock";
const LIMIT: usize = 4 * 1024 * 1024;

mod desktop;
mod plugin_store;
pub mod side;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Owned,
    Upstream,
    DefaultAlias,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Active,
    Retained,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LauncherBinding {
    pub format_version: u32,
    pub executable: PathBuf,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Registration {
    pub registration_id: String,
    pub name: String,
    pub kind: Kind,
    pub state: State,
    #[serde(deserialize_with = "nullable")]
    pub directory: Option<PathBuf>,
    #[serde(deserialize_with = "nullable")]
    pub directory_identity: Option<FileIdentity>,
    #[serde(deserialize_with = "nullable")]
    pub profile_id: Option<String>,
    pub upstream_linked_default: bool,
    #[serde(deserialize_with = "nullable")]
    pub launcher_binding: Option<LauncherBinding>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub schema_version: u32,
    pub root_id: String,
    pub generation: u64,
    pub registrations: Vec<Registration>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RootMarker {
    schema_version: u32,
    root_id: String,
    root_identity: FileIdentity,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileMarker {
    schema_version: u32,
    root_id: String,
    profile_id: String,
    directory_identity: FileIdentity,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Operation {
    Initialize,
    Add,
    Register,
    Reuse,
    Remove,
    Purge,
    TokenSet,
    TokenClear,
    DesktopCreate,
    StoreCreate,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Prepared,
    Publishing,
    Committed,
    Cleanup,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Role {
    Profile,
    Launcher,
    Token,
    Registry,
    Marker,
    StagingDirectory,
    DesktopData,
    Store,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Action {
    Create,
    Replace,
    Delete,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Form {
    Sh,
    Cmd,
    Ps1,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ObjectState {
    object_identity: FileIdentity,
    #[serde(deserialize_with = "nullable")]
    profile_id: Option<String>,
    #[serde(deserialize_with = "nullable")]
    launcher_binding: Option<LauncherBinding>,
    #[serde(deserialize_with = "nullable")]
    launcher_form: Option<Form>,
    #[serde(deserialize_with = "nullable")]
    registry_generation: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Staged {
    path: PathBuf,
    object_identity: FileIdentity,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    role: Role,
    destination: PathBuf,
    #[serde(deserialize_with = "nullable")]
    staged: Option<Staged>,
    #[serde(deserialize_with = "nullable")]
    before: Option<ObjectState>,
    #[serde(deserialize_with = "nullable")]
    after: Option<ObjectState>,
    action: Action,
    completed: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    schema_version: u32,
    root_id: String,
    operation_id: String,
    operation: Operation,
    #[serde(deserialize_with = "nullable")]
    registration_id: Option<String>,
    prior_generation: u64,
    next_generation: u64,
    phase: Phase,
    artifacts: Vec<Artifact>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenMode {
    /// Never recovers or mutates anything.
    Read,
    Mutate,
    RetryPurge,
    /// Launch preparation: like Read (never recovers), but may replace side files
    /// (`state.json`, `sets.json`) while no intent is pending. Its one journaled
    /// mutation is `desktop_create`: the first `roost desktop` launch of a
    /// registration creates its Desktop data folder (spec: first-use creation at
    /// launch), through the same pending-intent and root checks as Mutate. No other
    /// journaled operation is allowed, and a Launch open never creates a root.
    Launch,
}
impl OpenMode {
    /// Whether this mode may run journaled mutations (and so create a root).
    fn journaled(self) -> bool {
        matches!(self, OpenMode::Mutate | OpenMode::RetryPurge)
    }
}
pub struct Store {
    pub root: PathBuf,
    pub root_id: String,
    pub registry: Registry,
    directory: Directory,
    profiles_dir: Directory,
    bin: Directory,
    stages: Directory,
    _lock: File,
    lock_identity: FileIdentity,
    journal_identity: Option<FileIdentity>,
    registry_identity: Option<FileIdentity>,
    intent: Option<Intent>,
    mode: OpenMode,
}

fn err(code: &'static str, message: impl Into<String>) -> Error {
    Error::new(code, message)
}
fn recovery(message: impl Into<String>) -> Error {
    err("ownership", message).next("Inspect the pending operation with roost doctor; retry the explicit operation only after checking the changed state.")
}
fn valid_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn validate_identity(identity: &FileIdentity) -> Result<()> {
    let decimal = |s: &str| s.parse::<u64>().is_ok_and(|n| n.to_string() == s);
    let valid = match identity {
        FileIdentity::Unix { device, inode } => cfg!(unix) && decimal(device) && decimal(inode),
        FileIdentity::Windows { volume, index } => {
            cfg!(windows) && decimal(volume) && decimal(index)
        }
    };
    if !valid {
        return Err(err(
            "ownership",
            "Invalid or unsupported physical object identity",
        ));
    }
    Ok(())
}
fn checked_path(path: &Path) -> Result<()> {
    if !path.is_absolute() || platform::absolute(path)? != path {
        return Err(err(
            "unsafe_path",
            "Recorded path must be an absolute unchanged path",
        ));
    }
    Ok(())
}
pub fn validate_name(name: &str) -> Result<()> {
    let bytes = name.as_bytes();
    let device = name.to_ascii_uppercase();
    if bytes.is_empty()
        || bytes.len() > 32
        || !bytes[0].is_ascii_alphanumeric()
        || !bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
        || ["CON", "PRN", "AUX", "NUL"].contains(&device.as_str())
        || (device.len() == 4
            && (device.starts_with("COM") || device.starts_with("LPT"))
            && (b'1'..=b'9').contains(&device.as_bytes()[3]))
    {
        return Err(err(
            "usage",
            "Profile name must be 1–32 ASCII letters/digits/underscore/hyphen, start with a letter or digit, and avoid reserved device names",
        ));
    }
    Ok(())
}
fn nullable<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}
fn parse<T: for<'de> Deserialize<'de>>(bytes: &[u8], label: &str) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|_| {
        err(
            "ownership",
            format!("Invalid or unsupported {label}; preserve it for inspection"),
        )
    })
}
fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec_pretty(value).map_err(|_| err("io", "Could not encode manager metadata"))
}
fn read_record<T: for<'de> Deserialize<'de>>(dir: &Directory, name: &str) -> Result<T> {
    parse(
        &dir.read(name, true, LIMIT)?.ok_or_else(|| {
            err(
                "not_found",
                format!("Missing manager record {}", dir.path.join(name).display()),
            )
        })?,
        name,
    )
}
fn state(identity: FileIdentity) -> ObjectState {
    ObjectState {
        object_identity: identity,
        profile_id: None,
        launcher_binding: None,
        launcher_form: None,
        registry_generation: None,
    }
}
fn file_state(dir: &Directory, name: &str, private: bool) -> Result<Option<ObjectState>> {
    let Some(entry) = dir.entry(name)? else {
        return Ok(None);
    };
    if !entry.is_file || entry.is_link || entry.nlink != 1 {
        return Err(err(
            "unsafe_path",
            format!("Unsafe file {}", dir.path.join(name).display()),
        ));
    }
    let file = dir.open_file(name, private, false)?;
    let identity = platform::file_identity(&file)?;
    if identity != entry.identity
        || dir.entry(name)?.as_ref().map(|e| &e.identity) != Some(&identity)
    {
        return Err(err(
            "unsafe_path",
            "File changed while opening protected object",
        ));
    }
    Ok(Some(state(identity)))
}
fn launcher_bytes(directory: &Directory, name: &str) -> Result<Option<Vec<u8>>> {
    if directory.entry(name)?.is_none() {
        return Ok(None);
    }
    let file = directory.open_file(name, true, false)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if file
            .metadata()
            .map_err(|e| Error::io("inspect launcher", &directory.path.join(name), e))?
            .permissions()
            .mode()
            & 0o100
            == 0
        {
            return Err(err(
                "unsafe_path",
                format!(
                    "Owned launcher is not executable: {}",
                    directory.path.join(name).display()
                ),
            ));
        }
    }
    directory.read(name, true, 1024 * 1024)
}
fn file_name(path: &Path) -> Result<&str> {
    path.file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| err("unsafe_path", "Invalid object name"))
}
fn form(path: &Path) -> Form {
    match path.extension().and_then(|x| x.to_str()) {
        Some("cmd") => Form::Cmd,
        Some("ps1") => Form::Ps1,
        _ => Form::Sh,
    }
}
fn lock(dir: &Directory) -> Result<File> {
    let file = dir.open_file(LOCK, true, true)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if platform::cancelled() {
            return Err(Error::cancelled());
        }
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(25))
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(err("io", "Another Roost operation holds this root's lock")
                    .next("Retry after the other operation finishes."));
            }
            Err(std::fs::TryLockError::Error(e)) => {
                return Err(Error::io("lock storage", &dir.path, e));
            }
        }
    }
}
fn current_binding() -> Result<LauncherBinding> {
    let path = std::env::current_exe()
        .map_err(|e| Error::io("resolve executable", Path::new("roost"), e))?;
    Ok(LauncherBinding {
        format_version: 1,
        executable: platform::absolute(&path)?,
    })
}

impl Store {
    pub fn open(root: &Path, create: bool, mode: OpenMode) -> Result<Self> {
        let root = platform::absolute(root)?;
        let parent = Directory::open(
            root.parent()
                .ok_or_else(|| err("unsafe_path", "Storage cannot be a filesystem root"))?,
            false,
        )?;
        let name = file_name(&root)?;
        let directory = match parent.entry(name)? {
            Some(_) => parent.child(name, true)?,
            None if create && mode.journaled() => parent.create_dir(name)?,
            None => return Err(err("not_found", "Roost storage is not initialized")),
        };
        if directory.entry(MARKER)?.is_none() {
            if !create || !mode.journaled() {
                return Err(err("ownership", "Storage has no Roost root marker"));
            }
            let entries = directory.entries()?;
            if !entries.is_empty() {
                return Err(err(
                    "ownership",
                    "Nonempty storage without a root marker is foreign",
                ));
            }
            directory.write_new(LOCK, b"", 0o600)?;
        }
        let held_lock = lock(&directory)?;
        let lock_identity = platform::file_identity(&held_lock)?;
        if directory.entry(LOCK)?.as_ref().map(|e| &e.identity) != Some(&lock_identity) {
            return Err(recovery("Stable lock changed while acquiring it"));
        }
        let marker: RootMarker = if directory.entry(MARKER)?.is_some() {
            read_record(&directory, MARKER)?
        } else {
            let m = RootMarker {
                schema_version: 1,
                root_id: platform::random_id()?,
                root_identity: directory.identity()?,
            };
            directory.write_new(MARKER, &encode(&m)?, 0o600)?;
            directory.sync()?;
            m
        };
        if marker.schema_version != 1
            || !valid_id(&marker.root_id)
            || marker.root_identity != directory.identity()?
        {
            return Err(err(
                "ownership",
                "Root marker does not match this protected storage object",
            ));
        }
        let absent_registry = directory.entry(REGISTRY)?.is_none();
        if absent_registry && !mode.journaled() {
            return Err(recovery("Root marker exists without a committed registry"));
        }
        // Interrupted bootstrap is recognizable only by its exact private root marker,
        // stable lock and initial namespace; an existing profile is never imported.
        if absent_registry {
            let initializing = directory.entry(JOURNAL)?.is_some();
            for name in directory.entries()? {
                if ![MARKER, LOCK, JOURNAL, "profiles", "bin", ".roost-stage"]
                    .contains(&name.as_str())
                {
                    return Err(recovery(
                        "Unrecognized object in interrupted initialization",
                    ));
                }
                if ["profiles", "bin"].contains(&name.as_str())
                    && !directory.child(&name, true)?.entries()?.is_empty()
                    || name == ".roost-stage"
                        && !initializing
                        && !directory.child(&name, true)?.entries()?.is_empty()
                {
                    return Err(recovery("Nonempty directory in interrupted initialization"));
                }
            }
        }
        let child = |name: &str| -> Result<Directory> {
            match directory.entry(name)? {
                Some(_) => directory.child(name, true),
                None if absent_registry => directory.create_dir(name),
                None => Err(recovery(format!(
                    "Missing required managed directory {name}"
                ))),
            }
        };
        let profiles_dir = child("profiles")?;
        let bin = child("bin")?;
        let stages = child(".roost-stage")?;
        let registry = if absent_registry {
            Registry {
                schema_version: 1,
                root_id: marker.root_id.clone(),
                generation: 0,
                registrations: vec![],
            }
        } else {
            read_record(&directory, REGISTRY)?
        };
        let journal_identity = file_state(&directory, JOURNAL, true)?.map(|s| s.object_identity);
        let registry_identity = file_state(&directory, REGISTRY, true)?.map(|s| s.object_identity);
        let intent = directory
            .read(JOURNAL, true, LIMIT)?
            .map(|b| parse::<Intent>(&b, "operation journal"))
            .transpose()?;
        let mut store = Self {
            root,
            root_id: marker.root_id,
            registry,
            directory,
            profiles_dir,
            bin,
            stages,
            _lock: held_lock,
            lock_identity,
            journal_identity,
            registry_identity,
            intent,
            mode,
        };
        if !absent_registry {
            store.validate_registry()?;
        }
        if absent_registry
            && store
                .intent
                .as_ref()
                .is_some_and(|j| j.operation != Operation::Initialize)
        {
            return Err(recovery(
                "Missing registry with a non-initialization journal",
            ));
        }
        if let Some(journal) = &store.intent {
            store.validate_intent(journal)?;
        }
        if absent_registry && store.intent.is_none() {
            let stage = store.begin(Operation::Initialize, None)?;
            store.commit(store.registry.clone(), &stage)?;
        }
        if mode == OpenMode::Mutate && store.pending() {
            store.recover()?;
        }
        if mode == OpenMode::RetryPurge
            && store
                .intent
                .as_ref()
                .is_some_and(|j| j.operation != Operation::Purge)
        {
            store.recover()?;
        }
        store.validate_registry()?;
        store.check_residue()?;
        Ok(store)
    }

    pub fn pending(&self) -> bool {
        self.intent.is_some()
    }
    pub fn find(&self, name: &str, retained: bool) -> Result<&Registration> {
        validate_name(name)?;
        self.registry
            .registrations
            .iter()
            .find(|r| r.name.eq_ignore_ascii_case(name) && (retained || r.state == State::Active))
            .ok_or_else(|| {
                err(
                    "not_found",
                    format!(
                        "No {}profile named {name}",
                        if retained { "" } else { "active " }
                    ),
                )
            })
    }
    fn validate_registry(&self) -> Result<()> {
        if self.registry.schema_version != 1
            || self.registry.root_id != self.root_id
            || self.registry.generation == 0
        {
            return Err(err("ownership", "Unsupported registry or root mismatch"));
        }
        let mut names = HashSet::new();
        let mut ids = HashSet::from([self.root_id.clone()]);
        let mut identities = Vec::new();
        for r in &self.registry.registrations {
            validate_name(&r.name).map_err(|_| err("ownership", "Invalid name in registry"))?;
            if !valid_id(&r.registration_id)
                || !names.insert(r.name.to_ascii_lowercase())
                || !ids.insert(r.registration_id.clone())
            {
                return Err(err(
                    "ownership",
                    "Duplicate or invalid registration name/ID",
                ));
            }
            if let Some(binding) = &r.launcher_binding {
                if binding.format_version != 1 {
                    return Err(err("ownership", "Unsupported launcher binding version"));
                }
                checked_path(&binding.executable)?;
            } else {
                return Err(err("ownership", "Registration has no launcher binding"));
            }
            if let Some(path) = &r.directory {
                checked_path(path)?;
            }
            if let Some(id) = &r.directory_identity {
                validate_identity(id)?;
                if identities.contains(id) {
                    return Err(err("ownership", "Duplicate physical profile registration"));
                }
                identities.push(id.clone());
            }
            let good = match r.kind {
                Kind::Owned => {
                    r.directory.as_ref() == Some(&self.root.join("profiles").join(&r.name))
                        && r.directory_identity.is_some()
                        && !r.upstream_linked_default
                        && r.profile_id
                            .as_ref()
                            .is_some_and(|id| valid_id(id) && ids.insert(id.clone()))
                }
                Kind::Upstream => {
                    r.state == State::Active
                        && r.directory.is_some()
                        && r.directory_identity.is_some()
                        && r.profile_id.is_none()
                        && !r.upstream_linked_default
                }
                Kind::DefaultAlias => {
                    r.state == State::Active
                        && r.profile_id.is_none()
                        && if r.upstream_linked_default {
                            r.directory.is_some() && r.directory_identity.is_some()
                        } else {
                            r.directory.is_none() && r.directory_identity.is_none()
                        }
                }
            };
            if !good {
                return Err(err("ownership", "Impossible registration fields"));
            }
            if r.kind != Kind::Owned
                && let Some(path) = &r.directory
            {
                self.upstream_shape(path)?;
            }
        }
        Ok(())
    }
    fn check_root(&self) -> Result<()> {
        let reopened = Directory::open(&self.root, true)?;
        if reopened.identity()? != self.directory.identity()? {
            return Err(err("unsafe_path", "Storage namespace was replaced"));
        }
        let m: RootMarker = read_record(&self.directory, MARKER)?;
        if m.schema_version != 1
            || m.root_id != self.root_id
            || m.root_identity != self.directory.identity()?
        {
            return Err(err("ownership", "Root ownership changed"));
        }
        for (name, expected) in [
            ("profiles", &self.profiles_dir),
            ("bin", &self.bin),
            (".roost-stage", &self.stages),
        ] {
            if self.directory.child(name, true)?.identity()? != expected.identity()? {
                return Err(err(
                    "unsafe_path",
                    format!("Managed {name} namespace was replaced"),
                ));
            }
        }
        let locked = self.directory.open_file(LOCK, true, true)?;
        if platform::file_identity(&locked)? != self.lock_identity
            || self.directory.entry(LOCK)?.as_ref().map(|e| &e.identity)
                != Some(&self.lock_identity)
        {
            return Err(recovery("Stable lock object was replaced"));
        }
        if self.directory.entry(REGISTRY)?.map(|e| e.identity) != self.registry_identity {
            return Err(recovery("Committed registry object was replaced"));
        }
        Ok(())
    }
    pub(crate) fn profile_directory(&self, r: &Registration) -> Result<Directory> {
        let path = r
            .directory
            .as_ref()
            .ok_or_else(|| err("ownership", "Registration has no profile directory"))?;
        let directory = if r.kind == Kind::Owned {
            self.profiles_dir.child(&r.name, true)?
        } else {
            Directory::open(path, false)?
        };
        if Some(directory.identity()?) != r.directory_identity {
            return Err(err(
                "ownership",
                format!("Profile {} directory identity changed", r.name),
            ));
        }
        if r.kind == Kind::Owned {
            let m: ProfileMarker = read_record(&directory, PROFILE_MARKER)?;
            if m.schema_version != 1
                || m.root_id != self.root_id
                || Some(&m.profile_id) != r.profile_id.as_ref()
                || m.directory_identity != directory.identity()?
            {
                return Err(err(
                    "ownership",
                    format!("Profile {} ownership marker does not match", r.name),
                ));
            }
        } else {
            let linked = self.linked_default(&directory)?;
            if linked != r.upstream_linked_default {
                return Err(err(
                    "ownership",
                    format!("Upstream profile {} linked-default marker changed", r.name),
                ));
            }
        }
        Ok(directory)
    }
    pub fn validate(&self, r: &Registration) -> Result<()> {
        self.check_root()?;
        if !self.registry.registrations.iter().any(|record| record == r) {
            return Err(err(
                "ownership",
                "Registration is not the current stored record",
            ));
        }
        if self.pending() {
            return Err(recovery(
                "Pending operation blocks profile use until recovery",
            ));
        }
        if r.directory.is_some() {
            let _ = self.profile_directory(r)?;
        }
        Ok(())
    }
    pub fn token_present(&self, r: &Registration) -> Result<bool> {
        self.check_root()?;
        if r.kind == Kind::DefaultAlias {
            return Ok(false);
        }
        let directory = self.profile_directory(r)?;
        let name = if r.kind == Kind::Owned {
            ".roost-token"
        } else {
            ".ccm-oauth-token"
        };
        Ok(file_state(&directory, name, true)?.is_some())
    }
    pub fn token(&self, r: &Registration) -> Result<Option<String>> {
        self.check_root()?;
        if r.kind == Kind::DefaultAlias {
            return Ok(None);
        }
        let directory = self.profile_directory(r)?;
        let name = if r.kind == Kind::Owned {
            ".roost-token"
        } else {
            ".ccm-oauth-token"
        };
        let Some(bytes) = directory.read(name, true, 65536)? else {
            return Ok(None);
        };
        Ok(Some(platform::validate_token(&bytes)?))
    }
    fn alias_directory() -> Result<PathBuf> {
        match std::env::var_os("CLAUDE_CONFIG_DIR").filter(|x| !x.is_empty()) {
            Some(path) => platform::absolute(Path::new(&path)),
            None => platform::default_directory(),
        }
    }
    pub fn profiles(&self, retained: bool) -> Result<Vec<Value>> {
        let mut records = self
            .registry
            .registrations
            .iter()
            .filter(|r| retained || r.state == State::Active)
            .collect::<Vec<_>>();
        records.sort_by_key(|r| (r.name.to_ascii_lowercase(), r.name.clone()));
        records.into_iter().map(|r| {
            let directory = if r.kind == Kind::DefaultAlias { Self::alias_directory().ok() } else { r.directory.clone() };
            let token_present = if r.kind == Kind::DefaultAlias { Some(false) } else { self.token_present(r).ok() };
            let mut launchers = launch::templates(&self.root, &self.root_id, r)?.into_iter().map(|(path, bytes)| {
                let condition = match self.bin.entry(file_name(&path)?) {
                    Ok(None) => if r.state == State::Retained { "not_required" } else { "missing" },
                    Ok(Some(_)) => match launcher_bytes(&self.bin, file_name(&path)?) { Ok(Some(actual)) if actual == bytes => if r.state == State::Retained || self.pending() { "stale" } else { "ready" }, Ok(Some(_)) => "collision", _ => "unsafe" },
                    Err(_) => "unsafe",
                };
                Ok(json!({"path":path,"condition":condition}))
            }).collect::<Result<Vec<_>>>()?;
            launchers.sort_by_key(|v| v["path"].as_str().unwrap_or("").to_owned());
            // sets/last_used/selected/most_recent are placeholders until sets (05) and
            // selection (04) fill them; probe is filled only by `list --full`.
            Ok(json!({"name":r.name,"kind":r.kind,"state":r.state,"directory":directory,"token_present":token_present,"launchers":launchers,"sets":[],"last_used":null,"selected":false,"most_recent":false,"probe":null}))
        }).collect()
    }
    fn linked_default(&self, dir: &Directory) -> Result<bool> {
        // Upstream defines linked kind by marker presence, regardless of contents.
        Ok(file_state(dir, ".ccm-linked-default", false)?.is_some())
    }
    fn upstream_shape(&self, path: &Path) -> Result<()> {
        checked_path(path)?;
        if path
            .parent()
            .and_then(Path::file_name)
            .and_then(|x| x.to_str())
            != Some("profiles")
            || path
                .parent()
                .and_then(Path::parent)
                .and_then(Path::file_name)
                .and_then(|x| x.to_str())
                != Some(".ccm")
        {
            return Err(err(
                "ownership",
                "Register requires a real upstream-home/.ccm/profiles/name directory",
            ));
        }
        Ok(())
    }
    fn no_overlap(&self, path: &Path, identity: &FileIdentity, registering: bool) -> Result<()> {
        // Compare open-object ancestor identities, rather than assuming lexical prefixes
        // or canonical path strings establish ownership.
        let root_id = self.directory.identity()?;
        for ancestor in path.ancestors() {
            if Directory::open(ancestor, false)?.identity()? == root_id {
                return Err(err("unsafe_path", "Profile/source overlaps Roost storage"));
            }
        }
        for ancestor in self.root.ancestors() {
            if Directory::open(ancestor, false)?.identity()? == *identity {
                return Err(err("unsafe_path", "Profile/source contains Roost storage"));
            }
        }
        if !registering {
            return Ok(());
        }
        for r in &self.registry.registrations {
            if r.directory_identity.as_ref() == Some(identity) {
                return Err(err("collision", "Directory is already registered"));
            }
            if let Some(registered) = &r.directory {
                for ancestor in path.ancestors() {
                    if Some(Directory::open(ancestor, false)?.identity()?) == r.directory_identity {
                        return Err(err(
                            "unsafe_path",
                            "Directory overlaps a registered profile",
                        ));
                    }
                }
                for ancestor in registered.ancestors() {
                    if Directory::open(ancestor, false)?.identity()? == *identity {
                        return Err(err(
                            "unsafe_path",
                            "Directory contains a registered profile",
                        ));
                    }
                }
            }
        }
        Ok(())
    }
    fn mutation_ready(&self) -> Result<()> {
        self.journal_ready(false)
    }
    fn journal_ready(&self, launch_allowed: bool) -> Result<()> {
        if !self.mode.journaled() && !(launch_allowed && self.desktop_create_allowed()) {
            return Err(err("ownership", "Read-only store cannot mutate"));
        }
        self.check_root()?;
        if self.pending() {
            return Err(recovery(
                "Pending operation must be recovered before another mutation",
            ));
        }
        if platform::cancelled() {
            return Err(Error::cancelled());
        }
        Ok(())
    }
    fn check_residue(&self) -> Result<()> {
        for name in self.directory.entries()? {
            if ![
                MARKER,
                LOCK,
                REGISTRY,
                JOURNAL,
                "profiles",
                "bin",
                ".roost-stage",
            ]
            .contains(&name.as_str())
                && !side::admitted(&self.directory, &name)?
            {
                return Err(recovery(format!(
                    "Unjournaled root residue at {}",
                    self.root.join(name).display()
                )));
            }
        }
        for name in self.profiles_dir.entries()? {
            let known = self
                .registry
                .registrations
                .iter()
                .any(|r| r.kind == Kind::Owned && r.name == name)
                || self.intent.as_ref().is_some_and(|j| {
                    j.artifacts.iter().any(|a| {
                        a.role == Role::Profile
                            && a.destination == self.profiles_dir.path.join(&name)
                    })
                });
            if !known {
                return Err(recovery(format!(
                    "Unmanaged profile residue at {}",
                    self.profiles_dir.path.join(name).display()
                )));
            }
        }
        for name in self.stages.entries()? {
            if !self.intent.as_ref().is_some_and(|j| j.operation_id == name) {
                return Err(recovery(
                    "Unjournaled staging directory; preserve it for inspection",
                ));
            }
        }
        Ok(())
    }
    fn save_intent(&mut self) -> Result<()> {
        let intent = self
            .intent
            .as_ref()
            .ok_or_else(|| recovery("No transaction to persist"))?;
        let bytes = encode(intent)?;
        if bytes.len() > LIMIT {
            return Err(err("io", "Operation journal exceeds metadata limit"));
        }
        let temp = format!(".roost-tmp-journal-{}", platform::random_id()?);
        let published_identity = self.directory.write_new(&temp, &bytes, 0o600)?;
        // read validates protection on the existing journal; do not replace a malformed
        // or redirected object even when this transaction owns its directory.
        if self.directory.entry(JOURNAL)?.map(|e| e.identity) != self.journal_identity {
            return Err(recovery("Operation journal identity changed"));
        }
        if self.journal_identity.is_some() {
            let old: Intent = read_record(&self.directory, JOURNAL)?;
            if old.root_id != self.root_id || old.operation_id != intent.operation_id {
                return Err(recovery("Operation journal changed during publication"));
            }
        }
        self.directory.rename(&temp, &self.directory, JOURNAL)?;
        self.journal_identity = Some(published_identity);
        self.directory.sync()
    }
    fn begin(&mut self, operation: Operation, id: Option<String>) -> Result<Directory> {
        self.journal_ready(operation == Operation::DesktopCreate)?;
        let operation_id = platform::random_id()?;
        let next_generation = self
            .registry
            .generation
            .checked_add(1)
            .ok_or_else(|| err("ownership", "Registry generation exhausted"))?;
        self.intent = Some(Intent {
            schema_version: 1,
            root_id: self.root_id.clone(),
            operation_id: operation_id.clone(),
            operation,
            registration_id: id,
            prior_generation: self.registry.generation,
            next_generation,
            phase: Phase::Prepared,
            artifacts: vec![],
        });
        self.save_intent()?;
        let stage = self.stages.create_dir(&operation_id)?;
        let evidence = state(stage.identity()?);
        self.intent.as_mut().unwrap().artifacts.push(Artifact {
            role: Role::StagingDirectory,
            destination: stage.path.clone(),
            staged: None,
            before: None,
            after: Some(evidence),
            action: Action::Create,
            completed: true,
        });
        self.save_intent()?;
        self.stages.sync()?;
        Ok(stage)
    }
    fn record(&mut self, artifact: Artifact) -> Result<()> {
        self.intent.as_mut().unwrap().artifacts.push(artifact);
        self.save_intent()
    }
    #[allow(clippy::too_many_arguments)] // Explicit before/after evidence avoids a parallel transaction builder.
    fn stage_file(
        &mut self,
        stage: &Directory,
        role: Role,
        destination: PathBuf,
        before: Option<ObjectState>,
        bytes: &[u8],
        mode: u32,
        mut after: ObjectState,
    ) -> Result<()> {
        let staged_name = format!(
            ".roost-tmp-{}",
            self.intent.as_ref().unwrap().artifacts.len()
        );
        let identity = stage.write_new(&staged_name, bytes, mode)?;
        after.object_identity = identity.clone();
        self.record(Artifact {
            role,
            destination,
            staged: Some(Staged {
                path: stage.path.join(staged_name),
                object_identity: identity,
            }),
            action: if before.is_none() {
                Action::Create
            } else {
                Action::Replace
            },
            before,
            after: Some(after),
            completed: false,
        })
    }
    fn stage_launchers(
        &mut self,
        stage: &Directory,
        old: Option<&Registration>,
        new: &Registration,
    ) -> Result<()> {
        for (destination, bytes) in launch::templates(&self.root, &self.root_id, new)? {
            let before = match old {
                Some(r) => self.launcher_state(r, &destination)?,
                None => {
                    if self.bin.entry(file_name(&destination)?)?.is_some() {
                        return Err(err(
                            "collision",
                            format!("Foreign launcher destination {}", destination.display()),
                        ));
                    }
                    None
                }
            };
            let mut after = state(self.directory.identity()?);
            after.launcher_binding = new.launcher_binding.clone();
            after.launcher_form = Some(form(&destination));
            self.stage_file(
                stage,
                Role::Launcher,
                destination,
                before,
                &bytes,
                0o700,
                after,
            )?;
        }
        Ok(())
    }
    fn launcher_state(&self, r: &Registration, path: &Path) -> Result<Option<ObjectState>> {
        let name = file_name(path)?;
        let Some(mut evidence) = file_state(&self.bin, name, true)? else {
            return Ok(None);
        };
        let expected = launch::templates(&self.root, &self.root_id, r)?
            .into_iter()
            .find(|(p, _)| p == path)
            .ok_or_else(|| err("ownership", "Unexpected launcher form"))?
            .1;
        if launcher_bytes(&self.bin, name)?.as_deref() != Some(expected.as_slice()) {
            return Err(err(
                "collision",
                format!(
                    "Launcher is not the exact owned template: {}",
                    path.display()
                ),
            ));
        }
        evidence.launcher_binding = r.launcher_binding.clone();
        evidence.launcher_form = Some(form(path));
        Ok(Some(evidence))
    }
    fn delete_launchers(&mut self, r: &Registration) -> Result<()> {
        // Preflight every form before publication starts: a later collision must never
        // cause an earlier launcher to disappear.
        let artifacts = launch::templates(&self.root, &self.root_id, r)?
            .into_iter()
            .map(|(path, _)| {
                Ok(self.launcher_state(r, &path)?.map(|before| Artifact {
                    role: Role::Launcher,
                    destination: path,
                    staged: None,
                    before: Some(before),
                    after: None,
                    action: Action::Delete,
                    completed: false,
                }))
            })
            .collect::<Result<Vec<_>>>()?;
        for a in artifacts.into_iter().flatten() {
            if !self
                .intent
                .as_ref()
                .unwrap()
                .artifacts
                .iter()
                .any(|record| record.destination == a.destination)
            {
                self.record(a)?;
            }
        }
        Ok(())
    }
    fn parent_for(&self, path: &Path) -> Result<Directory> {
        let parent = path
            .parent()
            .ok_or_else(|| recovery("Invalid journal destination"))?;
        // Opening the entire absolute chain again rejects namespace redirection. The
        // expected parent identity is separately verified for all managed roots.
        let directory = Directory::open(parent, true)?;
        let expected = if parent == self.root {
            Some(&self.directory)
        } else if parent == self.bin.path {
            Some(&self.bin)
        } else if parent == self.profiles_dir.path {
            Some(&self.profiles_dir)
        } else if parent == self.stages.path {
            Some(&self.stages)
        } else {
            None
        };
        if let Some(expected) = expected
            && directory.identity()? != expected.identity()?
        {
            return Err(recovery("Destination parent was replaced"));
        }
        if parent.parent() == Some(self.profiles_dir.path.as_path()) {
            let registration = self
                .registry
                .registrations
                .iter()
                .find(|r| r.directory.as_deref() == Some(parent))
                .ok_or_else(|| recovery("Token destination has no owned registration"))?;
            if Some(directory.identity()?) != registration.directory_identity {
                return Err(recovery("Profile destination parent was replaced"));
            }
        }
        if parent.parent() == Some(self.stages.path.as_path()) {
            let j = self
                .intent
                .as_ref()
                .ok_or_else(|| recovery("Staging parent has no journal"))?;
            let artifact = j
                .artifacts
                .iter()
                .find(|a| a.role == Role::StagingDirectory && a.destination == parent)
                .ok_or_else(|| recovery("Staging parent has no recorded identity"))?;
            if Some(directory.identity()?)
                != artifact.after.as_ref().map(|s| s.object_identity.clone())
            {
                return Err(recovery("Staging parent was replaced"));
            }
        }
        Ok(directory)
    }
    fn journal_registration(&self, a: &Artifact, evidence: &ObjectState) -> Result<Registration> {
        let j = self
            .intent
            .as_ref()
            .ok_or_else(|| recovery("Launcher evidence has no journal"))?;
        let registration_id = j
            .registration_id
            .clone()
            .ok_or_else(|| recovery("Launcher evidence has no registration ID"))?;
        let filename = file_name(&a.destination)?;
        let filename = match evidence.launcher_form {
            Some(Form::Cmd) => filename.strip_suffix(".cmd"),
            Some(Form::Ps1) => filename.strip_suffix(".ps1"),
            Some(Form::Sh) => Some(filename),
            None => None,
        }
        .ok_or_else(|| recovery("Invalid launcher form"))?;
        let name = filename
            .strip_prefix("roost-")
            .ok_or_else(|| recovery("Invalid launcher destination"))?
            .to_owned();
        validate_name(&name).map_err(|_| recovery("Invalid journal profile name"))?;
        Ok(Registration {
            registration_id,
            name,
            kind: Kind::DefaultAlias,
            state: State::Active,
            directory: None,
            directory_identity: None,
            profile_id: None,
            upstream_linked_default: false,
            launcher_binding: evidence.launcher_binding.clone(),
        })
    }
    fn matches(&self, artifact: &Artifact, expected: Option<&ObjectState>) -> Result<bool> {
        if artifact.role == Role::Marker
            && self
                .intent
                .as_ref()
                .is_some_and(|j| j.operation == Operation::Purge)
        {
            let profile = artifact
                .destination
                .parent()
                .ok_or_else(|| recovery("Marker has no parent"))?;
            if profile.parent() == Some(self.profiles_dir.path.as_path())
                && self.profiles_dir.entry(file_name(profile)?)?.is_none()
            {
                return Ok(expected.is_none());
            }
        }
        let parent = self.parent_for(&artifact.destination)?;
        let name = file_name(&artifact.destination)?;
        let actual = parent.entry(name)?;
        let Some(expected) = expected else {
            return Ok(actual.is_none());
        };
        let Some(entry) = actual else {
            return Ok(false);
        };
        if entry.identity != expected.object_identity || entry.is_link {
            return Ok(false);
        }
        match artifact.role {
            Role::Profile => {
                if !entry.is_dir {
                    return Ok(false);
                }
                let child = parent.child(name, true)?;
                if child.entry(PROFILE_MARKER)?.is_none() {
                    return Ok(self.intent.as_ref().is_some_and(|j| {
                        j.operation == Operation::Add && j.phase == Phase::Cleanup
                    }) && child.entries()?.is_empty());
                }
                let marker: ProfileMarker = read_record(&child, PROFILE_MARKER)?;
                Ok(marker.schema_version == 1
                    && marker.root_id == self.root_id
                    && marker.directory_identity == expected.object_identity
                    && Some(&marker.profile_id) == expected.profile_id.as_ref())
            }
            Role::StagingDirectory => {
                let _ = parent.child(name, true)?;
                Ok(entry.is_dir)
            }
            Role::Registry => {
                let registry: Registry = read_record(&parent, name)?;
                Ok(registry.schema_version == 1
                    && registry.root_id == self.root_id
                    && Some(registry.generation) == expected.registry_generation)
            }
            Role::Launcher => {
                let r = self.journal_registration(artifact, expected)?;
                let bytes = launch::templates(&self.root, &self.root_id, &r)?
                    .into_iter()
                    .find(|(p, _)| *p == artifact.destination)
                    .ok_or_else(|| recovery("Invalid journal launcher"))?
                    .1;
                Ok(entry.is_file
                    && entry.nlink == 1
                    && launcher_bytes(&parent, name)?.as_deref() == Some(bytes.as_slice()))
            }
            Role::Token => {
                let _ = parent.open_file(name, true, false)?;
                Ok(entry.is_file && entry.nlink == 1)
            }
            Role::DesktopData => self.desktop_matches(&parent, name, &entry, artifact.action),
            Role::Store => self.store_matches(&parent, name, &entry),
            Role::Marker => {
                let marker: ProfileMarker = read_record(&parent, name)?;
                Ok(entry.is_file
                    && entry.nlink == 1
                    && marker.schema_version == 1
                    && marker.root_id == self.root_id
                    && Some(&marker.profile_id) == expected.profile_id.as_ref()
                    && marker.directory_identity == parent.identity()?)
            }
        }
    }
    fn publish(&mut self, index: usize) -> Result<()> {
        self.check_root()?;
        if platform::cancelled() {
            return Err(Error::cancelled());
        }
        let a = self.intent.as_ref().unwrap().artifacts[index].clone();
        if a.role == Role::StagingDirectory {
            return Ok(());
        }
        if self.matches(&a, a.after.as_ref())? {
            self.intent.as_mut().unwrap().artifacts[index].completed = true;
            return self.save_intent();
        }
        if !self.matches(&a, a.before.as_ref())? && !self.purge_empty_proven(&a)? {
            return Err(recovery(format!(
                "Destination changed before publication: {}",
                a.destination.display()
            )));
        }
        let parent = self.parent_for(&a.destination)?;
        let name = file_name(&a.destination)?;
        if a.action == Action::Delete {
            parent.remove(name, matches!(a.role, Role::Profile | Role::DesktopData))?;
        } else {
            let staged = a
                .staged
                .as_ref()
                .ok_or_else(|| recovery("Replacement has no staged object"))?;
            let stage_parent = self.parent_for(&staged.path)?;
            let staged_name = file_name(&staged.path)?;
            if stage_parent
                .entry(staged_name)?
                .as_ref()
                .map(|e| &e.identity)
                != Some(&staged.object_identity)
            {
                return Err(recovery("Staged object was replaced"));
            }
            stage_parent.rename(staged_name, &parent, name)?;
            stage_parent.sync()?;
        }
        parent.sync()?;
        if a.role == Role::Registry {
            self.registry_identity = a.after.as_ref().map(|s| s.object_identity.clone());
        }
        if !self.matches(&a, a.after.as_ref())? {
            return Err(recovery(
                "Published object does not match recorded evidence",
            ));
        }
        self.intent.as_mut().unwrap().artifacts[index].completed = true;
        self.save_intent()
    }
    fn commit(&mut self, mut next: Registry, stage: &Directory) -> Result<()> {
        next.generation = self.intent.as_ref().unwrap().next_generation;
        let bytes = encode(&next)?;
        if bytes.len() > LIMIT {
            return Err(err("io", "Registry exceeds metadata limit"));
        }
        let mut before = file_state(&self.directory, REGISTRY, true)?;
        if let Some(before) = &mut before {
            before.registry_generation = Some(self.registry.generation);
        }
        if before.is_none() && self.intent.as_ref().unwrap().operation != Operation::Initialize {
            return Err(recovery("Committed registry disappeared"));
        }
        let mut after = state(self.directory.identity()?);
        after.registry_generation = Some(next.generation);
        self.stage_file(
            stage,
            Role::Registry,
            self.root.join(REGISTRY),
            before,
            &bytes,
            0o600,
            after,
        )?;
        self.intent.as_mut().unwrap().phase = Phase::Publishing;
        self.save_intent()?;
        for index in 0..self.intent.as_ref().unwrap().artifacts.len() {
            self.publish(index)?;
        }
        self.registry = next;
        self.intent.as_mut().unwrap().phase = Phase::Committed;
        self.save_intent()?;
        self.cleanup()
    }
    fn cleanup(&mut self) -> Result<()> {
        let journal = self
            .intent
            .clone()
            .ok_or_else(|| recovery("Missing cleanup journal"))?;
        if let Some(a) = journal
            .artifacts
            .iter()
            .find(|a| a.role == Role::StagingDirectory)
        {
            let name = file_name(&a.destination)?;
            if self.stages.entry(name)?.is_some() {
                if !self.matches(a, a.after.as_ref())? {
                    return Err(recovery("Staging identity changed; preserve it"));
                }
                let stage = self.stages.child(name, true)?;
                for entry in stage.entries()? {
                    let path = stage.path.join(&entry);
                    let recorded = journal
                        .artifacts
                        .iter()
                        .find(|a| a.staged.as_ref().is_some_and(|s| s.path == path))
                        .ok_or_else(|| {
                            recovery("Unrecorded staged object; preserve pending state")
                        })?;
                    let expected = &recorded.staged.as_ref().unwrap().object_identity;
                    if stage.entry(&entry)?.as_ref().map(|e| &e.identity) != Some(expected) {
                        return Err(recovery("Staged cleanup object changed"));
                    }
                    if recorded.role == Role::Profile {
                        let mut probe = recorded.clone();
                        probe.destination = path.clone();
                        if !self.matches(&probe, recorded.after.as_ref())? {
                            return Err(recovery("Staged profile ownership changed"));
                        }
                        let child = stage.child(&entry, true)?;
                        if child.entry(PROFILE_MARKER)?.is_some() {
                            child.purge_children(PROFILE_MARKER)?;
                            if !self.matches(&probe, recorded.after.as_ref())? {
                                return Err(recovery(
                                    "Staged profile ownership changed during cleanup",
                                ));
                            }
                            child.remove(PROFILE_MARKER, false)?;
                        }
                        stage.remove(&entry, true)?;
                    } else {
                        let _ = stage.open_file(&entry, true, false)?;
                        stage.remove(&entry, false)?;
                    }
                }
                self.stages.remove(name, true)?;
                self.stages.sync()?;
            }
        } else if !self.stages.entries()?.is_empty() {
            return Err(recovery(
                "Unrecorded staging parent; preserve pending state",
            ));
        }
        if file_state(&self.directory, JOURNAL, true)?.map(|s| s.object_identity)
            != self.journal_identity
        {
            return Err(recovery("Journal identity changed before cleanup"));
        }
        let actual: Intent = read_record(&self.directory, JOURNAL)?;
        if actual.operation_id != journal.operation_id || actual.root_id != self.root_id {
            return Err(recovery("Journal changed before cleanup"));
        }
        self.directory.remove(JOURNAL, false)?;
        self.directory.sync()?;
        self.intent = None;
        self.journal_identity = None;
        Ok(())
    }
    fn validate_intent(&self, j: &Intent) -> Result<()> {
        if j.schema_version != 1
            || j.root_id != self.root_id
            || !valid_id(&j.operation_id)
            || j.prior_generation.checked_add(1) != Some(j.next_generation)
            || (self.registry.generation != j.prior_generation
                && self.registry.generation != j.next_generation)
            || j.registration_id.as_ref().is_some_and(|id| !valid_id(id))
            || (!matches!(j.operation, Operation::Initialize | Operation::StoreCreate)
                && j.registration_id.is_none())
        {
            return Err(recovery("Invalid operation journal header or generation"));
        }
        let stage_path = self.stages.path.join(&j.operation_id);
        let registration = self
            .registry
            .registrations
            .iter()
            .find(|r| Some(&r.registration_id) == j.registration_id.as_ref());
        let prior = self.registry.generation == j.prior_generation;
        let mapping_valid = match j.operation {
            Operation::Initialize => {
                j.registration_id.is_none()
                    && j.prior_generation == 0
                    && self.registry.registrations.is_empty()
            }
            Operation::Add | Operation::Register => {
                if prior {
                    registration.is_none()
                } else {
                    registration.is_some()
                }
            }
            Operation::Reuse => registration.is_some(),
            Operation::Remove => !prior || registration.is_some(),
            Operation::Purge => {
                if prior {
                    registration.is_some_and(|r| r.kind == Kind::Owned)
                } else {
                    registration.is_none()
                }
            }
            Operation::TokenSet | Operation::TokenClear => {
                registration.is_some_and(|r| r.kind == Kind::Owned && r.state == State::Active)
            }
            Operation::DesktopCreate => registration
                .is_some_and(|r| r.kind != Kind::DefaultAlias && r.state == State::Active),
            Operation::StoreCreate => j.registration_id.is_none(),
        };
        if !mapping_valid {
            return Err(recovery(
                "Operation conflicts with committed registration mapping",
            ));
        }
        let mut destinations = HashSet::new();
        let mut stage_count = 0;
        let mut registry_count = 0;
        let mut journal_name: Option<String> = registration.map(|r| r.name.clone());
        for a in &j.artifacts {
            checked_path(&a.destination)?;
            if !destinations.insert(a.destination.clone()) {
                return Err(recovery("Duplicate journal destination"));
            }
            match a.action {
                Action::Create if a.before.is_none() && a.after.is_some() => (),
                Action::Replace if a.before.is_some() && a.after.is_some() => (),
                Action::Delete if a.before.is_some() && a.after.is_none() && a.staged.is_none() => {
                }
                _ => return Err(recovery("Impossible artifact action/evidence")),
            }
            let action_valid = match a.role {
                Role::Profile => match j.operation {
                    Operation::Add => a.action == Action::Create,
                    Operation::Purge => a.action == Action::Delete,
                    _ => false,
                },
                Role::Launcher => match j.operation {
                    Operation::Add | Operation::Register => a.action == Action::Create,
                    Operation::Reuse => a.action != Action::Delete || j.phase == Phase::Cleanup,
                    Operation::Remove | Operation::Purge => a.action == Action::Delete,
                    _ => false,
                },
                Role::Token => match j.operation {
                    Operation::TokenSet => a.action != Action::Delete,
                    Operation::TokenClear => a.action == Action::Delete,
                    _ => false,
                },
                Role::Marker => j.operation == Operation::Purge && a.action == Action::Delete,
                Role::Registry => {
                    a.action
                        == if j.operation == Operation::Initialize {
                            Action::Create
                        } else {
                            Action::Replace
                        }
                }
                Role::StagingDirectory => a.action == Action::Create,
                Role::DesktopData => match j.operation {
                    Operation::DesktopCreate => a.action == Action::Create,
                    Operation::Purge | Operation::Remove => a.action == Action::Delete,
                    _ => false,
                },
                Role::Store => j.operation == Operation::StoreCreate && a.action == Action::Create,
            };
            if !action_valid {
                return Err(recovery("Artifact action conflicts with operation"));
            }
            if let Some(staged) = &a.staged {
                checked_path(&staged.path)?;
                if staged.path.parent() != Some(stage_path.as_path())
                    || !file_name(&staged.path)?.starts_with(".roost-tmp-")
                    || a.after.as_ref().map(|s| &s.object_identity) != Some(&staged.object_identity)
                {
                    return Err(recovery("Invalid staged object evidence"));
                }
            }
            let name = match a.role {
                Role::Profile => {
                    if a.destination.parent() != Some(self.profiles_dir.path.as_path()) {
                        return Err(recovery("Invalid profile destination"));
                    }
                    Some(file_name(&a.destination)?.to_owned())
                }
                Role::Launcher => {
                    if a.destination.parent() != Some(self.bin.path.as_path()) {
                        return Err(recovery("Invalid launcher destination"));
                    }
                    let evidence = a.after.as_ref().or(a.before.as_ref()).unwrap();
                    Some(self.journal_registration(a, evidence)?.name)
                }
                Role::Token | Role::Marker => {
                    if let Some(r) = registration.filter(|r| r.kind == Kind::Owned) {
                        let expected =
                            r.directory
                                .as_ref()
                                .unwrap()
                                .join(if a.role == Role::Token {
                                    ".roost-token"
                                } else {
                                    PROFILE_MARKER
                                });
                        if a.destination != expected {
                            return Err(recovery("Invalid token/marker destination"));
                        }
                        Some(r.name.clone())
                    } else if a.role == Role::Marker && j.operation == Operation::Purge {
                        let profile = j
                            .artifacts
                            .iter()
                            .find(|p| {
                                p.role == Role::Profile
                                    && a.destination == p.destination.join(PROFILE_MARKER)
                            })
                            .ok_or_else(|| recovery("Marker has no profile evidence"))?;
                        Some(file_name(&profile.destination)?.to_owned())
                    } else {
                        return Err(recovery("Token/marker operation has no owned registration"));
                    }
                }
                Role::Registry => {
                    registry_count += 1;
                    if a.destination != self.root.join(REGISTRY)
                        || a.after.as_ref().and_then(|s| s.registry_generation)
                            != Some(j.next_generation)
                        || (j.operation != Operation::Initialize
                            && a.before.as_ref().and_then(|s| s.registry_generation)
                                != Some(j.prior_generation))
                    {
                        return Err(recovery("Invalid registry artifact"));
                    }
                    None
                }
                Role::StagingDirectory => {
                    stage_count += 1;
                    if a.destination != stage_path
                        || a.action != Action::Create
                        || a.staged.is_some()
                    {
                        return Err(recovery("Invalid staging parent evidence"));
                    }
                    None
                }
                Role::DesktopData => {
                    self.validate_desktop_artifact(a, j.registration_id.as_ref())?;
                    None
                }
                Role::Store => {
                    self.validate_store_artifact(a)?;
                    None
                }
            };
            if let Some(name) = name {
                validate_name(&name).map_err(|_| recovery("Invalid profile name in journal"))?;
                if journal_name
                    .as_ref()
                    .is_some_and(|expected| *expected != name)
                {
                    return Err(recovery("Journal artifacts name different registrations"));
                }
                journal_name = Some(name);
            }
            for s in [a.before.as_ref(), a.after.as_ref()].into_iter().flatten() {
                validate_identity(&s.object_identity)?;
                if s.profile_id.as_ref().is_some_and(|id| !valid_id(id)) {
                    return Err(recovery("Invalid profile ID in journal"));
                }
                let valid = match a.role {
                    Role::Profile | Role::Marker => {
                        s.profile_id.is_some()
                            && s.launcher_binding.is_none()
                            && s.launcher_form.is_none()
                            && s.registry_generation.is_none()
                    }
                    Role::Launcher => {
                        s.profile_id.is_none()
                            && s.launcher_binding
                                .as_ref()
                                .is_some_and(|b| b.format_version == 1)
                            && s.launcher_form.is_some()
                            && s.registry_generation.is_none()
                    }
                    Role::Registry => {
                        s.profile_id.is_none()
                            && s.launcher_binding.is_none()
                            && s.launcher_form.is_none()
                            && s.registry_generation.is_some()
                    }
                    Role::Token | Role::StagingDirectory | Role::DesktopData | Role::Store => {
                        s.profile_id.is_none()
                            && s.launcher_binding.is_none()
                            && s.launcher_form.is_none()
                            && s.registry_generation.is_none()
                    }
                };
                if !valid {
                    return Err(recovery("Inapplicable/missing artifact state fields"));
                }
                if let Some(binding) = &s.launcher_binding {
                    checked_path(&binding.executable)?;
                }
            }
        }
        if stage_count > 1 || registry_count > 1 {
            return Err(recovery("Duplicate staging/registry artifacts"));
        }
        if matches!(j.phase, Phase::Publishing | Phase::Committed) && registry_count != 1 {
            return Err(recovery("Publishing journal has no registry artifact"));
        }
        if j.operation == Operation::Purge
            && matches!(j.phase, Phase::Publishing | Phase::Committed)
            && (!j.artifacts.iter().any(|a| a.role == Role::Profile)
                || !j.artifacts.iter().any(|a| a.role == Role::Marker))
        {
            return Err(recovery(
                "Publishing purge lacks complete ownership/deletion evidence",
            ));
        }
        Ok(())
    }
    fn recover(&mut self) -> Result<()> {
        let journal = self.intent.clone().unwrap();
        self.validate_intent(&journal)?;
        if journal.operation == Operation::Purge {
            return Err(recovery(
                "Partial purge requires explicit remove NAME --purge and a renewed quiet-writer acknowledgement; deletion will not continue automatically",
            ));
        }
        if self.registry.generation == journal.next_generation {
            // Registry last means all prior artifact publication must be provable.
            for a in &journal.artifacts {
                if a.role != Role::StagingDirectory && !self.matches(a, a.after.as_ref())? {
                    return Err(recovery("Committed operation has a changed destination"));
                }
            }
            return self.cleanup();
        }
        if journal.operation == Operation::Reuse {
            return self.rollback_refresh();
        }
        if journal.operation == Operation::DesktopCreate {
            return self.recover_desktop_create();
        }
        if journal.operation == Operation::StoreCreate {
            return self.recover_store_create();
        }
        let mut published = Vec::new();
        for (index, a) in journal.artifacts.iter().enumerate() {
            if a.role == Role::StagingDirectory {
                continue;
            }
            if self.matches(a, a.before.as_ref())? {
                continue;
            }
            if self.matches(a, a.after.as_ref())? {
                published.push(index);
            } else {
                return Err(recovery(format!(
                    "Pending destination matches neither before nor after: {}",
                    a.destination.display()
                )));
            }
        }
        match journal.operation {
            Operation::Add | Operation::Register => {
                self.intent.as_mut().unwrap().phase = Phase::Cleanup;
                self.save_intent()?;
                for index in published.into_iter().rev() {
                    let a = &journal.artifacts[index];
                    if a.role == Role::Registry {
                        return Err(recovery("Registry state changed during recovery"));
                    }
                    if a.before.is_some() {
                        return Err(recovery("Unexpected replacement in new registration"));
                    }
                    let parent = self.parent_for(&a.destination)?;
                    if !self.matches(a, a.after.as_ref())? {
                        return Err(recovery("Recovery destination changed"));
                    }
                    if a.role == Role::Profile {
                        let directory = parent.child(file_name(&a.destination)?, true)?;
                        if directory.entry(PROFILE_MARKER)?.is_some() {
                            directory.purge_children(PROFILE_MARKER)?;
                            if !self.matches(a, a.after.as_ref())? {
                                return Err(recovery("Profile ownership changed during rollback"));
                            }
                            directory.remove(PROFILE_MARKER, false)?;
                        }
                    }
                    parent.remove(file_name(&a.destination)?, a.role == Role::Profile)?;
                    parent.sync()?;
                }
                self.cleanup()
            }
            Operation::Reuse => self.rollback_refresh(),
            Operation::Remove => {
                self.cleanup()?;
                // Preserve prior bookkeeping; the explicit next reuse/remove supplies
                // intent to repair or finish any missing owned launchers.
                Err(err("ownership", "Interrupted removal preserved the previous registration; some owned launchers may already be absent").next("Run roost reuse NAME to restore launchers, or repeat roost remove NAME."))
            }
            Operation::TokenSet | Operation::TokenClear => {
                if published
                    .iter()
                    .any(|i| journal.artifacts[*i].role == Role::Token)
                {
                    let stage_artifact = journal
                        .artifacts
                        .iter()
                        .find(|a| a.role == Role::StagingDirectory)
                        .ok_or_else(|| recovery("Token recovery has no staged directory"))?;
                    if !self.matches(stage_artifact, stage_artifact.after.as_ref())? {
                        return Err(recovery("Token staging changed"));
                    }
                    let stage = self
                        .stages
                        .child(file_name(&stage_artifact.destination)?, true)?;
                    // If the staged complete next registry already exists, publish it;
                    // otherwise stage bookkeeping now without inspecting token bytes.
                    if let Some(index) = journal
                        .artifacts
                        .iter()
                        .position(|a| a.role == Role::Registry)
                    {
                        self.publish(index)?;
                        self.registry = read_record(&self.directory, REGISTRY)?;
                        self.cleanup()?;
                    } else {
                        let next = self.registry.clone();
                        self.commit(next, &stage)?;
                    }
                    Err(err("ownership", "Interrupted token operation left the published new/absent manager token; bookkeeping is now complete").next("Repeat roost token NAME if another change is intended."))
                } else {
                    self.cleanup()
                }
            }
            Operation::Initialize => {
                let stage_artifact = journal
                    .artifacts
                    .iter()
                    .find(|a| a.role == Role::StagingDirectory)
                    .ok_or_else(|| recovery("Initialization stage was not recorded"))?;
                if !self.matches(stage_artifact, stage_artifact.after.as_ref())? {
                    return Err(recovery("Initialization staging identity changed"));
                }
                let stage = self
                    .stages
                    .child(file_name(&stage_artifact.destination)?, true)?;
                if let Some(index) = journal
                    .artifacts
                    .iter()
                    .position(|a| a.role == Role::Registry)
                {
                    self.publish(index)?;
                    self.registry = read_record(&self.directory, REGISTRY)?;
                    self.cleanup()
                } else {
                    self.commit(self.registry.clone(), &stage)
                }
            }
            Operation::Purge | Operation::DesktopCreate | Operation::StoreCreate => unreachable!(),
        }
    }
    fn rollback_refresh(&mut self) -> Result<()> {
        let journal = self.intent.clone().unwrap();
        let old = self
            .registry
            .registrations
            .iter()
            .find(|r| Some(&r.registration_id) == journal.registration_id.as_ref())
            .cloned()
            .ok_or_else(|| recovery("Refresh has no previous registration"))?;
        let stage_artifact = journal
            .artifacts
            .iter()
            .find(|a| a.role == Role::StagingDirectory)
            .ok_or_else(|| recovery("Refresh has no staging identity"))?;
        if !self.matches(stage_artifact, stage_artifact.after.as_ref())? {
            return Err(recovery("Refresh staging identity changed"));
        }
        let stage = self
            .stages
            .child(file_name(&stage_artifact.destination)?, true)?;
        self.intent.as_mut().unwrap().phase = Phase::Cleanup;
        self.save_intent()?;
        for index in 0..self.intent.as_ref().unwrap().artifacts.len() {
            let a = self.intent.as_ref().unwrap().artifacts[index].clone();
            if a.role != Role::Launcher {
                continue;
            }
            let at_before = self.matches(&a, a.before.as_ref())?;
            let at_after = self.matches(&a, a.after.as_ref())?;
            if !at_before && !at_after {
                return Err(recovery("Refresh destination changed during rollback"));
            }
            let restores_old = a.staged.as_ref().is_some_and(|s| {
                file_name(&s.path).is_ok_and(|n| n.starts_with(".roost-tmp-rollback-"))
            });
            if restores_old {
                // A rollback intent was persisted before its publication. Its inode
                // belongs to the newly staged old template, not the overwritten file.
                if at_before {
                    self.publish(index)?;
                }
                continue;
            }
            if a.action == Action::Delete {
                if at_before {
                    self.publish(index)?;
                }
                continue;
            }
            if at_before {
                continue;
            } // The original replacement never published.
            let mut replacement = a.clone();
            replacement.before = a.after.clone();
            replacement.completed = false;
            if let Some(before) = &a.before {
                if before.launcher_binding != old.launcher_binding {
                    return Err(recovery(
                        "Refresh old binding differs from committed registration",
                    ));
                }
                let bytes = launch::templates(&self.root, &self.root_id, &old)?
                    .into_iter()
                    .find(|(p, _)| *p == a.destination)
                    .ok_or_else(|| recovery("Refresh form is unsupported"))?
                    .1;
                let name = format!(".roost-tmp-rollback-{}", index);
                let identity = stage.write_new(&name, &bytes, 0o700)?;
                let mut after = before.clone();
                after.object_identity = identity.clone();
                replacement.after = Some(after);
                replacement.staged = Some(Staged {
                    path: stage.path.join(name),
                    object_identity: identity,
                });
                replacement.action = Action::Replace;
            } else {
                replacement.action = Action::Delete;
                replacement.after = None;
                replacement.staged = None;
            }
            self.intent.as_mut().unwrap().artifacts[index] = replacement;
            self.save_intent()?;
            self.publish(index)?;
        }
        self.cleanup()
    }
    pub fn add(
        &mut self,
        name: &str,
        link_default: bool,
        copy: Option<&Path>,
    ) -> Result<Vec<String>> {
        self.mutation_ready()?;
        validate_name(name)?;
        if link_default && copy.is_some() {
            return Err(err("usage", "Link-default and copying cannot be combined"));
        }
        if self
            .registry
            .registrations
            .iter()
            .any(|r| r.name.eq_ignore_ascii_case(name))
        {
            return Err(err(
                "collision",
                format!("Active or retained profile {name} already exists"),
            ));
        }
        if self.profiles_dir.entry(name)?.is_some() {
            return Err(err("collision", "Profile destination already exists"));
        }
        let source = copy.map(platform::absolute).transpose()?;
        if let Some(path) = &source {
            let directory = Directory::open(path, false)?;
            let identity = directory.identity()?;
            // Copying from an owned profile is allowed even though it lies inside the
            // root: copy excludes Roost markers and tokens at every depth.
            let owned_source = self.registry.registrations.iter().any(|r| {
                r.kind == Kind::Owned
                    && r.directory_identity.as_ref() == Some(&identity)
                    && self.profile_directory(r).is_ok()
            });
            if !owned_source {
                self.no_overlap(path, &identity, false)?;
            }
        }
        let mut registration = Registration {
            registration_id: platform::random_id()?,
            name: name.to_owned(),
            kind: if link_default {
                Kind::DefaultAlias
            } else {
                Kind::Owned
            },
            state: State::Active,
            directory: None,
            directory_identity: None,
            profile_id: None,
            upstream_linked_default: false,
            launcher_binding: Some(current_binding()?),
        };
        // Preflight every launcher before creating any journal/staged profile.
        for (path, _) in launch::templates(&self.root, &self.root_id, &registration)? {
            if self.bin.entry(file_name(&path)?)?.is_some() {
                return Err(err(
                    "collision",
                    format!("Launcher destination exists: {}", path.display()),
                ));
            }
        }
        let stage = self.begin(Operation::Add, Some(registration.registration_id.clone()))?;
        let mut messages = vec![];
        if !link_default {
            let profile = stage.create_dir(".roost-tmp-profile")?;
            let identity = profile.identity()?;
            let profile_id = platform::random_id()?;
            let marker = ProfileMarker {
                schema_version: 1,
                root_id: self.root_id.clone(),
                profile_id: profile_id.clone(),
                directory_identity: identity.clone(),
            };
            profile.write_new(PROFILE_MARKER, &encode(&marker)?, 0o600)?;
            profile.sync()?;
            registration.directory = Some(self.profiles_dir.path.join(name));
            registration.directory_identity = Some(identity.clone());
            registration.profile_id = Some(profile_id.clone());
            let mut after = state(identity.clone());
            after.profile_id = Some(profile_id);
            self.record(Artifact {
                role: Role::Profile,
                destination: registration.directory.clone().unwrap(),
                staged: Some(Staged {
                    path: profile.path.clone(),
                    object_identity: identity,
                }),
                before: None,
                after: Some(after),
                action: Action::Create,
                completed: false,
            })?;
            if let Some(source) = source {
                messages.extend(platform::copy_profile(&source, &profile)?);
                messages.push("Copied settings/history require native login/authentication verification; copying does not transfer an account.".into());
            }
        }
        self.stage_launchers(&stage, None, &registration)?;
        let mut next = self.registry.clone();
        next.registrations.push(registration);
        self.commit(next, &stage)?;
        Ok(messages)
    }
    pub fn register(&mut self, name: &str, path: &Path) -> Result<Vec<String>> {
        self.mutation_ready()?;
        validate_name(name)?;
        let path = platform::absolute(path)?;
        self.upstream_shape(&path)?;
        let directory = Directory::open(&path, false)?;
        let identity = directory.identity()?;
        let linked = self.linked_default(&directory)?;
        if let Some(r) = self
            .registry
            .registrations
            .iter()
            .find(|r| r.name.eq_ignore_ascii_case(name))
            .cloned()
        {
            if r.kind == Kind::Owned
                || r.directory.as_ref() != Some(&path)
                || r.directory_identity.as_ref() != Some(&identity)
                || r.upstream_linked_default != linked
            {
                return Err(err(
                    "collision",
                    "Existing registration mapping or upstream marker differs",
                ));
            }
            return self.reuse(&r.name);
        }
        self.no_overlap(&path, &identity, true)?;
        let registration = Registration {
            registration_id: platform::random_id()?,
            name: name.to_owned(),
            kind: if linked {
                Kind::DefaultAlias
            } else {
                Kind::Upstream
            },
            state: State::Active,
            directory: Some(path),
            directory_identity: Some(identity),
            profile_id: None,
            upstream_linked_default: linked,
            launcher_binding: Some(current_binding()?),
        };
        for (path, _) in launch::templates(&self.root, &self.root_id, &registration)? {
            if self.bin.entry(file_name(&path)?)?.is_some() {
                return Err(err("collision", "Launcher destination already exists"));
            }
        }
        let stage = self.begin(
            Operation::Register,
            Some(registration.registration_id.clone()),
        )?;
        self.stage_launchers(&stage, None, &registration)?;
        let mut next = self.registry.clone();
        next.registrations.push(registration);
        self.commit(next, &stage)?;
        Ok(vec![
            "Upstream data, tokens and launchers remain under upstream ownership.".into(),
        ])
    }
    pub fn reuse(&mut self, name: &str) -> Result<Vec<String>> {
        self.mutation_ready()?;
        let old = self.find(name, true)?.clone();
        self.validate(&old)?;
        for (path, _) in launch::templates(&self.root, &self.root_id, &old)? {
            let _ = self.launcher_state(&old, &path)?;
        }
        let mut new = old.clone();
        new.state = State::Active;
        new.launcher_binding = Some(current_binding()?);
        let stage = self.begin(Operation::Reuse, Some(old.registration_id.clone()))?;
        self.stage_launchers(&stage, Some(&old), &new)?;
        let mut next = self.registry.clone();
        *next
            .registrations
            .iter_mut()
            .find(|r| r.registration_id == old.registration_id)
            .unwrap() = new;
        self.commit(next, &stage)?;
        Ok(vec![])
    }
    fn purge_record(&self, name: &str) -> Result<Registration> {
        if let Ok(r) = self.find(name, true) {
            return Ok(r.clone());
        }
        let j = self
            .intent
            .as_ref()
            .filter(|j| j.operation == Operation::Purge)
            .ok_or_else(|| err("not_found", "No profile or pending purge matches this name"))?;
        let a = j
            .artifacts
            .iter()
            .find(|a| {
                a.role == Role::Profile
                    && file_name(&a.destination).is_ok_and(|n| n.eq_ignore_ascii_case(name))
            })
            .ok_or_else(|| err("not_found", "Pending purge is for a different profile"))?;
        let before = a
            .before
            .as_ref()
            .ok_or_else(|| recovery("Pending purge lacks ownership evidence"))?;
        Ok(Registration {
            registration_id: j.registration_id.clone().unwrap(),
            name: file_name(&a.destination)?.into(),
            kind: Kind::Owned,
            state: State::Retained,
            directory: Some(a.destination.clone()),
            directory_identity: Some(before.object_identity.clone()),
            profile_id: before.profile_id.clone(),
            upstream_linked_default: false,
            launcher_binding: j
                .artifacts
                .iter()
                .find_map(|a| a.before.as_ref().and_then(|s| s.launcher_binding.clone())),
        })
    }
    fn purge_empty_proven(&self, a: &Artifact) -> Result<bool> {
        if a.role != Role::Profile
            || a.action != Action::Delete
            || !self
                .intent
                .as_ref()
                .is_some_and(|j| j.operation == Operation::Purge)
        {
            return Ok(false);
        }
        let Some(entry) = self.profiles_dir.entry(file_name(&a.destination)?)? else {
            return Ok(false);
        };
        if !entry.is_dir
            || entry.is_link
            || Some(&entry.identity) != a.before.as_ref().map(|s| &s.object_identity)
        {
            return Ok(false);
        }
        let child = self.profiles_dir.child(file_name(&a.destination)?, true)?;
        if !child.entries()?.is_empty() {
            return Ok(false);
        }
        let marker = self.intent.as_ref().unwrap().artifacts.iter().find(|m| {
            m.role == Role::Marker && m.destination == a.destination.join(PROFILE_MARKER)
        });
        Ok(marker.is_some_and(|m| {
            m.action == Action::Delete
                && m.before.as_ref().and_then(|s| s.profile_id.as_ref())
                    == a.before.as_ref().and_then(|s| s.profile_id.as_ref())
        }))
    }
    pub fn preflight_remove(&self, name: &str, purge: bool) -> Result<(String, Registration)> {
        self.check_root()?;
        let r = if purge {
            self.purge_record(name)?
        } else {
            self.find(name, true)?.clone()
        };
        if purge && r.kind != Kind::Owned {
            return Err(err(
                "ownership",
                "Only owned isolated profile data can be purged",
            ));
        }
        if let Some(j) = &self.intent {
            if !purge
                || j.operation != Operation::Purge
                || j.registration_id.as_ref() != Some(&r.registration_id)
            {
                return Err(recovery(
                    "Pending operation is not this explicit purge retry",
                ));
            }
            for a in &j.artifacts {
                if a.role == Role::StagingDirectory {
                    continue;
                }
                if !self.matches(a, a.before.as_ref())?
                    && !self.matches(a, a.after.as_ref())?
                    && !self.purge_empty_proven(a)?
                {
                    return Err(recovery(
                        "Partial purge destination changed; deletion will not continue",
                    ));
                }
            }
        } else {
            self.validate(&r)?;
            for (path, _) in launch::templates(&self.root, &self.root_id, &r)? {
                let _ = self.launcher_state(&r, &path)?;
            }
        }
        let scope = if purge {
            format!(
                "Permanently delete verified owned profile data at {}{}. Stop all sessions and other writers first. Native login is not revoked.{}",
                r.directory.as_ref().unwrap().display(),
                match self.desktop_folder(&r) {
                    Ok(Some(folder)) => format!(
                        " and its Claude Desktop data folder at {}",
                        folder.display()
                    ),
                    _ => String::new(),
                },
                if self.pending() {
                    " This retries a partial irreversible deletion."
                } else {
                    ""
                }
            )
        } else {
            format!("Remove owned launchers/registration for {}", r.name)
        };
        Ok((scope, r))
    }
    pub fn remove(&mut self, name: &str, purge: bool) -> Result<Vec<String>> {
        if !self.mode.journaled() {
            return Err(err("ownership", "Read-only store cannot mutate"));
        }
        let (_, r) = self.preflight_remove(name, purge)?;
        if !purge && r.state == State::Retained {
            return Ok(vec![
                "Profile data and manager token remain retained; native login is unchanged.".into(),
            ]);
        }
        if purge {
            return self.purge(&r);
        }
        let desktop = if r.kind == Kind::Upstream {
            self.desktop_deletion(&r)?
        } else {
            None
        };
        let stage = self.begin(Operation::Remove, Some(r.registration_id.clone()))?;
        self.delete_launchers(&r)?;
        if let Some(desktop) = desktop {
            // Roost's own Desktop folder (the upstream profile's Desktop sign-in),
            // confirmed by the caller; borrowed upstream data is never touched.
            self.record(desktop)?;
            self.delete_desktop(self.intent.as_ref().unwrap().artifacts.len() - 1)?;
        }
        let mut next = self.registry.clone();
        if r.kind == Kind::Owned {
            next.registrations
                .iter_mut()
                .find(|x| x.registration_id == r.registration_id)
                .unwrap()
                .state = State::Retained;
        } else {
            next.registrations
                .retain(|x| x.registration_id != r.registration_id);
        }
        self.commit(next, &stage)?;
        Ok(vec![if r.kind == Kind::Owned { "Profile data, manager token and ownership evidence remain retained; native login is unchanged." } else { "Upstream/default data and authentication remain unchanged." }.into()])
    }
    fn purge(&mut self, r: &Registration) -> Result<Vec<String>> {
        if self.intent.is_none() {
            let _stage = self.begin(Operation::Purge, Some(r.registration_id.clone()))?;
        }
        if self.intent.as_ref().unwrap().phase == Phase::Prepared {
            // Crashes may leave only the staging record, some launcher records or
            // the marker record. Complete proof before publishing or deleting data.
            self.delete_launchers(r)?;
            let profile = self.profile_directory(r)?;
            if !self
                .intent
                .as_ref()
                .unwrap()
                .artifacts
                .iter()
                .any(|a| a.role == Role::Marker)
            {
                let mut before = file_state(&profile, PROFILE_MARKER, true)?
                    .ok_or_else(|| recovery("Profile marker disappeared"))?;
                before.profile_id = r.profile_id.clone();
                self.record(Artifact {
                    role: Role::Marker,
                    destination: profile.path.join(PROFILE_MARKER),
                    staged: None,
                    before: Some(before),
                    after: None,
                    action: Action::Delete,
                    completed: false,
                })?;
            }
            if !self
                .intent
                .as_ref()
                .unwrap()
                .artifacts
                .iter()
                .any(|a| a.role == Role::Profile)
            {
                let mut before = state(profile.identity()?);
                before.profile_id = r.profile_id.clone();
                self.record(Artifact {
                    role: Role::Profile,
                    destination: profile.path.clone(),
                    staged: None,
                    before: Some(before),
                    after: None,
                    action: Action::Delete,
                    completed: false,
                })?;
            }
            if !self
                .intent
                .as_ref()
                .unwrap()
                .artifacts
                .iter()
                .any(|a| a.role == Role::DesktopData)
                && let Some(desktop) = self.desktop_deletion(r)?
            {
                self.record(desktop)?;
            }
        }
        let journal = self.intent.clone().unwrap();
        if self.registry.generation == journal.next_generation {
            for a in &journal.artifacts {
                if a.role != Role::StagingDirectory && !self.matches(a, a.after.as_ref())? {
                    return Err(recovery("Committed purge destination changed"));
                }
            }
            self.cleanup()?;
            return Ok(vec!["Previously completed purge bookkeeping is now clean; native authentication was not revoked.".into()]);
        }
        let stage_artifact = journal
            .artifacts
            .iter()
            .find(|a| a.role == Role::StagingDirectory)
            .ok_or_else(|| recovery("Purge staging identity is missing"))?;
        if !self.matches(stage_artifact, stage_artifact.after.as_ref())? {
            return Err(recovery("Purge staging directory changed"));
        }
        let stage = self
            .stages
            .child(file_name(&stage_artifact.destination)?, true)?;
        let mut next = self.registry.clone();
        next.registrations
            .retain(|x| x.registration_id != r.registration_id);
        if !journal.artifacts.iter().any(|a| a.role == Role::Registry) {
            next.generation = journal.next_generation;
            let mut before = file_state(&self.directory, REGISTRY, true)?
                .ok_or_else(|| recovery("Registry disappeared during purge"))?;
            before.registry_generation = Some(self.registry.generation);
            let mut after = state(self.directory.identity()?);
            after.registry_generation = Some(next.generation);
            self.stage_file(
                &stage,
                Role::Registry,
                self.root.join(REGISTRY),
                Some(before),
                &encode(&next)?,
                0o600,
                after,
            )?;
        }
        self.intent.as_mut().unwrap().phase = Phase::Publishing;
        self.save_intent()?;
        let artifacts = self.intent.as_ref().unwrap().artifacts.clone();
        for (index, a) in artifacts
            .iter()
            .enumerate()
            .filter(|(_, a)| a.role == Role::Launcher)
        {
            let _ = a;
            self.publish(index)?;
        }
        let profile_artifact = artifacts
            .iter()
            .find(|a| a.role == Role::Profile)
            .ok_or_else(|| recovery("Purge has no complete profile deletion evidence"))?;
        if self.profiles_dir.entry(&r.name)?.is_some() {
            if self.matches(profile_artifact, profile_artifact.before.as_ref())? {
                let profile = self.profile_directory(r)?;
                if platform::cancelled() {
                    return Err(Error::cancelled());
                }
                profile.purge_children(PROFILE_MARKER).map_err(|e| {
                    let mut error = Error::new(e.code, format!("Partial deletion at {}; owned marker and journal remain; {}", profile.path.display(), e.message)).next("Stop writers, inspect roost doctor, then explicitly repeat remove NAME --purge.");
                    error.exit_code = e.exit_code;
                    error
                })?;
                self.check_root()?;
                if !self.matches(profile_artifact, profile_artifact.before.as_ref())? {
                    return Err(recovery("Profile ownership changed during purge"));
                }
                if profile.entries()? != vec![PROFILE_MARKER.to_owned()] {
                    return Err(recovery(
                        "Profile contains newly introduced data; marker is retained",
                    ));
                }
                let index = artifacts
                    .iter()
                    .position(|a| a.role == Role::Marker)
                    .ok_or_else(|| recovery("Purge has no marker deletion evidence"))?;
                self.publish(index)?;
            } else if !self.purge_empty_proven(profile_artifact)? {
                return Err(recovery(
                    "Missing marker with nonempty or changed profile; purge refuses",
                ));
            }
            let index = artifacts
                .iter()
                .position(|a| a.role == Role::Profile)
                .unwrap();
            self.publish(index)?;
        }
        if let Some(index) = artifacts.iter().position(|a| a.role == Role::DesktopData) {
            self.delete_desktop(index)?;
        }
        let registry_index = artifacts
            .iter()
            .position(|a| a.role == Role::Registry)
            .unwrap_or(self.intent.as_ref().unwrap().artifacts.len() - 1);
        self.publish(registry_index)?;
        self.registry = read_record(&self.directory, REGISTRY)?;
        self.intent.as_mut().unwrap().phase = Phase::Committed;
        self.save_intent()?;
        self.cleanup()?;
        Ok(vec![
            "Owned profile data was purged; native logout/token revocation was not performed."
                .into(),
        ])
    }
    pub fn set_token(&mut self, name: &str, value: Option<&str>) -> Result<Vec<String>> {
        self.mutation_ready()?;
        let r = self.find(name, false)?.clone();
        self.validate(&r)?;
        if r.kind != Kind::Owned {
            return Err(err(
                "ownership",
                "Token changes require an active owned isolated profile",
            ));
        }
        let normalized = value
            .map(|v| platform::validate_token(v.as_bytes()))
            .transpose()?;
        let value = normalized.as_deref();
        let profile = self.profile_directory(&r)?;
        let before = file_state(&profile, ".roost-token", true)?;
        if value.is_none() && before.is_none() {
            return Ok(vec![
                "No manager token was present; native login is unchanged.".into(),
            ]);
        }
        let stage = self.begin(
            if value.is_some() {
                Operation::TokenSet
            } else {
                Operation::TokenClear
            },
            Some(r.registration_id.clone()),
        )?;
        let destination = profile.path.join(".roost-token");
        if let Some(value) = value {
            self.stage_file(
                &stage,
                Role::Token,
                destination,
                before,
                value.as_bytes(),
                0o600,
                state(profile.identity()?),
            )?;
        } else {
            self.record(Artifact {
                role: Role::Token,
                destination,
                staged: None,
                before,
                after: None,
                action: Action::Delete,
                completed: false,
            })?;
        }
        let next = self.registry.clone();
        self.commit(next, &stage)?;
        Ok(vec![
            if value.is_none() {
                "Manager token cleared; native login credentials may become effective."
            } else {
                "Manager token updated; presence does not establish authentication validity."
            }
            .into(),
        ])
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };
    struct Fixture {
        base: PathBuf,
        root: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let base = std::env::temp_dir().join(format!(
                "roost-store-test-{}",
                platform::random_id().unwrap()
            ));
            fs::create_dir(&base).unwrap();
            fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).unwrap();
            Self {
                root: base.join("managed"),
                base,
            }
        }
        fn open(&self) -> Store {
            Store::open(&self.root, true, OpenMode::Mutate).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.base);
        }
    }
    fn alias(name: &str) -> Registration {
        Registration {
            registration_id: platform::random_id().unwrap(),
            name: name.into(),
            kind: Kind::DefaultAlias,
            state: State::Active,
            directory: None,
            directory_identity: None,
            profile_id: None,
            upstream_linked_default: false,
            launcher_binding: Some(current_binding().unwrap()),
        }
    }
    fn prepared_alias(store: &mut Store, name: &str) -> (Directory, Registration) {
        let r = alias(name);
        let stage = store
            .begin(Operation::Add, Some(r.registration_id.clone()))
            .unwrap();
        store.stage_launchers(&stage, None, &r).unwrap();
        (stage, r)
    }
    fn stage_registry(store: &mut Store, stage: &Directory, mut next: Registry) {
        next.generation = store.intent.as_ref().unwrap().next_generation;
        let mut before = file_state(&store.directory, REGISTRY, true)
            .unwrap()
            .unwrap();
        before.registry_generation = Some(store.registry.generation);
        let mut after = state(store.directory.identity().unwrap());
        after.registry_generation = Some(next.generation);
        store
            .stage_file(
                stage,
                Role::Registry,
                store.root.join(REGISTRY),
                Some(before),
                &encode(&next).unwrap(),
                0o600,
                after,
            )
            .unwrap();
        store.intent.as_mut().unwrap().phase = Phase::Publishing;
        store.save_intent().unwrap();
    }
    #[test]
    fn exact_schema_rejects_missing_null_unknown_fields_and_names() {
        let r = alias("Work");
        let mut value = serde_json::to_value(r).unwrap();
        assert!(parse::<Registration>(&serde_json::to_vec(&value).unwrap(), "fixture").is_ok());
        value.as_object_mut().unwrap().remove("profile_id");
        assert!(parse::<Registration>(&serde_json::to_vec(&value).unwrap(), "fixture").is_err());
        value["profile_id"] = Value::Null;
        value["unknown"] = json!(true);
        assert!(parse::<Registration>(&serde_json::to_vec(&value).unwrap(), "fixture").is_err());
        for name in [
            "",
            ".",
            "-abc",
            "CON",
            "com1",
            "LPT9",
            "space name",
            "é",
            "a/b",
        ] {
            assert!(validate_name(name).is_err(), "{name}");
        }
        for name in ["Work", "a-b_3", "COM10", "LPT0"] {
            assert!(validate_name(name).is_ok(), "{name}");
        }
    }
    #[test]
    fn owned_retention_case_reuse_tokens_and_purge() {
        let fixture = Fixture::new();
        let mut store = fixture.open();
        store.add("Work", false, None).unwrap();
        let r = store.find("work", false).unwrap().clone();
        store.set_token("WORK", Some("fake-fixture-token")).unwrap();
        assert_eq!(
            store.token(&r).unwrap().as_deref(),
            Some("fake-fixture-token")
        );
        assert!(store.add("work", false, None).is_err());
        let journal_text = fs::read_to_string(fixture.root.join(REGISTRY)).unwrap();
        assert!(!journal_text.contains("fake-fixture-token"));
        store.remove("work", false).unwrap();
        assert_eq!(store.find("WORK", true).unwrap().state, State::Retained);
        assert!(store.find("WORK", false).is_err());
        let list = store.profiles(true).unwrap();
        assert_eq!(list[0]["launchers"][0]["condition"], "not_required");
        store.reuse("WORK").unwrap();
        assert_eq!(
            store.find("work", false).unwrap().registration_id,
            r.registration_id
        );
        assert_eq!(store.find("work", false).unwrap().name, "Work");
        store.set_token("work", None).unwrap();
        assert_eq!(
            store.token(store.find("work", false).unwrap()).unwrap(),
            None
        );
        let outside = fixture.base.join("must-survive");
        fs::write(&outside, b"unrelated").unwrap();
        symlink(&outside, fixture.root.join("profiles/Work/link")).unwrap();
        store.remove("work", true).unwrap();
        assert_eq!(fs::read(outside).unwrap(), b"unrelated");
        assert!(!fixture.root.join("profiles/Work").exists());
        assert!(store.registry.registrations.is_empty());
        assert!(!store.pending());
    }
    #[test]
    fn borrowed_unsafe_token_blocks_read_but_not_unregistration() {
        let fixture = Fixture::new();
        let mut store = fixture.open();
        let upstream = fixture.base.join("upstream/.ccm/profiles/Personal");
        fs::create_dir_all(&upstream).unwrap();
        let token = upstream.join(".ccm-oauth-token");
        fs::write(&token, b"fixture-secret").unwrap();
        fs::set_permissions(&token, fs::Permissions::from_mode(0o644)).unwrap();
        store.register("Personal", &upstream).unwrap();
        let r = store.find("personal", false).unwrap().clone();
        assert!(store.token(&r).is_err());
        assert!(store.preflight_remove("personal", true).is_err());
        store.remove("PERSONAL", false).unwrap();
        assert_eq!(fs::read(&token).unwrap(), b"fixture-secret");
        assert_eq!(
            fs::metadata(&token).unwrap().permissions().mode() & 0o777,
            0o644
        );
    }
    #[test]
    fn read_does_not_recover_and_uncommitted_add_rolls_back_exact_launcher() {
        let fixture = Fixture::new();
        let mut store = fixture.open();
        let (_stage, _r) = prepared_alias(&mut store, "Work");
        let index = store
            .intent
            .as_ref()
            .unwrap()
            .artifacts
            .iter()
            .position(|a| a.role == Role::Launcher)
            .unwrap();
        store.publish(index).unwrap();
        let journal = fs::read(fixture.root.join(JOURNAL)).unwrap();
        drop(store);
        let read = Store::open(&fixture.root, false, OpenMode::Read).unwrap();
        assert!(read.pending());
        drop(read);
        assert_eq!(fs::read(fixture.root.join(JOURNAL)).unwrap(), journal);
        let recovered = Store::open(&fixture.root, false, OpenMode::Mutate).unwrap();
        assert!(!recovered.pending());
        assert!(recovered.registry.registrations.is_empty());
        assert!(!fixture.root.join("bin/roost-Work").exists());
    }
    #[test]
    fn committed_registry_finishes_only_proven_cleanup() {
        let fixture = Fixture::new();
        let mut store = fixture.open();
        let (stage, r) = prepared_alias(&mut store, "Work");
        let mut next = store.registry.clone();
        next.registrations.push(r);
        stage_registry(&mut store, &stage, next);
        for index in 0..store.intent.as_ref().unwrap().artifacts.len() {
            store.publish(index).unwrap();
        }
        drop(store);
        let recovered = Store::open(&fixture.root, false, OpenMode::Mutate).unwrap();
        assert!(!recovered.pending());
        assert_eq!(
            recovered.find("work", false).unwrap().kind,
            Kind::DefaultAlias
        );
    }
    #[test]
    fn foreign_replacement_and_unjournaled_stages_are_preserved() {
        let fixture = Fixture::new();
        let mut store = fixture.open();
        let (_stage, _) = prepared_alias(&mut store, "Work");
        let index = store
            .intent
            .as_ref()
            .unwrap()
            .artifacts
            .iter()
            .position(|a| a.role == Role::Launcher)
            .unwrap();
        store.publish(index).unwrap();
        let launcher = fixture.root.join("bin/roost-Work");
        fs::write(&launcher, b"foreign contents").unwrap();
        drop(store);
        assert!(Store::open(&fixture.root, false, OpenMode::Mutate).is_err());
        assert_eq!(fs::read(&launcher).unwrap(), b"foreign contents");
        assert!(fixture.root.join(JOURNAL).exists());
    }
    #[test]
    fn interrupted_token_publication_finishes_bookkeeping_without_secret_rollback() {
        let fixture = Fixture::new();
        let mut store = fixture.open();
        store.add("Work", false, None).unwrap();
        let r = store.find("work", false).unwrap().clone();
        let old_generation = store.registry.generation;
        let profile = store.profile_directory(&r).unwrap();
        let stage = store
            .begin(Operation::TokenSet, Some(r.registration_id.clone()))
            .unwrap();
        store
            .stage_file(
                &stage,
                Role::Token,
                profile.path.join(".roost-token"),
                None,
                b"fake-new-token",
                0o600,
                state(profile.identity().unwrap()),
            )
            .unwrap();
        let next = store.registry.clone();
        stage_registry(&mut store, &stage, next);
        let index = store
            .intent
            .as_ref()
            .unwrap()
            .artifacts
            .iter()
            .position(|a| a.role == Role::Token)
            .unwrap();
        store.publish(index).unwrap();
        assert!(
            !fs::read_to_string(fixture.root.join(JOURNAL))
                .unwrap()
                .contains("fake-new-token")
        );
        drop(store);
        assert!(Store::open(&fixture.root, false, OpenMode::Mutate).is_err());
        let read = Store::open(&fixture.root, false, OpenMode::Read).unwrap();
        assert!(!read.pending());
        assert_eq!(read.registry.generation, old_generation + 1);
        assert_eq!(
            read.token(read.find("work", false).unwrap())
                .unwrap()
                .as_deref(),
            Some("fake-new-token")
        );
    }
    fn prepared_purge(store: &mut Store, name: &str) -> Registration {
        let r = store.find(name, true).unwrap().clone();
        let stage = store
            .begin(Operation::Purge, Some(r.registration_id.clone()))
            .unwrap();
        store.delete_launchers(&r).unwrap();
        let profile = store.profile_directory(&r).unwrap();
        let mut marker = file_state(&profile, PROFILE_MARKER, true).unwrap().unwrap();
        marker.profile_id = r.profile_id.clone();
        store
            .record(Artifact {
                role: Role::Marker,
                destination: profile.path.join(PROFILE_MARKER),
                staged: None,
                before: Some(marker),
                after: None,
                action: Action::Delete,
                completed: false,
            })
            .unwrap();
        let mut before = state(profile.identity().unwrap());
        before.profile_id = r.profile_id.clone();
        store
            .record(Artifact {
                role: Role::Profile,
                destination: profile.path.clone(),
                staged: None,
                before: Some(before),
                after: None,
                action: Action::Delete,
                completed: false,
            })
            .unwrap();
        let mut next = store.registry.clone();
        next.registrations
            .retain(|x| x.registration_id != r.registration_id);
        stage_registry(store, &stage, next);
        r
    }
    #[test]
    fn explicit_purge_retry_completes_each_incomplete_preparation_boundary() {
        for boundary in 0..3 {
            let fixture = Fixture::new();
            let mut store = fixture.open();
            store.add("Work", false, None).unwrap();
            let r = store.find("Work", false).unwrap().clone();
            let profile = store.profile_directory(&r).unwrap();
            profile.write_new("data", b"fixture", 0o600).unwrap();
            store
                .begin(Operation::Purge, Some(r.registration_id.clone()))
                .unwrap();
            if boundary == 1 {
                store.delete_launchers(&r).unwrap();
            }
            if boundary == 2 {
                let mut before = file_state(&profile, PROFILE_MARKER, true).unwrap().unwrap();
                before.profile_id = r.profile_id.clone();
                store
                    .record(Artifact {
                        role: Role::Marker,
                        destination: profile.path.join(PROFILE_MARKER),
                        staged: None,
                        before: Some(before),
                        after: None,
                        action: Action::Delete,
                        completed: false,
                    })
                    .unwrap();
            }
            drop(store);
            let read = Store::open(&fixture.root, false, OpenMode::Read).unwrap();
            let (_, confirmed) = read.preflight_remove("work", true).unwrap();
            assert_eq!(confirmed, r);
            drop(read);
            assert_eq!(
                fs::read(fixture.root.join("profiles/Work/data")).unwrap(),
                b"fixture"
            );
            assert!(Store::open(&fixture.root, false, OpenMode::Mutate).is_err());
            let mut retry = Store::open(&fixture.root, false, OpenMode::RetryPurge).unwrap();
            retry.remove("WORK", true).unwrap();
            assert!(!retry.pending());
            assert!(retry.registry.registrations.is_empty());
            assert!(!fixture.root.join("profiles/Work").exists());
        }
    }
    #[test]
    fn add_rollback_final_empty_boundary_retries_staged_and_published_profiles() {
        for published in [false, true] {
            for introduce in [false, true] {
                let fixture = Fixture::new();
                let mut store = fixture.open();
                let r = alias("Work");
                let stage = store
                    .begin(Operation::Add, Some(r.registration_id))
                    .unwrap();
                let profile = stage.create_dir(".roost-tmp-profile").unwrap();
                let identity = profile.identity().unwrap();
                let profile_id = platform::random_id().unwrap();
                let marker = ProfileMarker {
                    schema_version: 1,
                    root_id: store.root_id.clone(),
                    profile_id: profile_id.clone(),
                    directory_identity: identity.clone(),
                };
                profile
                    .write_new(PROFILE_MARKER, &encode(&marker).unwrap(), 0o600)
                    .unwrap();
                let mut after = state(identity.clone());
                after.profile_id = Some(profile_id);
                let destination = fixture.root.join("profiles/Work");
                store
                    .record(Artifact {
                        role: Role::Profile,
                        destination: destination.clone(),
                        staged: Some(Staged {
                            path: profile.path.clone(),
                            object_identity: identity,
                        }),
                        before: None,
                        after: Some(after),
                        action: Action::Create,
                        completed: false,
                    })
                    .unwrap();
                let index = store
                    .intent
                    .as_ref()
                    .unwrap()
                    .artifacts
                    .iter()
                    .position(|a| a.role == Role::Profile)
                    .unwrap();
                if published {
                    store.publish(index).unwrap();
                }
                store.intent.as_mut().unwrap().phase = Phase::Cleanup;
                store.save_intent().unwrap();
                profile.remove(PROFILE_MARKER, false).unwrap();
                if introduce {
                    profile
                        .write_new("introduced-data", b"preserve", 0o600)
                        .unwrap();
                }
                let actual_path = if published {
                    destination
                } else {
                    profile.path.clone()
                };
                drop(store);
                let result = Store::open(&fixture.root, false, OpenMode::Mutate);
                if introduce {
                    assert!(result.is_err());
                    assert_eq!(
                        fs::read(actual_path.join("introduced-data")).unwrap(),
                        b"preserve"
                    );
                    assert!(fixture.root.join(JOURNAL).exists());
                } else {
                    let recovered = result.unwrap();
                    assert!(!recovered.pending());
                    assert!(!actual_path.exists());
                }
            }
        }
    }
    #[test]
    fn partial_purge_never_auto_continues_and_explicit_retry_revalidates_marker() {
        let fixture = Fixture::new();
        let mut store = fixture.open();
        store.add("Work", false, None).unwrap();
        let r = prepared_purge(&mut store, "work");
        let profile = store.profile_directory(&r).unwrap();
        profile
            .write_new("remaining-data", b"fixture", 0o600)
            .unwrap();
        drop(store);
        assert!(Store::open(&fixture.root, false, OpenMode::Mutate).is_err());
        assert_eq!(
            fs::read(fixture.root.join("profiles/Work/remaining-data")).unwrap(),
            b"fixture"
        );
        let mut retry = Store::open(&fixture.root, false, OpenMode::RetryPurge).unwrap();
        assert!(retry.preflight_remove("Other", true).is_err());
        assert!(
            retry
                .preflight_remove("WORK", true)
                .unwrap()
                .0
                .contains("partial")
        );
        retry.remove("work", true).unwrap();
        assert!(retry.registry.registrations.is_empty());
        assert!(!retry.pending());
    }
    #[test]
    fn marker_final_interval_only_finishes_empty_original_directory() {
        for introduce in [false, true] {
            let fixture = Fixture::new();
            let mut store = fixture.open();
            store.add("Work", false, None).unwrap();
            let r = prepared_purge(&mut store, "Work");
            let profile = store.profile_directory(&r).unwrap();
            profile.purge_children(PROFILE_MARKER).unwrap();
            let marker_index = store
                .intent
                .as_ref()
                .unwrap()
                .artifacts
                .iter()
                .position(|a| a.role == Role::Marker)
                .unwrap();
            store.publish(marker_index).unwrap();
            if introduce {
                profile
                    .write_new("foreign-new-data", b"preserve", 0o600)
                    .unwrap();
            }
            drop(store);
            assert!(Store::open(&fixture.root, false, OpenMode::Mutate).is_err());
            let mut retry = Store::open(&fixture.root, false, OpenMode::RetryPurge).unwrap();
            if introduce {
                assert!(retry.preflight_remove("Work", true).is_err());
                assert!(retry.remove("Work", true).is_err());
                assert_eq!(
                    fs::read(fixture.root.join("profiles/Work/foreign-new-data")).unwrap(),
                    b"preserve"
                );
            } else {
                retry.remove("Work", true).unwrap();
                assert!(!fixture.root.join("profiles/Work").exists());
            }
        }
    }
    #[test]
    fn purge_committed_before_journal_cleanup_remains_explicit() {
        let fixture = Fixture::new();
        let mut store = fixture.open();
        store.add("Work", false, None).unwrap();
        let r = prepared_purge(&mut store, "Work");
        let profile = store.profile_directory(&r).unwrap();
        profile.purge_children(PROFILE_MARKER).unwrap();
        let roles = [Role::Launcher, Role::Marker, Role::Profile, Role::Registry];
        for role in roles {
            let indices = store
                .intent
                .as_ref()
                .unwrap()
                .artifacts
                .iter()
                .enumerate()
                .filter(|(_, a)| a.role == role)
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            for index in indices {
                store.publish(index).unwrap();
            }
        }
        drop(store);
        assert!(Store::open(&fixture.root, false, OpenMode::Mutate).is_err());
        let mut retry = Store::open(&fixture.root, false, OpenMode::RetryPurge).unwrap();
        retry.preflight_remove("work", true).unwrap();
        retry.remove("WORK", true).unwrap();
        assert!(!retry.pending());
    }
    #[test]
    fn refresh_rollback_uses_recorded_old_binding_and_new_stage_identity() {
        let fixture = Fixture::new();
        let mut store = fixture.open();
        store.add("Work", false, None).unwrap();
        let old = store.find("Work", false).unwrap().clone();
        let mut new = old.clone();
        new.launcher_binding.as_mut().unwrap().executable = fixture.base.join("relocated roost");
        let stage = store
            .begin(Operation::Reuse, Some(old.registration_id.clone()))
            .unwrap();
        store.stage_launchers(&stage, Some(&old), &new).unwrap();
        let mut next = store.registry.clone();
        *next
            .registrations
            .iter_mut()
            .find(|r| r.registration_id == old.registration_id)
            .unwrap() = new;
        stage_registry(&mut store, &stage, next);
        let index = store
            .intent
            .as_ref()
            .unwrap()
            .artifacts
            .iter()
            .position(|a| a.role == Role::Launcher)
            .unwrap();
        store.publish(index).unwrap();
        drop(store);
        let recovered = Store::open(&fixture.root, false, OpenMode::Mutate).unwrap();
        assert_eq!(
            recovered.find("work", false).unwrap().launcher_binding,
            old.launcher_binding
        );
        assert_eq!(recovered.registry.generation, 2);
        assert!(!recovered.pending());
        for (path, bytes) in launch::templates(&recovered.root, &recovered.root_id, &old).unwrap() {
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
    }
    #[test]
    fn initializing_journal_finishes_staged_registry_without_resetting_root_id() {
        let fixture = Fixture::new();
        let mut store = fixture.open();
        let root_id = store.root_id.clone();
        store.directory.remove(REGISTRY, false).unwrap();
        store.registry_identity = None;
        store.registry.generation = 0;
        let _stage = store.begin(Operation::Initialize, None).unwrap();
        drop(store);
        assert!(Store::open(&fixture.root, false, OpenMode::Read).is_err());
        let recovered = Store::open(&fixture.root, false, OpenMode::Mutate).unwrap();
        assert_eq!(recovered.root_id, root_id);
        assert_eq!(recovered.registry.generation, 1);
        assert!(!recovered.pending());
    }
    #[test]
    fn profile_listing_checks_token_metadata_without_parsing_secret_contents() {
        let fixture = Fixture::new();
        let mut store = fixture.open();
        store.add("Work", false, None).unwrap();
        let r = store.find("Work", false).unwrap().clone();
        store
            .profile_directory(&r)
            .unwrap()
            .write_new(".roost-token", b"invalid fake whitespace", 0o600)
            .unwrap();
        assert!(store.token(&r).is_err());
        assert!(store.token_present(&r).unwrap());
        assert_eq!(store.profiles(false).unwrap()[0]["token_present"], true);
        store.set_token("work", None).unwrap();
        assert!(!store.token_present(&r).unwrap());
    }
    #[test]
    fn namespace_and_stable_lock_replacement_refuse_mutation() {
        let fixture = Fixture::new();
        let mut store = fixture.open();
        fs::rename(fixture.root.join(LOCK), fixture.root.join("moved-lock")).unwrap();
        fs::write(fixture.root.join(LOCK), b"").unwrap();
        fs::set_permissions(fixture.root.join(LOCK), fs::Permissions::from_mode(0o600)).unwrap();
        assert!(store.add("Work", false, None).is_err());
        assert!(store.registry.registrations.is_empty());
    }
    #[test]
    fn existing_launcher_without_owner_execute_is_unsafe() {
        let fixture = Fixture::new();
        let mut store = fixture.open();
        store.add("Work", false, None).unwrap();
        fs::set_permissions(
            fixture.root.join("bin/roost-Work"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        assert_eq!(
            store.profiles(false).unwrap()[0]["launchers"][0]["condition"],
            "unsafe"
        );
        assert!(store.reuse("work").is_err());
        assert!(!store.pending());
    }
    #[test]
    fn malformed_new_registration_intent_cannot_claim_existing_owned_data() {
        let fixture = Fixture::new();
        let mut store = fixture.open();
        store.add("Work", false, None).unwrap();
        let r = store.find("Work", false).unwrap().clone();
        let mut journal = Intent {
            schema_version: 1,
            root_id: store.root_id.clone(),
            operation_id: platform::random_id().unwrap(),
            operation: Operation::Add,
            registration_id: Some(r.registration_id.clone()),
            prior_generation: store.registry.generation,
            next_generation: store.registry.generation + 1,
            phase: Phase::Prepared,
            artifacts: vec![],
        };
        assert!(store.validate_intent(&journal).is_err());
        journal.operation = Operation::Purge;
        let mut after = state(r.directory_identity.unwrap());
        after.profile_id = r.profile_id;
        journal.artifacts.push(Artifact {
            role: Role::Profile,
            destination: r.directory.unwrap(),
            staged: None,
            before: None,
            after: Some(after),
            action: Action::Create,
            completed: false,
        });
        assert!(store.validate_intent(&journal).is_err());
        assert!(
            fixture
                .root
                .join("profiles/Work/.roost-profile.json")
                .exists()
        );
    }
    #[test]
    fn foreign_launcher_preflight_keeps_existing_artifacts() {
        let fixture = Fixture::new();
        let mut store = fixture.open();
        store.add("Work", false, None).unwrap();
        fs::write(fixture.root.join("bin/roost-Work"), b"foreign").unwrap();
        assert!(store.remove("work", false).is_err());
        assert!(!store.pending());
        assert_eq!(store.find("work", false).unwrap().state, State::Active);
    }
}
