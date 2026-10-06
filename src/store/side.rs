//! Manager side files and reserved root entries from the workflow UX amendment.
//!
//! `state.json` and `sets.json` are not journaled: they are replaced atomically under
//! the root lock through an exclusive private `.roost-state-<ID>.tmp` in the root.
//! Records are keyed by registration ID; entries naming an unknown registration are
//! ignored on read and dropped by the next write.

use super::{
    Directory, LIMIT, OpenMode, Registry, Result, Store, encode, err, file_state, nullable, parse,
    valid_id, validate_identity,
};
use crate::platform::{self, FileIdentity};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::HashSet,
    path::{Component, Path, PathBuf},
};

/// `state.json`: launch-written state. Absent means empty.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StateFile {
    pub schema_version: u32,
    pub root_id: String,
    pub selections: Vec<Selection>,
    pub last_used: Vec<Timestamp>,
    pub links: Vec<LinkRecord>,
    pub desktop_launched: Vec<Timestamp>,
    #[serde(deserialize_with = "nullable")]
    pub plugin_auto_update_at: Option<u64>,
    /// The registration borrowing the conventional Desktop data folder (at most one).
    /// Optional on read and omitted while empty, so older documents stay valid.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub desktop_links: Vec<DesktopLink>,
}
/// A project's selected profile; `project` is the absolute project key.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub project: PathBuf,
    pub registration_id: String,
}
/// A per-registration time in Unix seconds (`last_used`, `desktop_launched`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Timestamp {
    pub registration_id: String,
    pub at: u64,
}
/// A registration whose Desktop borrows the conventional Desktop data folder in place.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DesktopLink {
    pub registration_id: String,
}
/// A link Roost created in an owned profile; `path` is relative to the profile.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LinkRecord {
    pub registration_id: String,
    pub path: PathBuf,
    pub target: PathBuf,
    pub link_identity: FileIdentity,
}

/// `sets.json`: shared sets, subscriptions and plugin settings. Absent means empty.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SetsFile {
    pub schema_version: u32,
    pub root_id: String,
    pub sets: Vec<SharedSet>,
    pub subscriptions: Vec<Subscription>,
    pub plugin_auto_update: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SharedSet {
    pub name: String,
    pub default: bool,
    pub items: Vec<Item>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub kind: ItemKind,
    pub value: String,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Skill,
    SkillSource,
    Instruction,
    InstructionSource,
    Plugin,
    Agent,
    AgentSource,
    Command,
    CommandSource,
    OutputStyle,
    OutputStyleSource,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Subscription {
    pub registration_id: String,
    pub set: String,
}

/// A side-file document: exact schema, root-ID match and no duplicate keys.
trait SideFile: Serialize + DeserializeOwned {
    const NAME: &'static str;
    fn empty_for(root_id: &str) -> Self;
    fn header(&self) -> (u32, &str);
    /// Rejects duplicate keys and malformed records.
    fn check(&self) -> Result<()>;
    /// Drops records naming a registration absent from the registry.
    fn retain_registered(&mut self, known: &HashSet<&str>);
}

fn invalid(name: &str) -> crate::Error {
    err(
        "ownership",
        format!("Invalid or unsupported {name}; preserve it for inspection"),
    )
}
fn unique<K: Eq + std::hash::Hash>(name: &str, keys: impl IntoIterator<Item = K>) -> Result<()> {
    let mut seen = HashSet::new();
    for key in keys {
        if !seen.insert(key) {
            return Err(invalid(name));
        }
    }
    Ok(())
}
fn ids<'a>(name: &str, mut values: impl Iterator<Item = &'a String>) -> Result<()> {
    if values.all(|id| valid_id(id)) {
        Ok(())
    } else {
        Err(invalid(name))
    }
}
fn absolute(name: &str, path: &Path) -> Result<()> {
    match platform::absolute(path) {
        Ok(normal) if path.is_absolute() && normal == path => Ok(()),
        _ => Err(invalid(name)),
    }
}

impl StateFile {
    pub fn empty(root_id: &str) -> Self {
        Self {
            schema_version: 1,
            root_id: root_id.to_owned(),
            selections: vec![],
            last_used: vec![],
            links: vec![],
            desktop_launched: vec![],
            plugin_auto_update_at: None,
            desktop_links: vec![],
        }
    }
}
impl SideFile for StateFile {
    const NAME: &'static str = STATE;
    fn empty_for(root_id: &str) -> Self {
        Self::empty(root_id)
    }
    fn header(&self) -> (u32, &str) {
        (self.schema_version, &self.root_id)
    }
    fn check(&self) -> Result<()> {
        ids(
            STATE,
            self.selections
                .iter()
                .map(|s| &s.registration_id)
                .chain(self.last_used.iter().map(|t| &t.registration_id))
                .chain(self.links.iter().map(|l| &l.registration_id))
                .chain(self.desktop_launched.iter().map(|t| &t.registration_id))
                .chain(self.desktop_links.iter().map(|l| &l.registration_id)),
        )?;
        if self.desktop_links.len() > 1 {
            return Err(invalid(STATE));
        }
        for selection in &self.selections {
            absolute(STATE, &selection.project)?;
        }
        for link in &self.links {
            absolute(STATE, &link.target)?;
            validate_identity(&link.link_identity)?;
            let relative = link
                .path
                .to_str()
                .is_some_and(|t| !t.contains(['\r', '\n']))
                && link.path.components().next().is_some()
                && link
                    .path
                    .components()
                    .all(|c| matches!(c, Component::Normal(_)));
            if !relative {
                return Err(invalid(STATE));
            }
        }
        unique(STATE, self.selections.iter().map(|s| &s.project))?;
        unique(STATE, self.last_used.iter().map(|t| &t.registration_id))?;
        unique(
            STATE,
            self.links.iter().map(|l| (&l.registration_id, &l.path)),
        )?;
        unique(
            STATE,
            self.desktop_launched.iter().map(|t| &t.registration_id),
        )
    }
    fn retain_registered(&mut self, known: &HashSet<&str>) {
        let keep = |id: &String| known.contains(id.as_str());
        self.selections.retain(|s| keep(&s.registration_id));
        self.last_used.retain(|t| keep(&t.registration_id));
        self.links.retain(|l| keep(&l.registration_id));
        self.desktop_launched.retain(|t| keep(&t.registration_id));
        self.desktop_links.retain(|l| keep(&l.registration_id));
    }
}
impl SetsFile {
    pub fn empty(root_id: &str) -> Self {
        Self {
            schema_version: 1,
            root_id: root_id.to_owned(),
            sets: vec![],
            subscriptions: vec![],
            plugin_auto_update: false,
        }
    }
}
impl SideFile for SetsFile {
    const NAME: &'static str = SETS;
    fn empty_for(root_id: &str) -> Self {
        Self::empty(root_id)
    }
    fn header(&self) -> (u32, &str) {
        (self.schema_version, &self.root_id)
    }
    fn check(&self) -> Result<()> {
        ids(SETS, self.subscriptions.iter().map(|s| &s.registration_id))?;
        unique(SETS, self.sets.iter().map(|s| &s.name))?;
        for set in &self.sets {
            unique(SETS, set.items.iter().map(|i| (i.kind, &i.value)))?;
        }
        unique(
            SETS,
            self.subscriptions
                .iter()
                .map(|s| (&s.registration_id, &s.set)),
        )
    }
    fn retain_registered(&mut self, known: &HashSet<&str>) {
        self.subscriptions
            .retain(|s| known.contains(s.registration_id.as_str()));
    }
}

fn known(registry: &Registry) -> HashSet<&str> {
    registry
        .registrations
        .iter()
        .map(|r| r.registration_id.as_str())
        .collect()
}

impl Store {
    /// Reads `state.json` (empty when absent). Any mode; never creates or repairs.
    /// Records naming registrations absent from the registry are left out.
    pub fn read_state(&self) -> Result<StateFile> {
        self.read_side()
    }
    /// Like `read_state`, but keeps records naming registrations absent from the
    /// registry, so a stale selection can still be reported.
    pub fn read_state_unfiltered(&self) -> Result<StateFile> {
        self.read_side_unfiltered()
    }
    /// Reads `sets.json` (empty when absent). Any mode; never creates or repairs.
    pub fn read_sets(&self) -> Result<SetsFile> {
        self.read_side()
    }
    /// Read-modify-write of `state.json` under the held root lock, outside the
    /// journal. Refused in Read mode or while an operation is pending; an error from
    /// `change` aborts without writing. Validation failures use the registry's
    /// `ownership` category.
    pub fn update_state(&self, change: impl FnOnce(&mut StateFile) -> Result<()>) -> Result<()> {
        self.update_side(change)
    }
    /// Read-modify-write of `sets.json`; same rules as `update_state`.
    pub fn update_sets(&self, change: impl FnOnce(&mut SetsFile) -> Result<()>) -> Result<()> {
        self.update_side(change)
    }

    /// Opens the reserved root directory `name` (`PLUGIN_STORE` or `DESKTOP`) through
    /// the verified root handle without following links; `None` when absent. With
    /// `create`, an absent directory is created private (0700), which Read mode and a
    /// pending operation refuse. The plugin store's own creation is journaled
    /// (`store_create`), so its owner should create it through the journal instead.
    pub fn reserved_dir(&self, name: &str, create: bool) -> Result<Option<Directory>> {
        if ![PLUGIN_STORE, DESKTOP].contains(&name) {
            return Err(err("usage", format!("{name} is not a reserved directory")));
        }
        if self.directory.entry(name)?.is_some() {
            return self.directory.child(name, true).map(Some);
        }
        if !create {
            return Ok(None);
        }
        self.side_ready()?;
        let created = self.directory.create_dir(name)?;
        self.directory.sync()?;
        Ok(Some(created))
    }

    fn side_ready(&self) -> Result<()> {
        if self.mode == OpenMode::Read {
            return Err(err(
                "ownership",
                "Read-only store cannot write manager state",
            ));
        }
        if self.pending() {
            return Err(
                err("ownership", "Pending operation blocks manager state writes")
                    .next("Inspect doctor and complete the pending operation first"),
            );
        }
        self.check_root()
    }

    fn read_side<T: SideFile>(&self) -> Result<T> {
        let mut value: T = self.read_side_unfiltered()?;
        value.retain_registered(&known(&self.registry));
        Ok(value)
    }

    fn read_side_unfiltered<T: SideFile>(&self) -> Result<T> {
        if file_state(&self.directory, T::NAME, true)?.is_none() {
            return Ok(T::empty_for(&self.root_id));
        }
        let bytes = self
            .directory
            .read(T::NAME, true, LIMIT)?
            .ok_or_else(|| err("unsafe_path", format!("{} changed while reading", T::NAME)))?;
        let value: T = parse(&bytes, T::NAME)?;
        if value.header() != (1, self.root_id.as_str()) {
            return Err(invalid(T::NAME));
        }
        value.check()?;
        Ok(value)
    }

    fn update_side<T: SideFile>(&self, change: impl FnOnce(&mut T) -> Result<()>) -> Result<()> {
        self.side_ready()?;
        let mut value = self.read_side::<T>()?;
        change(&mut value)?;
        value.retain_registered(&known(&self.registry));
        if value.header() != (1, self.root_id.as_str()) {
            return Err(invalid(T::NAME));
        }
        value.check()?;
        self.replace_side(T::NAME, &encode(&value)?)
    }

    /// Atomic private replacement through an exclusive temp in the root; the
    /// destination must still be the object validated before the temp was written.
    fn replace_side(&self, name: &str, bytes: &[u8]) -> Result<()> {
        if bytes.len() > LIMIT {
            return Err(err("io", format!("{name} exceeds metadata limit")));
        }
        self.remove_stale_temps();
        let before = file_state(&self.directory, name, true)?.map(|s| s.object_identity);
        let temp = format!("{TEMP_PREFIX}{}{TEMP_SUFFIX}", platform::random_id()?);
        self.directory.write_new(&temp, bytes, 0o600)?;
        let unchanged = file_state(&self.directory, name, true)
            .is_ok_and(|now| now.map(|s| s.object_identity) == before);
        if !unchanged {
            let _ = self.directory.remove(&temp, false);
            return Err(err(
                "unsafe_path",
                format!("{name} changed during replacement"),
            ));
        }
        if let Err(error) = self.directory.rename(&temp, &self.directory, name) {
            let _ = self.directory.remove(&temp, false);
            return Err(error);
        }
        self.directory.sync()
    }

    /// Best effort: removes leftover side-file temps that are private regular
    /// single-link current-user files; anything else is left for inspection.
    fn remove_stale_temps(&self) {
        let Ok(names) = self.directory.entries() else {
            return;
        };
        for name in names.into_iter().filter(|n| is_temp(n)) {
            if let Ok(Some(state)) = file_state(&self.directory, &name, true)
                && self
                    .directory
                    .entry(&name)
                    .ok()
                    .flatten()
                    .map(|e| e.identity)
                    == Some(state.object_identity)
            {
                let _ = self.directory.remove(&name, false);
            }
        }
    }
}

/// Launch-written state (selections, last use, recorded links, Desktop launches).
pub const STATE: &str = "state.json";
/// Shared sets, subscriptions and plugin settings.
pub const SETS: &str = "sets.json";
/// Lazily created plugin store (journaled `store_create`, ticket 07).
pub const PLUGIN_STORE: &str = "plugin-store";
/// Lazily created parent of per-registration Desktop folders (ticket 09).
pub const DESKTOP: &str = "desktop";
const TEMP_PREFIX: &str = ".roost-state-";
const TEMP_SUFFIX: &str = ".tmp";

fn is_temp(name: &str) -> bool {
    name.strip_prefix(TEMP_PREFIX)
        .and_then(|rest| rest.strip_suffix(TEMP_SUFFIX))
        .is_some_and(valid_id)
}

/// Root validation: whether `name` is a reserved workflow entry of the right object
/// type. A reserved name holding the wrong type (or a link) is unsafe residue.
pub(super) fn admitted(root: &Directory, name: &str) -> Result<bool> {
    let directory = match name {
        STATE | SETS => false,
        PLUGIN_STORE | DESKTOP => true,
        _ if is_temp(name) => false,
        _ => return Ok(false),
    };
    let Some(entry) = root.entry(name)? else {
        return Ok(false);
    };
    let matches = !entry.is_link
        && if directory {
            entry.is_dir
        } else {
            entry.is_file
        };
    if !matches {
        return Err(err(
            "unsafe_path",
            format!(
                "Unexpected object type at {}",
                root.path.join(name).display()
            ),
        ));
    }
    Ok(true)
}

#[cfg(all(test, unix))]
mod tests {
    use super::super::*;
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};

    struct Fixture {
        base: PathBuf,
        root: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let base = std::env::temp_dir().join(format!(
                "roost-side-test-{}",
                platform::random_id().unwrap()
            ));
            fs::create_dir(&base).unwrap();
            fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).unwrap();
            let root = base.join("managed");
            drop(Store::open(&root, true, OpenMode::Mutate).unwrap());
            Self { base, root }
        }
        fn open(&self, mode: OpenMode) -> Result<Store> {
            Store::open(&self.root, false, mode)
        }
        fn private_file(&self, name: &str) {
            let path = self.root.join(name);
            fs::write(&path, b"{}").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.base);
        }
    }

    #[test]
    fn root_validation_admits_the_reserved_workflow_entries() {
        let f = Fixture::new();
        f.private_file("state.json");
        f.private_file("sets.json");
        f.private_file(&format!(
            ".roost-state-{}.tmp",
            platform::random_id().unwrap()
        ));
        for name in ["plugin-store", "desktop"] {
            fs::create_dir(f.root.join(name)).unwrap();
            fs::set_permissions(f.root.join(name), fs::Permissions::from_mode(0o700)).unwrap();
        }
        assert!(f.open(OpenMode::Read).is_ok());
        assert!(f.open(OpenMode::Mutate).is_ok());
    }

    #[test]
    fn root_validation_rejects_lookalikes_and_wrong_object_types() {
        for setup in [
            |f: &Fixture| f.private_file(".roost-state-short.tmp"),
            |f: &Fixture| f.private_file("state.json.bak"),
            |f: &Fixture| f.private_file("desktop"),
            |f: &Fixture| fs::create_dir(f.root.join("state.json")).unwrap(),
            |f: &Fixture| std::os::unix::fs::symlink(&f.base, f.root.join("plugin-store")).unwrap(),
            |f: &Fixture| {
                let name = format!(".roost-state-{}.tmp", platform::random_id().unwrap());
                fs::create_dir(f.root.join(name)).unwrap()
            },
        ] {
            let f = Fixture::new();
            setup(&f);
            assert!(f.open(OpenMode::Read).is_err());
        }
    }

    fn mode_of(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn absent_side_files_read_as_empty_without_being_created() {
        let f = Fixture::new();
        let store = f.open(OpenMode::Read).unwrap();
        let state = store.read_state().unwrap();
        assert_eq!(state, StateFile::empty(&store.root_id));
        assert!(state.selections.is_empty() && state.plugin_auto_update_at.is_none());
        let sets = store.read_sets().unwrap();
        assert_eq!(sets, SetsFile::empty(&store.root_id));
        assert!(!sets.plugin_auto_update);
        assert!(!f.root.join(STATE).exists() && !f.root.join(SETS).exists());
    }

    #[test]
    fn sets_written_before_agent_command_and_style_kinds_still_read() {
        let f = Fixture::new();
        let root_id = f.open(OpenMode::Read).unwrap().root_id.clone();
        let old = format!(
            r#"{{"schema_version":1,"root_id":"{root_id}","sets":[{{"name":"core","default":true,"items":[
                {{"kind":"skill","value":"/s/a"}},{{"kind":"skill_source","value":"/s"}},
                {{"kind":"instruction","value":"/n/a.md"}},{{"kind":"instruction_source","value":"/n"}},
                {{"kind":"plugin","value":"p@m"}}]}}],"subscriptions":[],"plugin_auto_update":false}}"#
        );
        fs::write(f.root.join(SETS), old).unwrap();
        fs::set_permissions(f.root.join(SETS), fs::Permissions::from_mode(0o600)).unwrap();
        let store = f.open(OpenMode::Read).unwrap();
        let kinds: Vec<ItemKind> = store.read_sets().unwrap().sets[0]
            .items
            .iter()
            .map(|i| i.kind)
            .collect();
        assert_eq!(
            kinds,
            [
                ItemKind::Skill,
                ItemKind::SkillSource,
                ItemKind::Instruction,
                ItemKind::InstructionSource,
                ItemKind::Plugin
            ]
        );
        let names: Vec<_> = [
            ItemKind::Agent,
            ItemKind::AgentSource,
            ItemKind::Command,
            ItemKind::CommandSource,
            ItemKind::OutputStyle,
            ItemKind::OutputStyleSource,
        ]
        .iter()
        .map(|k| serde_json::to_string(k).unwrap())
        .collect();
        assert_eq!(
            names,
            [
                "\"agent\"",
                "\"agent_source\"",
                "\"command\"",
                "\"command_source\"",
                "\"output_style\"",
                "\"output_style_source\""
            ]
        );
    }

    #[test]
    fn writes_replace_privately_and_read_back() {
        let f = Fixture::new();
        let mut store = f.open(OpenMode::Mutate).unwrap();
        store.add("Work", false, None).unwrap();
        let id = store.find("Work", false).unwrap().registration_id.clone();
        store
            .update_state(|state| {
                state.selections.push(Selection {
                    project: PathBuf::from("/src/app"),
                    registration_id: id.clone(),
                });
                state.last_used.push(Timestamp {
                    registration_id: id.clone(),
                    at: 1_790_000_000,
                });
                Ok(())
            })
            .unwrap();
        store
            .update_sets(|sets| {
                sets.sets.push(SharedSet {
                    name: "core".into(),
                    default: true,
                    items: vec![Item {
                        kind: ItemKind::SkillSource,
                        value: "/home/u/.claude/skills".into(),
                    }],
                });
                sets.subscriptions.push(Subscription {
                    registration_id: id.clone(),
                    set: "core".into(),
                });
                Ok(())
            })
            .unwrap();
        assert_eq!(mode_of(&f.root.join(STATE)), 0o600);
        assert_eq!(mode_of(&f.root.join(SETS)), 0o600);
        drop(store);
        let read = f.open(OpenMode::Read).unwrap();
        let state = read.read_state().unwrap();
        assert_eq!(state.selections[0].project, PathBuf::from("/src/app"));
        assert_eq!(state.last_used[0].at, 1_790_000_000);
        let sets = read.read_sets().unwrap();
        assert_eq!(sets.sets[0].items[0].kind, ItemKind::SkillSource);
        assert_eq!(sets.subscriptions[0].registration_id, id);
        let text = fs::read_to_string(f.root.join(SETS)).unwrap();
        assert!(text.contains("\"skill_source\""), "{text}");
        let leftovers: Vec<_> = fs::read_dir(&f.root)
            .unwrap()
            .filter_map(|e| e.unwrap().file_name().into_string().ok())
            .filter(|n| n.starts_with(".roost-state-"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn read_mode_and_pending_operations_refuse_side_writes() {
        let f = Fixture::new();
        let store = f.open(OpenMode::Read).unwrap();
        let error = store.update_state(|_| Ok(())).unwrap_err();
        assert_eq!(error.code, "ownership");
        assert!(!f.root.join(STATE).exists());
        drop(store);
        let launch = f.open(OpenMode::Launch).unwrap();
        launch.update_state(|_| Ok(())).unwrap();
        assert!(f.root.join(STATE).exists());
        drop(launch);
        let mut store = f.open(OpenMode::Mutate).unwrap();
        let _stage = store.begin(Operation::Add, None).unwrap();
        let error = store.update_sets(|_| Ok(())).unwrap_err();
        assert_eq!(error.code, "ownership");
        assert!(!f.root.join(SETS).exists());
    }

    #[test]
    fn next_write_removes_only_private_temp_leftovers() {
        let f = Fixture::new();
        let private = format!(".roost-state-{}.tmp", platform::random_id().unwrap());
        let shared = format!(".roost-state-{}.tmp", platform::random_id().unwrap());
        f.private_file(&private);
        f.private_file(&shared);
        fs::set_permissions(f.root.join(&shared), fs::Permissions::from_mode(0o644)).unwrap();
        let store = f.open(OpenMode::Mutate).unwrap();
        store.update_sets(|_| Ok(())).unwrap();
        assert!(!f.root.join(&private).exists());
        assert!(f.root.join(&shared).exists());
        assert!(f.root.join(SETS).exists());
    }

    #[test]
    fn unknown_registrations_are_ignored_on_read_and_dropped_on_write() {
        let f = Fixture::new();
        let mut store = f.open(OpenMode::Mutate).unwrap();
        store.add("Work", false, None).unwrap();
        let id = store.find("Work", false).unwrap().registration_id.clone();
        let gone = platform::random_id().unwrap();
        let mut state = StateFile::empty(&store.root_id);
        for registration_id in [&id, &gone] {
            state.last_used.push(Timestamp {
                registration_id: registration_id.clone(),
                at: 5,
            });
        }
        fs::write(f.root.join(STATE), serde_json::to_vec(&state).unwrap()).unwrap();
        fs::set_permissions(f.root.join(STATE), fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(store.read_state().unwrap().last_used.len(), 1);
        assert!(
            fs::read_to_string(f.root.join(STATE))
                .unwrap()
                .contains(&gone)
        );
        store.update_state(|_| Ok(())).unwrap();
        let text = fs::read_to_string(f.root.join(STATE)).unwrap();
        assert!(text.contains(&id) && !text.contains(&gone));
    }

    #[test]
    fn invalid_or_unsafe_side_files_are_refused_and_preserved() {
        let f = Fixture::new();
        let store = f.open(OpenMode::Mutate).unwrap();
        let root_id = store.root_id.clone();
        let id = platform::random_id().unwrap();
        let duplicate = serde_json::json!({"schema_version":1,"root_id":root_id,
            "sets":[{"name":"a","default":false,"items":[]},{"name":"a","default":true,"items":[]}],
            "subscriptions":[],"plugin_auto_update":false});
        let foreign_root = serde_json::json!({"schema_version":1,"root_id":id,
            "sets":[],"subscriptions":[],"plugin_auto_update":false});
        let unknown_field = serde_json::json!({"schema_version":1,"root_id":root_id,
            "sets":[],"subscriptions":[],"plugin_auto_update":false,"extra":1});
        let version = serde_json::json!({"schema_version":2,"root_id":root_id,
            "sets":[],"subscriptions":[],"plugin_auto_update":false});
        for document in [
            b"not json".to_vec(),
            serde_json::to_vec(&duplicate).unwrap(),
            serde_json::to_vec(&foreign_root).unwrap(),
            serde_json::to_vec(&unknown_field).unwrap(),
            serde_json::to_vec(&version).unwrap(),
        ] {
            fs::write(f.root.join(SETS), &document).unwrap();
            fs::set_permissions(f.root.join(SETS), fs::Permissions::from_mode(0o600)).unwrap();
            assert_eq!(store.read_sets().unwrap_err().code, "ownership");
            assert_eq!(store.update_sets(|_| Ok(())).unwrap_err().code, "ownership");
            assert_eq!(fs::read(f.root.join(SETS)).unwrap(), document);
        }
        // A permissive or hardlinked destination is never read or replaced.
        fs::write(f.root.join(SETS), serde_json::to_vec(&version).unwrap()).unwrap();
        fs::set_permissions(f.root.join(SETS), fs::Permissions::from_mode(0o644)).unwrap();
        assert!(store.read_sets().is_err());
        assert!(store.update_sets(|_| Ok(())).is_err());
        fs::remove_file(f.root.join(SETS)).unwrap();
        store.update_sets(|_| Ok(())).unwrap();
        fs::hard_link(f.root.join(SETS), f.base.join("alias")).unwrap();
        assert_eq!(
            store.update_sets(|_| Ok(())).unwrap_err().code,
            "unsafe_path"
        );
        // A failing change aborts without writing anything.
        let error = store.update_state(|_| Err(err("usage", "no"))).unwrap_err();
        assert_eq!(error.code, "usage");
        assert!(!f.root.join(STATE).exists());
    }

    #[test]
    fn desktop_link_is_optional_in_older_state_and_holds_one_registration() {
        let f = Fixture::new();
        let mut store = f.open(OpenMode::Mutate).unwrap();
        store.add("Work", false, None).unwrap();
        store.add("Other", false, None).unwrap();
        let work = store.find("Work", false).unwrap().registration_id.clone();
        let other = store.find("Other", false).unwrap().registration_id.clone();
        // A state.json written before Desktop links existed still reads.
        let old = serde_json::json!({"schema_version":1,"root_id":store.root_id,
            "selections":[],"last_used":[{"registration_id":work,"at":5}],"links":[],
            "desktop_launched":[],"plugin_auto_update_at":null});
        fs::write(f.root.join(STATE), serde_json::to_vec(&old).unwrap()).unwrap();
        fs::set_permissions(f.root.join(STATE), fs::Permissions::from_mode(0o600)).unwrap();
        let state = store.read_state().unwrap();
        assert!(state.desktop_links.is_empty());
        assert_eq!(state.last_used[0].at, 5);
        // Without a link the written document keeps the older shape.
        store.update_state(|_| Ok(())).unwrap();
        let text = fs::read_to_string(f.root.join(STATE)).unwrap();
        assert!(!text.contains("desktop_links"), "{text}");
        store
            .update_state(|state| {
                state.desktop_links.push(DesktopLink {
                    registration_id: work.clone(),
                });
                Ok(())
            })
            .unwrap();
        assert_eq!(
            store.read_state().unwrap().desktop_links[0].registration_id,
            work
        );
        // Only one registration may borrow the conventional folder.
        let error = store
            .update_state(|state| {
                state.desktop_links.push(DesktopLink {
                    registration_id: other.clone(),
                });
                Ok(())
            })
            .unwrap_err();
        assert_eq!(error.code, "ownership");
        assert_eq!(store.read_state().unwrap().desktop_links.len(), 1);
    }

    #[test]
    fn reserved_directories_are_opened_or_created_lazily_and_privately() {
        let f = Fixture::new();
        let read = f.open(OpenMode::Read).unwrap();
        assert!(read.reserved_dir(DESKTOP, false).unwrap().is_none());
        assert_eq!(
            read.reserved_dir(DESKTOP, true).err().unwrap().code,
            "ownership"
        );
        assert!(!f.root.join(DESKTOP).exists());
        assert_eq!(
            read.reserved_dir("profiles", false).err().unwrap().code,
            "usage"
        );
        drop(read);
        let store = f.open(OpenMode::Launch).unwrap();
        let created = store.reserved_dir(DESKTOP, true).unwrap().unwrap();
        assert_eq!(mode_of(&f.root.join(DESKTOP)), 0o700);
        let again = store.reserved_dir(DESKTOP, false).unwrap().unwrap();
        assert_eq!(created.identity().unwrap(), again.identity().unwrap());
        assert!(store.reserved_dir(PLUGIN_STORE, false).unwrap().is_none());
    }
}
