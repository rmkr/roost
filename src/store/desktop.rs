//! Per-registration Claude Desktop data folders, `<root>/desktop/<REGISTRATION_ID>/`.
//!
//! A folder is Roost-owned data (its Desktop sign-in), never borrowed upstream data.
//! Creation is the journaled `desktop_create` operation: exclusively create the
//! private directory, record its identity, write `.roost-desktop.json`, then increment
//! the registry generation. Deletion (purge, upstream remove) is a journaled
//! `desktop_data` artifact that keeps the marker until every other entry is gone.

use super::{
    Action, Artifact, Directory, Error, Kind, LIMIT, Operation, Registration, Result, Role, Store,
    encode, err, file_name, partial_deletion, recovery,
    side::{DESKTOP, Reserved},
    state,
};
use crate::platform::{self, Entry, FileIdentity};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub(super) const DESKTOP_MARKER: &str = ".roost-desktop.json";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DesktopMarker {
    schema_version: u32,
    root_id: String,
    registration_id: String,
    directory_identity: FileIdentity,
}

/// A verified Desktop folder: its open handle and whether it carries a valid marker
/// (an unmarked folder is admitted only while empty).
struct Folder {
    directory: Directory,
    identity: FileIdentity,
    marked: bool,
}

impl Store {
    fn desktop_marker_valid(&self, folder: &Directory, id: &str) -> Result<bool> {
        let Some(bytes) = folder.read(DESKTOP_MARKER, true, LIMIT)? else {
            return Ok(false);
        };
        Ok(
            serde_json::from_slice::<DesktopMarker>(&bytes).is_ok_and(|m| {
                m.schema_version == 1
                    && m.root_id == self.root_id
                    && m.registration_id == id
                    && folder.identity().is_ok_and(|i| i == m.directory_identity)
            }),
        )
    }

    /// Opens `desktop/<id>` without following links. Absent is `None`; a folder with
    /// data but no valid marker is never claimed.
    fn desktop_folder_at(&self, id: &str) -> Result<Option<Folder>> {
        let Some(parent) = self.reserved_dir(Reserved::Desktop, false)? else {
            return Ok(None);
        };
        let Some(entry) = parent.entry(id)? else {
            return Ok(None);
        };
        let path = parent.path.join(id);
        if entry.is_link || !entry.is_dir {
            return Err(err(
                "unsafe_path",
                format!("Desktop folder is not a real directory: {}", path.display()),
            ));
        }
        let directory = parent.child(id, true)?;
        if directory.identity()? != entry.identity {
            return Err(err(
                "unsafe_path",
                "Desktop folder changed while opening it",
            ));
        }
        let marked = if directory.entry(DESKTOP_MARKER)?.is_some() {
            if !self.desktop_marker_valid(&directory, id)? {
                return Err(err(
                    "ownership",
                    format!(
                        "Desktop folder marker does not match this registration: {}",
                        path.display()
                    ),
                )
                .next("Preserve the folder for inspection; Roost will not claim it"));
            }
            true
        } else if directory.entries()?.is_empty() {
            false
        } else {
            return Err(err(
                "ownership",
                format!(
                    "Desktop folder holds data but no Roost marker: {}",
                    path.display()
                ),
            )
            .next("Preserve the folder for inspection; Roost will not claim it"));
        };
        Ok(Some(Folder {
            identity: entry.identity,
            directory,
            marked,
        }))
    }

    /// The registration's verified Desktop folder, when one exists (never for aliases).
    pub fn desktop_folder(&self, r: &Registration) -> Result<Option<PathBuf>> {
        self.check_root()?;
        if r.kind == Kind::DefaultAlias {
            return Ok(None);
        }
        Ok(self
            .desktop_folder_at(&r.registration_id)?
            .map(|f| f.directory.path))
    }

    /// Returns the registration's marked Desktop folder, creating it on first use
    /// through the journaled `desktop_create` operation. Allowed in Launch mode.
    pub fn ensure_desktop_folder(&mut self, r: &Registration) -> Result<PathBuf> {
        if r.kind == Kind::DefaultAlias {
            return Err(err("usage", "Default aliases have no Roost Desktop folder"));
        }
        let id = r.registration_id.clone();
        if let Some(folder) = self.desktop_folder_at(&id)? {
            if folder.marked {
                return Ok(folder.directory.path);
            }
            return Err(err(
                "ownership",
                format!(
                    "Desktop folder has no Roost marker: {}",
                    folder.directory.path.display()
                ),
            )
            .next("Remove that empty folder, then retry"));
        }
        let parent = self
            .reserved_dir(Reserved::Desktop, true)?
            .ok_or_else(|| err("io", "Desktop parent directory is unavailable"))?;
        let marker = |root_id: String, identity| DesktopMarker {
            schema_version: 1,
            root_id,
            registration_id: id.clone(),
            directory_identity: identity,
        };
        let folder = self.create_marked_dir(
            Operation::DesktopCreate,
            Some(&parent),
            &id,
            Role::DesktopData,
            DESKTOP_MARKER,
            |root_id, identity| encode(&marker(root_id, identity)),
        )?;
        Ok(folder.path)
    }

    /// Registration IDs of every Desktop folder with its `SingletonLock` link text,
    /// read without following anything. Unreadable entries are skipped.
    pub fn desktop_locks(&self) -> Result<Vec<(String, Option<PathBuf>)>> {
        let Some(parent) = self.reserved_dir(Reserved::Desktop, false)? else {
            return Ok(vec![]);
        };
        let mut locks = vec![];
        for name in parent.entries()? {
            let Ok(folder) = parent.child(&name, false) else {
                continue;
            };
            locks.push((name, folder.read_link("SingletonLock").ok().flatten()));
        }
        Ok(locks)
    }

    /// Desktop folders of `ids` holding nothing besides the marker (Desktop wrote no
    /// Electron data there, so it may have ignored `--user-data-dir`).
    pub fn desktop_without_data(&self, ids: &[String]) -> Result<Vec<PathBuf>> {
        let mut empty = vec![];
        for id in ids {
            if let Ok(Some(folder)) = self.desktop_folder_at(id)
                && folder.marked
                && folder.directory.entries()? == [DESKTOP_MARKER]
            {
                empty.push(folder.directory.path);
            }
        }
        Ok(empty)
    }

    /// A journaled deletion of the registration's Desktop folder, if it has one.
    pub(super) fn desktop_deletion(&self, r: &Registration) -> Result<Option<Artifact>> {
        Ok(self
            .desktop_folder_at(&r.registration_id)?
            .map(|folder| Artifact {
                role: Role::DesktopData,
                destination: folder.directory.path,
                staged: None,
                before: Some(state(folder.identity)),
                after: None,
                action: Action::Delete,
                completed: false,
            }))
    }

    /// Deletes the registration's own Desktop folder, if it has one, as the journaled
    /// `desktop_delete` operation (the caller confirmed it); registrations are
    /// unchanged. Returns whether a folder was deleted.
    pub fn delete_desktop_folder(&mut self, r: &Registration) -> Result<bool> {
        self.writable(false)?;
        let Some(artifact) = self.desktop_deletion(r)? else {
            return Ok(false);
        };
        let stage = self.begin(Operation::DesktopDelete, Some(r.registration_id.clone()))?;
        self.record(artifact)?;
        self.delete_desktop(self.intent.as_ref().unwrap().artifacts.len() - 1)?;
        let next = self.registry.clone();
        self.commit(next, &stage)?;
        Ok(true)
    }

    /// Whether a present Desktop folder entry is the journaled object: a marked
    /// folder, or (while deleting, after the marker went last) an empty one.
    pub(super) fn desktop_matches(
        &self,
        parent: &Directory,
        name: &str,
        entry: &Entry,
        action: Action,
    ) -> Result<bool> {
        if !entry.is_dir {
            return Ok(false);
        }
        let folder = parent.child(name, true)?;
        if folder.entry(DESKTOP_MARKER)?.is_some() {
            return self.desktop_marker_valid(&folder, name);
        }
        Ok(action == Action::Delete && folder.entries()?.is_empty())
    }

    /// Validates a `desktop_data` journal destination: `<root>/desktop/<journal ID>`.
    pub(super) fn validate_desktop_artifact(
        &self,
        a: &Artifact,
        id: Option<&String>,
    ) -> Result<()> {
        if a.destination.parent() != Some(self.root.join(DESKTOP).as_path())
            || Some(file_name(&a.destination)?) != id.map(String::as_str)
            || a.staged.is_some()
        {
            return Err(recovery("Invalid Desktop folder destination"));
        }
        Ok(())
    }

    /// Deletes the journaled Desktop folder at `index`: every entry but the marker,
    /// then the marker, then the empty folder. A failure leaves the marker and journal.
    pub(super) fn delete_desktop(&mut self, index: usize) -> Result<()> {
        let a = self.intent.as_ref().unwrap().artifacts[index].clone();
        if self.matches(&a, None)? {
            return self.publish(index);
        }
        if !self.matches(&a, a.before.as_ref())? {
            return Err(recovery(format!(
                "Desktop folder changed; deletion will not continue: {}",
                a.destination.display()
            )));
        }
        let parent = self.parent_for(&a.destination)?;
        let folder = parent.child(file_name(&a.destination)?, true)?;
        if folder.entry(DESKTOP_MARKER)?.is_some() {
            if platform::cancelled() {
                return Err(Error::cancelled());
            }
            folder.purge_children(DESKTOP_MARKER).map_err(|e| {
                partial_deletion(e, &folder.path, "Desktop marker", "Quit Claude Desktop for this profile, inspect roost doctor, then repeat the removal.")
            })?;
            self.check_root()?;
            if !self.matches(&a, a.before.as_ref())? {
                return Err(recovery("Desktop folder ownership changed during deletion"));
            }
            if folder.entries()? != [DESKTOP_MARKER] {
                return Err(recovery(
                    "Desktop folder contains newly introduced data; marker is retained",
                ));
            }
            folder.remove(DESKTOP_MARKER, false)?;
            folder.sync()?;
        }
        self.publish(index)
    }

    /// Recovery of an uncommitted `desktop_create` (a later launch refuses an
    /// unmarked folder left for inspection).
    pub(super) fn recover_desktop_create(&mut self) -> Result<()> {
        self.recover_marked_dir_create(Role::DesktopData, DESKTOP_MARKER, "Desktop folder", |b| {
            serde_json::from_slice::<DesktopMarker>(b).is_ok()
        })
    }
}

#[cfg(test)]
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
            let base = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "roost-desktop-test-{}",
                platform::random_id().unwrap()
            ));
            fs::create_dir(&base).unwrap();
            fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).unwrap();
            let root = base.join("managed");
            Store::open(&root, true, OpenMode::Mutate)
                .unwrap()
                .add("Work", false, None)
                .unwrap();
            Self { base, root }
        }
        fn open(&self, mode: OpenMode) -> Result<Store> {
            Store::open(&self.root, false, mode)
        }
        fn work(&self) -> Registration {
            self.open(OpenMode::Read)
                .unwrap()
                .find("Work", false)
                .unwrap()
                .clone()
        }
        /// Starts `desktop_create` and stops ("crashes") after creating the folder
        /// and recording its identity, then lets `contents` populate it.
        fn interrupted(&self, contents: impl FnOnce(&Store, &Directory)) -> PathBuf {
            let r = self.work();
            let mut store = self.open(OpenMode::Launch).unwrap();
            let parent = store
                .reserved_dir(Reserved::Desktop, true)
                .unwrap()
                .unwrap();
            let _stage = store
                .begin(Operation::DesktopCreate, Some(r.registration_id.clone()))
                .unwrap();
            let folder = parent.create_dir(&r.registration_id).unwrap();
            store
                .record(Artifact {
                    role: Role::DesktopData,
                    destination: folder.path.clone(),
                    staged: None,
                    before: None,
                    after: Some(state(folder.identity().unwrap())),
                    action: Action::Create,
                    completed: false,
                })
                .unwrap();
            contents(&store, &folder);
            folder.path.clone()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.base);
        }
    }
    fn valid_marker(store: &Store, folder: &Directory) {
        let marker = DesktopMarker {
            schema_version: 1,
            root_id: store.root_id.clone(),
            registration_id: file_name(&folder.path).unwrap().to_owned(),
            directory_identity: folder.identity().unwrap(),
        };
        folder
            .write_new(DESKTOP_MARKER, &encode(&marker).unwrap(), 0o600)
            .unwrap();
    }

    #[test]
    fn creation_is_journaled_once_and_then_reused() {
        let f = Fixture::new();
        let r = f.work();
        let mut store = f.open(OpenMode::Launch).unwrap();
        let generation = store.registry.generation;
        let folder = store.ensure_desktop_folder(&r).unwrap();
        assert_eq!(folder, f.root.join(DESKTOP).join(&r.registration_id));
        assert_eq!(store.registry.generation, generation + 1);
        assert!(!store.pending());
        assert_eq!(store.ensure_desktop_folder(&r).unwrap(), folder);
        assert_eq!(store.registry.generation, generation + 1);
        drop(store);
        let read = f.open(OpenMode::Read).unwrap();
        assert_eq!(read.desktop_folder(&r).unwrap(), Some(folder));
    }

    #[test]
    fn read_mode_cannot_create_a_desktop_folder() {
        let f = Fixture::new();
        let r = f.work();
        let mut store = f.open(OpenMode::Read).unwrap();
        assert!(store.ensure_desktop_folder(&r).is_err());
        assert!(!f.root.join(DESKTOP).exists());
    }

    #[test]
    fn interrupted_creation_blocks_launch_and_rolls_back_an_unmarked_folder() {
        let f = Fixture::new();
        let folder = f.interrupted(|_, _| ());
        let work = f.work();
        let launch = f.open(OpenMode::Launch).unwrap();
        assert!(launch.pending());
        assert!(crate::launch::profile_env(&launch, &work, false).is_err());
        drop(launch);
        let store = f.open(OpenMode::Mutate).unwrap();
        assert!(!store.pending());
        assert!(!folder.exists());
        assert!(f.root.join(DESKTOP).exists());
    }

    #[test]
    fn interrupted_creation_removes_a_partial_marker() {
        let f = Fixture::new();
        let folder = f.interrupted(|_, folder| {
            folder
                .write_new(DESKTOP_MARKER, b"{\"schema", 0o600)
                .unwrap();
        });
        assert!(!f.open(OpenMode::Mutate).unwrap().pending());
        assert!(!folder.exists());
    }

    #[test]
    fn interrupted_creation_completes_bookkeeping_for_a_marked_folder() {
        let f = Fixture::new();
        let generation = f.open(OpenMode::Read).unwrap().registry.generation;
        let folder = f.interrupted(valid_marker);
        let store = f.open(OpenMode::Mutate).unwrap();
        assert!(!store.pending());
        assert_eq!(store.registry.generation, generation + 1);
        assert!(folder.join(DESKTOP_MARKER).exists());
        drop(store);
        let r = f.work();
        let mut launch = f.open(OpenMode::Launch).unwrap();
        assert_eq!(launch.ensure_desktop_folder(&r).unwrap(), folder);
    }

    #[test]
    fn interrupted_creation_preserves_unexpected_data() {
        let f = Fixture::new();
        let folder = f.interrupted(|_, folder| {
            folder.write_new("Preferences", b"{}", 0o600).unwrap();
        });
        assert!(f.open(OpenMode::Mutate).is_err());
        assert!(folder.join("Preferences").exists());
        assert!(f.open(OpenMode::Read).unwrap().pending());
    }

    #[test]
    fn desktop_delete_is_journaled_and_an_interruption_never_continues_it() {
        let f = Fixture::new();
        let r = f.work();
        let mut launch = f.open(OpenMode::Launch).unwrap();
        let folder = launch.ensure_desktop_folder(&r).unwrap();
        fs::write(folder.join("Preferences"), "{}").unwrap();
        assert!(launch.delete_desktop_folder(&r).is_err());
        drop(launch);
        // Interrupted after recording the deletion: recovery stops once, keeping data.
        let mut store = f.open(OpenMode::Mutate).unwrap();
        let artifact = store.desktop_deletion(&r).unwrap().unwrap();
        let _stage = store
            .begin(Operation::DesktopDelete, Some(r.registration_id.clone()))
            .unwrap();
        store.record(artifact).unwrap();
        drop(store);
        assert_eq!(f.open(OpenMode::Mutate).err().unwrap().code, "ownership");
        assert!(folder.join("Preferences").exists());
        let mut store = f.open(OpenMode::Mutate).unwrap();
        assert!(!store.pending());
        let generation = store.registry.generation;
        assert!(store.delete_desktop_folder(&r).unwrap());
        assert!(!folder.exists());
        assert_eq!(store.registry.generation, generation + 1);
        assert!(!store.delete_desktop_folder(&r).unwrap());
        assert_eq!(store.registry.registrations.len(), 1);
    }

    #[test]
    fn unmarked_folders_are_never_claimed_or_purged() {
        let f = Fixture::new();
        let r = f.work();
        let folder = f.root.join(DESKTOP).join(&r.registration_id);
        fs::create_dir_all(&folder).unwrap();
        fs::set_permissions(f.root.join(DESKTOP), fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o700)).unwrap();
        let mut store = f.open(OpenMode::Launch).unwrap();
        assert_eq!(
            store.ensure_desktop_folder(&r).unwrap_err().code,
            "ownership"
        );
        fs::write(folder.join("Preferences"), "{}").unwrap();
        assert_eq!(
            store.ensure_desktop_folder(&r).unwrap_err().code,
            "ownership"
        );
        drop(store);
        let mut store = f.open(OpenMode::RetryPurge).unwrap();
        assert!(store.remove("Work", true).is_err());
        assert!(folder.join("Preferences").exists());
        assert!(f.root.join("profiles/Work").exists());
    }
}
