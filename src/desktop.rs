//! `roost desktop NAME` (experimental, Linux only): Claude Desktop per profile through
//! Electron's undocumented `--user-data-dir` ([ADR 0002]). Owned and upstream
//! registrations get a Roost-owned data folder (their own Desktop sign-in) plus the
//! profile's isolated environment; a default alias starts plain `claude-desktop`.
//!
//! [ADR 0002]: ../docs/adr/0002-desktop-per-profile-via-user-data-dir.md
use crate::{
    Error, Result, launch, platform,
    store::{Kind, OpenMode, Registration, Store, side::StateFile},
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

const COWORK: &str = "Another Claude Desktop is already running; Cowork is untested with a second concurrent Desktop";

/// Prepares and starts Claude Desktop for `name`, or without it for the picker's
/// choice; returns the manager exit code.
pub fn run(name: Option<&str>, foreground: bool) -> Result<i32> {
    if !cfg!(target_os = "linux") {
        return Err(Error::new(
            "claude_unavailable",
            "roost desktop is experimental and supported only on Linux",
        )
        .next("Start Claude Desktop directly on this platform"));
    }
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
        None => {
            let store = crate::select::open_launch(&root)?;
            crate::select::choose(
                store,
                &root,
                "Choose a profile for Claude Desktop",
                no_terminal,
                picker_rows,
            )?
        }
    };
    start(store, registration, foreground)
}

/// Annotates the picker records with their Desktop state and renders them; the
/// initial row is the most recent Desktop launch, else the most recent launch.
fn picker_rows(store: &Store, records: &mut [Value]) -> (Vec<String>, usize) {
    let _ = crate::select::annotate(store, records, None);
    let host = linux::hostname();
    let locks = store.desktop_locks().unwrap_or_default();
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
        record["desktop"] = json!(match folder {
            _ if registration.kind == Kind::DefaultAlias => "plain",
            Some((_, Some(target))) if linux::live_lock(target, host.as_deref()) => "running",
            Some(_) => "signed_in",
            None => "never",
        });
    }
    let initial = initial
        .or_else(|| records.iter().position(|r| r["most_recent"] == true))
        .unwrap_or(0);
    (
        crate::table::desktop_profiles(records, crate::table::stderr_color()),
        initial,
    )
}

fn start(mut store: Store, registration: Registration, foreground: bool) -> Result<i32> {
    let mut env = launch::profile_env(&store, &registration, false)?;
    let program = launch::resolve_program("claude-desktop")?;
    let host = linux::hostname();
    let others_running = |store: &Store, own: Option<&str>| {
        let roost = store
            .desktop_locks()
            .unwrap_or_default()
            .into_iter()
            .any(|(id, lock)| {
                Some(id.as_str()) != own
                    && lock.is_some_and(|target| linux::live_lock(&target, host.as_deref()))
            });
        roost
            || conventional_lock().is_some_and(|target| linux::live_lock(&target, host.as_deref()))
    };
    let mut warnings = vec![];
    let command = if registration.kind == Kind::DefaultAlias {
        // Plain Desktop with the caller's environment: no folder, nothing recorded.
        if others_running(&store, None) {
            warnings.push(COWORK.to_owned());
        }
        drop(store);
        env.command(program)
    } else {
        let folder = store.ensure_desktop_folder(&registration)?;
        let id = registration.registration_id.as_str();
        let own = store
            .desktop_locks()?
            .into_iter()
            .find(|(lock_id, _)| lock_id == id)
            .and_then(|(_, lock)| lock);
        if own.is_some_and(|target| linux::live_lock(&target, host.as_deref())) {
            return Err(Error::new(
                "desktop_running",
                format!(
                    "this profile's Desktop is already running ({})",
                    registration.name
                ),
            )
            .next("Switch to the open Claude Desktop window, or quit it before relaunching"));
        }
        if others_running(&store, Some(id)) {
            warnings.push(COWORK.to_owned());
        }
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
        let mut argument = std::ffi::OsString::from("--user-data-dir=");
        argument.push(folder.as_os_str());
        let mut command = env.command(program);
        command.arg(argument);
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

/// Q51: purge and upstream remove refuse while the registration's Desktop folder holds
/// a live `SingletonLock` (its Desktop is running and would keep writing there).
pub fn refuse_if_running(store: &Store, registration: &Registration) -> Result<()> {
    let host = linux::hostname();
    let running = store.desktop_locks()?.into_iter().any(|(id, lock)| {
        id == registration.registration_id
            && lock.is_some_and(|target| linux::live_lock(&target, host.as_deref()))
    });
    if running {
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

fn record_launch(state: &mut StateFile, id: &str) {
    let at = platform::unix_seconds();
    crate::select::stamp(&mut state.last_used, id, at);
    crate::select::stamp(&mut state.desktop_launched, id, at);
}

/// The `SingletonLock` link of the conventional Desktop folder, if any.
fn conventional_lock() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| platform::home().ok().map(|home| home.join(".config")))?;
    platform::Directory::open(&config.join("Claude"), false)
        .ok()?
        .read_link("SingletonLock")
        .ok()
        .flatten()
}

/// Doctor: Desktop folders with a recorded launch but no Electron data.
pub fn findings(store: &Store) -> Vec<Value> {
    let Ok(state) = store.read_state() else {
        return vec![];
    };
    let ids: Vec<String> = state
        .desktop_launched
        .iter()
        .map(|t| t.registration_id.clone())
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

    /// Electron's `SingletonLock` link text is `HOST-PID`; it is live when it names
    /// this host and a process that exists.
    pub fn live_lock(target: &Path, host: Option<&str>) -> bool {
        let Some((lock_host, pid)) = target.to_str().and_then(|t| t.rsplit_once('-')) else {
            return false;
        };
        let Ok(pid) = pid.parse::<libc::pid_t>() else {
            return false;
        };
        if Some(lock_host) != host || pid <= 0 {
            return false;
        }
        // Signal 0 only checks that the process exists.
        let exists = unsafe { libc::kill(pid, 0) } == 0;
        exists || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
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
}
