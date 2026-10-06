//! The plugin store directory, `<root>/plugin-store/`: a Roost-owned Claude config
//! directory with no account that never runs sessions ([ADR 0001]).
//!
//! Creation is the journaled `store_create` operation: exclusively create the
//! private directory, record its identity, write `.roost-store.json`, then increment
//! the registry generation (registrations are unchanged). Everything inside the
//! store besides Roost's own files is written by Claude's CLI (see `crate::plugins`).
//!
//! [ADR 0001]: ../../docs/adr/0001-shared-plugins-injected-at-launch.md

use super::{
    Action, Artifact, Directory, LIMIT, Operation, REGISTRY, Result, Role, Store, encode, err,
    file_name, file_state, read_record, recovery, side::PLUGIN_STORE, state,
};
use crate::platform::{Entry, FileIdentity};
use serde::{Deserialize, Serialize};

/// The store marker.
pub(super) const STORE_MARKER: &str = ".roost-store.json";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct StoreMarker {
    schema_version: u32,
    root_id: String,
    directory_identity: FileIdentity,
}

impl Store {
    fn store_marker_valid(&self, store: &Directory) -> Result<bool> {
        let Some(bytes) = store.read(STORE_MARKER, true, LIMIT)? else {
            return Ok(false);
        };
        Ok(
            serde_json::from_slice::<StoreMarker>(&bytes).is_ok_and(|m| {
                m.schema_version == 1
                    && m.root_id == self.root_id
                    && store.identity().is_ok_and(|i| i == m.directory_identity)
            }),
        )
    }

    /// Opens the plugin store without following links; `None` when it was never
    /// created. A store without this root's valid marker is never used.
    pub fn plugin_store(&self) -> Result<Option<Directory>> {
        let Some(store) = self.reserved_dir(PLUGIN_STORE, false)? else {
            return Ok(None);
        };
        if !self.store_marker_valid(&store)? {
            return Err(err(
                "ownership",
                format!(
                    "Plugin store has no valid Roost marker: {}",
                    store.path.display()
                ),
            )
            .next("Preserve the directory for inspection; run roost doctor"));
        }
        Ok(Some(store))
    }

    /// Returns the marked plugin store, creating it on first use through the
    /// journaled `store_create` operation (mutating modes only).
    pub fn ensure_plugin_store(&mut self) -> Result<Directory> {
        if let Some(store) = self.plugin_store()? {
            return Ok(store);
        }
        let stage = self.begin(Operation::StoreCreate, None)?;
        // Exclusive creation: an object appearing meanwhile fails here.
        let created = self.directory.create_dir(PLUGIN_STORE)?;
        let identity = created.identity()?;
        self.record(Artifact {
            role: Role::Store,
            destination: created.path.clone(),
            staged: None,
            before: None,
            after: Some(state(identity.clone())),
            action: Action::Create,
            completed: false,
        })?;
        let marker = StoreMarker {
            schema_version: 1,
            root_id: self.root_id.clone(),
            directory_identity: identity,
        };
        created.write_new(STORE_MARKER, &encode(&marker)?, 0o600)?;
        created.sync()?;
        self.directory.sync()?;
        let next = self.registry.clone();
        self.commit(next, &stage)?;
        Ok(created)
    }

    /// Whether a present store entry is the journaled, marked store directory.
    pub(super) fn store_matches(
        &self,
        parent: &Directory,
        name: &str,
        entry: &Entry,
    ) -> Result<bool> {
        if !entry.is_dir {
            return Ok(false);
        }
        self.store_marker_valid(&parent.child(name, true)?)
    }

    /// Validates a `store` journal destination: exactly `<root>/plugin-store`.
    pub(super) fn validate_store_artifact(&self, a: &Artifact) -> Result<()> {
        if a.destination != self.root.join(PLUGIN_STORE) || a.staged.is_some() {
            return Err(recovery("Invalid plugin store destination"));
        }
        Ok(())
    }

    /// Recovery of an uncommitted `store_create`: remove a proven new directory that
    /// is empty or holds only a partial marker, or complete bookkeeping for a marked
    /// one.
    pub(super) fn recover_store_create(&mut self) -> Result<()> {
        let journal = self.intent.clone().unwrap();
        let stage_artifact = journal
            .artifacts
            .iter()
            .find(|a| a.role == Role::StagingDirectory)
            .ok_or_else(|| recovery("Plugin store creation has no staging identity"))?;
        if !self.matches(stage_artifact, stage_artifact.after.as_ref())? {
            return Err(recovery("Plugin store creation staging identity changed"));
        }
        let stage = self
            .stages
            .child(file_name(&stage_artifact.destination)?, true)?;
        // Without a recorded identity nothing authorizes cleanup: any directory stays
        // for inspection (an unmarked store is never used).
        let Some(a) = journal.artifacts.iter().find(|a| a.role == Role::Store) else {
            return self.cleanup();
        };
        let parent = self.parent_for(&a.destination)?;
        let name = file_name(&a.destination)?;
        let Some(entry) = parent.entry(name)? else {
            return self.cleanup();
        };
        if entry.is_link
            || !entry.is_dir
            || Some(&entry.identity) != a.after.as_ref().map(|s| &s.object_identity)
        {
            return Err(recovery(format!(
                "Plugin store matches neither recorded state: {}",
                a.destination.display()
            )));
        }
        if self.matches(a, a.after.as_ref())? {
            if let Some(index) = journal
                .artifacts
                .iter()
                .position(|a| a.role == Role::Registry)
            {
                self.publish(index)?;
                self.registry = read_record(&self.directory, REGISTRY)?;
                return self.cleanup();
            }
            let next = self.registry.clone();
            return self.commit(next, &stage);
        }
        let store = parent.child(name, true)?;
        let entries = store.entries()?;
        if entries == [STORE_MARKER] {
            let bytes = store.read(STORE_MARKER, true, LIMIT)?.unwrap_or_default();
            if serde_json::from_slice::<StoreMarker>(&bytes).is_ok()
                || file_state(&store, STORE_MARKER, true)?.is_none()
            {
                return Err(recovery(format!(
                    "Plugin store marker belongs elsewhere; preserve {}",
                    a.destination.display()
                )));
            }
            store.remove(STORE_MARKER, false)?;
        } else if !entries.is_empty() {
            return Err(recovery(format!(
                "New plugin store holds unexpected data; preserve {}",
                a.destination.display()
            )));
        }
        if parent.entry(name)?.map(|e| e.identity) != Some(entry.identity) {
            return Err(recovery("Plugin store changed during recovery"));
        }
        parent.remove(name, true)?;
        parent.sync()?;
        self.cleanup()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::super::*;
    use super::*;
    use crate::platform;
    use std::{fs, os::unix::fs::PermissionsExt};

    struct Fixture {
        base: PathBuf,
        root: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let base = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "roost-store-create-test-{}",
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
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.base);
        }
    }

    #[test]
    fn creation_is_journaled_and_bumps_only_the_generation() {
        let f = Fixture::new();
        let mut store = f.open(OpenMode::Mutate).unwrap();
        let generation = store.registry.generation;
        assert!(store.plugin_store().unwrap().is_none());
        let created = store.ensure_plugin_store().unwrap();
        assert_eq!(store.registry.generation, generation + 1);
        assert!(store.registry.registrations.is_empty());
        assert!(!store.pending());
        let again = store.ensure_plugin_store().unwrap();
        assert_eq!(created.identity().unwrap(), again.identity().unwrap());
        assert_eq!(store.registry.generation, generation + 1);
        drop(store);
        let read = f.open(OpenMode::Read).unwrap();
        assert!(read.plugin_store().unwrap().is_some());
        assert_eq!(
            read.reserved_dir(PLUGIN_STORE, false)
                .unwrap()
                .unwrap()
                .identity()
                .unwrap(),
            created.identity().unwrap()
        );
    }

    #[test]
    fn read_mode_cannot_create_and_unmarked_stores_are_refused() {
        let f = Fixture::new();
        let mut read = f.open(OpenMode::Read).unwrap();
        assert!(read.ensure_plugin_store().is_err());
        assert!(!f.root.join(PLUGIN_STORE).exists());
        drop(read);
        fs::create_dir(f.root.join(PLUGIN_STORE)).unwrap();
        fs::set_permissions(f.root.join(PLUGIN_STORE), fs::Permissions::from_mode(0o700)).unwrap();
        let mut store = f.open(OpenMode::Mutate).unwrap();
        assert_eq!(store.plugin_store().err().unwrap().code, "ownership");
        assert_eq!(store.ensure_plugin_store().err().unwrap().code, "ownership");
    }

    /// Interrupts creation after the directory's identity was journaled.
    fn interrupted(f: &Fixture, marker: bool) {
        let mut store = f.open(OpenMode::Mutate).unwrap();
        store.begin(Operation::StoreCreate, None).unwrap();
        let created = store.directory.create_dir(PLUGIN_STORE).unwrap();
        let identity = created.identity().unwrap();
        store
            .record(Artifact {
                role: Role::Store,
                destination: created.path.clone(),
                staged: None,
                before: None,
                after: Some(state(identity.clone())),
                action: Action::Create,
                completed: false,
            })
            .unwrap();
        if marker {
            let m = StoreMarker {
                schema_version: 1,
                root_id: store.root_id.clone(),
                directory_identity: identity,
            };
            created
                .write_new(STORE_MARKER, &encode(&m).unwrap(), 0o600)
                .unwrap();
        }
    }

    #[test]
    fn recovery_removes_an_unmarked_new_store_and_completes_a_marked_one() {
        let f = Fixture::new();
        interrupted(&f, false);
        let read = f.open(OpenMode::Read).unwrap();
        assert!(read.pending());
        drop(read);
        let store = f.open(OpenMode::Mutate).unwrap();
        assert!(!store.pending());
        assert!(!f.root.join(PLUGIN_STORE).exists());
        drop(store);

        let f = Fixture::new();
        interrupted(&f, true);
        let store = f.open(OpenMode::Mutate).unwrap();
        assert!(!store.pending());
        assert!(store.plugin_store().unwrap().is_some());
    }

    #[test]
    fn recovery_preserves_a_new_store_holding_unexpected_data() {
        let f = Fixture::new();
        interrupted(&f, false);
        fs::write(f.root.join(PLUGIN_STORE).join("foreign"), b"x").unwrap();
        assert!(f.open(OpenMode::Mutate).is_err());
        assert!(f.root.join(PLUGIN_STORE).join("foreign").exists());
    }
}
