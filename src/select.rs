//! Selected profile per project, `roost switch`, bare `roost` and last use.
//!
//! Selections and last-use times live in the manager's `state.json`, keyed by
//! registration ID; nothing is ever written into upstream or default data.
use crate::{
    Error, Outcome, Result, launch, platform, sets,
    store::{
        Kind, OpenMode, Registration, State, Store,
        side::{Selection, Timestamp},
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
    match list.iter_mut().find(|t| t.registration_id == id) {
        Some(time) => time.at = at,
        None => list.push(Timestamp {
            registration_id: id.to_owned(),
            at,
        }),
    }
}

pub(crate) fn open_launch(root: &Path) -> Result<Store> {
    Store::open(root, false, OpenMode::Launch).map_err(|e| {
        if e.code == "not_found" && e.next_step.is_none() {
            e.next("Create a profile with roost add NAME")
        } else {
            e
        }
    })
}

/// Records a launch of `registration` as its last use. Default aliases record
/// nothing. Failure warns on stderr; the launch continues.
pub fn launched(store: &Store, registration: &Registration) {
    if registration.kind == Kind::DefaultAlias {
        return;
    }
    let at = platform::unix_seconds();
    let result = store.update_state(|state| {
        stamp(&mut state.last_used, &registration.registration_id, at);
        Ok(())
    });
    if let Err(e) = result {
        eprintln!(
            "warning: Could not record the last use of {}: {}",
            registration.name, e.message
        );
    }
}

fn record_selection(store: &Store, project: &Path, registration: &Registration) -> Result<()> {
    store.update_state(|state| {
        state.selections.retain(|s| s.project != project);
        state.selections.push(Selection {
            project: project.to_owned(),
            registration_id: registration.registration_id.clone(),
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
    launched(&store, registration);
    sets::reconcile_at_launch(&store, registration);
    drop(store);
    plugins.finish(&mut env);
    Ok(Outcome {
        exit: launch::execute(env.command(program), arguments)?,
        ..Outcome::quiet()
    })
}

/// Bare `roost`: launches this project's selection, or asks with the picker.
pub fn launch_selected(allow_auth_env: bool, arguments: &[OsString]) -> Result<Outcome> {
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
    let current = selected.as_ref().map(|id| {
        store
            .registry
            .registrations
            .iter()
            .find(|r| &r.registration_id == id)
    });
    let (store, registration) = match current {
        Some(Some(r)) if r.state == State::Active => {
            let r = r.clone();
            (store, r)
        }
        stale => {
            match stale {
                Some(Some(r)) => eprintln!(
                    "warning: The profile selected for this project ({}) is no longer active; choose another",
                    r.name
                ),
                Some(None) => eprintln!(
                    "warning: The profile selected for this project no longer exists; choose another"
                ),
                None => (),
            }
            let (store, registration, recorded) = pick(store, &root, &key)?;
            warn_unrecorded(&registration, recorded);
            (store, registration)
        }
    };
    start(store, &registration, allow_auth_env, arguments)
}

fn warn_unrecorded(registration: &Registration, recorded: Result<()>) {
    if let Err(e) = recorded {
        eprintln!(
            "warning: Could not remember {} for this project: {}",
            registration.name, e.message
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
fn pick(store: Store, root: &Path, key: &Path) -> Result<(Store, Registration, Result<()>)> {
    let no_terminal = || {
        Error::new(
            "usage",
            "No profile is selected for this project and no terminal is available to choose one",
        )
        .next("roost switch NAME selects one for this project; roost run NAME launches once")
    };
    let title = "Choose a profile for this project";
    let (store, chosen) = choose(store, root, title, no_terminal, |store, records| {
        let _ = annotate(store, records, Some(key));
        let lines = table::profiles(records, false, table::stderr_color(), None);
        let initial = records
            .iter()
            .position(|r| r["selected"] == true)
            .or_else(|| records.iter().position(|r| r["most_recent"] == true))
            .unwrap_or(0);
        (lines, initial)
    })?;
    let recorded = record_selection(&store, key, &chosen);
    Ok((store, chosen, recorded))
}

/// The shared picker flow: with an active registration and a terminal (else
/// `no_terminal`), lists active profiles as `ls` records (with sets), lets `render`
/// annotate them and return the table lines and initial row, shows the picker with
/// the lock released, then reacquires and revalidates the root and the choice.
pub(crate) fn choose(
    store: Store,
    root: &Path,
    title: &str,
    no_terminal: impl FnOnce() -> Error,
    render: impl FnOnce(&Store, &mut [Value]) -> (Vec<String>, usize),
) -> Result<(Store, Registration)> {
    match choose_with(store, root, title, None, no_terminal, render)? {
        Choice::Chosen(store, registration) => Ok((*store, registration)),
        Choice::Action(..) => unreachable!("no action keys"),
    }
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
    Chosen(Box<Store>, Registration),
    Action(Registration, usize),
}

/// `choose` with optional action keys.
pub(crate) fn choose_with(
    store: Store,
    root: &Path,
    title: &str,
    actions: Option<Actions>,
    no_terminal: impl FnOnce() -> Error,
    render: impl FnOnce(&Store, &mut [Value]) -> (Vec<String>, usize),
) -> Result<Choice> {
    let active = store
        .registry
        .registrations
        .iter()
        .any(|r| r.state == State::Active);
    if !active {
        return Err(Error::new("not_found", "No active profiles to choose from")
            .next("Create a profile with roost add NAME"));
    }
    if !terminal() {
        return Err(no_terminal());
    }
    let mut records = store.profiles(false)?;
    let _ = sets::annotate(&store, &mut records);
    let choices: Vec<Registration> = records
        .iter()
        .map(|record| {
            store
                .registry
                .registrations
                .iter()
                .find(|r| r.state == State::Active && record["name"] == r.name.as_str())
                .cloned()
                .ok_or_else(|| Error::new("ownership", "Profile list changed while reading"))
        })
        .collect::<Result<_>>()?;
    let (lines, initial) = render(&store, &mut records);
    let root_id = store.root_id.clone();
    drop(store);
    let rows = &lines[1..=records.len()];
    let index = match actions {
        None => platform::pick(title, &lines[0], rows, initial)?,
        Some(actions) => {
            let initial = actions.initial.unwrap_or(initial);
            match platform::pick_with(title, actions.hint, &lines[0], rows, initial, actions.keys)?
            {
                platform::Picked::Chosen(index) => index,
                platform::Picked::Action(_, index) => {
                    return Ok(Choice::Action(choices[index].clone(), index));
                }
            }
        }
    };
    let chosen = &choices[index];
    let store = open_launch(root)?;
    let current = store
        .registry
        .registrations
        .iter()
        .find(|r| r.registration_id == chosen.registration_id);
    if store.root_id != root_id || current != Some(chosen) {
        return Err(
            Error::new("ownership", "Profile changed while the picker was open")
                .next("Repeat the command to choose from the current profiles"),
        );
    }
    Ok(Choice::Chosen(Box::new(store), chosen.clone()))
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
    let (store, registration, recorded) = match name {
        Some(name) => {
            let registration = store.find(name, false)?.clone();
            store.validate(&registration)?;
            let recorded = record_selection(&store, &key, &registration);
            (store, registration, recorded)
        }
        None => pick(store, &root, &key)?,
    };
    if no_launch {
        recorded?;
        return Ok(Outcome::lines(vec![format!(
            "Selected {} for {}",
            registration.name,
            key.display()
        )]));
    }
    warn_unrecorded(&registration, recorded);
    start(store, &registration, allow_auth_env, arguments)
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
    let mut most_recent: Option<&Timestamp> = None;
    for time in &state.last_used {
        if most_recent.is_none_or(|m| time.at > m.at) {
            most_recent = Some(time);
        }
    }
    for profile in profiles.iter_mut() {
        let Some(registration) = store
            .registry
            .registrations
            .iter()
            .find(|r| profile["name"] == r.name.as_str())
        else {
            continue;
        };
        let id = registration.registration_id.as_str();
        let last_used = state
            .last_used
            .iter()
            .find(|t| t.registration_id == id)
            .map(|t| t.at);
        profile["last_used"] = json!(last_used);
        profile["selected"] = json!(selected == Some(id) && registration.state == State::Active);
        profile["most_recent"] = json!(most_recent.is_some_and(|t| t.registration_id == id));
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
    let named = store
        .read_state_unfiltered()
        .map(|state| state.selections.iter().any(|s| &s.registration_id == id));
    let result = match named {
        Ok(false) => Ok(()),
        Ok(true) => store.update_state(|state| {
            state.selections.retain(|s| &s.registration_id != id);
            Ok(())
        }),
        Err(e) => Err(e),
    };
    match result {
        Ok(()) => vec![],
        Err(e) => vec![format!(
            "Selections naming {} were not cleared: {}; run roost switch --forget in affected projects",
            registration.name, e.message
        )],
    }
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
