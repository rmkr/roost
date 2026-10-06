//! Shared sets: `roost set ...`, default/copied subscriptions on `add`, the `sets`
//! list field and launch-time link reconciliation for owned profiles.
//!
//! Definitions and subscriptions live in `sets.json`; links Roost created live in
//! `state.json` (`links`). Reconciliation only ever removes a link it recorded and
//! that is still the recorded symlink; anything else in a profile wins.
//!
//! Extension points:
//! - Link kinds: [`PLACEMENTS`] maps item kinds to a profile subdirectory and how a
//!   source expands; [`LINKED`] lists the kinds reconciled at launch: skills under
//!   `skills/` and instruction fragments under `rules/` (config-dir `rules/` loads
//!   like `~/.claude/rules/`, so no `CLAUDE.md` import block is needed).
//! - Plugins: [`plugin_installed`] gates `set add --plugin`; [`plugin_items`] gives
//!   a registration's subscribed plugin IDs for injection.

use crate::{
    Error, Result,
    cli::{ItemArgs, SetAction},
    platform::{self, Directory},
    store::{
        self, Kind, OpenMode, Registration, State, Store,
        side::{Item, ItemKind, LinkRecord, SetsFile, SharedSet, Subscription},
    },
    table::{Cell, Style, Table},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// How one kind of linked item lands in a profile.
struct Placement {
    /// Item naming one object, linked by its basename.
    explicit: ItemKind,
    /// Item naming a directory whose matching children are each linked.
    source: ItemKind,
    /// Profile subdirectory holding the links.
    directory: &'static str,
    select: Select,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Select {
    /// Child directories (skills).
    Directories,
    /// Child regular `.md` files (instruction fragments).
    Markdown,
}
const PLACEMENTS: [Placement; 2] = [
    Placement {
        explicit: ItemKind::Skill,
        source: ItemKind::SkillSource,
        directory: "skills",
        select: Select::Directories,
    },
    Placement {
        explicit: ItemKind::Instruction,
        source: ItemKind::InstructionSource,
        directory: "rules",
        select: Select::Markdown,
    },
];
/// Item kinds linked into owned profiles at launch.
const LINKED: &[ItemKind] = &[
    ItemKind::Skill,
    ItemKind::SkillSource,
    ItemKind::Instruction,
    ItemKind::InstructionSource,
];
/// Claude-managed per-account names that are never linked, replaced or removed.
const SYNCED: &str = "synced";

fn placement(kind: ItemKind) -> Option<&'static Placement> {
    PLACEMENTS
        .iter()
        .find(|p| p.explicit == kind || p.source == kind)
}

/// Whether `name` may become a link name: Unicode, no line breaks, not hidden and
/// not the Claude-managed `synced` entry.
fn linkable(name: &str, select: Select) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && !name.contains(['\r', '\n', '/', '\0'])
        && name != SYNCED
        && (select == Select::Directories || name.ends_with(".md"))
}

/// One desired link: profile-relative path and absolute target.
type Links = BTreeMap<PathBuf, BTreeSet<PathBuf>>;

/// Expands one item into desired links (relative path → target). Unreadable
/// sources yield nothing and a warning; sources are only listed, never written.
fn expand(item: &Item, links: &mut Links, warnings: &mut Vec<String>) {
    let Some(placement) = placement(item.kind) else {
        return;
    };
    let value = PathBuf::from(&item.value);
    let mut push = |name: &str, target: PathBuf| {
        links
            .entry(Path::new(placement.directory).join(name))
            .or_default()
            .insert(target);
    };
    if item.kind == placement.explicit {
        if let Some(name) = value.file_name().and_then(|n| n.to_str())
            && linkable(name, placement.select)
        {
            push(name, value.clone());
        }
        return;
    }
    let entries = match std::fs::read_dir(&value) {
        Ok(entries) => entries,
        Err(error) => {
            warnings.push(format!(
                "Skipped source {}: {}",
                value.display(),
                error.kind()
            ));
            return;
        }
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let matches = match placement.select {
            Select::Directories => kind.is_dir(),
            Select::Markdown => kind.is_file(),
        };
        if matches && linkable(&name, placement.select) {
            push(&name, value.join(&name));
        }
    }
}

fn find_set<'a>(sets: &'a SetsFile, name: &str) -> Result<&'a SharedSet> {
    sets.sets
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| not_found(name))
}
fn find_set_mut<'a>(sets: &'a mut SetsFile, name: &str) -> Result<&'a mut SharedSet> {
    sets.sets
        .iter_mut()
        .find(|s| s.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| not_found(name))
}
fn not_found(name: &str) -> Error {
    Error::new("not_found", format!("No set named {name}")).next("List sets with roost set list")
}
fn validate_set_name(name: &str) -> Result<()> {
    store::validate_name(name).map_err(|_| Error::new("usage", format!("Invalid set name {name}")))
}

/// Items of every set `registration_id` subscribes to, in set order.
fn subscribed<'a>(sets: &'a SetsFile, registration_id: &str) -> Vec<&'a Item> {
    sets.sets
        .iter()
        .filter(|set| {
            sets.subscriptions.iter().any(|s| {
                s.registration_id == registration_id && s.set.eq_ignore_ascii_case(&set.name)
            })
        })
        .flat_map(|set| set.items.iter())
        .collect()
}

/// Subscribed plugin IDs of a registration, deduplicated (plugin injection).
pub fn plugin_items(sets: &SetsFile, registration_id: &str) -> Vec<String> {
    let mut ids: Vec<String> = subscribed(sets, registration_id)
        .into_iter()
        .filter(|i| i.kind == ItemKind::Plugin)
        .map(|i| i.value.clone())
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// Whether a plugin ID is installed in the plugin store.
fn plugin_installed(store: &Store, id: &str) -> Result<bool> {
    crate::plugins::installed(store, id)
}

/// Conflicts a registration's subscriptions would have: link paths produced by two
/// different targets (owned only), or one plugin name from two marketplaces.
pub(crate) fn conflicts(sets: &SetsFile, registration: &Registration) -> Vec<String> {
    let items = subscribed(sets, &registration.registration_id);
    let mut found = vec![];
    if registration.kind == Kind::Owned {
        let mut links = Links::new();
        let mut ignored = vec![];
        for item in &items {
            expand(item, &mut links, &mut ignored);
        }
        for (path, targets) in &links {
            if targets.len() > 1 {
                found.push(format!(
                    "{} would link {}",
                    path.display(),
                    targets
                        .iter()
                        .map(|t| t.display().to_string())
                        .collect::<Vec<_>>()
                        .join(" and ")
                ));
            }
        }
    }
    let mut plugins: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for item in items.iter().filter(|i| i.kind == ItemKind::Plugin) {
        if let Some((name, marketplace)) = item.value.split_once('@') {
            plugins.entry(name).or_default().insert(marketplace);
        }
    }
    for (name, marketplaces) in plugins {
        if marketplaces.len() > 1 {
            found.push(format!(
                "plugin {name} would come from {}",
                marketplaces.into_iter().collect::<Vec<_>>().join(" and ")
            ));
        }
    }
    found
}

fn item(args: ItemArgs) -> Result<Item> {
    let path = |kind: ItemKind, path: PathBuf| -> Result<Item> {
        let absolute = platform::absolute(&path)?;
        let value = absolute
            .to_str()
            .ok_or_else(|| Error::new("unsafe_path", "Path is not valid Unicode"))?
            .to_owned();
        if let Some(placement) = placement(kind)
            && kind == placement.explicit
        {
            let name = absolute.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !linkable(name, placement.select) {
                return Err(Error::new(
                    "usage",
                    format!(
                        "{} cannot be linked: hidden, named synced{}",
                        absolute.display(),
                        if placement.select == Select::Markdown {
                            " or not a .md file"
                        } else {
                            ""
                        }
                    ),
                ));
            }
        }
        Ok(Item { kind, value })
    };
    if let Some(p) = args.skill {
        return path(ItemKind::Skill, p);
    }
    if let Some(p) = args.skills_from {
        return path(ItemKind::SkillSource, p);
    }
    if let Some(p) = args.instruction {
        return path(ItemKind::Instruction, p);
    }
    if let Some(p) = args.instructions_from {
        return path(ItemKind::InstructionSource, p);
    }
    let id = args.plugin.unwrap_or_default();
    let valid = |part: &str| {
        (1..=128).contains(&part.chars().count())
            && !part
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '/' | '\\' | '@'))
    };
    match id.split_once('@') {
        Some((plugin, marketplace)) if valid(plugin) && valid(marketplace) => Ok(Item {
            kind: ItemKind::Plugin,
            value: id,
        }),
        _ => Err(Error::new(
            "usage",
            format!("Invalid plugin ID {id}; expected PLUGIN@MARKETPLACE"),
        )),
    }
}

fn subscriber_names(store: &Store, sets: &SetsFile, set: &str) -> Vec<String> {
    let mut names: Vec<String> = sets
        .subscriptions
        .iter()
        .filter(|s| s.set.eq_ignore_ascii_case(set))
        .filter_map(|s| {
            store
                .registry
                .registrations
                .iter()
                .find(|r| r.registration_id == s.registration_id)
                .map(|r| r.name.clone())
        })
        .collect();
    names.sort_by_key(|n| (n.to_ascii_lowercase(), n.clone()));
    names
}

/// A registration that may hold subscriptions.
fn subscriber(store: &Store, name: &str) -> Result<Registration> {
    let registration = store.find(name, true)?.clone();
    // Upstream registrations are never retained: removal drops their record.
    if registration.kind == Kind::DefaultAlias {
        return Err(Error::new(
            "usage",
            "Default aliases receive no shared items",
        ));
    }
    Ok(registration)
}

/// Runs a `roost set` subcommand; returns human lines and sets `data` for JSON.
pub fn command(action: SetAction, data: &mut Value) -> Result<Vec<String>> {
    let root = platform::root()?;
    if let SetAction::List { .. } = action {
        let store = match Store::open(&root, false, OpenMode::Read) {
            Ok(store) => store,
            Err(e)
                if e.code == "not_found"
                    && std::fs::symlink_metadata(&root)
                        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                *data = json!({"sets":[]});
                return Ok(vec!["No sets; create one with roost set create SET".into()]);
            }
            Err(e) => return Err(e),
        };
        let sets = store.read_sets()?;
        let mut records: Vec<Value> = sets
            .sets
            .iter()
            .map(|set| {
                json!({"name":set.name,"default":set.default,"items":set.items,
                    "subscribers":subscriber_names(&store, &sets, &set.name)})
            })
            .collect();
        records.sort_by_key(|r| {
            let name = r["name"].as_str().unwrap_or("").to_owned();
            (name.to_ascii_lowercase(), name)
        });
        *data = json!({ "sets": records });
        if records.is_empty() {
            return Ok(vec!["No sets; create one with roost set create SET".into()]);
        }
        let list = |key: &'static str| {
            move |r: &Value| {
                let items: Vec<String> = r[key]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|v| match v {
                        Value::String(s) => s.clone(),
                        v => format!(
                            "{}:{}",
                            v["kind"].as_str().unwrap_or("?"),
                            v["value"].as_str().unwrap_or("?")
                        ),
                    })
                    .collect();
                if items.is_empty() {
                    Cell::new("—", Style::Dim)
                } else {
                    Cell::plain(items.join(", "))
                }
            }
        };
        return Ok(Table::new()
            .column("Set", |r: &Value| {
                Cell::new(r["name"].as_str().unwrap_or("?"), Style::Bold)
            })
            .column("Default", |r: &Value| {
                Cell::plain(if r["default"] == true { "yes" } else { "no" })
            })
            .column("Subscribers", list("subscribers"))
            .column("Items", list("items"))
            .render(&records, crate::table::stdout_color()));
    }
    let store = Store::open(&root, true, OpenMode::Mutate)?;
    let mut lines = vec![];
    match action {
        SetAction::List { .. } => unreachable!(),
        SetAction::Create { set, default } => {
            validate_set_name(&set)?;
            store.update_sets(|sets| {
                if sets.sets.iter().any(|s| s.name.eq_ignore_ascii_case(&set)) {
                    return Err(Error::new("collision", format!("Set {set} already exists")));
                }
                sets.sets.push(SharedSet {
                    name: set.clone(),
                    default,
                    items: vec![],
                });
                Ok(())
            })?;
            lines.push(format!(
                "Created set {set}{}",
                if default { " (default)" } else { "" }
            ));
        }
        SetAction::Delete { set } => {
            validate_set_name(&set)?;
            store.update_sets(|sets| {
                let name = find_set(sets, &set)?.name.clone();
                sets.sets.retain(|s| s.name != name);
                sets.subscriptions
                    .retain(|s| !s.set.eq_ignore_ascii_case(&name));
                Ok(())
            })?;
            lines.push(format!(
                "Deleted set {set}; its links leave each profile at the next launch"
            ));
        }
        SetAction::Default { set, off } => {
            validate_set_name(&set)?;
            store.update_sets(|sets| {
                find_set_mut(sets, &set)?.default = !off;
                Ok(())
            })?;
            lines.push(format!(
                "Set {set} is {}default",
                if off { "no longer " } else { "" }
            ));
        }
        SetAction::Add { set, item: args } => {
            validate_set_name(&set)?;
            let item = item(args)?;
            if item.kind == ItemKind::Plugin && !plugin_installed(&store, &item.value)? {
                return Err(Error::new(
                    "not_found",
                    format!("Plugin {} is not installed in the plugin store", item.value),
                )
                .next(format!("roost plugin add {} --set {set}", item.value)));
            }
            let mut added = false;
            store.update_sets(|sets| {
                let target = find_set_mut(sets, &set)?;
                if !target.items.contains(&item) {
                    target.items.push(item.clone());
                    added = true;
                }
                Ok(())
            })?;
            lines.push(if added {
                format!("Added {} to {set}", item.value)
            } else {
                format!("{} is already in {set}", item.value)
            });
        }
        SetAction::Drop { set, item: args } => {
            validate_set_name(&set)?;
            let item = item(args)?;
            store.update_sets(|sets| {
                find_set_mut(sets, &set)?.items.retain(|i| *i != item);
                Ok(())
            })?;
            lines.push(format!(
                "Dropped {} from {set}; its links leave at the next launch",
                item.value
            ));
        }
        SetAction::Subscribe { name, sets: names } => {
            for set in &names {
                validate_set_name(set)?;
            }
            let registration = subscriber(&store, &name)?;
            store.update_sets(|sets| {
                for set in &names {
                    let set = find_set(sets, set)?.name.clone();
                    let exists = sets.subscriptions.iter().any(|s| {
                        s.registration_id == registration.registration_id
                            && s.set.eq_ignore_ascii_case(&set)
                    });
                    if !exists {
                        sets.subscriptions.push(Subscription {
                            registration_id: registration.registration_id.clone(),
                            set,
                        });
                    }
                }
                let found = conflicts(sets, &registration);
                if !found.is_empty() {
                    return Err(Error::new(
                        "collision",
                        format!("Subscribed sets conflict: {}", found.join("; ")),
                    )
                    .next("Drop one of the conflicting items or unsubscribe a set"));
                }
                Ok(())
            })?;
            lines.push(format!(
                "Subscribed {} to {}",
                registration.name,
                names.join(", ")
            ));
            if registration.kind == Kind::Upstream {
                lines.push(
                    "Upstream profiles receive only plugin items; skills and instructions are ignored"
                        .into(),
                );
            }
        }
        SetAction::Unsubscribe { name, sets: names } => {
            for set in &names {
                validate_set_name(set)?;
            }
            let registration = subscriber(&store, &name)?;
            store.update_sets(|sets| {
                for set in &names {
                    let set = find_set(sets, set)?.name.clone();
                    sets.subscriptions.retain(|s| {
                        s.registration_id != registration.registration_id || s.set != set
                    });
                }
                Ok(())
            })?;
            lines.push(format!(
                "Unsubscribed {} from {}; links leave at the next launch",
                registration.name,
                names.join(", ")
            ));
        }
    }
    Ok(lines)
}

/// After a committed owned `add`: subscribes the new registration to every
/// default set, or, when `copied_from` is the physical directory of an owned
/// registration, to that registration's sets. Returns warnings (never fails the add).
pub fn subscribe_new(store: &Store, name: &str, copied_from: Option<&Path>) -> Vec<String> {
    let result = (|| {
        let registration = store.find(name, false)?.clone();
        let source = match copied_from {
            Some(path) => {
                let identity = Some(Directory::open(path, false)?.identity()?);
                store
                    .registry
                    .registrations
                    .iter()
                    .find(|r| r.kind == Kind::Owned && r.directory_identity == identity)
                    .map(|r| r.registration_id.clone())
            }
            None => None,
        };
        let current = store.read_sets()?;
        let wanted: Vec<String> = match &source {
            Some(id) => current
                .subscriptions
                .iter()
                .filter(|s| &s.registration_id == id)
                .map(|s| s.set.clone())
                .collect(),
            None => current
                .sets
                .iter()
                .filter(|s| s.default)
                .map(|s| s.name.clone())
                .collect(),
        };
        if wanted.is_empty() {
            return Ok(());
        }
        store.update_sets(|sets| {
            for set in wanted {
                sets.subscriptions.push(Subscription {
                    registration_id: registration.registration_id.clone(),
                    set,
                });
            }
            Ok(())
        })
    })();
    match result {
        Ok(()) => vec![],
        Err(error) => vec![format!(
            "{name} was created but not subscribed to sets: {}; subscribe with roost set subscribe {name} SET...",
            error.message
        )],
    }
}

/// Adds each ProfileRecord's `sets` field (subscribed set names sorted by folded
/// name; empty for aliases). Returns warnings when `sets.json` is unreadable.
pub fn annotate(store: &Store, profiles: &mut [Value]) -> Vec<String> {
    let (sets, warnings) = match store.read_sets() {
        Ok(sets) => (Some(sets), vec![]),
        Err(error) => (None, vec![format!("Sets unavailable: {}", error.message)]),
    };
    for profile in profiles.iter_mut() {
        let registration = profile["name"].as_str().and_then(|name| {
            store
                .registry
                .registrations
                .iter()
                .find(|r| r.name == name && r.kind != Kind::DefaultAlias)
        });
        let mut names: Vec<String> = match (&sets, registration) {
            (Some(sets), Some(r)) => sets
                .sets
                .iter()
                .filter(|set| {
                    sets.subscriptions.iter().any(|s| {
                        s.registration_id == r.registration_id
                            && s.set.eq_ignore_ascii_case(&set.name)
                    })
                })
                .map(|set| set.name.clone())
                .collect(),
            _ => vec![],
        };
        names.sort_by_key(|n| (n.to_ascii_lowercase(), n.clone()));
        profile["sets"] = json!(names);
    }
    warnings
}

/// After `remove`/`purge` commits: drops subscriptions and link records of
/// registrations no longer in the registry (retained owned ones keep theirs).
pub fn forget_removed(store: &Store) -> Vec<String> {
    let mut warnings = vec![];
    let sets = store.update_sets(|_| Ok(()));
    let state = store.update_state(|_| Ok(()));
    for (result, step) in [
        (sets, "roost set unsubscribe NAME SET..."),
        (state, "roost doctor"),
    ] {
        if let Err(error) = result {
            warnings.push(format!(
                "Manager records were not cleaned: {}; readers ignore them, or inspect with {step}",
                error.message
            ));
        }
    }
    warnings
}

/// Status of a recorded link's object.
enum Recorded {
    /// Still the recorded symlink with the recorded text.
    Intact,
    /// Nothing at the path (or its directory is gone).
    Missing,
    /// Something else is there; never touched.
    Replaced,
}

/// Opens an existing link directory of an owned profile without following links.
fn link_directory(profile: &Directory, name: &str) -> Result<Option<Directory>> {
    let Some(entry) = profile.entry(name)? else {
        return Ok(None);
    };
    if entry.is_link || !entry.is_dir {
        return Err(Error::new(
            "unsafe_path",
            format!("{} is not a directory", profile.path.join(name).display()),
        ));
    }
    let directory = profile.child(name, false)?;
    if directory.identity()? != entry.identity {
        return Err(Error::new(
            "unsafe_path",
            format!("{} changed while opening", directory.path.display()),
        ));
    }
    Ok(Some(directory))
}

fn split(path: &Path) -> Option<(&str, &str)> {
    let mut parts = path.iter().map(|p| p.to_str());
    match (parts.next(), parts.next(), parts.next()) {
        (Some(Some(directory)), Some(Some(name)), None) if name != SYNCED => {
            Some((directory, name))
        }
        _ => None,
    }
}

fn inspect(profile: &Directory, record: &LinkRecord) -> Result<Recorded> {
    let Some((directory, name)) = split(&record.path) else {
        return Ok(Recorded::Replaced);
    };
    let Some(directory) = link_directory(profile, directory)? else {
        return Ok(Recorded::Missing);
    };
    Ok(match directory.entry(name)? {
        None => Recorded::Missing,
        Some(entry)
            if entry.is_link
                && entry.identity == record.link_identity
                && directory.read_link(name)?.as_deref() == Some(record.target.as_path()) =>
        {
            Recorded::Intact
        }
        Some(_) => Recorded::Replaced,
    })
}

/// Unlinks a recorded link after re-verifying it is still the recorded symlink.
fn unlink(profile: &Directory, record: &LinkRecord) -> Result<()> {
    let (directory, name) = split(&record.path)
        .ok_or_else(|| Error::new("unsafe_path", "Recorded link path is malformed"))?;
    let directory = link_directory(profile, directory)?
        .ok_or_else(|| Error::new("not_found", "Link directory disappeared"))?;
    let entry = directory.entry(name)?;
    if !entry.is_some_and(|e| e.is_link && e.identity == record.link_identity)
        || directory.read_link(name)?.as_deref() != Some(record.target.as_path())
    {
        return Err(Error::new("unsafe_path", "Link changed before removal"));
    }
    directory.remove(name, false)
}

/// Creates one desired link; `Ok(None)` when existing content wins.
fn create(profile: &Directory, path: &Path, target: &Path) -> Result<Option<LinkRecord>> {
    let (directory, name) =
        split(path).ok_or_else(|| Error::new("unsafe_path", "Malformed link path"))?;
    let directory = match link_directory(profile, directory)? {
        Some(directory) => directory,
        None => profile.create_dir(directory)?,
    };
    if directory.entry(name)?.is_some() {
        return Ok(None);
    }
    let link_identity = directory.symlink(target, name)?;
    Ok(Some(LinkRecord {
        registration_id: String::new(),
        path: path.to_owned(),
        target: target.to_owned(),
        link_identity,
    }))
}

/// Launch-time reconciliation for an owned active registration, under the held
/// lock in `OpenMode::Launch`. Never fails the launch: every problem is a warning.
/// Removes only links recorded for this registration that are still the recorded
/// symlink; creates desired links where nothing exists; existing content wins.
pub fn reconcile(store: &Store, registration: &Registration) -> Vec<String> {
    let mut warnings = vec![];
    if registration.kind != Kind::Owned || registration.state != State::Active {
        return warnings;
    }
    let (sets, state) = match (store.read_sets(), store.read_state()) {
        (Ok(sets), Ok(state)) => (sets, state),
        (Err(error), _) | (_, Err(error)) => {
            warnings.push(format!("Skipped shared sets: {}", error.message));
            return warnings;
        }
    };
    let mut links = Links::new();
    for item in subscribed(&sets, &registration.registration_id)
        .into_iter()
        .filter(|i| LINKED.contains(&i.kind))
    {
        expand(item, &mut links, &mut warnings);
    }
    let mut desired = BTreeMap::new();
    for (path, targets) in links {
        if targets.len() == 1 {
            desired.insert(path, targets.into_iter().next().unwrap());
        } else {
            warnings.push(format!(
                "Skipped {}: provided by more than one subscribed item",
                path.display()
            ));
        }
    }
    let recorded: Vec<LinkRecord> = state
        .links
        .iter()
        .filter(|l| l.registration_id == registration.registration_id)
        .cloned()
        .collect();
    if desired.is_empty() && recorded.is_empty() {
        return warnings;
    }
    let profile = match store.profile_directory(registration) {
        Ok(profile) => profile,
        Err(error) => {
            warnings.push(format!("Skipped shared sets: {}", error.message));
            return warnings;
        }
    };
    let mut keep = vec![];
    for record in recorded {
        let wanted = desired.get(&record.path) == Some(&record.target);
        match inspect(&profile, &record) {
            Ok(Recorded::Intact) if wanted => {
                desired.remove(&record.path);
                keep.push(record);
            }
            Ok(Recorded::Intact) => {
                if let Err(error) = unlink(&profile, &record) {
                    warnings.push(format!(
                        "Kept link {}: {}",
                        record.path.display(),
                        error.message
                    ));
                    keep.push(record);
                }
            }
            Ok(Recorded::Missing) => {
                if !wanted {
                    warnings.push(format!(
                        "Recorded link {} is already gone",
                        record.path.display()
                    ));
                }
            }
            Ok(Recorded::Replaced) => warnings.push(format!(
                "{} was replaced; left untouched and no longer managed",
                record.path.display()
            )),
            Err(error) => {
                warnings.push(format!(
                    "Kept record for {}: {}",
                    record.path.display(),
                    error.message
                ));
                keep.push(record);
            }
        }
    }
    let mut created = vec![];
    for (path, target) in desired {
        match create(&profile, &path, &target) {
            Ok(Some(mut record)) => {
                record.registration_id = registration.registration_id.clone();
                created.push(record);
            }
            Ok(None) => warnings.push(format!(
                "Skipped {}: existing content in the profile wins",
                path.display()
            )),
            Err(error) => warnings.push(format!("Skipped {}: {}", path.display(), error.message)),
        }
    }
    let mut records = keep;
    records.extend(created.iter().cloned());
    let id = &registration.registration_id;
    if let Err(error) = store.update_state(|state| {
        state.links.retain(|l| &l.registration_id != id);
        state.links.extend(records);
        Ok(())
    }) {
        warnings.push(format!("Link records not saved: {}", error.message));
        // Unrecorded links would become foreign content; undo this launch's links.
        for record in &created {
            let _ = unlink(&profile, record);
        }
    }
    warnings
}

/// Reconciles before handing off to Claude, printing warnings to stderr (the
/// launch outcome is never printed once the child is exec'd).
pub fn reconcile_at_launch(store: &Store, registration: &Registration) {
    for warning in reconcile(store, registration) {
        eprintln!("warning: {warning}");
    }
}
