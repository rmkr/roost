//! `roost desktop NAME` (experimental, Linux only): Claude Desktop per profile through
//! Electron's undocumented `--user-data-dir` ([ADR 0002]). Owned and upstream
//! registrations get a Roost-owned data folder (their own Desktop sign-in) plus the
//! profile's isolated environment; a default alias starts plain `claude-desktop`.
//! One owned or upstream registration may instead borrow the conventional Desktop
//! folder in place (`--link`); Roost never copies, moves, repairs or deletes it.
//!
//! [ADR 0002]: ../docs/adr/0002-desktop-per-profile-via-user-data-dir.md
use crate::{
    Error, Result, launch, platform,
    store::{
        Kind, OpenMode, Registration, Store,
        side::{DesktopLink, StateFile},
    },
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

const COWORK: &str = "Another Claude Desktop is already running; only one running Desktop can use Cowork (the first to use it), and the other gets it back only after both are closed";

fn supported() -> Result<()> {
    if cfg!(target_os = "linux") {
        return Ok(());
    }
    Err(Error::new(
        "claude_unavailable",
        "roost desktop is experimental and supported only on Linux",
    )
    .next("Start Claude Desktop directly on this platform"))
}

/// Prepares and starts Claude Desktop for `name`, or without it for the picker's
/// choice; returns the manager exit code.
pub fn run(name: Option<&str>, foreground: bool) -> Result<i32> {
    supported()?;
    let no_terminal = || {
        Error::new(
            "usage",
            "roost desktop without NAME needs a terminal to choose a profile",
        )
        .next("roost desktop NAME starts Claude Desktop for one profile")
    };
    if name.is_none() && !crate::select::terminal() {
        return Err(no_terminal());
    }
    let root = platform::root()?;
    let (store, registration) = match name {
        Some(name) => {
            let store = Store::open(&root, false, OpenMode::Launch)?;
            let registration = store.find(name, false)?.clone();
            (store, registration)
        }
        None => picker(&root, no_terminal)?,
    };
    start(store, registration, foreground)
}

/// The Desktop picker: Enter returns the choice; `x` closes the highlighted row's
/// running Desktop, reports the outcome above the list and shows it again (with
/// fresh Desktop states) on the same row.
fn picker(root: &Path, no_terminal: impl Fn() -> Error) -> Result<(Store, Registration)> {
    use crate::select::{Actions, Choice};
    let mut initial = None;
    loop {
        let store = crate::select::open_launch(root)?;
        let actions = Actions {
            hint: "↑/↓, Enter launch, x close, Esc cancel",
            keys: &['x'],
            initial,
        };
        match crate::select::choose_with(
            store,
            root,
            "Choose a profile for Claude Desktop",
            Some(actions),
            &no_terminal,
            picker_rows,
        )? {
            Choice::Chosen(store, registration) => return Ok((*store, registration)),
            Choice::Action(listed, index) => {
                initial = Some(index);
                let store = Store::open(root, false, OpenMode::Read)?;
                let current = store
                    .registry
                    .registrations
                    .iter()
                    .find(|r| r.registration_id == listed.registration_id)
                    .cloned();
                let outcome = match current {
                    Some(current) if current == listed => close_registration(store, &current),
                    _ => Err(Error::new(
                        "ownership",
                        "Profile changed while the picker was open",
                    )),
                };
                match outcome {
                    Ok(line) => eprintln!("{line}"),
                    Err(e) => {
                        eprintln!("{}: {}", e.code, e.message);
                        if let Some(step) = &e.next_step {
                            eprintln!("Next: {step}");
                        }
                    }
                }
            }
        }
    }
}

/// The registration (if any) borrowing the conventional Desktop folder.
fn link_holder(store: &Store) -> Result<Option<Registration>> {
    let state = store.read_state()?;
    Ok(state.desktop_links.first().and_then(|link| {
        store
            .registry
            .registrations
            .iter()
            .find(|r| r.registration_id == link.registration_id)
            .cloned()
    }))
}

/// Whether the conventional Desktop folder holds a live `SingletonLock`.
fn conventional_running(host: Option<&str>) -> bool {
    conventional_lock().is_some_and(|target| linux::live_lock(&target, host))
}

/// Annotates the picker records with their Desktop state and renders them; the
/// initial row is the most recent Desktop launch, else the most recent launch.
fn picker_rows(store: &Store, records: &mut [Value]) -> (Vec<String>, usize) {
    let _ = crate::select::annotate(store, records, None);
    let host = linux::hostname();
    let locks = store.desktop_locks().unwrap_or_default();
    let holder = link_holder(store).ok().flatten();
    let conventional = || {
        if conventional_running(host.as_deref()) {
            "running"
        } else if conventional_folder().is_some_and(|folder| folder.is_dir()) {
            "signed_in"
        } else {
            "never"
        }
    };
    let launched = store
        .read_state()
        .map(|state| state.desktop_launched)
        .unwrap_or_default();
    let latest = launched.iter().max_by_key(|t| t.at);
    let mut initial = None;
    for (index, record) in records.iter_mut().enumerate() {
        let Some(registration) = store
            .registry
            .registrations
            .iter()
            .find(|r| record["name"] == r.name.as_str())
        else {
            continue;
        };
        let id = registration.registration_id.as_str();
        if latest.is_some_and(|t| t.registration_id == id) {
            initial = Some(index);
        }
        let folder = locks.iter().find(|(lock_id, _)| lock_id == id);
        record["desktop"] = json!(match (&holder, folder) {
            (Some(holder), _) if holder.registration_id == id => conventional(),
            (Some(holder), _) if registration.kind == Kind::DefaultAlias => {
                record["desktop_shared_with"] = json!(holder.name);
                match conventional() {
                    "running" => "running",
                    _ => "shared",
                }
            }
            _ if registration.kind == Kind::DefaultAlias => match conventional() {
                "running" => "running",
                _ => "plain",
            },
            (_, Some((_, Some(target)))) if linux::live_lock(target, host.as_deref()) => {
                "running"
            }
            (_, Some(_)) => "signed_in",
            (_, None) => "never",
        });
    }
    let initial = initial
        .or_else(|| records.iter().position(|r| r["most_recent"] == true))
        .unwrap_or(0);
    (
        crate::table::profiles(
            records,
            crate::table::Columns::Desktop,
            crate::table::color_for(&std::io::stderr()),
            platform::picker_width(),
        ),
        initial,
    )
}

fn already_running(message: String) -> Error {
    Error::new("desktop_running", message)
        .next("Switch to the open Claude Desktop window, or quit it before relaunching")
}

fn start(mut store: Store, registration: Registration, foreground: bool) -> Result<i32> {
    let mut env = launch::profile_env(&store, &registration, false)?;
    let program = launch::resolve_program("claude-desktop")?;
    let host = linux::hostname();
    let roost_running = |store: &Store, own: Option<&str>| {
        store
            .desktop_locks()
            .unwrap_or_default()
            .into_iter()
            .any(|(id, lock)| {
                Some(id.as_str()) != own
                    && lock.is_some_and(|target| linux::live_lock(&target, host.as_deref()))
            })
    };
    let mut warnings = vec![];
    let command = if registration.kind == Kind::DefaultAlias {
        // Plain Desktop with the caller's environment: no folder, nothing recorded.
        // A second start only focuses the running instance of that folder and exits,
        // so refuse; a profile borrowing the folder shares that instance.
        if conventional_running(host.as_deref()) {
            return Err(already_running(match link_holder(&store) {
                Ok(Some(holder)) => format!(
                    "Claude Desktop shared with {} is already running",
                    holder.name
                ),
                _ => "Claude Desktop is already running".to_owned(),
            }));
        }
        if roost_running(&store, None) {
            warnings.push(COWORK.to_owned());
        }
        drop(store);
        env.command(program)
    } else {
        let id = registration.registration_id.as_str();
        // The link decides which data folder Desktop uses, so unreadable state fails
        // the launch instead of silently starting a different sign-in.
        let linked = link_holder(&store)
            .map_err(|e| {
                Error::new(
                    e.code,
                    format!(
                        "Cannot tell which Desktop data folder {} uses: {}",
                        registration.name, e.message
                    ),
                )
                .next("Inspect roost doctor, then repeat the command")
            })?
            .is_some_and(|holder| holder.registration_id == id);
        let folder = if linked {
            if conventional_running(host.as_deref()) {
                return Err(already_running(format!(
                    "this profile's Desktop is already running ({}, sharing the existing Claude Desktop data folder)",
                    registration.name
                )));
            }
            if roost_running(&store, Some(id)) {
                warnings.push(COWORK.to_owned());
            }
            None
        } else {
            let folder = store.ensure_desktop_folder(&registration)?;
            let own = store
                .desktop_locks()?
                .into_iter()
                .find(|(lock_id, _)| lock_id == id)
                .and_then(|(_, lock)| lock);
            if own.is_some_and(|target| linux::live_lock(&target, host.as_deref())) {
                return Err(already_running(format!(
                    "this profile's Desktop is already running ({})",
                    registration.name
                )));
            }
            if roost_running(&store, Some(id)) || conventional_running(host.as_deref()) {
                warnings.push(COWORK.to_owned());
            }
            Some(folder)
        };
        let plugins = crate::plugins::claim(&store, &registration, &env);
        if let Err(error) = store.update_state(|state| {
            record_launch(state, id);
            Ok(())
        }) {
            warnings.push(format!(
                "Could not record this Desktop launch: {}",
                error.message
            ));
        }
        crate::sets::reconcile_at_launch(&store, &registration);
        drop(store);
        plugins.finish(&mut env);
        let mut command = env.command(program);
        if let Some(folder) = folder {
            let mut argument = std::ffi::OsString::from("--user-data-dir=");
            argument.push(folder.as_os_str());
            command.arg(argument);
        }
        command
    };
    for warning in warnings {
        eprintln!("warning: {warning}");
    }
    if foreground {
        linux::foreground(command)
    } else {
        linux::detached(command, &registration.name)
    }
}

/// Refuses linking aliases and a second holder; `Ok(true)` when `registration`
/// already holds the link.
fn check_linkable(store: &Store, registration: &Registration) -> Result<bool> {
    if registration.kind == Kind::DefaultAlias {
        return Err(Error::new(
            "usage",
            format!(
                "{} is a default alias, which already uses the existing Claude Desktop data folder",
                registration.name
            ),
        )
        .next("Link an owned or upstream profile instead"));
    }
    match link_holder(store)? {
        Some(holder) if holder.registration_id == registration.registration_id => Ok(true),
        Some(holder) => Err(Error::new(
            "collision",
            format!(
                "{} already uses the existing Claude Desktop data folder",
                holder.name
            ),
        )
        .next(format!(
            "Run roost desktop --unlink {} first, then link {}",
            holder.name, registration.name
        ))),
        None => Ok(false),
    }
}

/// `roost desktop --link NAME [--replace [--yes]]`: NAME's Desktop borrows the
/// conventional Desktop folder in place. NAME's own Roost folder is a collision
/// unless `replace`, which deletes it (journaled, after confirmation) first.
pub fn link(name: &str, replace: bool, yes: bool) -> Result<Vec<String>> {
    supported()?;
    let conventional = conventional_folder().ok_or_else(|| {
        Error::new(
            "unsafe_path",
            "Cannot locate the existing Claude Desktop data folder",
        )
        .next("Set HOME or XDG_CONFIG_HOME to an absolute path, then repeat the command")
    })?;
    let root = platform::root()?;
    let open = || Store::open(&root, false, OpenMode::Mutate);
    let mut store = open()?;
    let registration = store.find(name, false)?.clone();
    let linked = format!(
        "{}'s Claude Desktop now uses the existing Claude Desktop data folder at {} in place",
        registration.name,
        conventional.display()
    );
    if check_linkable(&store, &registration)? {
        return Ok(vec![linked]);
    }
    let mut lines = vec![];
    if let Some(own) = store.desktop_folder(&registration)? {
        if !replace {
            return Err(Error::new(
                "collision",
                format!(
                    "{} already has its own Roost Claude Desktop data folder at {}",
                    registration.name,
                    own.display()
                ),
            )
            .next(format!(
                "Run roost desktop --link {} --replace to delete it (its Desktop sign-in) and link",
                registration.name
            )));
        }
        refuse_if_running(&store, &registration)?;
        let root_id = store.root_id.clone();
        drop(store);
        platform::confirm(
            &format!(
                "Delete Roost's Claude Desktop data folder for {} at {} (its own Desktop sign-in), then let it use the existing Claude Desktop data folder at {}. Quit that Claude Desktop first. The existing folder is not touched.",
                registration.name,
                own.display(),
                conventional.display()
            ),
            yes,
        )?;
        store = open()?;
        let current = store.find(name, false)?.clone();
        if store.root_id != root_id
            || current != registration
            || store.desktop_folder(&current)?.as_ref() != Some(&own)
            || check_linkable(&store, &current)?
        {
            return Err(Error::new(
                "ownership",
                "Profile changed while the confirmation was pending",
            )
            .next("Inspect the profile and repeat the command"));
        }
        refuse_if_running(&store, &current)?;
        store.delete_desktop_folder(&current)?;
        lines.push(format!(
            "Deleted Roost's Claude Desktop data folder for {} at {}",
            registration.name,
            own.display()
        ));
    }
    let id = registration.registration_id.clone();
    store.update_state(|state| {
        state.desktop_links = vec![DesktopLink {
            registration_id: id,
        }];
        Ok(())
    })?;
    lines.push(linked);
    Ok(lines)
}

/// `roost desktop --unlink NAME`: NAME's Desktop returns to its own Roost folder.
/// Unlinking a registration that holds no link succeeds without change.
pub fn unlink(name: &str) -> Result<Vec<String>> {
    supported()?;
    let store = Store::open(&platform::root()?, false, OpenMode::Mutate)?;
    let registration = store.find(name, false)?.clone();
    let id = &registration.registration_id;
    if !link_holder(&store)?.is_some_and(|holder| &holder.registration_id == id) {
        return Ok(vec![format!(
            "{} does not use the existing Claude Desktop data folder",
            registration.name
        )]);
    }
    store.update_state(|state| {
        state.desktop_links.retain(|l| &l.registration_id != id);
        Ok(())
    })?;
    Ok(vec![format!(
        "{}'s Claude Desktop now uses its own Roost data folder; the existing Claude Desktop data folder is unchanged",
        registration.name
    )])
}

/// Remove and purge: drop the registration's link (never touching the borrowed
/// folder). Failure warns; readers ignore links of removed registrations anyway.
pub fn forget_removed(store: &Store, registration: &Registration) -> Vec<String> {
    let id = &registration.registration_id;
    let named = |l: &DesktopLink| &l.registration_id == id;
    match crate::select::forget_in_state(store, |s| &mut s.desktop_links, named) {
        Ok(()) => vec![],
        Err(e) => vec![format!(
            "The Claude Desktop link of {} was not cleared: {}; run roost desktop --unlink {} if it is still listed",
            registration.name, e.message, registration.name
        )],
    }
}

/// Q51: purge and upstream remove refuse while the registration's Desktop folder (its
/// own, or the borrowed conventional one) holds a live `SingletonLock` (its Desktop is
/// running and would keep writing there).
pub fn refuse_if_running(store: &Store, registration: &Registration) -> Result<()> {
    let host = linux::hostname();
    let own = store.desktop_locks()?.into_iter().any(|(id, lock)| {
        id == registration.registration_id
            && lock.is_some_and(|target| linux::live_lock(&target, host.as_deref()))
    });
    let borrowed = || {
        link_holder(store)
            .ok()
            .flatten()
            .is_some_and(|holder| holder.registration_id == registration.registration_id)
            && conventional_running(host.as_deref())
    };
    if own || borrowed() {
        return Err(Error::new(
            "desktop_running",
            format!("Claude Desktop is running for {}", registration.name),
        )
        .next(format!(
            "Quit Claude Desktop for {}, then repeat the command",
            registration.name
        )));
    }
    Ok(())
}

/// `roost desktop --close NAME`: asks NAME's running Desktop to quit. Not running is
/// a success that says so.
pub fn close(name: &str) -> Result<Vec<String>> {
    supported()?;
    let store = Store::open(&platform::root()?, false, OpenMode::Read)?;
    let registration = store.find(name, false)?.clone();
    Ok(vec![close_registration(store, &registration)?])
}

/// Closes the Desktop instance holding `registration`'s `SingletonLock`: its own
/// Roost folder, or the conventional folder for a linked registration or a default
/// alias. The lock is released before signalling and waiting.
fn close_registration(store: Store, registration: &Registration) -> Result<String> {
    let host = linux::hostname();
    let borrowed = registration.kind == Kind::DefaultAlias
        || link_holder(&store)?
            .is_some_and(|holder| holder.registration_id == registration.registration_id);
    let lock = if borrowed {
        conventional_lock()
    } else {
        store
            .desktop_locks()?
            .into_iter()
            .find(|(id, _)| *id == registration.registration_id)
            .and_then(|(_, lock)| lock)
    };
    drop(store);
    let not_running = || format!("Claude Desktop is not running for {}", registration.name);
    let Some(target) = lock.filter(|target| linux::live_lock(target, host.as_deref())) else {
        return Ok(not_running());
    };
    let program = launch::resolve_program("claude-desktop")?;
    match linux::terminate(&target, host.as_deref(), &program, &registration.name)? {
        true => Ok(format!("Closed Claude Desktop for {}", registration.name)),
        false => Ok(not_running()),
    }
}

fn record_launch(state: &mut StateFile, id: &str) {
    let at = platform::unix_seconds();
    crate::select::stamp(&mut state.last_used, id, at);
    crate::select::stamp(&mut state.desktop_launched, id, at);
}

/// The conventional Desktop data folder from the caller's environment:
/// `$XDG_CONFIG_HOME/Claude` when that is absolute, else `~/.config/Claude`.
fn conventional_folder() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| platform::home().ok().map(|home| home.join(".config")))?;
    Some(config.join("Claude"))
}

/// The `SingletonLock` link of the conventional Desktop folder, if any. Read-only,
/// so symlinked ancestors (a stowed `~/.config`) are followed, as Electron does.
fn conventional_lock() -> Option<PathBuf> {
    std::fs::read_link(conventional_folder()?.join("SingletonLock")).ok()
}

/// Doctor: Desktop folders with a recorded launch but no Electron data. A linked
/// registration's launches use the borrowed folder, so it is not checked.
pub fn findings(store: &Store) -> Vec<Value> {
    let Ok(state) = store.read_state() else {
        return vec![];
    };
    let ids: Vec<String> = state
        .desktop_launched
        .iter()
        .map(|t| t.registration_id.clone())
        .filter(|id| !state.desktop_links.iter().any(|l| &l.registration_id == id))
        .collect();
    let Ok(folders) = store.desktop_without_data(&ids) else {
        return vec![];
    };
    folders
        .into_iter()
        .map(|folder| {
            let name = store
                .registry
                .registrations
                .iter()
                .find(|r| folder.file_name() == Some(r.registration_id.as_ref()))
                .map_or("NAME", |r| r.name.as_str());
            json!({
                "code": "desktop_data_not_isolated",
                "severity": "warning",
                "message": format!("Claude Desktop for {name} wrote no data into its Roost folder; --user-data-dir may have stopped working"),
                "path": folder.display().to_string(),
                "next_step": format!("Run roost desktop --foreground {name} and check where Desktop keeps its sign-in"),
            })
        })
        .collect()
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::{
        os::unix::process::CommandExt,
        process::Stdio,
        time::{Duration, Instant},
    };

    /// How long a detached Desktop is watched for an early exit.
    const WATCH: Duration = Duration::from_secs(2);

    pub fn hostname() -> Option<String> {
        let mut buffer = [0_u8; 256];
        // gethostname writes a NUL-terminated name into caller-owned storage.
        if unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) } != 0 {
            return None;
        }
        let end = buffer.iter().position(|b| *b == 0)?;
        String::from_utf8(buffer[..end].to_vec()).ok()
    }

    /// How long `terminate` waits for Desktop to exit after SIGTERM.
    const CLOSE_WAIT: Duration = Duration::from_secs(10);

    /// The PID of Electron's `SingletonLock` link text `HOST-PID` when it names this
    /// host.
    fn lock_pid(target: &Path, host: Option<&str>) -> Option<libc::pid_t> {
        let (lock_host, pid) = target.to_str()?.rsplit_once('-')?;
        let pid = pid.parse::<libc::pid_t>().ok()?;
        (Some(lock_host) == host && pid > 0).then_some(pid)
    }

    /// A lock is live when it names this host and a process that exists.
    pub fn live_lock(target: &Path, host: Option<&str>) -> bool {
        let Some(pid) = lock_pid(target, host) else {
            return false;
        };
        // Signal 0 only checks that the process exists; EPERM means it exists
        // under another user.
        let pid = rustix::process::Pid::from_raw(pid).expect("positive");
        matches!(
            rustix::process::test_kill_process(pid),
            Ok(()) | Err(rustix::io::Errno::PERM)
        )
    }

    /// Whether process `pid` runs `program` (the resolved `claude-desktop`), comparing
    /// canonical paths. Either its executable is `program`, or `program` is a `#!`
    /// script and the process is exactly the kernel's execution of it: the
    /// executable is the script's interpreter and the argument the kernel inserts
    /// after the interpreter (and its optional shebang argument) is `program`. The
    /// second form is how a script launcher (and the test fake) runs.
    pub(super) fn runs_program(pid: libc::pid_t, program: &Path) -> bool {
        use std::io::Read;
        let canonical = |path: &Path| std::fs::canonicalize(path).ok();
        let Some(program) = canonical(program) else {
            return false;
        };
        // The kernel's link names the running executable's real path (with a
        // " (deleted)" suffix once replaced, which then never matches).
        let Ok(exe) = std::fs::read_link(format!("/proc/{pid}/exe")) else {
            return false;
        };
        if exe == program {
            return true;
        }
        let mut head = [0_u8; 256];
        let Ok(count) = std::fs::File::open(&program).and_then(|mut f| f.read(&mut head)) else {
            return false;
        };
        let Some(line) = head[..count]
            .strip_prefix(b"#!")
            .and_then(|rest| rest.split(|b| *b == b'\n').next())
            .filter(|_| head[..count].contains(&b'\n'))
            .and_then(|line| std::str::from_utf8(line).ok())
        else {
            return false;
        };
        let line = line.trim();
        let (interpreter, argument) = match line.split_once([' ', '\t']) {
            Some((interpreter, argument)) => (interpreter, !argument.trim().is_empty()),
            None => (line, false),
        };
        if canonical(Path::new(interpreter)) != Some(exe) {
            return false;
        }
        let Ok(cmdline) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
            return false;
        };
        let script = cmdline
            .split(|b| *b == 0)
            .nth(if argument { 2 } else { 1 })
            .map(|arg| {
                use std::os::unix::ffi::OsStrExt;
                Path::new(std::ffi::OsStr::from_bytes(arg))
            });
        script.is_some_and(|script| script.is_absolute() && canonical(script) == Some(program))
    }

    /// Sends SIGTERM to the process named by the live `SingletonLock` `target` once
    /// it is verified to be `program`, then waits up to 10 seconds for it to exit.
    /// `Ok(false)` when it had already gone. Never escalates beyond SIGTERM.
    pub fn terminate(
        target: &Path,
        host: Option<&str>,
        program: &Path,
        name: &str,
    ) -> Result<bool> {
        use rustix::event::{PollFd, PollFlags, Timespec, poll};
        use rustix::process::{Pid, PidfdFlags, Signal, pidfd_open, pidfd_send_signal};
        let Some(pid) = lock_pid(target, host) else {
            return Ok(false);
        };
        // The pidfd pins this process: if it exits and the PID is reused after the
        // checks below, signalling fails instead of reaching the newcomer.
        let pidfd = match pidfd_open(Pid::from_raw(pid).expect("positive"), PidfdFlags::empty()) {
            Ok(fd) => fd,
            Err(rustix::io::Errno::SRCH) => return Ok(false),
            Err(_) => {
                return Err(Error::new(
                    "io",
                    "Cannot inspect the Claude Desktop process",
                ));
            }
        };
        if !runs_program(pid, program) {
            return Err(Error::new(
                "ownership",
                format!(
                    "The Claude Desktop lock for {name} names process {pid}, which is not Claude Desktop ({}); nothing was signalled",
                    program.display()
                ),
            )
            .next(format!("Quit Claude Desktop for {name} from its own window or menu")));
        }
        match pidfd_send_signal(&pidfd, Signal::TERM) {
            Ok(()) => (),
            Err(rustix::io::Errno::SRCH) => return Ok(false),
            Err(_) => {
                return Err(Error::new(
                    "ownership",
                    format!("Cannot signal Claude Desktop for {name} (process {pid})"),
                )
                .next(format!(
                    "Quit Claude Desktop for {name} from its own window or menu"
                )));
            }
        }
        let deadline = Instant::now() + CLOSE_WAIT;
        loop {
            // A pidfd becomes readable once its process exits.
            // Any error, EINTR included, just retries until the deadline.
            let left = deadline.saturating_duration_since(Instant::now());
            let timeout = Timespec {
                tv_sec: 0,
                tv_nsec: (left.as_millis().min(100) * 1_000_000) as _,
            };
            let ready = poll(&mut [PollFd::new(&pidfd, PollFlags::IN)], Some(&timeout));
            if matches!(ready, Ok(n) if n > 0) {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Err(Error::new(
                    "desktop_running",
                    format!(
                        "Claude Desktop for {name} is still running after 10 seconds; Roost sent it SIGTERM and does not force it to quit"
                    ),
                )
                .next(format!(
                    "Quit Claude Desktop for {name} from its own window or menu, then repeat the command"
                )));
            }
        }
    }

    /// Replaces Roost with Desktop, inheriting stdio, for debugging.
    pub fn foreground(mut command: Command) -> Result<i32> {
        platform::restore_for_exec()?;
        let _ = command.exec();
        eprintln!("roost: io: Could not execute Claude Desktop");
        std::process::exit(1);
    }

    /// Starts Desktop in its own session with null stdio and watches it briefly.
    pub fn detached(mut command: Command, name: &str) -> Result<i32> {
        if platform::cancelled() {
            return Err(Error::cancelled());
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // setsid is async-signal-safe; the forked child is never a group leader.
        unsafe {
            command.pre_exec(|| {
                rustix::process::setsid().map_err(std::io::Error::from)?;
                Ok(())
            });
        }
        let mut child = command.spawn().map_err(|_| {
            Error::new("claude_unavailable", "Could not start Claude Desktop")
                .next(format!("Run roost desktop --foreground {name} to see why"))
        })?;
        let deadline = Instant::now() + WATCH;
        while Instant::now() < deadline && !platform::cancelled() {
            if let Ok(Some(status)) = child.try_wait() {
                use std::os::unix::process::ExitStatusExt;
                let how = match (status.code(), status.signal()) {
                    (Some(code), _) => format!("exit status {code}"),
                    (None, Some(signal)) => format!("signal {signal}"),
                    _ => "unknown status".to_owned(),
                };
                return Err(Error::new(
                    "claude_unavailable",
                    format!("Claude Desktop exited during startup ({how})"),
                )
                .next(format!(
                    "Run roost desktop --foreground {name} to see its output"
                )));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        eprintln!("Started Claude Desktop for {name}");
        Ok(0)
    }
}

#[cfg(not(target_os = "linux"))]
mod linux {
    use super::*;
    pub fn hostname() -> Option<String> {
        None
    }
    pub fn live_lock(_: &Path, _: Option<&str>) -> bool {
        false
    }
    pub fn terminate(_: &Path, _: Option<&str>, _: &Path, _: &str) -> Result<bool> {
        Ok(false)
    }
    pub fn foreground(_: Command) -> Result<i32> {
        Err(Error::new(
            "claude_unavailable",
            "roost desktop is supported only on Linux",
        ))
    }
    pub fn detached(_: Command, _: &str) -> Result<i32> {
        Err(Error::new(
            "claude_unavailable",
            "roost desktop is supported only on Linux",
        ))
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn singleton_lock_is_live_only_for_this_host_and_an_existing_process() {
        let host = Some("my-host");
        let me = std::process::id();
        assert!(linux::live_lock(Path::new(&format!("my-host-{me}")), host));
        assert!(!linux::live_lock(
            Path::new(&format!("other-host-{me}")),
            host
        ));
        assert!(!linux::live_lock(Path::new("my-host-0"), host));
        assert!(!linux::live_lock(Path::new("my-host-x"), host));
        assert!(!linux::live_lock(Path::new("garbage"), host));
        assert!(!linux::live_lock(Path::new(&format!("my-host-{me}")), None));
        let mut child = Command::new("/bin/true").spawn().unwrap();
        let dead = child.id();
        child.wait().unwrap();
        assert!(!linux::live_lock(
            Path::new(&format!("my-host-{dead}")),
            host
        ));
    }

    #[test]
    fn a_process_runs_the_program_only_by_its_canonical_executable() {
        let me = std::process::id() as libc::pid_t;
        let exe = std::env::current_exe().unwrap();
        assert!(linux::runs_program(me, &exe));
        assert!(!linux::runs_program(me, Path::new("/bin/sleep")));
        assert!(!linux::runs_program(
            me,
            Path::new("/nonexistent/claude-desktop")
        ));
    }
}
