//! The plugin store: `roost plugin ...`, launch-time `CLAUDE_CODE_PLUGIN_DIRS`
//! injection, the opt-in daily auto-update and doctor's `plugin_shadowed` check
//! ([ADR 0001], [injected plugin research]).
//!
//! Only Claude's CLI writes the store (`claude plugin marketplace add|install|update|
//! uninstall` with `CLAUDE_CONFIG_DIR=<store>`), under the store lock
//! `plugin-store/.roost-store-lock`; the root lock is held only briefly and never
//! while that child runs. After each store mutation Roost runs the public
//! `claude plugin list --json` in the store and keeps what it needs (ID, install
//! path, version, manifest name, dependencies) in its own record
//! `plugin-store/.roost-store-plugins.json`. Launches read only that record and
//! recheck each path; Claude's `installed_plugins.json` is never parsed.
//!
//! [ADR 0001]: ../docs/adr/0001-shared-plugins-injected-at-launch.md
//! [injected plugin research]: ../.scratch/workflow-ux/research/injected-plugins.md

use crate::{
    Error, Result,
    cli::{MarketplaceAction, PluginAction},
    launch::{self, ProfileEnv},
    platform::{self, Directory},
    sets,
    store::{
        Kind, OpenMode, Registration, State, Store,
        side::{Item, ItemKind, PLUGIN_STORE, SetsFile},
    },
    table::{Cell, Style, Table},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs::File,
    io::IsTerminal,
    path::{Component, Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Roost's own record of the store's installed plugins.
const RECORD: &str = ".roost-store-plugins.json";
const RECORD_TEMP: &str = ".roost-store-plugins-";
/// The store lock, separate from the root lock.
const STORE_LOCK: &str = ".roost-store-lock";
const LOCK_WAIT: Duration = Duration::from_secs(10);
const RECORD_LIMIT: usize = 4 * 1024 * 1024;
const MANIFEST_LIMIT: usize = 1024 * 1024;
const AUTO_UPDATE_DEADLINE: Duration = Duration::from_secs(8);
const REFRESH_RESERVE: Duration = Duration::from_secs(2);
const AUTO_UPDATE_INTERVAL: u64 = 24 * 60 * 60;
/// Claude's documented grace period for orphaned plugin versions.
const ORPHAN_AGE_MS: u128 = 14 * 24 * 60 * 60 * 1000;
const PLUGIN_DIRS: &str = "CLAUDE_CODE_PLUGIN_DIRS";
/// Removed from store children: the store has no account and runs no session.
const STORE_REMOVED_ENV: &[&str] = &[
    "CLAUDE_CODE_OAUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_REFRESH_TOKEN",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    PLUGIN_DIRS,
];

/// `plugin-store/.roost-store-plugins.json`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Record {
    schema_version: u32,
    root_id: String,
    plugins: Vec<Installed>,
}
/// One installed store plugin as Claude listed it, plus its manifest facts.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Installed {
    id: String,
    /// Manifest name: the `<name>` of `<name>@inline` in a profile.
    name: String,
    install_path: PathBuf,
    version: Option<String>,
    /// Store plugin IDs this plugin depends on.
    dependencies: Vec<String>,
}
impl Record {
    fn empty(root_id: &str) -> Self {
        Self {
            schema_version: 1,
            root_id: root_id.to_owned(),
            plugins: vec![],
        }
    }
    fn get(&self, id: &str) -> Option<&Installed> {
        self.plugins.iter().find(|p| p.id == id)
    }
}

/// PLUGIN or MARKETPLACE: 1–128 characters, no whitespace, control characters,
/// `/`, `\` or `@`.
fn valid_part(part: &str) -> bool {
    (1..=128).contains(&part.chars().count())
        && !part
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '/' | '\\' | '@'))
}
/// Parses `PLUGIN[@MARKETPLACE]`.
fn parse_id(text: &str) -> Result<(String, Option<String>)> {
    let invalid = || {
        Error::new(
            "usage",
            format!("Invalid plugin {text}; expected PLUGIN or PLUGIN@MARKETPLACE"),
        )
    };
    match text.split_once('@') {
        Some((plugin, marketplace)) if valid_part(plugin) && valid_part(marketplace) => {
            Ok((plugin.to_owned(), Some(marketplace.to_owned())))
        }
        None if valid_part(text) => Ok((text.to_owned(), None)),
        _ => Err(invalid()),
    }
}
fn plugin_name(id: &str) -> &str {
    id.split_once('@').map_or(id, |(name, _)| name)
}
pub(crate) fn valid_id(id: &str) -> bool {
    matches!(id.split_once('@'), Some((p, m)) if valid_part(p) && valid_part(m))
}

// ---------------------------------------------------------------------------
// Store lock, children and Roost's record

/// Takes the store lock; with `wait`, polls up to 10 seconds like the root lock,
/// otherwise returns `None` at once when it is held. Never inherited by children.
fn store_lock(store: &Directory, wait: bool) -> Result<Option<File>> {
    if store.entry(STORE_LOCK)?.is_none()
        && let Err(error) = store.write_new(STORE_LOCK, b"", 0o600)
        && store.entry(STORE_LOCK)?.is_none()
    {
        return Err(error);
    }
    let file = store.open_file(STORE_LOCK, true, true)?;
    let deadline = Instant::now() + LOCK_WAIT;
    loop {
        if platform::cancelled() {
            return Err(Error::cancelled());
        }
        match file.try_lock() {
            Ok(()) => return Ok(Some(file)),
            Err(std::fs::TryLockError::WouldBlock) if !wait => return Ok(None),
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(25))
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(Error::new(
                    "io",
                    "Another Roost plugin operation holds the plugin store lock",
                )
                .next("Retry after the other operation finishes."));
            }
            Err(std::fs::TryLockError::Error(e)) => {
                return Err(Error::io("lock plugin store", &store.path, e));
            }
        }
    }
}

/// A Claude command against the store: caller environment plus the store as its
/// config directory, without credentials, injected plugins or auto-updates.
fn store_command(claude: &Path, store: &Path) -> Command {
    let mut command = Command::new(claude);
    for name in STORE_REMOVED_ENV {
        command.env_remove(name);
    }
    command
        .env("CLAUDE_CONFIG_DIR", store)
        .env("DISABLE_AUTOUPDATER", "1");
    command
}

/// A store command in progress: the version-checked Claude, the verified store
/// and the held store lock (the root lock is already released).
struct Session {
    claude: PathBuf,
    root_id: String,
    store: Directory,
    _lock: File,
}
impl Session {
    fn open(root: &Path) -> Result<Self> {
        let (claude, _) = launch::claude_info()?;
        let mut manager = Store::open(root, true, OpenMode::Mutate)?;
        let store = manager.ensure_plugin_store()?;
        let root_id = manager.root_id.clone();
        drop(manager);
        let lock = store_lock(&store, true)?.expect("waiting lock");
        Ok(Self {
            claude,
            root_id,
            store,
            _lock: lock,
        })
    }
    /// Runs `claude ARGS` in the store with inherited stdio and no deadline.
    fn run(&self, args: &[&str], retry: &str) -> Result<()> {
        let status = store_command(&self.claude, &self.store.path)
            .args(args)
            .status()
            .map_err(|_| {
                Error::new(
                    "plugin_store",
                    "Could not start Claude for the plugin store",
                )
            })?;
        if !status.success() {
            return Err(Error::new(
                "plugin_store",
                format!(
                    "claude {} failed in the plugin store ({})",
                    args.join(" "),
                    status
                        .code()
                        .map_or("terminated".to_owned(), |c| format!("exit {c}"))
                ),
            )
            .next(format!("Check Claude's output above, then retry: {retry}")));
        }
        Ok(())
    }
    fn listing(&self, available: bool, timeout: Duration) -> Result<Value> {
        listing(&self.claude, &self.store.path, available, timeout)
    }
    /// Re-learns installed plugins from Claude's listing and replaces the record.
    fn refresh(&self, timeout: Duration) -> Result<(Record, Vec<String>)> {
        refresh(&self.claude, &self.store, &self.root_id, timeout)
    }
}

/// `claude plugin list --json [--available]` in the store, bounded.
fn listing(claude: &Path, store: &Path, available: bool, timeout: Duration) -> Result<Value> {
    let mut command = store_command(claude, store);
    command.args(["plugin", "list", "--json"]);
    if available {
        command.arg("--available");
    }
    let (stdout, exit) = launch::probe(command, "plugin_store", timeout)?;
    let unreadable = || {
        Error::new("plugin_store", "Claude's plugin listing was not recognized")
            .next("Check the selected Claude installation with roost doctor")
    };
    if !exit.success() {
        return Err(unreadable());
    }
    serde_json::from_slice(&stdout).map_err(|_| unreadable())
}

/// Installed rows `(id, installPath, version)` of a listing: a JSON array, or the
/// `installed` array of an `--available` listing. Only well-formed fields are used.
fn installed_rows(
    listing: &Value,
    user_scope_only: bool,
) -> Vec<(String, PathBuf, Option<String>)> {
    let rows = listing
        .as_array()
        .or_else(|| listing["installed"].as_array());
    rows.into_iter()
        .flatten()
        .filter_map(|row| {
            let id = row["id"].as_str().filter(|id| valid_id(id))?;
            if user_scope_only && row["scope"] != "user" {
                return None;
            }
            let path = row["installPath"]
                .as_str()
                .filter(|p| !p.contains(['\r', '\n', '\0']))
                .map(PathBuf::from)
                .unwrap_or_default();
            let version = row["version"]
                .as_str()
                .filter(|v| v.len() <= 128 && !v.chars().any(char::is_control))
                .map(str::to_owned);
            Some((id.to_owned(), path, version))
        })
        .collect()
}

/// Plugin IDs available from the store's marketplaces in an `--available` listing.
fn available_ids(listing: &Value) -> Vec<String> {
    listing["available"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let id = match row["pluginId"].as_str() {
                Some(id) => id.to_owned(),
                None => format!(
                    "{}@{}",
                    row["name"].as_str()?,
                    row["marketplaceName"]
                        .as_str()
                        .or_else(|| row["marketplace"].as_str())?
                ),
            };
            valid_id(&id).then_some(id)
        })
        .collect()
}

/// Opens an install path through the store without following links. It must be
/// `<store>/plugins/cache/<marketplace>/<name>/<version>` with no `:`.
fn open_install(store: &Directory, path: &Path) -> Result<Directory> {
    let unsafe_path = || {
        Error::new(
            "unsafe_path",
            format!(
                "{} is not a plugin directory inside the store cache",
                path.display()
            ),
        )
    };
    let text = path.to_str().ok_or_else(unsafe_path)?;
    if text.contains(':') || !path.is_absolute() {
        return Err(unsafe_path());
    }
    let relative = path
        .strip_prefix(store.path.join("plugins").join("cache"))
        .map_err(|_| unsafe_path())?;
    let parts: Vec<&str> = relative
        .components()
        .map(|c| match c {
            Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect::<Option<_>>()
        .ok_or_else(unsafe_path)?;
    if parts.len() != 3 {
        return Err(unsafe_path());
    }
    let mut directory = store.child("plugins", false)?.child("cache", false)?;
    for part in parts {
        directory = directory.child(part, false)?;
    }
    Ok(directory)
}

/// Manifest name and raw dependency references of an installed plugin.
fn manifest(directory: &Directory) -> (Option<String>, Vec<(String, Option<String>)>) {
    let Some(value) = directory
        .child(".claude-plugin", false)
        .ok()
        .and_then(|d| d.read("plugin.json", false, MANIFEST_LIMIT).ok().flatten())
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
    else {
        return (None, vec![]);
    };
    let name = value["name"]
        .as_str()
        .filter(|n| valid_part(n))
        .map(str::to_owned);
    let dependencies = value["dependencies"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| match d {
            Value::String(text) => parse_id(text).ok(),
            Value::Object(_) => {
                let name = d["name"].as_str().filter(|n| valid_part(n))?;
                let marketplace = d["marketplace"].as_str().filter(|m| valid_part(m));
                Some((name.to_owned(), marketplace.map(str::to_owned)))
            }
            _ => None,
        })
        .collect();
    (name, dependencies)
}

fn refresh(
    claude: &Path,
    store: &Directory,
    root_id: &str,
    timeout: Duration,
) -> Result<(Record, Vec<String>)> {
    let listing = listing(claude, &store.path, false, timeout)?;
    let rows = installed_rows(&listing, true);
    let ids: Vec<&str> = rows.iter().map(|(id, _, _)| id.as_str()).collect();
    let mut warnings = vec![];
    let mut record = Record::empty(root_id);
    for (id, path, version) in &rows {
        let directory = match open_install(store, path) {
            Ok(directory) => directory,
            Err(error) => {
                warnings.push(format!(
                    "Store plugin {id} was not recorded: {}",
                    error.message
                ));
                continue;
            }
        };
        let (name, references) = manifest(&directory);
        let marketplace = id.split_once('@').map(|(_, m)| m).unwrap_or_default();
        let dependencies = references
            .into_iter()
            .filter_map(|(name, market)| {
                let wanted = format!("{name}@{}", market.as_deref().unwrap_or(marketplace));
                if ids.contains(&wanted.as_str()) {
                    return Some(wanted);
                }
                let any: Vec<&&str> = ids.iter().filter(|i| plugin_name(i) == name).collect();
                (market.is_none() && any.len() == 1).then(|| any[0].to_string())
            })
            .collect();
        record.plugins.push(Installed {
            id: id.clone(),
            name: name.unwrap_or_else(|| plugin_name(id).to_owned()),
            install_path: path.clone(),
            version: version.clone(),
            dependencies,
        });
    }
    record.plugins.sort_by(|a, b| a.id.cmp(&b.id));
    write_record(store, &record)?;
    Ok((record, warnings))
}

/// Atomic private replacement of the record under the store lock.
fn write_record(store: &Directory, record: &Record) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(record)
        .map_err(|_| Error::new("io", "Could not encode the plugin store record"))?;
    for name in store.entries()? {
        if name.starts_with(RECORD_TEMP) && name.ends_with(".tmp") {
            let _ = store
                .open_file(&name, true, false)
                .and_then(|_| store.remove(&name, false));
        }
    }
    let temp = format!("{RECORD_TEMP}{}.tmp", platform::random_id()?);
    store.write_new(&temp, &bytes, 0o600)?;
    if let Some(entry) = store.entry(RECORD)?
        && (!entry.is_file || entry.is_link || entry.nlink != 1)
    {
        let _ = store.remove(&temp, false);
        return Err(Error::new(
            "unsafe_path",
            format!(
                "Unsafe plugin store record {}",
                store.path.join(RECORD).display()
            ),
        ));
    }
    if let Err(error) = store.rename(&temp, store, RECORD) {
        let _ = store.remove(&temp, false);
        return Err(error);
    }
    store.sync()
}

/// Reads the record of a verified store (empty before the first install).
fn read_record_in(store: &Directory, root_id: &str) -> Result<Record> {
    let Some(bytes) = store.read(RECORD, true, RECORD_LIMIT)? else {
        return Ok(Record::empty(root_id));
    };
    let record: Record = serde_json::from_slice(&bytes).map_err(|_| {
        Error::new(
            "plugin_store",
            "The plugin store record is malformed or unsupported",
        )
        .next("Run roost plugin update to rebuild it")
    })?;
    if record.schema_version != 1 || record.root_id != root_id {
        return Err(Error::new(
            "plugin_store",
            "The plugin store record belongs to another root or schema",
        )
        .next("Run roost plugin update to rebuild it"));
    }
    Ok(record)
}

/// The store and its record; `None` when the store was never created.
fn read_record(store: &Store) -> Result<Option<(Directory, Record)>> {
    let Some(directory) = store.plugin_store()? else {
        return Ok(None);
    };
    let record = read_record_in(&directory, &store.root_id)?;
    Ok(Some((directory, record)))
}

/// Whether a plugin ID is installed in the plugin store (gates `set add --plugin`).
pub fn installed(store: &Store, id: &str) -> Result<bool> {
    Ok(read_record(store)?.is_some_and(|(_, record)| record.get(id).is_some()))
}

// ---------------------------------------------------------------------------
// Orphaned versions

/// Deletes store version directories whose `.orphaned_at` is more than 14 days old
/// and that the record does not reference. Returns warnings.
fn prune(store: &Directory, record: &Record) -> Vec<String> {
    let referenced: HashSet<&Path> = record
        .plugins
        .iter()
        .map(|p| p.install_path.as_path())
        .collect();
    let mut warnings = vec![];
    let Ok(cache) = store
        .child("plugins", false)
        .and_then(|p| p.child("cache", false))
    else {
        return warnings;
    };
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let directories = |parent: &Directory| -> Vec<(String, Directory)> {
        parent
            .entries()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|name| {
                let entry = parent.entry(&name).ok()??;
                if entry.is_link || !entry.is_dir {
                    return None;
                }
                let child = parent.child(&name, false).ok()?;
                (child.identity().ok()? == entry.identity).then_some((name, child))
            })
            .collect()
    };
    for (_, marketplace) in directories(&cache) {
        for (_, plugin) in directories(&marketplace) {
            for (name, version) in directories(&plugin) {
                let orphaned_at = version
                    .read(".orphaned_at", false, 64)
                    .ok()
                    .flatten()
                    .and_then(|b| String::from_utf8(b).ok())
                    .and_then(|t| t.trim().parse::<u128>().ok());
                let Some(at) = orphaned_at else { continue };
                if now_ms.saturating_sub(at) <= ORPHAN_AGE_MS
                    || referenced.contains(version.path.as_path())
                {
                    continue;
                }
                let still = plugin.entry(&name).ok().flatten().is_some_and(|e| {
                    !e.is_link && e.is_dir && Some(e.identity) == version.identity().ok()
                });
                let result = if still {
                    std::fs::remove_dir_all(&version.path)
                        .map_err(|e| Error::io("remove orphaned plugin version", &version.path, e))
                } else {
                    Err(Error::new(
                        "unsafe_path",
                        "Orphaned version changed before removal",
                    ))
                };
                if let Err(error) = result {
                    warnings.push(format!(
                        "Could not prune orphaned plugin version {}: {}",
                        version.path.display(),
                        error.message
                    ));
                }
            }
        }
    }
    warnings
}

// ---------------------------------------------------------------------------
// Launch: auto-update and injection

/// Store-plugin work for one launch. [`claim`] captures it under the root lock
/// (subscribed plugin IDs, the verified store handle and, when due, the claimed
/// auto-update timestamp); [`LaunchPlugins::finish`] runs after the caller has
/// released the root lock: the auto-update under the store lock only, then
/// injection from the record as it is after that update (so the launch already
/// gets updated versions). Nothing here stops the launch; warnings go to stderr.
pub struct LaunchPlugins {
    store: Option<Directory>,
    /// `<root>/plugin-store`: inherited plugin dirs under it are an outer launch's.
    owned: PathBuf,
    root_id: String,
    wanted: Vec<String>,
    update: bool,
    warnings: Vec<String>,
}

/// Under the held root lock: what this launch needs from the plugin store. A
/// default alias (not isolated) gets nothing. A due auto-update writes its new
/// timestamp here, first, so concurrent launches skip it.
pub fn claim(store: &Store, registration: &Registration, env: &ProfileEnv) -> LaunchPlugins {
    let mut plugins = LaunchPlugins {
        store: None,
        owned: store.root.join(PLUGIN_STORE),
        root_id: store.root_id.clone(),
        wanted: vec![],
        update: false,
        warnings: vec![],
    };
    if !env.is_isolated() {
        return plugins;
    }
    let sets = match store.read_sets() {
        Ok(sets) => sets,
        Err(error) => {
            plugins
                .warnings
                .push(format!("Skipped store plugins: {}", error.message));
            return plugins;
        }
    };
    plugins.wanted = sets::plugin_items(&sets, &registration.registration_id);
    plugins.update = sets.plugin_auto_update && claim_update(store, &mut plugins.warnings);
    if plugins.update || !plugins.wanted.is_empty() {
        match store.plugin_store() {
            Ok(directory) => plugins.store = directory,
            Err(error) => {
                plugins.warnings.push(format!(
                    "Skipped store plugins: {}; launching without them",
                    error.message
                ));
                plugins.update = false;
                plugins.wanted.clear();
            }
        }
    }
    plugins
}

impl LaunchPlugins {
    /// After the root lock is released: a claimed auto-update, then injection
    /// into `env`, printing warnings to stderr.
    pub fn finish(self, env: &mut ProfileEnv) {
        let mut warnings = self.warnings;
        if self.update
            && let Some(directory) = &self.store
        {
            warnings.extend(auto_update(directory, &self.root_id));
        }
        let paths = match self.wanted.is_empty() {
            true => vec![],
            false => inject(
                self.store.as_ref(),
                &self.root_id,
                self.wanted,
                &mut warnings,
            ),
        };
        if let Err(error) = env.append_paths(PLUGIN_DIRS, &self.owned, &paths) {
            warnings.push(format!("Skipped store plugins: {}", error.message));
        }
        for warning in warnings {
            eprintln!("warning: {warning}");
        }
    }
}

/// The store directories for `CLAUDE_CODE_PLUGIN_DIRS`: the dependency closure of
/// the `wanted` subscribed store plugins. Missing, orphaned or unsafe directories
/// and duplicate manifest names are skipped with warnings; an unreadable record
/// skips injection.
fn inject(
    directory: Option<&Directory>,
    root_id: &str,
    wanted: Vec<String>,
    warnings: &mut Vec<String>,
) -> Vec<PathBuf> {
    let record = match directory.map(|d| read_record_in(d, root_id)).transpose() {
        Ok(record) => record.unwrap_or_else(|| Record::empty(root_id)),
        Err(error) => {
            warnings.push(format!(
                "Skipped store plugins: {}; launching without them",
                error.message
            ));
            return vec![];
        }
    };
    // Dependency closure, subscribed plugins first.
    let mut order: Vec<String> = vec![];
    let mut queue: Vec<String> = wanted;
    while !queue.is_empty() {
        let id = queue.remove(0);
        if order.contains(&id) {
            continue;
        }
        match record.get(&id) {
            Some(plugin) => {
                queue.extend(plugin.dependencies.iter().cloned());
                order.push(id);
            }
            None => warnings.push(format!(
                "Skipped plugin {id}: not installed in the plugin store (roost plugin add {id})"
            )),
        }
    }
    let mut names: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for id in &order {
        names
            .entry(record.get(id).unwrap().name.as_str())
            .or_default()
            .push(id);
    }
    let mut paths = vec![];
    for id in &order {
        let plugin = record.get(id).unwrap();
        let same = &names[plugin.name.as_str()];
        if same.len() > 1 {
            if same[0] == id {
                warnings.push(format!(
                    "Skipped plugins {}: they share the plugin name {}",
                    same.join(", "),
                    plugin.name
                ));
            }
            continue;
        }
        let checked = directory
            .ok_or_else(|| Error::new("not_found", "the plugin store does not exist"))
            .and_then(|store| open_install(store, &plugin.install_path))
            .and_then(|version| match version.entry(".orphaned_at")? {
                Some(_) => Err(Error::new("not_found", "its store version is orphaned")),
                None => Ok(()),
            });
        match checked {
            Ok(()) => paths.push(plugin.install_path.clone()),
            Err(error) => warnings.push(format!(
                "Skipped plugin {id}: {}; run roost plugin update {id}",
                error.message
            )),
        }
    }
    paths
}

/// Under the held root lock, with auto-update on: when the daily update is due,
/// records the new time first (so concurrent launches skip) and returns true.
fn claim_update(store: &Store, warnings: &mut Vec<String>) -> bool {
    let state = match store.read_state() {
        Ok(state) => state,
        Err(error) => {
            warnings.push(format!("Skipped plugin auto-update: {}", error.message));
            return false;
        }
    };
    let now = platform::unix_seconds();
    if state
        .plugin_auto_update_at
        .is_some_and(|at| at <= now && now - at < AUTO_UPDATE_INTERVAL)
    {
        return false;
    }
    if let Err(error) = store.update_state(|state| {
        state.plugin_auto_update_at = Some(now);
        Ok(())
    }) {
        warnings.push(format!("Skipped plugin auto-update: {}", error.message));
        return false;
    }
    true
}

/// The claimed daily update, after the root lock is released: only if the store
/// lock is free (otherwise skip silently), `claude plugin update ID` per recorded
/// plugin, then the record refresh and orphan prune, all under the store lock,
/// within 8 seconds, output discarded. Never fails the launch.
fn auto_update(directory: &Directory, root_id: &str) -> Vec<String> {
    let _lock = match store_lock(directory, false) {
        Ok(Some(lock)) => lock,
        Ok(None) => return vec![],
        Err(error) => return vec![format!("Skipped plugin auto-update: {}", error.message)],
    };
    let result = (|| -> Result<Vec<String>> {
        let record = read_record_in(directory, root_id)?;
        if record.plugins.is_empty() {
            return Ok(vec![]);
        }
        let claude = launch::resolve_program("claude")?;
        let deadline = Instant::now() + AUTO_UPDATE_DEADLINE;
        // The updates stop early enough to leave the refresh its share of the deadline.
        let updates_by = deadline - REFRESH_RESERVE;
        let left = |until: Instant| {
            until
                .checked_duration_since(Instant::now())
                .filter(|d| !d.is_zero())
                .ok_or_else(|| Error::new("plugin_store", "it exceeded its 8-second deadline"))
        };
        let failed = record.plugins.iter().find_map(|plugin| {
            // Claude's CLI updates exactly one plugin per call.
            let mut command = store_command(&claude, &directory.path);
            command.args(["plugin", "update", &plugin.id]);
            match left(updates_by).and_then(|t| launch::probe(command, "plugin_store", t)) {
                Ok((_, exit)) if exit.success() => None,
                Ok(_) => Some(Error::new(
                    "plugin_store",
                    format!("claude plugin update {} failed", plugin.id),
                )),
                Err(error) => Some(Error::new("plugin_store", error.message)),
            }
        });
        // Claude may have changed versions before failing; re-learn the paths either way.
        let refreshed = left(deadline).and_then(|t| refresh(&claude, directory, root_id, t));
        if let Some(error) = failed {
            return Err(error);
        }
        let (record, mut warnings) = refreshed?;
        warnings.extend(prune(directory, &record));
        Ok(warnings)
    })();
    match result {
        Ok(warnings) => warnings,
        Err(error) => vec![format!(
            "Plugin auto-update stopped: {}; launching with the current store plugins (retry: roost plugin update)",
            error.message
        )],
    }
}

// ---------------------------------------------------------------------------
// Doctor

/// Store plugins one owned profile receives, by manifest name, for doctor.
pub struct ShadowJob {
    profile: String,
    directory: PathBuf,
    names: BTreeMap<String, String>,
}

/// Owned active profiles subscribed to store plugins, collected under the lock.
pub fn shadow_jobs(store: &Store) -> Vec<ShadowJob> {
    let (Ok(sets), Ok(Some((_, record)))) = (store.read_sets(), read_record(store)) else {
        return vec![];
    };
    store
        .registry
        .registrations
        .iter()
        .filter(|r| r.kind == Kind::Owned && r.state == State::Active)
        .filter_map(|r| {
            let names: BTreeMap<String, String> = sets::plugin_items(&sets, &r.registration_id)
                .into_iter()
                .filter_map(|id| record.get(&id).map(|p| (p.name.clone(), id)))
                .collect();
            (!names.is_empty()).then(|| ShadowJob {
                profile: r.name.clone(),
                directory: r.directory.clone().unwrap_or_default(),
                names,
            })
        })
        .collect()
}

/// Doctor findings (run after the Store is released): `plugin_shadowed` when an
/// injected store plugin would silently override a native install in the profile.
pub fn shadow_findings(jobs: Vec<ShadowJob>) -> Vec<Value> {
    let finding = |message: String, path: &Path, step: Option<String>| {
        json!({"code":"plugin_shadowed","severity":"warning","message":message,
            "path":path.display().to_string(),"next_step":step})
    };
    if jobs.is_empty() {
        return vec![];
    }
    let Ok(claude) = launch::resolve_program("claude") else {
        return vec![];
    };
    let mut findings = vec![];
    for job in jobs {
        let mut command = Command::new(&claude);
        command
            .env_remove(PLUGIN_DIRS)
            .env("CLAUDE_CONFIG_DIR", &job.directory)
            .env("DISABLE_AUTOUPDATER", "1")
            .args(["plugin", "list", "--json"]);
        let listing = launch::probe(command, "diagnostics", launch::PROBE_TIMEOUT)
            .ok()
            .filter(|(_, exit)| exit.success())
            .and_then(|(stdout, _)| serde_json::from_slice::<Value>(&stdout).ok());
        let Some(listing) = listing else {
            findings.push(finding(
                format!(
                    "Could not check {} for natively installed store plugins",
                    job.profile
                ),
                &job.directory,
                None,
            ));
            continue;
        };
        for (native, _, _) in installed_rows(&listing, false) {
            if let Some(store_id) = job.names.get(plugin_name(&native)) {
                findings.push(finding(
                    format!(
                        "{}: store plugin {store_id} silently overrides the native install {native}",
                        job.profile
                    ),
                    &job.directory,
                    Some(format!("roost run {} plugin uninstall {native}", job.profile)),
                ));
            }
        }
    }
    findings
}

// ---------------------------------------------------------------------------
// Commands

/// Runs a `roost plugin` subcommand; returns human lines and warnings and sets
/// `data` for JSON.
pub fn command(action: PluginAction, data: &mut Value) -> Result<(Vec<String>, Vec<String>)> {
    let root = platform::root()?;
    match action {
        PluginAction::List { .. } => list(&root, data),
        PluginAction::Marketplace {
            action: MarketplaceAction::Add { source },
        } => {
            if source.is_empty() || source.chars().any(char::is_control) {
                return Err(Error::new("usage", "Invalid marketplace SOURCE"));
            }
            let session = Session::open(&root)?;
            session.run(
                &["plugin", "marketplace", "add", &source],
                &format!("roost plugin marketplace add {source}"),
            )?;
            Ok((
                vec![format!("Added marketplace {source} to the plugin store")],
                vec![],
            ))
        }
        PluginAction::Marketplace {
            action: MarketplaceAction::Remove { name },
        } => remove_marketplace(&root, &name),
        PluginAction::Add {
            plugin,
            sets: named,
            no_set,
        } => add(&root, &plugin, named, no_set),
        PluginAction::Update { plugin } => update(&root, plugin.as_deref()),
        PluginAction::AutoUpdate { on, .. } => {
            let store = Store::open(&root, true, OpenMode::Mutate)?;
            store.update_sets(|sets| {
                sets.plugin_auto_update = on;
                Ok(())
            })?;
            Ok((
                vec![if on {
                    "Plugin auto-update is on: launches update store plugins at most once a day"
                        .into()
                } else {
                    "Plugin auto-update is off".into()
                }],
                vec![],
            ))
        }
        PluginAction::Remove { plugin } => remove(&root, &plugin),
    }
}

fn list(root: &Path, data: &mut Value) -> Result<(Vec<String>, Vec<String>)> {
    let Some(store) = Store::open_if_present(root)? else {
        return Ok((
            vec!["No store plugins; install one with roost plugin add PLUGIN".into()],
            vec![],
        ));
    };
    let sets = store.read_sets()?;
    let state = store.read_state()?;
    let mut warnings = vec![];
    let record = match read_record(&store) {
        Ok(found) => found.map(|(_, r)| r),
        Err(error) => {
            warnings.push(format!(
                "Plugin store record unavailable: {}",
                error.message
            ));
            None
        }
    };
    let plugins: Vec<Value> = known_ids(record.as_ref(), &sets)
        .into_iter()
        .map(|id| {
            let mut names: Vec<String> = sets
                .sets
                .iter()
                .filter(|s| {
                    s.items
                        .iter()
                        .any(|i| i.kind == ItemKind::Plugin && i.value == id)
                })
                .map(|s| s.name.clone())
                .collect();
            names.sort_by_key(|n| (n.to_ascii_lowercase(), n.clone()));
            let installed = record.as_ref().is_some_and(|r| r.get(&id).is_some());
            json!({"id":id,"sets":names,"installed":installed})
        })
        .collect();
    let last = state.plugin_auto_update_at;
    let mut lines = if plugins.is_empty() {
        vec!["No store plugins; install one with roost plugin add PLUGIN".into()]
    } else {
        Table::new()
            .column("Plugin", |r: &Value| {
                Cell::new(r["id"].as_str().unwrap_or("?"), Style::Bold)
            })
            .column("Sets", |r: &Value| {
                let sets: Vec<&str> = r["sets"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                if sets.is_empty() {
                    Cell::new("—", Style::Dim)
                } else {
                    Cell::plain(sets.join(", "))
                }
            })
            .column("Store", |r: &Value| {
                if r["installed"] == true {
                    Cell::new("installed", Style::Green)
                } else {
                    Cell::new("missing", Style::Yellow)
                }
            })
            .render(&plugins, crate::table::color_for(&std::io::stdout()))
    };
    lines.push(format!(
        "Auto-update: {}",
        if sets.plugin_auto_update { "on" } else { "off" }
    ));
    let plugins: Vec<Value> = plugins
        .into_iter()
        .map(|p| json!({"id":p["id"],"sets":p["sets"]}))
        .collect();
    *data =
        json!({"auto_update":sets.plugin_auto_update,"last_auto_update":last,"plugins":plugins});
    Ok((lines, warnings))
}

/// Store plugin IDs Roost knows: recorded in the store or named by a set.
fn known_ids(record: Option<&Record>, sets: &SetsFile) -> BTreeSet<String> {
    let recorded = record
        .into_iter()
        .flat_map(|r| r.plugins.iter().map(|p| p.id.clone()));
    let named = sets
        .sets
        .iter()
        .flat_map(|s| s.items.iter())
        .filter(|i| i.kind == ItemKind::Plugin)
        .map(|i| i.value.clone());
    recorded.chain(named).collect()
}

/// Sets the plugin goes into: `--set`, `--no-set`, or a terminal prompt.
fn choose_sets(root: &Path, named: Vec<String>, no_set: bool) -> Result<Vec<String>> {
    let chosen = if no_set || !named.is_empty() {
        named
    } else {
        prompt_sets(root)?
    };
    for set in &chosen {
        sets::validate_set_name(set)?;
    }
    Ok(chosen)
}

fn prompt_sets(root: &Path) -> Result<Vec<String>> {
    let available: Vec<String> = match Store::open(root, false, OpenMode::Read) {
        Ok(store) => store
            .read_sets()?
            .sets
            .into_iter()
            .map(|s| s.name)
            .collect(),
        Err(e) if e.code == "not_found" => vec![],
        Err(e) => return Err(e),
    };
    if available.is_empty() {
        return Ok(vec![]);
    }
    let usage = || {
        Error::new(
            "usage",
            "Choose sets for the plugin: --set SET (repeatable) or --no-set",
        )
    };
    if !std::io::stdin().is_terminal() {
        return Err(usage());
    }
    let answer = platform::prompt_line(&format!(
        "Sets: {}\nAdd the plugin to which sets? (comma-separated, empty for none): ",
        available.join(", ")
    ))
    .map_err(|e| if e.code == "usage" { usage() } else { e })?;
    Ok(answer
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Checks that every set exists and that adding `item` to them keeps each
/// subscriber free of conflicts. Returns the stored set names.
fn check_sets(
    store: &Store,
    sets: &SetsFile,
    chosen: &[String],
    item: &Item,
) -> Result<Vec<String>> {
    let mut simulated = sets.clone();
    let mut names = vec![];
    for set in chosen {
        let target = sets::find_set_mut(&mut simulated, set)?;
        names.push(target.name.clone());
        if !target.items.contains(item) {
            target.items.push(item.clone());
        }
    }
    for registration in &store.registry.registrations {
        let found = sets::conflicts(&simulated, registration);
        if !found.is_empty() {
            return Err(Error::new(
                "collision",
                format!(
                    "Adding {} would conflict for {}: {}",
                    item.value,
                    registration.name,
                    found.join("; ")
                ),
            )
            .next("Choose other sets or drop the conflicting plugin first"));
        }
    }
    Ok(names)
}

/// After reacquiring the root lock: refuses when the root was replaced since the
/// store command started (its root ID differs).
fn same_root(current: &str, root_id: &str) -> Result<()> {
    if current != root_id {
        return Err(Error::new(
            "ownership",
            "The Roost root changed while the plugin command was running",
        )
        .next("Inspect the root with roost doctor, then repeat the command"));
    }
    Ok(())
}

fn add(
    root: &Path,
    plugin: &str,
    named: Vec<String>,
    no_set: bool,
) -> Result<(Vec<String>, Vec<String>)> {
    let (name, marketplace) = parse_id(plugin)?;
    let chosen = choose_sets(root, named, no_set)?;
    let session = Session::open(root)?;
    let id = match marketplace {
        Some(marketplace) => format!("{name}@{marketplace}"),
        None => {
            let listing = session.listing(true, Duration::from_secs(10))?;
            let mut candidates: BTreeSet<String> = available_ids(&listing).into_iter().collect();
            candidates.extend(
                installed_rows(&listing, true)
                    .into_iter()
                    .map(|(id, _, _)| id),
            );
            let missing = Error::new(
                "not_found",
                format!("No marketplace in the plugin store offers {name}"),
            )
            .next("Add its marketplace with roost plugin marketplace add SOURCE");
            resolve_known(&candidates, plugin, missing, "Name one: roost plugin add")?
        }
    };
    let item = Item {
        kind: ItemKind::Plugin,
        value: id.clone(),
    };
    {
        let manager = Store::open(root, false, OpenMode::Read)?;
        same_root(&manager.root_id, &session.root_id)?;
        check_sets(&manager, &manager.read_sets()?, &chosen, &item)?;
    }
    let mut retry = format!("roost plugin add {id}");
    if chosen.is_empty() {
        retry.push_str(" --no-set");
    }
    for set in &chosen {
        retry.push_str(&format!(" --set {set}"));
    }
    session.run(&["plugin", "install", &id], &retry)?;
    let (record, mut warnings) = session.refresh(Duration::from_secs(10))?;
    let root_id = session.root_id.clone();
    drop(session);
    if record.get(&id).is_none() {
        warnings.push(format!(
            "Claude did not list {id} after installing it; launches will skip it"
        ));
    }
    let mut lines = vec![format!("Installed {id} in the plugin store")];
    if !chosen.is_empty() {
        let manager = Store::open(root, false, OpenMode::Mutate)?;
        // The sets are re-checked (existence, conflicts) inside update_sets.
        same_root(&manager.root_id, &root_id).map_err(|e| {
            Error::new(
                e.code,
                format!("{id} is installed but was not added to sets: {}", e.message),
            )
            .next("Inspect the root with roost doctor, then add it with roost set add SET --plugin ID")
        })?;
        let mut added = vec![];
        manager
            .update_sets(|sets| {
                added = check_sets(&manager, sets, &chosen, &item)?;
                for set in sets.sets.iter_mut().filter(|s| added.contains(&s.name)) {
                    if !set.items.contains(&item) {
                        set.items.push(item.clone());
                    }
                }
                Ok(())
            })
            .map_err(|e| {
                Error::new(
                    e.code,
                    format!("{id} is installed but was not added to sets: {}", e.message),
                )
                .next(format!("roost set add SET --plugin {id}"))
            })?;
        lines.push(format!("Added {id} to {}", added.join(", ")));
    }
    Ok((lines, warnings))
}

/// Resolves PLUGIN[@MARKETPLACE] against known store plugins (record and sets).
fn resolve_store(known: &BTreeSet<String>, plugin: &str) -> Result<String> {
    let missing = Error::new("not_found", format!("No store plugin named {plugin}"))
        .next("List store plugins with roost plugin list");
    resolve_known(known, plugin, missing, "Name one as")
}

/// Resolves PLUGIN[@MARKETPLACE] against `known`: `missing` when no ID has
/// that name, and `name_one` prefixes the hint when several do.
fn resolve_known(
    known: &BTreeSet<String>,
    plugin: &str,
    missing: Error,
    name_one: &str,
) -> Result<String> {
    let (name, marketplace) = parse_id(plugin)?;
    if marketplace.is_some() {
        return Ok(plugin.to_owned());
    }
    let matches: Vec<&String> = known.iter().filter(|id| plugin_name(id) == name).collect();
    match matches.as_slice() {
        [id] => Ok((*id).clone()),
        [] => Err(missing),
        _ => Err(Error::new(
            "not_found",
            format!(
                "{name} is ambiguous: {}",
                matches
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )
        .next(format!("{name_one} {name}@MARKETPLACE"))),
    }
}

fn update(root: &Path, plugin: Option<&str>) -> Result<(Vec<String>, Vec<String>)> {
    if let Some(plugin) = plugin {
        parse_id(plugin)?;
    }
    let session = Session::open(root)?;
    // An unreadable record is rebuilt from Claude's listing.
    let record = read_record_in(&session.store, &session.root_id)
        .or_else(|_| session.refresh(Duration::from_secs(10)).map(|(r, _)| r))?;
    let known: BTreeSet<String> = record.plugins.iter().map(|p| p.id.clone()).collect();
    let ids: Vec<String> = match plugin {
        Some(plugin) => {
            let id = resolve_store(&known, plugin)?;
            if !known.contains(&id) {
                return Err(Error::new(
                    "not_found",
                    format!("{id} is not installed in the plugin store"),
                )
                .next(format!("roost plugin add {id}")));
            }
            vec![id]
        }
        None => known.into_iter().collect(),
    };
    if ids.is_empty() {
        return Ok((vec!["No store plugins to update".into()], vec![]));
    }
    let retry = match plugin {
        Some(_) => format!("roost plugin update {}", ids[0]),
        None => "roost plugin update".to_owned(),
    };
    for id in &ids {
        if let Err(error) = session.run(&["plugin", "update", id], &retry) {
            // Claude may have changed versions before failing; re-learn the paths.
            let _ = session.refresh(Duration::from_secs(10));
            return Err(error);
        }
    }
    let (record, mut warnings) = session.refresh(Duration::from_secs(10))?;
    warnings.extend(prune(&session.store, &record));
    Ok((
        vec![format!(
            "Updated {}; profiles pick it up at their next launch",
            ids.join(", ")
        )],
        warnings,
    ))
}

/// Refuses while a set still names a plugin from the marketplace; Claude's
/// remove may also uninstall the marketplace's plugins, so the record is refreshed.
fn remove_marketplace(root: &Path, name: &str) -> Result<(Vec<String>, Vec<String>)> {
    if !valid_part(name) {
        return Err(Error::new("usage", format!("Invalid marketplace {name}")));
    }
    let mut used: Vec<String> = Store::open(root, true, OpenMode::Mutate)?
        .read_sets()?
        .sets
        .iter()
        .flat_map(|s| &s.items)
        .filter(|i| {
            i.kind == ItemKind::Plugin && i.value.split_once('@').map(|(_, m)| m) == Some(name)
        })
        .map(|i| i.value.clone())
        .collect();
    used.sort();
    used.dedup();
    if let Some(first) = used.first() {
        return Err(Error::new(
            "plugin_store",
            format!("Sets still use plugins from {name}: {}", used.join(", ")),
        )
        .next(format!("Remove them first: roost plugin remove {first}")));
    }
    let session = Session::open(root)?;
    session.run(
        &["plugin", "marketplace", "remove", name],
        &format!("roost plugin marketplace remove {name}"),
    )?;
    let warnings = session.refresh(Duration::from_secs(10))?.1;
    Ok((
        vec![format!("Removed marketplace {name} from the plugin store")],
        warnings,
    ))
}

fn remove(root: &Path, plugin: &str) -> Result<(Vec<String>, Vec<String>)> {
    parse_id(plugin)?;
    let mut warnings = vec![];
    let (id, installed, root_id) = {
        let manager = Store::open(root, true, OpenMode::Mutate)?;
        // Like list: an unreadable record warns, and the store is checked below.
        let (record, unreadable) = match read_record(&manager) {
            Ok(found) => (found.map(|(_, r)| r), false),
            Err(error) => {
                warnings.push(format!(
                    "Plugin store record unavailable: {}",
                    error.message
                ));
                (None, true)
            }
        };
        let known = known_ids(record.as_ref(), &manager.read_sets()?);
        let id = resolve_store(&known, plugin)?;
        manager.update_sets(|sets| {
            for set in &mut sets.sets {
                set.items
                    .retain(|i| !(i.kind == ItemKind::Plugin && i.value == id));
            }
            Ok(())
        })?;
        let installed = unreadable || record.is_some_and(|r| r.get(&id).is_some());
        (id, installed, manager.root_id.clone())
    };
    let mut lines = vec![format!("Removed {id} from every set")];
    if !installed {
        lines.push(format!("{id} was not installed in the plugin store"));
        return Ok((lines, warnings));
    }
    let session = Session::open(root)?;
    // Reacquired: the same root, and the plugin is still recorded in its store.
    same_root(&session.root_id, &root_id)?;
    let record = read_record_in(&session.store, &session.root_id)
        .or_else(|_| session.refresh(Duration::from_secs(10)).map(|(r, _)| r))?;
    if record.get(&id).is_none() {
        lines.push(format!("{id} is no longer installed in the plugin store"));
        return Ok((lines, warnings));
    }
    session.run(
        &["plugin", "uninstall", &id],
        &format!("roost plugin remove {id}"),
    )?;
    warnings.extend(session.refresh(Duration::from_secs(10))?.1);
    lines.push(format!(
        "Uninstalled {id} from the plugin store; profiles lose it at their next launch"
    ));
    Ok((lines, warnings))
}
