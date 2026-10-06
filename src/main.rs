mod cli;
mod launch;
mod platform;
mod select;
mod sets;
mod store;
mod table;

use cli::Action;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process,
};
use store::{Kind, OpenMode, State, Store};

#[derive(Debug, Clone)]
pub struct Error {
    pub code: &'static str,
    pub message: String,
    pub next_step: Option<String>,
    pub exit_code: i32,
}
pub type Result<T> = std::result::Result<T, Error>;
impl Error {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            next_step: None,
            exit_code: if code == "usage" { 2 } else { 1 },
        }
    }
    pub fn next(mut self, step: impl Into<String>) -> Self {
        self.next_step = Some(step.into());
        self
    }
    pub fn cancelled() -> Self {
        Self {
            code: "cancelled",
            message: "Operation cancelled".into(),
            next_step: None,
            exit_code: 130,
        }
    }
    pub fn io(operation: &str, path: &Path, error: std::io::Error) -> Self {
        Self::new("io", format!("{operation} {}: {error}", path.display()))
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for Error {}

struct Outcome {
    lines: Vec<String>,
    warnings: Vec<String>,
    exit: i32,
    error: Option<Error>,
}
impl Outcome {
    fn lines(lines: Vec<String>) -> Self {
        Self {
            lines,
            warnings: vec![],
            exit: 0,
            error: None,
        }
    }
    fn quiet() -> Self {
        Self::lines(vec![])
    }
}

fn initial_data(action: &Action) -> Value {
    match action {
        Action::List { .. } => json!({"project":null,"profiles":[]}),
        Action::Status { name, .. } => {
            json!({"name":name,"kind":null,"reported_logged_in":null,"auth_method":null,"config_directory":null,"scope":null})
        }
        Action::Set {
            action: cli::SetAction::List { .. },
        } => json!({"sets":[]}),
        Action::Doctor { .. } => {
            json!({"claude_path":null,"claude_version":null,"storage_directory":null,"launcher_directory":null,"path_member":null,"profiles":[],"findings":[]})
        }
        _ => Value::Null,
    }
}

fn print_json(data: Value, warnings: Vec<String>, error: Option<&Error>) {
    let error = error.map(|e| json!({"code":e.code,"message":e.message,"next_step":e.next_step}));
    println!(
        "{}",
        json!({"schema_version":1,"data":data,"warnings":warnings,"error":error})
    );
}

fn main() {
    let action = match cli::parse() {
        Ok(v) => v,
        Err(e) => {
            let code = e.exit_code();
            let _ = e.print();
            process::exit(code);
        }
    };
    let json_mode = action.json();
    let mut data = initial_data(&action);
    let result = (|| {
        if let Some(name) = action.name() {
            store::validate_name(name)?;
        }
        if !matches!(action, Action::Help { .. } | Action::Version) {
            platform::install_cancel_handler()?;
        }
        dispatch(action, &mut data)
    })();
    match result {
        Ok(output) => {
            if json_mode {
                print_json(data, output.warnings, output.error.as_ref());
            } else {
                for line in output.lines {
                    println!("{line}");
                }
                for warning in output.warnings {
                    eprintln!("warning: {warning}");
                }
                if let Some(error) = output.error {
                    human_error(&error);
                }
            }
            process::exit(output.exit)
        }
        Err(error) => {
            if json_mode && error.code != "usage" {
                print_json(data, vec![], Some(&error));
            } else {
                human_error(&error);
            }
            process::exit(error.exit_code)
        }
    }
}

fn human_error(error: &Error) {
    eprintln!("{}: {}", error.code, error.message);
    if let Some(step) = &error.next_step {
        eprintln!("Next: {step}");
    }
}

fn open(root: &Path, mode: OpenMode) -> Result<Store> {
    Store::open(root, false, mode)
}
fn effective_directory(reg: &store::Registration) -> Result<PathBuf> {
    if reg.kind == Kind::DefaultAlias {
        match std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
            Some(path) => platform::absolute(Path::new(&path)),
            None => platform::default_directory(),
        }
    } else {
        reg.directory
            .clone()
            .ok_or_else(|| Error::new("ownership", "profile has no configuration directory"))
    }
}

/// One `list --full` status probe: the record index and its prepared command, or
/// why it could not be prepared. Built while the Store is open; run after it drops.
type ProbeJob = (usize, Result<(std::process::Command, store::Registration)>);

/// Prepares probes for active owned/upstream records; aliases and retained
/// records are never probed and keep a null `probe`.
fn probe_jobs(store: &Store, profiles: &[Value]) -> Vec<ProbeJob> {
    profiles
        .iter()
        .enumerate()
        .filter_map(|(index, record)| {
            let reg = store.registry.registrations.iter().find(|r| {
                r.state == State::Active
                    && r.kind != Kind::DefaultAlias
                    && record["name"] == r.name.as_str()
                    && record["state"] == "active"
            })?;
            Some((
                index,
                launch::prepare(store, reg, false).map(|command| (command, reg.clone())),
            ))
        })
        .collect()
}

/// Runs the prepared probes in parallel, fills each record's `probe` and returns
/// safe warnings naming the profile. Failures leave every probe field null.
fn run_probes(profiles: &mut [Value], jobs: Vec<ProbeJob>) -> Vec<String> {
    let mut warnings = Vec::new();
    for (index, result) in launch::probe_all(jobs) {
        let name = profiles[index]["name"].as_str().unwrap_or("?").to_owned();
        let probe = match result {
            Ok(status) => {
                warnings.extend(status.warnings.into_iter().map(|w| format!("{name}: {w}")));
                json!({"reported_logged_in":status.data["reported_logged_in"],"auth_method":status.data["auth_method"],"config_directory":status.data["config_directory"]})
            }
            Err(e) => {
                warnings.push(format!("{name}: status probe failed: {}", e.message));
                json!({"reported_logged_in":null,"auth_method":null,"config_directory":null})
            }
        };
        profiles[index]["probe"] = probe;
    }
    warnings
}

fn dispatch(action: Action, data: &mut Value) -> Result<Outcome> {
    match action {
        Action::Help { command } => Ok(Outcome::lines(vec![cli::help(command.as_deref())?])),
        Action::Version => Ok(Outcome::lines(vec![env!("CARGO_PKG_VERSION").into()])),
        Action::Update => Ok(Outcome {
            exit: launch::update()?,
            ..Outcome::quiet()
        }),
        Action::SetupPath { shell, apply } => Ok(Outcome::lines(platform::setup_path(
            &platform::root()?,
            shell.as_deref(),
            apply,
        )?)),
        Action::Doctor { .. } => doctor(data),
        Action::List { retained, full, .. } => {
            let root = platform::root()?;
            let store = match open(&root, OpenMode::Read) {
                Ok(store) => Some(store),
                Err(e)
                    if e.code == "not_found"
                        && std::fs::symlink_metadata(&root)
                            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
                {
                    None
                }
                Err(e) => return Err(e),
            };
            let mut profiles = if let Some(store) = &store {
                store.profiles(retained)?
            } else {
                vec![]
            };
            let (project, mut warnings) = select::list_project();
            if let Some(store) = &store {
                warnings.extend(sets::annotate(store, &mut profiles));
                warnings.extend(select::annotate(store, &mut profiles, project.as_deref()));
            }
            if store.as_ref().is_some_and(Store::pending) {
                warnings.push("Pending operation: inspect roost doctor before recovery".into());
            }
            let jobs = match (&store, full) {
                (Some(store), true) => probe_jobs(store, &profiles),
                _ => vec![],
            };
            drop(store);
            warnings.extend(run_probes(&mut profiles, jobs));
            let lines = table::profiles(
                &profiles,
                full,
                table::stdout_color(),
                platform::stdout_width(),
            );
            *data = json!({"project":project,"profiles":profiles});
            Ok(Outcome {
                warnings,
                ..Outcome::lines(lines)
            })
        }
        Action::Where { name } => {
            let store = open(&platform::root()?, OpenMode::Read)?;
            if store.pending() {
                return Err(
                    Error::new("ownership", "Pending operation blocks path lookup")
                        .next("Run roost doctor and follow its recovery guidance"),
                );
            }
            let reg = store.find(&name, true)?;
            store.validate(reg)?;
            Ok(Outcome::lines(vec![
                effective_directory(reg)?.display().to_string(),
            ]))
        }
        Action::Run {
            name,
            allow_auth_env,
            arguments,
        } => {
            let store = open(&platform::root()?, OpenMode::Launch)?;
            let reg = store.find(&name, false)?;
            let command = launch::prepare(&store, reg, allow_auth_env)?;
            select::launched(&store, reg);
            sets::reconcile_at_launch(&store, reg);
            drop(store);
            Ok(Outcome {
                exit: launch::execute(command, &arguments)?,
                ..Outcome::quiet()
            })
        }
        Action::Internal {
            root,
            root_id,
            registration_id,
            arguments,
        } => {
            if !root.is_absolute() {
                return Err(Error::new(
                    "ownership",
                    "Launcher root binding must be absolute",
                ));
            }
            let store = open(&platform::absolute(&root)?, OpenMode::Launch)?;
            if store.root_id != root_id {
                return Err(Error::new(
                    "ownership",
                    "Launcher root identity does not match",
                ));
            }
            let reg = store
                .registry
                .registrations
                .iter()
                .find(|r| r.registration_id == registration_id)
                .ok_or_else(|| {
                    Error::new("not_found", "Launcher registration is absent")
                        .next("Refresh an active registration with roost reuse NAME")
                })?;
            let expected = reg
                .launcher_binding
                .as_ref()
                .ok_or_else(|| Error::new("ownership", "Launcher binding is missing"))?;
            let executable = platform::absolute(
                &std::env::current_exe().map_err(|e| Error::new("io", e.to_string()))?,
            )?;
            if expected.format_version != 1 || expected.executable != executable {
                return Err(
                    Error::new("ownership", "Launcher executable binding is stale").next(format!(
                        "Run the selected Roost executable: roost reuse {}",
                        reg.name
                    )),
                );
            }
            let command = launch::prepare(&store, reg, false)?;
            select::launched(&store, reg);
            sets::reconcile_at_launch(&store, reg);
            drop(store);
            Ok(Outcome {
                exit: launch::execute(command, &arguments)?,
                ..Outcome::quiet()
            })
        }
        Action::Status {
            name,
            allow_auth_env,
            ..
        } => {
            let store = open(&platform::root()?, OpenMode::Read)?;
            let reg = store.find(&name, false)?.clone();
            *data = json!({"name":reg.name,"kind":reg.kind,"reported_logged_in":null,"auth_method":null,"config_directory":null,"scope":if reg.kind==Kind::DefaultAlias{"pass_through"}else{"isolated"}});
            let command = launch::prepare(&store, &reg, allow_auth_env)?;
            drop(store);
            let status = launch::status(command, &reg)?;
            *data = status.data;
            let logged_in = data["reported_logged_in"]
                .as_bool()
                .map(|v| if v { "logged in" } else { "logged out" })
                .unwrap_or("unknown");
            Ok(Outcome {
                lines: vec![format!(
                    "{}: {} ({})",
                    data["name"].as_str().unwrap_or("?"),
                    logged_in,
                    data["scope"].as_str().unwrap_or("unknown")
                )],
                warnings: status.warnings,
                exit: status.exit_code,
                error: None,
            })
        }
        Action::Add {
            name,
            link_default,
            copy_default,
            source,
            yes,
            no_sets,
        } => {
            let root = platform::root()?;
            let source = if copy_default {
                let selected = match source {
                    Some(path) => platform::absolute(&path)?,
                    None => match std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
                        Some(path) => platform::absolute(Path::new(&path))?,
                        None => platform::default_directory()?,
                    },
                };
                platform::Directory::open(&selected, false)?;
                platform::confirm(
                    &format!(
                        "Seed {} from {}. Stop sessions/external writers first; copied settings may contain secrets/helpers.",
                        root.join("profiles").join(&name).display(),
                        selected.display()
                    ),
                    yes,
                )?;
                Some(selected)
            } else {
                None
            };
            let mut store = Store::open(&root, true, OpenMode::Mutate)?;
            let warnings = store.add(&name, link_default, source.as_deref())?;
            let mut out = Outcome::lines(vec![format!(
                "Created {name}{}",
                if link_default {
                    " (pass-through default alias)"
                } else {
                    ""
                }
            )]);
            out.warnings = warnings;
            if !link_default && !no_sets {
                out.warnings
                    .extend(sets::subscribe_new(&store, &name, source.as_deref()));
            }
            if !link_default {
                out.lines
                    .push(format!("Login: roost run {name} auth login"));
                out.lines.push(format!("Verify: roost status {name}"));
            }
            Ok(out)
        }
        Action::Register { name, path } => {
            let mut store = Store::open(&platform::root()?, true, OpenMode::Mutate)?;
            let warnings = store.register(&name, &platform::absolute(&path)?)?;
            Ok(Outcome {
                warnings,
                ..Outcome::lines(vec![format!(
                    "Registered {name} at its unchanged upstream path"
                )])
            })
        }
        Action::Reuse { name } => {
            let mut store = open(&platform::root()?, OpenMode::Mutate)?;
            let warnings = store.reuse(&name)?;
            Ok(Outcome {
                warnings,
                ..Outcome::lines(vec![format!("Restored/refreshed {name}")])
            })
        }
        Action::Remove { name, purge, yes } => {
            let root = platform::root()?;
            let selection = {
                let store = open(
                    &root,
                    if purge {
                        OpenMode::RetryPurge
                    } else {
                        OpenMode::Mutate
                    },
                )?;
                let (scope, registration) = store.preflight_remove(&name, purge)?;
                let root_id = store.root_id.clone();
                drop(store);
                if purge {
                    platform::confirm(
                        &format!(
                            "Purge {scope}. Stop sessions/external writers first. This deletes owned data, not native logout or token revocation."
                        ),
                        yes,
                    )?;
                }
                (root_id, registration)
            };
            let mut store = open(
                &root,
                if purge {
                    OpenMode::RetryPurge
                } else {
                    OpenMode::Mutate
                },
            )?;
            let (_, current) = store.preflight_remove(&name, purge)?;
            if store.root_id != selection.0 || current != selection.1 {
                return Err(
                    Error::new("ownership", "Profile changed while removal was pending")
                        .next("Inspect the profile and repeat the command for the intended scope"),
                );
            }
            let messages = store.remove(&name, purge)?;
            let mut out = Outcome::lines(messages);
            out.warnings
                .extend(select::forget_removed(&store, &current));
            out.warnings.extend(sets::forget_removed(&store));
            Ok(out)
        }
        Action::Set { action } => Ok(Outcome::lines(sets::command(action, data)?)),
        Action::Switch {
            allow_auth_env,
            no_launch,
            forget,
            name,
            arguments,
        } => select::switch(
            name.as_deref(),
            no_launch,
            forget,
            allow_auth_env,
            &arguments,
        ),
        Action::Launch {
            allow_auth_env,
            arguments,
        } => select::launch_selected(allow_auth_env, &arguments),
        Action::Token { name, stdin, clear } => {
            let root = platform::root()?;
            let selection = {
                let store = open(&root, OpenMode::Mutate)?;
                let reg = store.find(&name, false)?;
                if reg.kind != Kind::Owned || reg.state != State::Active {
                    return Err(Error::new(
                        "ownership",
                        "Manager token writes require an active owned isolated profile",
                    ));
                }
                store.validate(reg)?;
                let directory = platform::Directory::open(
                    reg.directory
                        .as_deref()
                        .ok_or_else(|| Error::new("ownership", "Missing owned directory"))?,
                    true,
                )?;
                if directory.entry(".roost-token")?.is_some() {
                    drop(directory.open_file(".roost-token", true, false)?);
                }
                (store.root_id.clone(), reg.clone())
            };
            let token = if clear {
                None
            } else {
                Some(platform::token_input(stdin)?)
            };
            let mut store = open(&root, OpenMode::Mutate)?;
            let current = store.find(&name, false)?;
            if store.root_id != selection.0 || *current != selection.1 {
                return Err(Error::new(
                    "ownership",
                    "Profile changed while token input was pending",
                )
                .next("Inspect the profile and repeat token input for the intended scope"));
            }
            let messages = store.set_token(&name, token.as_deref())?;
            Ok(Outcome::lines(messages))
        }
    }
}

fn doctor(data: &mut Value) -> Result<Outcome> {
    let root = platform::root()?;
    let launcher = root.join("bin");
    *data = json!({"claude_path":null,"claude_version":null,"storage_directory":root,"launcher_directory":launcher,"path_member":platform::path_member(&launcher),"profiles":[],"findings":[]});
    let mut findings = vec![];
    let finding = |code: &str,
                   severity: &str,
                   message: String,
                   path: Option<&Path>,
                   step: Option<String>| json!({"code":code,"severity":severity,"message":message,"path":path.map(|p|p.display().to_string()),"next_step":step});
    match launch::claude_info() {
        Ok((path, version)) => {
            data["claude_path"] = json!(path);
            data["claude_version"] = json!(version);
        }
        Err(e) => findings.push(finding(e.code, "error", e.message, None, e.next_step)),
    }
    if !platform::path_member(&launcher) {
        findings.push(finding(
            "path",
            "warning",
            "Launcher directory is not in caller PATH".into(),
            Some(&launcher),
            Some("Run roost setup-path; restart the terminal after applying".into()),
        ));
    }
    match open(&root, OpenMode::Read) {
        Ok(store) => {
            if store.pending() {
                findings.push(finding("recovery","error","Pending operation requires classified recovery; purge deletion is never automatically continued".into(),Some(&root),Some("Retry the original acknowledged mutation after inspection; explicit purge retries require quiet writers".into())));
            }
            match store.profiles(true) {
                Ok(mut profiles) => {
                    let _ = sets::annotate(&store, &mut profiles);
                    let _ = select::annotate(
                        &store,
                        &mut profiles,
                        select::list_project().0.as_deref(),
                    );
                    for p in &profiles {
                        for l in p["launchers"].as_array().into_iter().flatten() {
                            let condition = l["condition"].as_str().unwrap_or("unsafe");
                            if condition != "ready" && condition != "not_required" {
                                let path = l["path"].as_str().map(Path::new);
                                findings.push(finding("launcher","error",format!("{} launcher is {condition}",p["name"].as_str().unwrap_or("?")),path,Some(format!("Inspect ownership; roost reuse {} repairs verified owned launchers",p["name"].as_str().unwrap_or("NAME")))));
                            }
                        }
                    }
                    data["profiles"] = json!(profiles);
                }
                Err(e) => findings.push(finding(
                    e.code,
                    "error",
                    e.message,
                    Some(&root),
                    e.next_step,
                )),
            }
            for reg in &store.registry.registrations {
                if let Err(e) = store.validate(reg) {
                    findings.push(finding(
                        e.code,
                        "error",
                        e.message,
                        reg.directory.as_deref(),
                        e.next_step,
                    ));
                }
                if let Err(e) = store.token_present(reg) {
                    findings.push(finding(
                        e.code,
                        "error",
                        e.message,
                        reg.directory.as_deref(),
                        e.next_step,
                    ));
                }
            }
        }
        Err(e)
            if e.code == "not_found"
                && std::fs::symlink_metadata(&root)
                    .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            findings.push(finding(
                "storage",
                "warning",
                "Manager storage is not initialized".into(),
                Some(&root),
                None,
            ))
        }
        Err(e) => findings.push(finding(
            e.code,
            "error",
            e.message,
            Some(&root),
            e.next_step,
        )),
    }
    findings.sort_by_key(|f| {
        (
            f["code"].as_str().unwrap_or("").to_owned(),
            f["path"].as_str().unwrap_or("").to_owned(),
        )
    });
    let failed = findings.iter().any(|f| f["severity"] == "error");
    let warnings = findings
        .iter()
        .filter(|f| f["severity"] == "warning")
        .map(|f| f["message"].as_str().unwrap_or("").to_owned())
        .collect();
    let mut lines = vec![
        format!(
            "Claude: {}",
            data["claude_version"]
                .as_str()
                .unwrap_or("unavailable/unsupported")
        ),
        format!("Storage: {}", root.display()),
    ];
    lines.extend(
        findings
            .iter()
            .filter(|f| f["severity"] == "error")
            .map(|f| {
                format!(
                    "{}: {}",
                    f["code"].as_str().unwrap_or("diagnostics"),
                    f["message"].as_str().unwrap_or("check failed")
                )
            }),
    );
    data["findings"] = json!(findings);
    Ok(Outcome {
        lines,
        warnings,
        exit: if failed { 1 } else { 0 },
        error: failed.then(|| {
            Error::new("diagnostics", "One or more doctor checks failed")
                .next("Inspect the reported findings before mutation")
        }),
    })
}
