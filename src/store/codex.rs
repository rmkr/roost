use super::{Directory, FileIdentity, Result, Store, encode, err, read_record, side::Reserved};
use crate::platform;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const MARKER: &str = ".roost-codex.json";
const GENERATION: &str = ".roost-codex-generation";
const CONFIG: &str = "config.toml";
pub(crate) const FILE_AUTH: &str = "cli_auth_credentials_store = \"file\"";

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct HomeMarker {
    schema_version: u32,
    root_id: String,
    name: String,
    directory_identity: FileIdentity,
}

pub(crate) struct CodexHome {
    pub name: String,
    pub path: PathBuf,
    pub id: String,
    // Pins legacy directories across the unlocked picker until selection.
    pub unbound: Option<Directory>,
}

impl CodexHome {
    pub fn bind(mut self, store: &Store) -> Result<Self> {
        store.side_ready()?;
        if let Some(directory) = self.unbound.take() {
            directory.verify_namespace()?;
            let generation = platform::random_id()?;
            let temp = format!(".roost-tmp-{generation}");
            let identity = directory.write_new(&temp, generation.as_bytes(), 0o600)?;
            directory.publish_new_entry(&temp, identity, GENERATION)?;
            directory.sync()?;
            self.id = format!("codex:{}:{generation}", self.name);
        }
        Ok(self)
    }
}

/// Recognize old filesystem-only IDs as stale selections when reading state.
pub(crate) fn codex_selection_name(id: &str) -> Option<&str> {
    let (name, identity) = id.strip_prefix("codex:")?.split_once(':')?;
    if super::valid_id(identity) {
        return super::validate_name(name).ok().map(|()| name);
    }
    let (device, inode) = identity.split_once(':')?;
    (super::validate_name(name).is_ok()
        && device.parse::<u64>().is_ok()
        && inode.parse::<u64>().is_ok())
    .then_some(name)
}

impl Store {
    fn codex_marker(&self, name: &str, home: &Directory) -> Result<HomeMarker> {
        Ok(HomeMarker {
            schema_version: 1,
            root_id: self.root_id.clone(),
            name: name.to_owned(),
            directory_identity: home.identity()?,
        })
    }

    pub(crate) fn add_codex_home(&self, name: &str) -> Result<PathBuf> {
        self.side_ready()?;
        if platform::cancelled() {
            return Err(crate::Error::cancelled());
        }
        let parent = self.reserved_dir(Reserved::Codex, true)?.unwrap();
        if parent.entry(name)?.is_some() {
            return Err(err(
                "collision",
                format!("Codex home {name} already exists"),
            ));
        }
        // shortcut: Interrupted setup leaves a private staged directory. Clean these up if Roost adds home removal.
        let staged_name = format!(".roost-tmp-{}", platform::random_id()?);
        let staged = parent.create_dir(&staged_name)?;
        staged.write_new(CONFIG, format!("{FILE_AUTH}\n").as_bytes(), 0o600)?;
        staged.write_new(MARKER, &encode(&self.codex_marker(name, &staged)?)?, 0o600)?;
        staged.write_new(GENERATION, platform::random_id()?.as_bytes(), 0o600)?;
        staged.sync()?;
        parent.publish_new_entry(&staged_name, staged.identity()?, name)?;
        parent.sync()?;
        Ok(parent.path.join(name))
    }

    pub(crate) fn codex_profile(&self, name: &str) -> Result<CodexHome> {
        let missing = || {
            err("not_found", format!("Codex home {name} does not exist"))
                .next(format!("Run roost codex add {name}"))
        };
        let parent = self
            .reserved_dir(Reserved::Codex, false)?
            .ok_or_else(missing)?;
        let home = parent
            .child(name, true)
            .map_err(|e| if e.code == "not_found" { missing() } else { e })?;
        let marker: HomeMarker = read_record(&home, MARKER).map_err(|e| {
            if e.code == "not_found" {
                err(
                    "ownership",
                    format!("Codex home {name} has no Roost marker"),
                )
            } else {
                e
            }
        })?;
        if marker != self.codex_marker(name, &home)? {
            return Err(err(
                "ownership",
                format!("Codex home {name} does not match its Roost marker"),
            ));
        }
        home.open_file(CONFIG, false, false)?;
        if home.entry("auth.json")?.is_some() {
            home.open_file("auth.json", true, false)?;
        }
        self.check_root()?;
        home.verify_namespace()?;
        let path = home.path.clone();
        let (id, unbound) = match home.read(GENERATION, true, 32)? {
            Some(bytes) => {
                let generation = String::from_utf8(bytes)
                    .ok()
                    .filter(|id| super::valid_id(id))
                    .ok_or_else(|| err("ownership", "Invalid Codex home generation"))?;
                (format!("codex:{name}:{generation}"), None)
            }
            None => (format!("codex:{name}:unbound"), Some(home)),
        };
        Ok(CodexHome {
            name: name.into(),
            path,
            id,
            unbound,
        })
    }

    pub(crate) fn codex_home(&self, name: &str) -> Result<PathBuf> {
        Ok(self.codex_profile(name)?.path)
    }

    pub(crate) fn codex_profiles(&self, warnings: &mut Vec<String>) -> Result<Vec<CodexHome>> {
        let Some(parent) = self.reserved_dir(Reserved::Codex, false)? else {
            return Ok(vec![]);
        };
        let mut homes = Vec::new();
        for name in parent.matching_entries(|entry| {
            Ok(entry.is_some_and(|name| {
                super::validate_name(name).is_ok() && name == name.to_ascii_lowercase()
            }))
        })? {
            match self.codex_profile(&name) {
                Ok(home) => homes.push(home),
                Err(e) => warnings.push(format!("Skipping Codex home {name}: {}", e.message)),
            }
        }
        Ok(homes)
    }
}
