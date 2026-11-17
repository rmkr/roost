//! Selected profile per project, `roost switch`, bare `roost` and last use.
//!
//! Selections and last-use times live in the manager's `state.json`, keyed by
//! Claude registration ID or Codex home identity.
use crate::{
    Error, Outcome, Result, cli, launch, platform, sets,
    store::{
        CodexHome, Kind, OpenMode, Registration, State, Store, codex_selection_name,
        side::{Selection, StateFile, Timestamp},
    },
    table,
};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

/// The current project key and any safe warnings from computing it.
pub struct Project {
    pub key: Result<PathBuf>,
    pub warnings: Vec<String>,
}

/// Computes the project key for the current directory (see [`project_key`]).
pub fn project() -> Project {
    match std::env::current_dir() {
        Ok(cwd) => project_key(&cwd),
        Err(e) => Project {
            key: Err(Error::new(
                "io",
                format!("Cannot read the current directory: {e}"),
            )),
            warnings: vec![],
        },
    }
}

/// Walks from `cwd` toward the root to the first `.git` entry. A directory is the
/// git directory; a `gitdir: PATH` file names it relative to the file's directory.
/// A `commondir` file in the git directory names the shared common directory, so all
/// worktrees of a repository share one key. Without `.git` the key is `cwd`; a
/// malformed `.git` file or `commondir` falls back to `cwd` with a warning. Git is
/// never run and `GIT_DIR`/`GIT_COMMON_DIR` are never read.
pub fn project_key(cwd: &Path) -> Project {
    let fallback = |warning: Option<String>| Project {
        key: checked_key(cwd),
        warnings: warning.into_iter().collect(),
    };
    for directory in cwd.ancestors() {
        let dot_git = directory.join(".git");
        if fs::symlink_metadata(&dot_git).is_err() {
            continue;
        }
        return match common_directory(directory, &dot_git) {
            Some(common) => Project {
                key: checked_key(&common),
                warnings: vec![],
            },
            None => fallback(Some(format!(
                "Malformed git metadata at {}; using the current directory as the project",
                dot_git.display()
            ))),
        };
    }
    fallback(None)
}

fn checked_key(path: &Path) -> Result<PathBuf> {
    platform::absolute(path).map_err(|_| {
        Error::new(
            "unsafe_path",
            "The project path is not valid Unicode or contains a line break",
        )
    })
}

/// Reads a small single-line file, without its trailing line ending.
fn single_line(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(4097)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 4096 {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    let text = text
        .strip_suffix('\n')
        .map(|t| t.strip_suffix('\r').unwrap_or(t))
        .unwrap_or(&text);
    (!text.is_empty() && !text.contains(['\r', '\n', '\0'])).then(|| text.to_owned())
}

fn common_directory(directory: &Path, dot_git: &Path) -> Option<PathBuf> {
    let metadata = fs::metadata(dot_git).ok()?;
    let git_directory = if metadata.is_dir() {
        dot_git.to_owned()
    } else if metadata.is_file() {
        let line = single_line(dot_git)?;
        let target = line.strip_prefix("gitdir:")?.trim_start();
        (!target.is_empty()).then(|| directory.join(target))?
    } else {
        return None;
    };
    if !git_directory.is_dir() {
        return None;
    }
    let commondir = git_directory.join("commondir");
    match fs::symlink_metadata(&commondir) {
        Err(_) => Some(git_directory),
        Ok(_) => Some(git_directory.join(single_line(&commondir)?)).filter(|p| p.is_dir()),
    }
}

/// The key for a selection command: prints fallback warnings, refuses unsafe keys.
fn selection_key() -> Result<PathBuf> {
    let project = project();
    for warning in project.warnings {
        eprintln!("warning: {warning}");
    }
    project.key
}

/// Sets `id`'s time in a per-registration timestamp list (`last_used`,
/// `desktop_launched`), adding an entry when absent.
pub(crate) fn stamp(list: &mut Vec<Timestamp>, id: &str, at: u64) {
    // Preserve launch order when multiple launches share the same Unix second.
    list.retain(|time| time.registration_id != id);
    list.push(Timestamp {
        registration_id: id.to_owned(),
        at,
    });
}

pub(crate) fn open_launch(root: &Path) -> Result<Store> {
    Store::open(root, false, OpenMode::Launch).map_err(|e| {
        if e.code == "not_found" && e.next_step.is_none() {
            e.next("Create a profile with roost add NAME (Claude) or roost codex add NAME (Codex)")
        } else {
            e
        }
    })
}

/// A launchable Claude registration or marker-verified Codex home.
pub(crate) enum Profile {
    Claude(Registration),
    Codex(CodexHome),
}

impl Profile {
    pub(crate) fn named(store: &Store, selector: &str) -> Result<Self> {
        if let Some(name) = selector.strip_prefix("claude:") {
            return Ok(Self::Claude(store.find(name, false)?.clone()));
        }
        if let Some(name) = selector.strip_prefix("codex:") {
            crate::store::validate_name(name)?;
            return Self::codex(store, name);
        }
        match store.find(selector, false) {
            Ok(r) => Ok(Self::Claude(r.clone())),
            Err(e) if e.code == "not_found" => Self::codex(store, selector).map_err(|codex_err| {
                if codex_err.code == "not_found" {
                    e
                } else {
                    codex_err
                }
            }),
            Err(e) => Err(e),
        }
    }

    fn codex(store: &Store, name: &str) -> Result<Self> {
        let home = store.codex_profile(&name.to_ascii_lowercase())?;
        Ok(Self::Codex(home.bind(store)?))
    }

    fn id(&self) -> &str {
        match self {
            Self::Claude(r) => &r.registration_id,
            Self::Codex(home) => &home.id,
        }
    }

    fn label(&self) -> String {
        match self {
            Self::Claude(r) => r.name.clone(),
            Self::Codex(home) => format!("codex:{}", home.name),
        }
    }

    fn validate(&self, store: &Store) -> Result<()> {
        match self {
            Self::Claude(r) => store.validate(r),
            Self::Codex(home) => {
                store.side_ready()?;
                if store.codex_profile(&home.name)?.id != home.id {
                    return Err(Error::new("ownership", "Codex home identity changed"));
                }
                Ok(())
            }
        }
    }

    pub(crate) fn launch(
        self,
        store: Store,
        allow_auth_env: bool,
        arguments: &[OsString],
    ) -> Result<Outcome> {
        match self {
            Self::Claude(r) => start(store, &r, allow_auth_env, arguments),
            Self::Codex(ref home) => {
                self.validate(&store)?;
                let command = launch::codex_command(&home.path, allow_auth_env)?;
                record_use(&store, &home.id, &home.name);
                drop(store);
                Ok(Outcome {
                    exit: launch::exec(command, arguments, "Codex")?,
                    ..Outcome::quiet()
                })
            }
        }
    }
}

fn record_use(store: &Store, id: &str, name: &str) {
    let at = platform::unix_seconds();
    let result = store.update_state(|state| {
        stamp(&mut state.last_used, id, at);
        Ok(())
    });
    if let Err(e) = result {
        eprintln!(
            "warning: Could not record the last use of {name}: {}",
            e.message
        );
    }
}

fn record_selection(store: &Store, project: &Path, profile: &Profile) -> Result<()> {
    store.update_state(|state| {
        state.selections.retain(|s| s.project != project);
        state.selections.push(Selection {
            project: project.to_owned(),
            registration_id: profile.id().to_owned(),
        });
        Ok(())
    })
}

/// The launch sequence shared by `run`, profile launchers, `switch` and bare
/// `roost`: under the held root lock, prepare the profile environment, claim store
/// plugin work, record last use and reconcile links; then release the lock, finish
/// the plugin work (auto-update, injection) and hand off to Claude.
pub(crate) fn start(
    store: Store,
    registration: &Registration,
    allow_auth_env: bool,
    arguments: &[OsString],
) -> Result<Outcome> {
    let mut env = launch::profile_env(&store, registration, allow_auth_env)?;
    let program = launch::resolve_program("claude")?;
    let plugins = crate::plugins::claim(&store, registration, &env);
    if registration.kind != Kind::DefaultAlias {
        record_use(&store, &registration.registration_id, &registration.name);
    }
    sets::reconcile_at_launch(&store, registration);
    drop(store);
    plugins.finish(&mut env);
    Ok(Outcome {
        exit: launch::execute(env.command(program), arguments)?,
        ..Outcome::quiet()
    })
}

/// Bare `roost`: launches this project's selection, or asks with the picker.
pub fn launch_selected(
    allow_auth_env: bool,
    arguments: &[OsString],
    claude_shortcut: bool,
) -> Result<Outcome> {
    let key = selection_key()?;
    let root = platform::root()?;
    let store = open_launch(&root)?;
    let selected = match store.read_state_unfiltered() {
        Ok(state) => state
            .selections
            .into_iter()
            .find(|s| s.project == key)
            .map(|s| s.registration_id),
        Err(e) => {
            eprintln!("warning: Selection state unavailable: {}", e.message);
            None
        }
    };
    let current = match selected.as_deref() {
        Some(id) => remembered(&store, id)?,
        None => None,
    };
    let (store, profile) = match current {
        Some(profile) => (store, profile),
        None => {
            let (store, profile, recorded) = pick(store, &root, &key, claude_shortcut)?;
            warn_unrecorded(&profile, recorded);
            (store, profile)
        }
    };
    if claude_shortcut && matches!(profile, Profile::Codex(_)) {
        return Err(Error::new(
            "usage",
            format!(
                "{} is selected, but this shortcut requires a Claude profile",
                profile.label()
            ),
        )
        .next(
            "Use roost run claude:NAME for Claude, or roost -- ARGS to pass arguments to Codex",
        ));
    }
    if matches!(profile, Profile::Claude(_))
        && arguments
            .first()
            .is_some_and(|a| cli::CLAUDE_SUBCOMMANDS.iter().any(|c| a == *c))
    {
        eprintln!("roost: using profile {}", profile.label());
    }
    profile.launch(store, allow_auth_env, arguments)
}

fn remembered(store: &Store, id: &str) -> Result<Option<Profile>> {
    if let Some(name) = codex_selection_name(id) {
        match store.codex_profile(name) {
            Ok(home) if home.id == id => return Ok(Some(Profile::Codex(home))),
            Ok(_) => (),
            Err(e) if e.code == "not_found" => (),
            Err(e) => return Err(e),
        }
        eprintln!(
            "warning: The Codex home selected for this project ({name}) has changed or no longer exists; choose another"
        );
    } else if let Some(r) = store
        .registry
        .registrations
        .iter()
        .find(|r| r.registration_id == id)
    {
        if r.state == State::Active {
            return Ok(Some(Profile::Claude(r.clone())));
        }
        eprintln!(
            "warning: The profile selected for this project ({}) is no longer active; choose another",
            r.name
        );
    } else {
        eprintln!(
            "warning: The profile selected for this project no longer exists; choose another"
        );
    }
    Ok(None)
}

fn warn_unrecorded(profile: &Profile, recorded: Result<()>) {
    if let Err(e) = recorded {
        eprintln!(
            "warning: Could not remember {} for this project: {}",
            profile.label(),
            e.message
        );
    }
}

pub(crate) fn terminal() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

/// Shows the picker with the lock released, highlighting this project's valid
/// selection, else the most recently launched profile, else the first row. Then
/// reacquires, revalidates the choice and records the selection, returning the
/// recording result for the caller to warn about or fail on.
fn pick(
    store: Store,
    root: &Path,
    key: &Path,
    claude_only: bool,
) -> Result<(Store, Profile, Result<()>)> {
    let no_terminal = || {
        Error::new(
            "usage",
            "No profile is selected for this project and no terminal is available to choose one",
        )
        .next("roost switch NAME selects one for this project; roost run NAME launches once")
    };
    let title = "Choose a profile for this project";
    let render = |store: &Store, records: &mut [Value]| {
        let _ = annotate(store, records, Some(key));
        let lines = table::profiles(
            records,
            table::Columns::Basic,
            table::color_for(&std::io::stderr()),
            platform::picker_width(),
        );
        let initial = records
            .iter()
            .position(|r| r["selected"] == true)
            .or_else(|| records.iter().position(|r| r["most_recent"] == true))
            .unwrap_or(0);
        (lines, initial)
    };
    let Choice::Chosen(store, chosen) = choose_with(
        store,
        root,
        title,
        Picker::Project { claude_only },
        no_terminal,
        render,
    )?
    else {
        unreachable!("no action keys")
    };
    let store = *store;
    let recorded = record_selection(&store, key, &chosen);
    Ok((store, chosen, recorded))
}

/// Extra picker keys for `choose_with`: the hint after the title, the action keys,
/// and a starting row that overrides `render`'s (to stay on the acted-on row).
pub(crate) struct Actions<'a> {
    pub hint: &'a str,
    pub keys: &'a [char],
    pub initial: Option<usize>,
}

/// A `choose_with` result: the revalidated choice with the store reacquired, or an
/// action key pressed on a row (the registration as listed, the row index), with
/// the store released for the caller to act and show the picker again.
pub(crate) enum Choice {
    Chosen(Box<Store>, Profile),
    Action(Registration, usize),
}

pub(crate) enum Picker<'a> {
    Project { claude_only: bool },
    Desktop(Actions<'a>),
}

fn codex_record(home: &CodexHome) -> Value {
    json!({
        "name": home.name, "provider": "codex", "selection_id": home.id,
        "kind": "codex", "state": "active", "directory": home.path,
        "token_present": null, "launchers": [], "sets": [], "last_used": null,
        "selected": false, "most_recent": false, "probe": null
    })
}

pub(crate) fn append_codex_profiles(
    store: &Store,
    records: &mut Vec<Value>,
    warnings: &mut Vec<String>,
) -> Result<Vec<CodexHome>> {
    let homes = store.codex_profiles(warnings)?;
    if !homes.is_empty() {
        records.extend(homes.iter().map(codex_record));
        records.sort_by_cached_key(|r| {
            let name = r["name"].as_str().unwrap_or_default();
            (
                name.to_ascii_lowercase(),
                name.to_owned(),
                r["provider"] == "codex",
            )
        });
    }
    Ok(homes)
}

/// The shared picker flow: with an active registration and a terminal (else
/// `no_terminal`), lists active profiles as `ls` records (with sets), lets `render`
/// annotate them and return the table lines and initial row, shows the picker with
/// the lock released (with optional action keys), then reacquires and revalidates
/// the root and the choice.
pub(crate) fn choose_with(
    store: Store,
    root: &Path,
    title: &str,
    picker: Picker,
    no_terminal: impl FnOnce() -> Error,
    render: impl FnOnce(&Store, &mut [Value]) -> (Vec<String>, usize),
) -> Result<Choice> {
    let mut records = store.profiles(false)?;
    let _ = sets::annotate(&store, &mut records);
    let homes = if matches!(picker, Picker::Project { claude_only: false }) {
        let mut warnings = Vec::new();
        let homes = append_codex_profiles(&store, &mut records, &mut warnings)?;
        for warning in warnings {
            eprintln!("warning: {warning}");
        }
        homes
    } else {
        vec![]
    };
    if records.is_empty() {
        let hint = if matches!(picker, Picker::Project { claude_only: false }) {
            "Create a profile with roost add NAME (Claude) or roost codex add NAME (Codex)"
        } else {
            "Create a Claude profile with roost add NAME"
        };
        return Err(Error::new("not_found", "No active profiles to choose from").next(hint));
    }
    if !terminal() {
        return Err(no_terminal());
    }
    let (lines, initial) = render(&store, &mut records);
    let root_id = store.root_id.clone();
    let registrations = store.registry.registrations.clone();
    drop(store);
    let listed_claude = |index: usize| {
        registrations
            .iter()
            .find(|r| records[index]["name"] == r.name.as_str())
            .cloned()
            .ok_or_else(|| Error::new("ownership", "Listed Claude registration is missing"))
    };
    let rows = &lines[1..=records.len()];
    let (hint, keys, initial) = match picker {
        Picker::Project { .. } => ("↑/↓, Enter; Esc cancels", &[][..], initial),
        Picker::Desktop(a) => (a.hint, a.keys, a.initial.unwrap_or(initial)),
    };
    let index = match platform::pick_with(title, hint, &lines[0], rows, initial, keys)? {
        platform::Picked::Chosen(index) => index,
        platform::Picked::Action(index) => {
            return Ok(Choice::Action(listed_claude(index)?, index));
        }
    };
    let store = open_launch(root)?;
    if store.root_id != root_id {
        return Err(
            Error::new("ownership", "Profile changed while the picker was open")
                .next("Repeat the command to choose from the current profiles"),
        );
    }
    let record = &records[index];
    let chosen = if record["provider"] == "codex" {
        let name = record["name"]
            .as_str()
            .ok_or_else(|| Error::new("ownership", "Listed Codex name is missing"))?;
        let listed = homes
            .iter()
            .find(|home| home.name == name)
            .ok_or_else(|| Error::new("ownership", "Listed Codex home is missing"))?;
        let current = store.codex_profile(name)?;
        if let Some(directory) = &listed.unbound {
            // The handle prevents inode reuse and permits another picker to bind
            // the same verified home while this picker is waiting.
            directory.verify_namespace()?;
        } else if current.id != listed.id {
            return Err(Error::new(
                "ownership",
                "Codex home changed while the picker was open",
            ));
        }
        Profile::Codex(current.bind(&store)?)
    } else {
        let r = listed_claude(index)?;
        store.validate(&r)?;
        Profile::Claude(r)
    };
    Ok(Choice::Chosen(Box::new(store), chosen))
}

/// `roost switch`: select NAME (or, without NAME, the picker's choice) for this
/// project and launch it, only select it (`no_launch`), or clear this project's
/// selection (`forget`).
pub fn switch(
    name: Option<&str>,
    no_launch: bool,
    forget: bool,
    allow_auth_env: bool,
    arguments: &[OsString],
) -> Result<Outcome> {
    if name.is_none() && !forget && !terminal() {
        return Err(Error::new(
            "usage",
            "roost switch without NAME needs a terminal to choose a profile",
        )
        .next("roost switch NAME selects one for this project; roost run NAME launches once"));
    }
    let key = selection_key()?;
    let root = platform::root()?;
    let store = open_launch(&root)?;
    if forget {
        let selected = store
            .read_state_unfiltered()?
            .selections
            .iter()
            .any(|s| s.project == key);
        if selected {
            store.update_state(|state| {
                state.selections.retain(|s| s.project != key);
                Ok(())
            })?;
        }
        return Ok(Outcome::lines(vec![format!(
            "{} {}",
            if selected {
                "Forgot the selection for"
            } else {
                "No selection to forget for"
            },
            key.display()
        )]));
    }
    let (store, profile, recorded) = match name {
        Some(name) => {
            let profile = Profile::named(&store, name)?;
            profile.validate(&store)?;
            let recorded = record_selection(&store, &key, &profile);
            (store, profile, recorded)
        }
        None => pick(store, &root, &key, false)?,
    };
    if no_launch {
        recorded?;
        return Ok(Outcome::lines(vec![format!(
            "Selected {} for {}",
            profile.label(),
            key.display()
        )]));
    }
    warn_unrecorded(&profile, recorded);
    profile.launch(store, allow_auth_env, arguments)
}

/// Fills each ProfileRecord's `last_used`, `selected` (valid selection for
/// `project`) and `most_recent`. Returns warnings when `state.json` is unreadable.
pub fn annotate(store: &Store, profiles: &mut [Value], project: Option<&Path>) -> Vec<String> {
    let state = match store.read_state() {
        Ok(state) => state,
        Err(e) => return vec![format!("Selection state unavailable: {}", e.message)],
    };
    let selected = project.and_then(|p| {
        state
            .selections
            .iter()
            .find(|s| s.project == p)
            .map(|s| s.registration_id.as_str())
    });
    let mut latest = None;
    for (index, profile) in profiles.iter_mut().enumerate() {
        let id = if profile["provider"] == "codex" {
            profile["selection_id"].as_str()
        } else {
            store
                .registry
                .registrations
                .iter()
                .find(|r| profile["name"] == r.name.as_str())
                .map(|r| r.registration_id.as_str())
        };
        let Some(id) = id else { continue };
        let timestamp = state
            .last_used
            .iter()
            .enumerate()
            .find(|(_, time)| time.registration_id == id)
            .map(|(order, time)| (time.at, order));
        let selected_here = selected == Some(id) && profile["state"] == "active";
        profile["last_used"] = json!(timestamp.map(|(at, _)| at));
        profile["selected"] = json!(selected_here);
        profile["most_recent"] = json!(false);
        if let Some(time) = timestamp
            && latest.is_none_or(|(_, previous)| time > previous)
        {
            latest = Some((index, time));
        }
    }
    if let Some((index, _)) = latest {
        profiles[index]["most_recent"] = json!(true);
    }
    vec![]
}

/// List's `project` value and fallback warnings; null when the key is unsafe.
pub fn list_project() -> (Option<PathBuf>, Vec<String>) {
    let project = project();
    (project.key.ok(), project.warnings)
}

/// After `remove`/`purge` commits: clears every selection naming `registration`,
/// so a later profile with the same name inherits nothing. Returns warnings.
pub fn forget_removed(store: &Store, registration: &Registration) -> Vec<String> {
    let id = &registration.registration_id;
    match forget_in_state(store, |s| &mut s.selections, |s| &s.registration_id == id) {
        Ok(()) => vec![],
        Err(e) => vec![format!(
            "Selections naming {} were not cleared: {}; run roost switch --forget in affected projects",
            registration.name, e.message
        )],
    }
}

/// Drops the entries `names` picks from one `state.json` list, reading first so
/// an unaffected file is not rewritten. Shared by remove's and purge's cleanups.
pub(crate) fn forget_in_state<T>(
    store: &Store,
    list: impl Fn(&mut StateFile) -> &mut Vec<T>,
    names: impl Fn(&T) -> bool,
) -> Result<()> {
    if !list(&mut store.read_state_unfiltered()?).iter().any(&names) {
        return Ok(());
    }
    store.update_state(|state| {
        list(state).retain(|entry| !names(entry));
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "roost-select-test-{}",
                platform::random_id().unwrap()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn dir(&self, relative: &str) -> PathBuf {
            let path = self.0.join(relative);
            fs::create_dir_all(&path).unwrap();
            path
        }
        fn file(&self, relative: &str, text: &str) {
            fs::write(self.0.join(relative), text).unwrap();
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn key(cwd: &Path) -> (PathBuf, usize) {
        let project = project_key(cwd);
        (project.key.unwrap(), project.warnings.len())
    }

    #[test]
    fn repository_and_its_worktrees_share_the_common_directory_key() {
        let t = Temp::new();
        let nested = t.dir("repo/src/deep");
        t.dir("repo/.git/worktrees/wt");
        t.file("repo/.git/worktrees/wt/commondir", "../..\n");
        let worktree = t.dir("wt/sub");
        t.file("wt/.git", "gitdir: ../repo/.git/worktrees/wt\n");
        let common = t.0.join("repo/.git");
        assert_eq!(key(&nested), (common.clone(), 0));
        assert_eq!(key(&worktree), (common.clone(), 0));
        // An absolute commondir and gitdir are honored as written.
        t.dir("abs/.git/worktrees/x");
        let other = t.dir("other");
        t.file(
            "other/.git",
            &format!("gitdir: {}", t.0.join("abs/.git/worktrees/x").display()),
        );
        t.file(
            "abs/.git/worktrees/x/commondir",
            &t.0.join("abs/.git").display().to_string(),
        );
        assert_eq!(key(&other), (t.0.join("abs/.git"), 0));
    }

    #[test]
    fn nested_repository_wins_and_non_git_uses_the_directory() {
        let t = Temp::new();
        t.dir("outer/.git");
        let inner = t.dir("outer/vendor/inner/src");
        t.dir("outer/vendor/inner/.git");
        assert_eq!(key(&inner), (t.0.join("outer/vendor/inner/.git"), 0));
        let plain = t.dir("plain/dir");
        // The temp directory itself is not inside a repository in test runs.
        let project = project_key(&plain);
        if project.key.as_ref().unwrap().ends_with(".git") {
            return;
        }
        assert_eq!(project.key.unwrap(), plain);
    }

    #[test]
    fn malformed_git_metadata_falls_back_to_the_directory_with_a_warning() {
        for (dot_git, commondir) in [
            ("not a gitdir line\n", None),
            ("gitdir: \n", None),
            ("gitdir: missing/dir\n", None),
            ("gitdir: gd\n", Some("")),
            ("gitdir: gd\n", Some("a\nb\n")),
            ("gitdir: gd\n", Some("nowhere\n")),
        ] {
            let t = Temp::new();
            let cwd = t.dir("work");
            t.dir("work/gd");
            t.file("work/.git", dot_git);
            if let Some(text) = commondir {
                t.file("work/gd/commondir", text);
            }
            assert_eq!(key(&cwd), (cwd.clone(), 1), "{dot_git:?} {commondir:?}");
        }
    }

    #[test]
    fn line_breaks_in_the_key_are_unsafe() {
        let t = Temp::new();
        let cwd = t.dir("bad\nname");
        let project = project_key(&cwd);
        if project.key.as_ref().is_ok_and(|k| k.ends_with(".git")) {
            return;
        }
        assert_eq!(project.key.unwrap_err().code, "unsafe_path");
    }
}
