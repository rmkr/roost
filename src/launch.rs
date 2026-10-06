//! Bound launchers, profile environment selection and native Claude delegation.
use crate::{
    Error, Result, platform,
    store::{Kind, Registration, State, Store},
};
use serde_json::{Value, json};
use std::{
    env,
    ffi::{OsStr, OsString},
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

const PROBE_LIMIT: usize = 1024 * 1024;
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const AUTH_ENV: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_REFRESH_TOKEN",
    "CLAUDE_CODE_OAUTH_SCOPES",
    "CLAUDE_CODE_USE_ANTHROPIC_AWS",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_FOUNDRY",
    "CLAUDE_CODE_USE_MANTLE",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_SKIP_ANTHROPIC_AWS_AUTH",
    "CLAUDE_CODE_SKIP_BEDROCK_AUTH",
    "CLAUDE_CODE_SKIP_FOUNDRY_AUTH",
    "CLAUDE_CODE_SKIP_MANTLE_AUTH",
    "CLAUDE_CODE_SKIP_VERTEX_AUTH",
    "ANTHROPIC_AWS_API_KEY",
    "ANTHROPIC_FOUNDRY_API_KEY",
    "ANTHROPIC_FOUNDRY_AUTH_TOKEN",
    "AWS_BEARER_TOKEN_BEDROCK",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_AWS_BASE_URL",
    "ANTHROPIC_AWS_WORKSPACE_ID",
    "ANTHROPIC_BEDROCK_BASE_URL",
    "ANTHROPIC_BEDROCK_MANTLE_BASE_URL",
    "ANTHROPIC_FOUNDRY_BASE_URL",
    "ANTHROPIC_FOUNDRY_RESOURCE",
    "ANTHROPIC_VERTEX_BASE_URL",
    "ANTHROPIC_VERTEX_PROJECT_ID",
    "ANTHROPIC_CUSTOM_HEADERS",
    "ANTHROPIC_PROFILE",
    "ANTHROPIC_CONFIG_DIR",
    "ANTHROPIC_FEDERATION_RULE_ID",
    "ANTHROPIC_ORGANIZATION_ID",
    "ANTHROPIC_SERVICE_ACCOUNT_ID",
    "ANTHROPIC_WORKSPACE_ID",
    "ANTHROPIC_IDENTITY_TOKEN",
    "ANTHROPIC_IDENTITY_TOKEN_FILE",
    "CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST",
    "CLAUDE_CODE_SIMPLE",
    "CLAUDE_CODE_CLIENT_CERT",
    "CLAUDE_CODE_CLIENT_KEY",
    "CLAUDE_CODE_CLIENT_KEY_PASSPHRASE",
];

fn fixed_path(path: &Path) -> Result<&str> {
    let value = path
        .to_str()
        .ok_or_else(|| Error::new("unsafe_path", "Launcher path is not Unicode"))?;
    if !path.is_absolute() || value.contains(['\r', '\n', '\0']) {
        return Err(Error::new(
            "unsafe_path",
            "Launcher binding requires an absolute path without line breaks",
        ));
    }
    Ok(value)
}
fn quoted_sh(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
#[cfg(windows)]
fn quoted_ps(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
#[cfg(windows)]
fn quoted_cmd(value: &str) -> Result<String> {
    if value.contains('"') {
        return Err(Error::new(
            "unsafe_path",
            "Windows launcher binding contains a quote",
        ));
    }
    Ok(format!("\"{}\"", value.replace('%', "%%")))
}

pub fn templates(
    root: &Path,
    root_id: &str,
    registration: &Registration,
) -> Result<Vec<(PathBuf, Vec<u8>)>> {
    crate::store::validate_name(&registration.name)?;
    for id in [root_id, registration.registration_id.as_str()] {
        if id.len() != 32
            || !id
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(Error::new("ownership", "Invalid launcher binding ID"));
        }
    }
    let binding = registration
        .launcher_binding
        .as_ref()
        .ok_or_else(|| Error::new("ownership", "Missing launcher binding"))?;
    if binding.format_version != 1 {
        return Err(Error::new(
            "ownership",
            "Unsupported launcher template version",
        ));
    }
    let executable = fixed_path(&binding.executable)?;
    let root_text = fixed_path(root)?;
    let operands = [
        "__roost_launch_v1",
        root_text,
        root_id,
        &registration.registration_id,
        "--",
    ];
    #[cfg(windows)]
    let sh_executable = executable.replace('\\', "/");
    #[cfg(not(windows))]
    let sh_executable = executable.to_owned();
    let sh = format!(
        "#!/bin/sh\nexec {} {} \"$@\"\n",
        quoted_sh(&sh_executable),
        operands
            .iter()
            .map(|s| quoted_sh(s))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let stem = root
        .join("bin")
        .join(format!("roost-{}", registration.name));
    #[allow(unused_mut)]
    let mut result = vec![(stem.clone(), sh.into_bytes())];
    #[cfg(windows)]
    {
        let cmd = format!(
            "@echo off\r\nsetlocal DisableDelayedExpansion\r\n{} {} %*\r\nendlocal & exit /b %errorlevel%\r\n",
            quoted_cmd(executable)?,
            operands
                .iter()
                .map(|s| quoted_cmd(s))
                .collect::<Result<Vec<_>>>()?
                .join(" ")
        );
        result.push((stem.with_extension("cmd"), cmd.into_bytes()));
        let fixed = operands
            .iter()
            .map(|s| quoted_ps(s))
            .collect::<Vec<_>>()
            .join(", ");
        let ps = format!(
            "\u{feff}# Roost bound launcher format 1\r\n$ErrorActionPreference = 'Stop'\r\nfunction Quote-RoostArgument([string] $Value) {{\r\n    if ($Value.Length -gt 0 -and $Value -notmatch '[\\s\"]') {{ return $Value }}\r\n    $Out = New-Object System.Text.StringBuilder\r\n    [void] $Out.Append('\"')\r\n    $Slashes = 0\r\n    foreach ($Char in $Value.ToCharArray()) {{\r\n        if ($Char -eq '\\') {{ $Slashes++; continue }}\r\n        if ($Char -eq '\"') {{\r\n            [void] $Out.Append(('\\' * (2 * $Slashes + 1)))\r\n        }} else {{ [void] $Out.Append(('\\' * $Slashes)) }}\r\n        [void] $Out.Append($Char)\r\n        $Slashes = 0\r\n    }}\r\n    [void] $Out.Append(('\\' * (2 * $Slashes)))\r\n    [void] $Out.Append('\"')\r\n    return $Out.ToString()\r\n}}\r\n$Start = New-Object System.Diagnostics.ProcessStartInfo\r\n$Start.FileName = {}\r\n$Start.UseShellExecute = $false\r\n$Tail = @({}) + @($args)\r\nif ($null -ne $Start.PSObject.Properties['ArgumentList']) {{\r\n    foreach ($Value in $Tail) {{ [void] $Start.ArgumentList.Add([string] $Value) }}\r\n}} else {{\r\n    $Start.Arguments = (($Tail | ForEach-Object {{ Quote-RoostArgument ([string] $_) }}) -join ' ')\r\n}}\r\n$Child = [System.Diagnostics.Process]::Start($Start)\r\ntry {{ $Child.WaitForExit(); $Result = $Child.ExitCode }} finally {{ $Child.Dispose() }}\r\nexit $Result\r\n",
            quoted_ps(executable),
            fixed
        );
        result.push((stem.with_extension("ps1"), ps.into_bytes()));
    }
    Ok(result)
}

fn auth_conflicts(mut lookup: impl FnMut(&str) -> Option<OsString>) -> Vec<&'static str> {
    AUTH_ENV
        .iter()
        .copied()
        .filter(|name| lookup(name).is_some_and(|value| !value.is_empty()))
        .collect()
}

/// The environment a profile launch adds to the caller's environment. Isolated
/// registrations set their configuration directory and update controls (plus an
/// eligible manager token); pass-through aliases add nothing. Holds the token only
/// until `apply`/`command` consumes it.
pub struct ProfileEnv {
    isolated: bool,
    vars: Vec<(&'static str, OsString)>,
}
impl ProfileEnv {
    fn isolated(directory: &Path, token: Option<String>) -> Self {
        let mut vars = vec![
            ("CLAUDE_CONFIG_DIR", directory.as_os_str().to_owned()),
            ("DISABLE_AUTOUPDATER", OsString::from("1")),
            ("FORCE_AUTOUPDATE_PLUGINS", OsString::from("1")),
        ];
        if let Some(value) = token {
            vars.push(("CLAUDE_CODE_OAUTH_TOKEN", OsString::from(value)));
        }
        Self {
            isolated: true,
            vars,
        }
    }
    fn pass_through() -> Self {
        Self {
            isolated: false,
            vars: Vec::new(),
        }
    }
    /// True for owned/upstream registrations; false for default aliases.
    pub fn is_isolated(&self) -> bool {
        self.isolated
    }
    /// Sets (or overrides) one variable on an isolated launch; ignored for aliases,
    /// whose caller environment always passes through unchanged.
    pub fn set(&mut self, name: &'static str, value: impl Into<OsString>) {
        if !self.isolated {
            return;
        }
        let value = value.into();
        match self.vars.iter_mut().find(|(key, _)| *key == name) {
            Some(slot) => slot.1 = value,
            None => self.vars.push((name, value)),
        }
    }
    /// Sets `name` to the caller's nonempty inherited value followed by `paths`,
    /// joined with the platform path-list separator (for CLAUDE_CODE_PLUGIN_DIRS).
    /// No-op for aliases or an empty list.
    pub fn append_paths(&mut self, name: &'static str, paths: &[PathBuf]) -> Result<()> {
        if !self.isolated || paths.is_empty() {
            return Ok(());
        }
        let value = path_list(env::var_os(name), paths)?;
        self.set(name, value);
        Ok(())
    }
    /// Applies the variables to `command`, consuming any token value.
    pub fn apply(self, command: &mut Command) {
        for (name, value) in self.vars {
            command.env(name, value);
        }
    }
    /// A command for `program` with this environment applied.
    pub fn command(self, program: PathBuf) -> Command {
        let mut command = Command::new(program);
        self.apply(&mut command);
        command
    }
}

fn path_list(inherited: Option<OsString>, paths: &[PathBuf]) -> Result<OsString> {
    env::join_paths(
        inherited
            .filter(|value| !value.is_empty())
            .into_iter()
            .map(PathBuf::from)
            .chain(paths.iter().cloned()),
    )
    .map_err(|_| {
        Error::new(
            "unsafe_path",
            "Path list entry contains the path-list separator",
        )
    })
}

/// Validates a launch of `registration` (no pending intent, active, stored record,
/// auth-environment conflicts, token protection) and returns its environment.
/// Callers must release the Store before any child/probe.
pub fn profile_env(
    store: &Store,
    registration: &Registration,
    allow_auth_env: bool,
) -> Result<ProfileEnv> {
    if store.pending() {
        return Err(Error::new("ownership", "Pending operation blocks launch")
            .next("Inspect doctor and complete the pending operation before launching"));
    }
    if registration.state != State::Active {
        return Err(Error::new("not_found", "Profile is retained and inactive")
            .next("Use roost reuse NAME before launching"));
    }
    store.validate(registration)?;
    if registration.kind != Kind::DefaultAlias && !allow_auth_env {
        let conflicts = auth_conflicts(|name| env::var_os(name));
        if !conflicts.is_empty() {
            return Err(Error::new("auth_conflict", format!("Inherited authentication controls: {}", conflicts.join(", "))).next("Use direct roost run/status --allow-auth-env to accept caller authentication precedence"));
        }
    }
    // Even override validates token protection, but never reads unused secret bytes.
    let token = if registration.kind != Kind::DefaultAlias {
        let present = store.token_present(registration)?;
        if present && !allow_auth_env {
            store.token(registration)?
        } else {
            None
        }
    } else {
        None
    };
    if registration.kind == Kind::DefaultAlias {
        return Ok(ProfileEnv::pass_through());
    }
    let directory = registration
        .directory
        .as_ref()
        .ok_or_else(|| Error::new("ownership", "Isolated profile has no directory"))?;
    Ok(ProfileEnv::isolated(
        directory,
        token.filter(|_| !allow_auth_env),
    ))
}

/// Constructs the profile's Claude command only: callers must release the Store
/// before any child/probe.
pub fn prepare(
    store: &Store,
    registration: &Registration,
    allow_auth_env: bool,
) -> Result<Command> {
    let env = profile_env(store, registration, allow_auth_env)?;
    Ok(env.command(resolve_program("claude")?))
}

/// Resolves `name` through the caller's PATH like a shell would (Unix: first
/// executable regular file; Windows: PATHEXT search requiring a native `.exe`).
pub fn resolve_program(name: &str) -> Result<PathBuf> {
    resolve_in(name, env::var_os("PATH"))
}

fn resolve_in(name: &str, path: Option<OsString>) -> Result<PathBuf> {
    let path = path
        .ok_or_else(|| Error::new("claude_unavailable", format!("PATH does not select {name}")))?;
    #[cfg(unix)]
    {
        for directory in env::split_paths(&path) {
            let candidate = platform::absolute(&directory.join(name))?;
            if let Ok(metadata) = candidate.metadata()
                && metadata.is_file()
                && rustix::fs::accessat(
                    rustix::fs::CWD,
                    &candidate,
                    rustix::fs::Access::EXEC_OK,
                    rustix::fs::AtFlags::EACCESS,
                )
                .is_ok()
            {
                return Ok(candidate);
            }
        }
    }
    #[cfg(windows)]
    {
        let extensions =
            env::var_os("PATHEXT").unwrap_or_else(|| OsString::from(".COM;.EXE;.BAT;.CMD"));
        let extensions = extensions
            .to_str()
            .ok_or_else(|| Error::new("claude_unavailable", "PATHEXT is not Unicode"))?;
        for directory in env::split_paths(&path) {
            for extension in extensions.split(';').filter(|s| !s.is_empty()) {
                let candidate = platform::absolute(&directory.join(format!("{name}{extension}")))?;
                if candidate.is_file() {
                    if !extension.eq_ignore_ascii_case(".exe") {
                        return Err(Error::new("claude_unsupported", format!("PATH-selected {name} is not a native {name}.exe")).next("Select a native installation first on PATH; batch shims are unsupported"));
                    }
                    return Ok(candidate);
                }
            }
        }
    }
    Err(Error::new(
        "claude_unavailable",
        format!("No executable {name} was found on PATH"),
    )
    .next(if name == "claude" {
        "Make the existing shared Claude installation available on PATH".to_owned()
    } else {
        format!("Make {name} available on PATH")
    }))
}

struct Probe {
    stdout: Vec<u8>,
    exit: ExitStatus,
}

#[cfg(unix)]
fn nonblocking(pipe: &impl std::os::fd::AsRawFd) -> io::Result<()> {
    // SAFETY: the live pipe owns this fd; fcntl does not retain the descriptor.
    let flags = unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
#[cfg(windows)]
fn nonblocking(_pipe: &impl std::os::windows::io::AsRawHandle) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn read_ready(
    pipe: &mut (impl Read + std::os::fd::AsRawFd),
    bytes: &mut [u8],
) -> io::Result<Option<usize>> {
    match pipe.read(bytes) {
        Err(error)
            if error.kind() == io::ErrorKind::WouldBlock
                || error.kind() == io::ErrorKind::Interrupted =>
        {
            Ok(None)
        }
        result => result.map(Some),
    }
}
#[cfg(windows)]
fn read_ready(
    pipe: &mut (impl Read + std::os::windows::io::AsRawHandle),
    bytes: &mut [u8],
) -> io::Result<Option<usize>> {
    use windows_sys::Win32::{
        Foundation::{ERROR_BROKEN_PIPE, GetLastError},
        System::Pipes::PeekNamedPipe,
    };
    let mut available = 0u32;
    // SAFETY: live pipe handle, valid out pointer, no retained references.
    if unsafe {
        PeekNamedPipe(
            pipe.as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut available,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return if unsafe { GetLastError() } == ERROR_BROKEN_PIPE {
            Ok(Some(0))
        } else {
            Err(io::Error::last_os_error())
        };
    }
    if available == 0 {
        return Ok(None);
    }
    let count = bytes.len().min(available as usize);
    pipe.read(&mut bytes[..count]).map(Some)
}

fn capture(
    mut command: Command,
    code: &'static str,
    timeout: Duration,
    limit: usize,
) -> Result<Probe> {
    let deadline = Instant::now() + timeout;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|_| Error::new(code, "Could not start Claude probe"))?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let result = (|| {
        nonblocking(&stdout)
            .and_then(|_| nonblocking(&stderr))
            .map_err(|_| Error::new(code, "Could not safely capture Claude probe"))?;
        let mut output = Vec::new();
        let mut stderr_size = 0usize;
        let mut stdout_done = false;
        let mut stderr_done = false;
        let mut exit = None;
        let mut buffer = [0u8; 8192];
        loop {
            if platform::cancelled() {
                return Err(Error::cancelled());
            }
            if Instant::now() >= deadline {
                return Err(Error::new(code, "Claude probe exceeded its deadline"));
            }
            // Fairly service both pipes; neither stream waits for EOF on the other.
            if !stdout_done
                && let Some(count) = read_ready(&mut stdout, &mut buffer)
                    .map_err(|_| Error::new(code, "Could not read Claude probe output"))?
            {
                stdout_done = count == 0;
                if output.len() + count > limit {
                    return Err(Error::new(code, "Claude probe exceeded its output limit"));
                }
                output.extend_from_slice(&buffer[..count]);
            }
            if !stderr_done
                && let Some(count) = read_ready(&mut stderr, &mut buffer)
                    .map_err(|_| Error::new(code, "Could not read Claude probe diagnostics"))?
            {
                stderr_done = count == 0;
                stderr_size += count;
                if stderr_size > limit {
                    return Err(Error::new(
                        code,
                        "Claude probe exceeded its diagnostic limit",
                    ));
                }
            }
            if exit.is_none() {
                exit = child
                    .try_wait()
                    .map_err(|_| Error::new(code, "Could not wait for Claude probe"))?;
            }
            if stdout_done
                && stderr_done
                && let Some(exit) = exit
            {
                return Ok(Probe {
                    stdout: output,
                    exit,
                });
            }
            thread::sleep(Duration::from_millis(2));
        }
    })();
    if result.is_err() {
        let _ = child.kill();
        child
            .wait()
            .map_err(|_| Error::new(code, "Could not reap terminated Claude probe"))?;
    }
    result
}

pub(crate) fn bounded_probe(command: Command, code: &'static str) -> Result<(Vec<u8>, ExitStatus)> {
    let probe = capture(command, code, PROBE_TIMEOUT, PROBE_LIMIT)?;
    Ok((probe.stdout, probe.exit))
}

/// A captured probe with its own deadline (plugin auto-update); 1 MiB per stream.
pub(crate) fn deadline_probe(
    command: Command,
    code: &'static str,
    timeout: Duration,
) -> Result<(Vec<u8>, ExitStatus)> {
    let probe = capture(command, code, timeout, PROBE_LIMIT)?;
    Ok((probe.stdout, probe.exit))
}

fn parse_version(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?.trim();
    let version = text.strip_suffix(" (Claude Code)").unwrap_or(text);
    if !version
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
        return None;
    }
    let parts = version
        .split('.')
        .map(str::parse::<u64>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .ok()?;
    if parts.len() != 3 || (parts[0], parts[1], parts[2]) < (2, 1, 280) {
        return None;
    }
    Some(format!("{}.{}.{}", parts[0], parts[1], parts[2]))
}
fn version_at(path: &OsStr) -> Result<String> {
    // Deliberately starts from caller environment, never a profile command/token.
    let mut command = Command::new(path);
    command.arg("--version");
    let probe = capture(command, "claude_unsupported", PROBE_TIMEOUT, PROBE_LIMIT)?;
    if !probe.exit.success() {
        return Err(Error::new(
            "claude_unsupported",
            "Claude version probe failed",
        ));
    }
    parse_version(&probe.stdout).ok_or_else(|| {
        Error::new(
            "claude_unsupported",
            "Claude version is unsupported or unrecognized",
        )
        .next("Use Claude Code 2.1.280 or later with supported native status")
    })
}
pub fn claude_info() -> Result<(PathBuf, String)> {
    let path = resolve_program("claude")?;
    let version = version_at(path.as_os_str())?;
    Ok((path, version))
}

pub fn execute(mut command: Command, arguments: &[OsString]) -> Result<i32> {
    version_at(command.get_program())?;
    if platform::cancelled() {
        return Err(Error::cancelled());
    }
    command.args(arguments);
    platform::restore_for_exec()?;
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let _ = command.exec();
        // A failed exec may already have changed process environment/stdio.
        // No returning to manager mutation/state reuse after this boundary.
        eprintln!("roost: io: Could not execute the selected Claude installation");
        std::process::exit(1);
    }
    #[cfg(windows)]
    {
        let mut child = command
            .spawn()
            .map_err(|_| Error::new("io", "Could not execute the selected Claude installation"))?;
        let status = child
            .wait()
            .map_err(|_| Error::new("io", "Could not wait for Claude"))?;
        Ok(status.code().unwrap_or(1))
    }
}
pub fn update() -> Result<i32> {
    eprintln!("roost: updating the shared Claude installation");
    execute(
        Command::new(resolve_program("claude")?),
        &[OsString::from("update")],
    )
}

pub struct Status {
    pub data: Value,
    pub warnings: Vec<String>,
    pub exit_code: i32,
}
fn recognized_status(
    bytes: &[u8],
    exit: Option<i32>,
    registration: &Registration,
) -> Result<Status> {
    let unknown = || {
        Error::new("auth_status", "Claude authentication status is unknown").next(
            "Check the selected supported Claude installation; no credential fallback is used",
        )
    };
    let logged_in = match exit {
        Some(0) => true,
        Some(1) => false,
        _ => return Err(unknown()),
    };
    let value: Value = serde_json::from_slice(bytes).map_err(|_| unknown())?;
    let object = value.as_object().ok_or_else(unknown)?;
    let method = object
        .get("authMethod")
        .and_then(Value::as_str)
        .filter(|value| {
            matches!(
                *value,
                "none" | "claude.ai" | "oauth_token" | "api_key" | "api_key_helper" | "third_party"
            )
        });
    let directory = object
        .get("configDirectory")
        .and_then(Value::as_str)
        .filter(|value| {
            !value.is_empty()
                && !value.contains(['\r', '\n', '\0'])
                && Path::new(value).is_absolute()
        });
    if method.is_none() && directory.is_none() {
        return Err(unknown());
    }
    if method.is_some_and(|method| (method == "none") == logged_in) {
        return Err(unknown());
    }
    let mut warnings = Vec::new();
    if method.is_none() {
        warnings.push("Claude did not report a recognized authentication method".to_owned());
    }
    if directory.is_none() {
        warnings.push("Claude did not report a valid absolute configuration directory".to_owned());
    }
    let expected = if registration.kind == Kind::DefaultAlias {
        match env::var_os("CLAUDE_CONFIG_DIR").filter(|value| !value.is_empty()) {
            Some(value) => platform::absolute(Path::new(&value)).ok(),
            None => platform::default_directory().ok(),
        }
    } else {
        registration.directory.clone()
    };
    if let (Some(observed), Some(expected)) = (directory, expected)
        && Path::new(observed) != expected
    {
        warnings.push("Claude reported a different configuration directory; this does not establish account identity".to_owned());
    }
    Ok(Status {
        data: json!({"name": registration.name, "kind": registration.kind, "reported_logged_in": logged_in, "auth_method": method, "config_directory": directory, "scope": if registration.kind == Kind::DefaultAlias { "pass_through" } else { "isolated" }}),
        warnings,
        exit_code: if logged_in { 0 } else { 3 },
    })
}
/// Runs independent status probes in parallel (each under the usual bounds),
/// keeping each job's key and passing preparation failures through unchanged.
/// Callers must have released the Store.
pub fn probe_all<K: Send>(
    jobs: Vec<(K, Result<(Command, Registration)>)>,
) -> Vec<(K, Result<Status>)> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = jobs
            .into_iter()
            .map(|(key, job)| {
                scope.spawn(move || {
                    let result = job.and_then(|(command, reg)| status(command, &reg));
                    (key, result)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect()
    })
}
pub fn status(mut command: Command, registration: &Registration) -> Result<Status> {
    version_at(command.get_program())?;
    command.args(["auth", "status"]);
    let probe = capture(command, "auth_status", PROBE_TIMEOUT, PROBE_LIMIT)?;
    recognized_status(&probe.stdout, probe.exit.code(), registration)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::LauncherBinding;

    fn registration() -> Registration {
        Registration {
            registration_id: "b".repeat(32),
            name: "Work".to_owned(),
            kind: Kind::Owned,
            state: State::Active,
            directory: Some(PathBuf::from(if cfg!(windows) {
                "C:\\profiles\\Work"
            } else {
                "/profiles/Work"
            })),
            directory_identity: None,
            profile_id: Some("c".repeat(32)),
            upstream_linked_default: false,
            launcher_binding: Some(LauncherBinding {
                format_version: 1,
                executable: PathBuf::from(if cfg!(windows) {
                    "C:\\manager\\roost.exe"
                } else {
                    "/manager/roost"
                }),
            }),
        }
    }

    fn envs(command: &Command) -> Vec<(String, Option<String>)> {
        command
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect()
    }

    #[test]
    fn isolated_profile_environment_applies_to_any_program_and_extends() {
        let mut env = ProfileEnv::isolated(Path::new("/profiles/Work"), Some("tok".into()));
        assert!(env.is_isolated());
        env.set("EXTRA", "1");
        env.append_paths("CLAUDE_CODE_PLUGIN_DIRS", &[PathBuf::from("/store/a")])
            .unwrap();
        let command = env.command(PathBuf::from("/usr/bin/claude-desktop"));
        assert_eq!(command.get_program(), "/usr/bin/claude-desktop");
        let mut vars = envs(&command);
        vars.sort();
        let inherited = env::var_os("CLAUDE_CODE_PLUGIN_DIRS").filter(|v| !v.is_empty());
        let plugin_dirs = env::join_paths(
            inherited
                .iter()
                .map(PathBuf::from)
                .chain([PathBuf::from("/store/a")]),
        )
        .unwrap();
        let mut expected = vec![
            ("CLAUDE_CODE_OAUTH_TOKEN".to_owned(), Some("tok".to_owned())),
            (
                "CLAUDE_CODE_PLUGIN_DIRS".to_owned(),
                Some(plugin_dirs.to_string_lossy().into_owned()),
            ),
            (
                "CLAUDE_CONFIG_DIR".to_owned(),
                Some("/profiles/Work".to_owned()),
            ),
            ("DISABLE_AUTOUPDATER".to_owned(), Some("1".to_owned())),
            ("EXTRA".to_owned(), Some("1".to_owned())),
            ("FORCE_AUTOUPDATE_PLUGINS".to_owned(), Some("1".to_owned())),
        ];
        expected.sort();
        assert_eq!(vars, expected);
    }

    #[test]
    fn pass_through_environment_sets_nothing() {
        let mut env = ProfileEnv::pass_through();
        assert!(!env.is_isolated());
        env.append_paths("CLAUDE_CODE_PLUGIN_DIRS", &[PathBuf::from("/store/a")])
            .unwrap();
        assert!(envs(&env.command(PathBuf::from("/bin/claude"))).is_empty());
    }

    #[test]
    fn path_lists_follow_a_nonempty_inherited_value() {
        let paths = [PathBuf::from("/a"), PathBuf::from("/b")];
        let sep = if cfg!(windows) { ";" } else { ":" };
        assert_eq!(
            path_list(Some("/x".into()), &paths).unwrap(),
            OsString::from(format!("/x{sep}/a{sep}/b"))
        );
        assert_eq!(
            path_list(Some("".into()), &paths).unwrap(),
            OsString::from(format!("/a{sep}/b"))
        );
        assert_eq!(path_list(None, &paths[..1]).unwrap(), OsString::from("/a"));
        assert!(path_list(None, &[PathBuf::from(format!("/a{sep}b"))]).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn program_resolution_selects_the_first_executable_on_path() {
        use std::os::unix::fs::PermissionsExt;
        let base = env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("roost-resolve-{}", platform::random_id().unwrap()));
        let (first, second) = (base.join("first"), base.join("second"));
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        std::fs::write(first.join("claude-desktop"), "").unwrap();
        std::fs::set_permissions(
            first.join("claude-desktop"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        std::fs::write(second.join("claude-desktop"), "").unwrap();
        std::fs::set_permissions(
            second.join("claude-desktop"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let path = env::join_paths([&first, &second]).unwrap();
        assert_eq!(
            resolve_in("claude-desktop", Some(path.clone())).unwrap(),
            second.join("claude-desktop")
        );
        let missing = resolve_in("claude", Some(path)).err().unwrap();
        assert_eq!(missing.code, "claude_unavailable");
        assert!(missing.message.contains("claude"));
        assert_eq!(
            resolve_in("claude", None).err().unwrap().code,
            "claude_unavailable"
        );
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn every_auth_control_conflicts_only_when_nonempty() {
        assert_eq!(AUTH_ENV.len(), 42);
        for &name in AUTH_ENV {
            for value in ["0", "false", " ", "nonempty"] {
                assert_eq!(
                    auth_conflicts(|key| (key == name).then(|| OsString::from(value))),
                    [name]
                );
            }
            assert!(auth_conflicts(|key| (key == name).then(OsString::new)).is_empty());
        }
        assert!(
            auth_conflicts(
                |key| (key == "AWS_SECRET_ACCESS_KEY").then(|| OsString::from("ambient"))
            )
            .is_empty()
        );
    }

    #[test]
    fn recognizer_ignores_undocumented_and_secret_fields() {
        let registration = registration();
        let native = json!({"authMethod":"oauth_token", "configDirectory":registration.directory, "email":"secret", "accessToken":"secret", "loggedIn":false}).to_string();
        let status = recognized_status(native.as_bytes(), Some(0), &registration).unwrap();
        assert_eq!(status.exit_code, 0);
        assert!(status.warnings.is_empty());
        assert_eq!(
            status.data,
            json!({"name":"Work", "kind":"owned", "reported_logged_in":true, "auth_method":"oauth_token", "config_directory":registration.directory, "scope":"isolated"})
        );
        let status =
            recognized_status(br#"{"authMethod":"none"}"#, Some(1), &registration).unwrap();
        assert_eq!(status.exit_code, 3);
        assert_eq!(status.data["config_directory"], Value::Null);
        assert_eq!(status.warnings.len(), 1);
        let status = recognized_status(
            json!({"authMethod":"new_native_method", "configDirectory":registration.directory})
                .to_string()
                .as_bytes(),
            Some(0),
            &registration,
        )
        .unwrap();
        assert_eq!(status.data["auth_method"], Value::Null);
        assert_eq!(status.warnings.len(), 1);
    }

    #[test]
    fn recognizer_rejects_unrecognized_or_contradictory_status() {
        for (bytes, exit) in [
            (&b"not-json"[..], Some(0)),
            (b"[]", Some(0)),
            (b"{}", Some(0)),
            (br#"{"loggedIn":true}"#, Some(0)),
            (br#"{"authMethod":4}"#, Some(0)),
            (br#"{"authMethod":"none"}"#, Some(0)),
            (br#"{"authMethod":"api_key"}"#, Some(1)),
            (br#"{"authMethod":"claude.ai"}"#, Some(2)),
            (br#"{"configDirectory":"relative"}"#, Some(0)),
            (br#"{"configDirectory":""}"#, Some(0)),
            (br#"{"configDirectory":"/line\nbreak"}"#, Some(0)),
        ] {
            assert_eq!(
                recognized_status(bytes, exit, &registration())
                    .err()
                    .unwrap()
                    .code,
                "auth_status"
            );
        }
    }

    #[test]
    fn version_floor_is_exact() {
        assert_eq!(
            parse_version(b"2.1.280 (Claude Code)\n").as_deref(),
            Some("2.1.280")
        );
        assert_eq!(parse_version(b"3.0.0"), Some("3.0.0".to_owned()));
        for bytes in [
            &b"2.1.279"[..],
            b"2.1.268",
            b"2.0.999",
            b"2.1",
            b"text 2.1.280",
            b"2.1.280-beta",
            b"2.1.280\nsecret",
        ] {
            assert!(parse_version(bytes).is_none());
        }
    }

    #[cfg(unix)]
    #[test]
    fn posix_quotes_and_bound_transport_preserve_literal_tail() {
        let values = [
            "",
            "plain",
            "a b",
            "a'b",
            "$HOME",
            "$(touch never)",
            "!%^&()",
            "double\"quote",
            "line\nbreak",
            "終",
        ];
        let literals = values
            .iter()
            .map(|s| quoted_sh(s))
            .collect::<Vec<_>>()
            .join(" ");
        let output = Command::new("sh")
            .args([
                "-c",
                &format!("printf '%s\\000' {literals} \"$@\""),
                "test",
                "--",
                "",
                "--allow-auth-env",
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        let expected = values
            .iter()
            .copied()
            .chain(["--", "", "--allow-auth-env"])
            .flat_map(|s| s.as_bytes().iter().copied().chain([0]))
            .collect::<Vec<_>>();
        assert_eq!(output.stdout, expected);
        let mut registration = registration();
        registration.launcher_binding.as_mut().unwrap().executable =
            PathBuf::from("/manager/a'b roost");
        let templates = templates(Path::new("/root/a'b"), &"a".repeat(32), &registration).unwrap();
        let script = std::str::from_utf8(&templates[0].1).unwrap();
        assert_eq!(templates[0].0, Path::new("/root/a'b/bin/roost-Work"));
        assert!(
            script.contains("exec '/manager/a'\\''b roost' '__roost_launch_v1' '/root/a'\\''b'")
        );
        assert!(script.ends_with("'--' \"$@\"\n"));
        assert!(!script.contains("ROOST_DIR="));
    }

    #[cfg(unix)]
    #[test]
    fn captured_probes_drain_both_streams_and_bound_failure() {
        let mut command = Command::new("sh");
        command.args(["-c", "i=0; while [ $i -lt 2000 ]; do printf 'stdout-0123456789'; printf 'stderr-0123456789' >&2; i=$((i+1)); done"]);
        let probe = capture(command, "auth_status", Duration::from_secs(3), 100_000).unwrap();
        assert!(probe.exit.success());
        assert_eq!(probe.stdout.len(), 17 * 2000);
        let mut command = Command::new("sh");
        command.args(["-c", "while :; do printf 'too-large-secret'; done"]);
        let error = capture(command, "auth_status", Duration::from_secs(2), 100)
            .err()
            .unwrap();
        assert!(error.message.contains("output limit"));
        assert!(!error.message.contains("secret"));
        let mut command = Command::new("sh");
        command.args(["-c", "while :; do printf 'private-diagnostics' >&2; done"]);
        let error = capture(command, "auth_status", Duration::from_secs(2), 100)
            .err()
            .unwrap();
        assert!(error.message.contains("diagnostic limit"));
        assert!(!error.message.contains("private"));
        let mut command = Command::new("sh");
        command.args(["-c", "exec sleep 2"]);
        let start = Instant::now();
        let error = capture(command, "auth_status", Duration::from_millis(30), 100)
            .err()
            .unwrap();
        assert!(error.message.contains("deadline"));
        assert!(start.elapsed() < Duration::from_secs(1));
        // A grandchild retaining a pipe cannot turn cleanup into an unbounded join.
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 0.2 & exit 0"]);
        assert!(capture(command, "auth_status", Duration::from_millis(30), 100).is_err());
    }
}
