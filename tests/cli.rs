#![cfg(unix)]
mod common;
use common::{Fixture, parsed, private};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

const FAKE: &str = r#"#!/usr/bin/python3
import json,os,sys,time
args=sys.argv[1:]
if args==['--version']:
    print(os.environ.get('FAKE_CLAUDE_VERSION','2.1.280 (Claude Code)'));sys.exit(0)
mode=os.environ.get('FAKE_CLAUDE_MODE','')
if mode=='slow':
    ready=os.environ.get('FAKE_CLAUDE_READY')
    if ready:open(ready,'w').close()
    time.sleep(30)
if mode=='overflow':print('X'*(1024*1024+1));sys.exit(1)
if args==['auth','status']:
    if mode=='malformed':print('secret status garbage');sys.exit(1)
    if mode=='unknown':print(json.dumps({'accessToken':'fake-secret'}));sys.exit(1)
    out={'authMethod':'none' if mode=='logout' else 'claude.ai','configDirectory':os.environ.get('CLAUDE_CONFIG_DIR',os.path.join(os.environ['HOME'],'.claude')),'accessToken':'fake-secret','email':'undocumented@example.invalid'}
    print(json.dumps(out));sys.exit(1 if mode=='logout' else 0)
print(json.dumps({'arguments':args,'env':dict(os.environ),'cwd':os.getcwd(),'stdin_tty':sys.stdin.isatty()}))
sys.exit(int(os.environ.get('FAKE_CLAUDE_EXIT','0')))
"#;

/// Never the real app: records argv/env/stdio per launch, writes Electron-like data
/// into --user-data-dir unless told to ignore it, then exits or lingers (`long`;
/// with FAKE_DESKTOP_LOCK it holds a live SingletonLock meanwhile).
const FAKE_DESKTOP: &str = r#"#!/usr/bin/python3
import json,os,socket,sys,time
args=sys.argv[1:]
record={'arguments':args,'env':dict(os.environ),'session_leader':os.getsid(0)==os.getpid(),
        'stdin':os.readlink('/proc/self/fd/0'),'stdout':os.readlink('/proc/self/fd/1'),'stderr':os.readlink('/proc/self/fd/2')}
with open(os.environ['FAKE_DESKTOP_RECORD'],'a') as f:f.write(json.dumps(record)+'\n')
data=[a.split('=',1)[1] for a in args if a.startswith('--user-data-dir=')]
if data and not os.environ.get('FAKE_DESKTOP_IGNORE_DIR'):open(os.path.join(data[0],'Preferences'),'w').close()
print('chromium console noise');print('[ERROR] gpu noise',file=sys.stderr)
if os.environ.get('FAKE_DESKTOP_MODE')=='long':
    # Like Electron: a SingletonLock link naming HOST-PID while running.
    # Without --user-data-dir: the conventional folder, when it exists.
    folder=data[0] if data else os.path.join(os.environ.get('XDG_CONFIG_HOME') or os.path.join(os.environ['HOME'],'.config'),'Claude')
    lock=os.path.join(folder,'SingletonLock') if os.path.isdir(folder) and os.environ.get('FAKE_DESKTOP_LOCK') else None
    if lock:os.symlink('%s-%d'%(socket.gethostname(),os.getpid()),lock)
    time.sleep(8)
    try:
        if lock:os.remove(lock)
    except OSError:pass
sys.exit(int(os.environ.get('FAKE_DESKTOP_EXIT','0')))
"#;

impl Fixture {
    fn new() -> Self {
        let f = Fixture::with_fakes("cli", &[("claude", FAKE), ("claude-desktop", FAKE_DESKTOP)]);
        let record = f.path.join("desktop.jsonl");
        f.with_var("FAKE_DESKTOP_RECORD", record)
    }
    fn add(&self, name: &str) {
        self.ok(&["add", name]);
    }
    /// Every fake `claude-desktop` launch so far, oldest first.
    fn desktop_launches(&self) -> Vec<Value> {
        fs::read_to_string(self.path.join("desktop.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
    fn registration_id(&self, name: &str) -> String {
        let registry: Value =
            serde_json::from_slice(&fs::read(self.root.join("registry.json")).unwrap()).unwrap();
        registry["registrations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == name)
            .unwrap()["registration_id"]
            .as_str()
            .unwrap()
            .to_owned()
    }
    fn desktop_folder(&self, name: &str) -> PathBuf {
        self.root.join("desktop").join(self.registration_id(name))
    }
}
fn input(command: &mut Command, bytes: &[u8]) -> Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn help_version_and_usage_do_not_touch_storage() {
    let f = Fixture::new();
    assert!(
        String::from_utf8(f.ok(&["--help"]).stdout)
            .unwrap()
            .contains("auth login")
    );
    assert_eq!(f.ok(&["--version"]).stdout, b"0.1.0\n");
    // Bare roost without storage names roost add and creates nothing.
    let bare = f.run(&[]);
    assert_eq!(bare.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&bare.stderr).contains("roost add NAME"));
    assert!(
        f.command()
            .env("ROOST_DIR", "")
            .arg("--help")
            .output()
            .unwrap()
            .status
            .success()
    );
    for args in [
        vec!["add", "CON"],
        vec!["add", "bad name"],
        vec!["add", "Work", "--yes"],
        vec!["token", "Work", "--stdin", "--clear"],
        vec!["unknown"],
        vec!["--allow-auth-env", "--help"],
        vec!["--allow-auth-env", "Work"],
        vec!["--bogus"],
        vec!["switch"],
        vec!["switch", "--forget", "Work"],
        vec!["switch", "--no-launch", "--forget"],
        vec!["switch", "--no-launch", "--allow-auth-env", "Work"],
        vec!["switch", "--allow-auth-env", "--forget"],
        vec!["switch", "--no-launch", "Work", "--"],
        vec!["switch", "--no-launch", "Work", "-p"],
        vec!["switch", "--no-launch", "--no-launch", "Work"],
        vec!["switch", "--unknown", "Work"],
        vec!["switch", "CON"],
    ] {
        assert_eq!(f.run(&args).status.code(), Some(2), "{args:?}");
    }
    assert!(!f.root.exists());
    let empty = parsed(&f.ok(&["list", "--json"]));
    assert_eq!(empty["data"]["profiles"], json!([]));
    assert!(!f.root.exists());
}

#[test]
fn human_list_is_a_plain_aligned_table_when_piped() {
    let f = Fixture::new();
    f.add("Work");
    f.ok(&["add", "personal", "--link-default"]);
    let mut command = f.command();
    command.env("NO_COLOR", "");
    let out = command.arg("ls").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(!text.contains('\x1b'), "{text}");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines,
        [
            "  Profile   Kind           Token  Launchers  Sets  Last used",
            "  personal  default_alias  –      ready      —     never",
            "  Work      owned          –      ready      —     never",
            "",
            "○ 2 profiles · 0 upstream · @ selected here · hidden: Path",
        ]
    );
}

#[test]
fn human_list_shows_token_launcher_state_and_upstream_count() {
    let f = Fixture::new();
    f.add("Work");
    f.add("Gone");
    f.ok(&["remove", "Gone"]);
    let old = f.upstream("Old");
    f.ok(&["register", "Old", "--path", old.to_str().unwrap()]);
    let set = input(
        f.command().args(["token", "Work", "--stdin"]),
        b"fake-token\n",
    );
    assert!(set.status.success());
    let text = String::from_utf8(f.ok(&["ls", "--retained"]).stdout).unwrap();
    assert_eq!(
        text.lines().collect::<Vec<_>>(),
        [
            "  Profile  Kind      Token  Launchers     Sets  Last used",
            "  Gone     owned     –      not_required  —     never",
            "  Old      upstream  –      ready         —     never",
            "  Work     owned     ✓      ready         —     never",
            "",
            "○ 3 profiles · 1 upstream · @ selected here · hidden: Path",
        ]
    );
}

#[test]
fn json_list_records_carry_every_spec_key_without_probes_by_default() {
    let f = Fixture::new();
    f.add("Work");
    let value = parsed(&f.ok(&["list", "--json"]));
    let data = value["data"].as_object().unwrap();
    let mut keys: Vec<&str> = data.keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(keys, ["profiles", "project"]);
    let record = &value["data"]["profiles"][0];
    let mut keys: Vec<&str> = record
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "directory",
            "kind",
            "last_used",
            "launchers",
            "most_recent",
            "name",
            "probe",
            "selected",
            "sets",
            "state",
            "token_present"
        ]
    );
    assert_eq!(record["probe"], Value::Null);
}

#[test]
fn full_list_probes_isolated_active_rows_only() {
    let f = Fixture::new();
    f.add("Work");
    f.add("Gone");
    f.ok(&["remove", "Gone"]);
    f.ok(&["add", "personal", "--link-default"]);
    let work = f.root.join("profiles/Work");
    let out = f.ok(&["list", "--full", "--retained", "--json"]);
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("fake-secret"),
        "raw status leaked"
    );
    let value = parsed(&out);
    let profiles = &value["data"]["profiles"];
    assert_eq!(profiles[0]["name"], "Gone");
    assert_eq!(profiles[0]["probe"], Value::Null);
    assert_eq!(profiles[1]["name"], "personal");
    assert_eq!(profiles[1]["probe"], Value::Null);
    assert_eq!(
        profiles[2]["probe"],
        json!({"reported_logged_in":true,"auth_method":"claude.ai","config_directory":work.to_str().unwrap()})
    );
    assert_eq!(value["warnings"], json!([]));

    let text = String::from_utf8(f.ok(&["ls", "--full"]).stdout).unwrap();
    assert_eq!(
        text.lines().collect::<Vec<_>>(),
        [
            "  Profile   Kind           Token  Launchers  Sets  Last used  Login  Auth       Claude dir",
            "  personal  default_alias  –      ready      —     never      —      —          —",
            &format!(
                "  Work      owned          –      ready      —     never      yes    claude.ai  {}",
                work.display()
            ),
            "",
            "○ 2 profiles · 0 upstream · @ selected here · hidden: Path",
        ]
    );
}

#[test]
fn full_list_probe_failure_is_unknown_with_a_safe_warning() {
    let f = Fixture::new();
    f.add("Work");
    let out = f
        .command()
        .args(["list", "--full", "--json"])
        .env("FAKE_CLAUDE_MODE", "unknown")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(!String::from_utf8_lossy(&out.stdout).contains("fake-secret"));
    let value = parsed(&out);
    assert_eq!(
        value["data"]["profiles"][0]["probe"],
        json!({"reported_logged_in":null,"auth_method":null,"config_directory":null})
    );
    let warnings = value["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].as_str().unwrap().contains("Work"),
        "{warnings:?}"
    );

    let out = f
        .command()
        .args(["ls", "--full"])
        .env("FAKE_CLAUDE_MODE", "logout")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        text.lines()
            .nth(1)
            .unwrap()
            .split_whitespace()
            .collect::<Vec<_>>()[6..8],
        ["no", "none"]
    );
    let out = f
        .command()
        .args(["ls", "--full"])
        .env("FAKE_CLAUDE_MODE", "malformed")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        text.lines()
            .nth(1)
            .unwrap()
            .split_whitespace()
            .collect::<Vec<_>>()[6..],
        ["?", "?", "?"]
    );
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.starts_with("warning: "), "{stderr}");
    assert!(!stderr.contains("garbage"), "{stderr}");
}

#[test]
fn terminal_list_is_colored_and_hides_columns_to_fit_the_width() {
    let f = Fixture::new();
    f.add("Work");
    let script = r#"
import errno,fcntl,json,os,pty,select,struct,sys,termios,time
exe,root,binpath,home,no_color=sys.argv[1:]
pid,fd=pty.fork()
if pid==0:
    fcntl.ioctl(1,termios.TIOCSWINSZ,struct.pack('HHHH',24,40,0,0))
    env={'ROOST_DIR':root,'HOME':home,'PATH':binpath+':/usr/bin:/bin'}
    if no_color:env['NO_COLOR']=no_color
    os.execve(exe,[exe,'ls','--full'],env)
data=b'';deadline=time.monotonic()+10
while time.monotonic()<deadline:
    if not select.select([fd],[],[],0.1)[0]:continue
    try:chunk=os.read(fd,65536)
    except OSError as e:
        if e.errno==errno.EIO:break
        raise
    if not chunk:break
    data+=chunk
else:
    os.kill(pid,9);raise RuntimeError('PTY deadline exceeded')
os.close(fd);_,status=os.waitpid(pid,0)
assert os.waitstatus_to_exitcode(status)==0,data.decode(errors='replace')
print(json.dumps(data.decode().replace('\r\n','\n')))
"#;
    let run = |no_color: &str| -> String {
        let out = Command::new("/usr/bin/python3")
            .args([
                "-c",
                script,
                env!("CARGO_BIN_EXE_roost"),
                f.root.to_str().unwrap(),
                f.bin.to_str().unwrap(),
                f.home.to_str().unwrap(),
                no_color,
            ])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        parsed(&out).as_str().unwrap().to_owned()
    };
    let colored = run("");
    assert!(colored.contains("\x1b[1mProfile\x1b[0m"), "{colored:?}");
    let plain = run("1");
    assert_eq!(
        plain.lines().collect::<Vec<_>>(),
        [
            "  Profile  Kind   Token  Launchers  Sets",
            "  Work     owned  –      ready      —",
            "",
            "○ 1 profile · 0 upstream · @ selected here · hidden: Path, Last used, Login, Auth, Claude dir",
        ]
    );
}

#[test]
fn full_list_probes_run_in_parallel_without_holding_the_lock() {
    use std::time::{Duration, Instant};
    let f = Fixture::new();
    f.add("One");
    f.add("Two");
    let ready = f.path.join("ready");
    let started = Instant::now();
    let child = f
        .command()
        .args(["list", "--full", "--json"])
        .env("FAKE_CLAUDE_MODE", "slow")
        .env("FAKE_CLAUDE_READY", &ready)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    while !ready.exists() && started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(ready.exists(), "probe never started");
    let added = Instant::now();
    f.add("Three");
    assert!(added.elapsed() < Duration::from_secs(3));
    let out = child.wait_with_output().unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(18),
        "probes ran serially"
    );
    assert_eq!(out.status.code(), Some(0));
    let value = parsed(&out);
    assert_eq!(value["warnings"].as_array().unwrap().len(), 2);
    for record in value["data"]["profiles"].as_array().unwrap() {
        assert_eq!(record["probe"]["reported_logged_in"], Value::Null);
    }
}

#[test]
fn full_is_accepted_only_by_list() {
    let f = Fixture::new();
    assert_eq!(f.run(&["status", "x", "--full"]).status.code(), Some(2));
    assert_eq!(f.run(&["doctor", "--full"]).status.code(), Some(2));
}

#[test]
fn lifecycle_retains_tokens_and_requires_explicit_reuse() {
    let f = Fixture::new();
    f.add("Work");
    assert_eq!(
        f.ok(&["where", "work"]).stdout,
        format!("{}\n", f.root.join("profiles/Work").display()).as_bytes()
    );
    assert!(!f.run(&["add", "work"]).status.success());
    let set = input(
        f.command().args(["token", "work", "--stdin"]),
        b"  fake-oauth-token\n",
    );
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );
    let token = f.root.join("profiles/Work/.roost-token");
    assert_eq!(
        fs::metadata(&token).unwrap().permissions().mode() & 0o777,
        0o600
    );
    f.ok(&["remove", "work"]);
    assert!(token.exists());
    assert!(!f.root.join("bin/roost-Work").exists());
    let records = parsed(&f.ok(&["list", "--retained", "--json"]));
    assert_eq!(records["data"]["profiles"][0]["state"], "retained");
    assert!(!f.run(&["run", "work"]).status.success());
    f.ok(&["reuse", "work"]);
    assert!(f.root.join("bin/roost-Work").exists());
    f.ok(&["token", "work", "--clear"]);
    assert!(!token.exists());
    f.ok(&["remove", "work", "--purge", "--yes"]);
    assert!(!f.root.join("profiles/Work").exists());
}

#[test]
fn run_and_bound_launcher_preserve_opaque_tail_and_correct_root() {
    let f = Fixture::new();
    f.add("Work");
    let out = f.ok(&[
        "run",
        "work",
        "--",
        "--",
        "--help",
        "--allow-auth-env",
        "",
        "quote\"value",
        "space value",
        "line\nbreak",
    ]);
    let probe = parsed(&out);
    assert_eq!(
        probe["arguments"],
        json!([
            "--",
            "--help",
            "--allow-auth-env",
            "",
            "quote\"value",
            "space value",
            "line\nbreak"
        ])
    );
    assert_eq!(
        probe["env"]["CLAUDE_CONFIG_DIR"],
        f.root.join("profiles/Work").to_str().unwrap()
    );
    assert_eq!(probe["env"]["DISABLE_AUTOUPDATER"], "1");
    let mut launcher = Command::new(f.root.join("bin/roost-Work"));
    launcher
        .env_clear()
        .env("HOME", &f.home)
        .env("PATH", format!("{}:/usr/bin:/bin", f.bin.display()))
        .env("ROOST_DIR", f.path.join("wrong-root"))
        .current_dir(&f.path)
        .args(["--", "--help", ""]);
    let result = launcher.output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let probe = parsed(&result);
    assert_eq!(probe["arguments"], json!(["--", "--help", ""]));
    assert_eq!(
        probe["env"]["ROOST_DIR"],
        f.path.join("wrong-root").to_str().unwrap()
    );
    assert_eq!(
        probe["env"]["CLAUDE_CONFIG_DIR"],
        f.root.join("profiles/Work").to_str().unwrap()
    );
}

#[test]
fn default_alias_preserves_auth_and_update_environment() {
    let f = Fixture::new();
    f.ok(&["add", "default", "--link-default"]);
    let out = f
        .command()
        .args(["run", "default", "--name", "native"])
        .env("ANTHROPIC_API_KEY", "fake-key")
        .env("CLAUDE_CONFIG_DIR", "")
        .env("DISABLE_AUTOUPDATER", "caller")
        .output()
        .unwrap();
    assert!(out.status.success());
    let probe = parsed(&out);
    assert_eq!(probe["env"]["ANTHROPIC_API_KEY"], "fake-key");
    assert_eq!(probe["env"]["CLAUDE_CONFIG_DIR"], "");
    assert_eq!(probe["env"]["DISABLE_AUTOUPDATER"], "caller");
    assert!(probe["env"].get("FORCE_AUTOUPDATE_PLUGINS").is_none());
    let out = f
        .command()
        .arg("update")
        .env("CLAUDE_CONFIG_DIR", "caller-context")
        .env("DISABLE_AUTOUPDATER", "caller")
        .output()
        .unwrap();
    assert!(out.status.success());
    let probe = parsed(&out);
    assert_eq!(probe["arguments"], json!(["update"]));
    assert_eq!(probe["env"]["CLAUDE_CONFIG_DIR"], "caller-context");
    assert_eq!(probe["env"]["DISABLE_AUTOUPDATER"], "caller");
    assert!(
        !f.run(&["remove", "default", "--purge", "--yes"])
            .status
            .success()
    );
}

#[test]
fn auth_conflicts_are_names_only_override_suppresses_owned_token() {
    let f = Fixture::new();
    f.add("Work");
    let set = input(
        f.command().args(["token", "Work", "--stdin"]),
        b"fake-stored-token",
    );
    assert!(set.status.success());
    let out = f
        .command()
        .args(["run", "Work"])
        .env("ANTHROPIC_API_KEY", "fake-secret-key")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("ANTHROPIC_API_KEY"));
    assert!(!stderr.contains("fake-secret-key"));
    let out = f
        .command()
        .args(["run", "--allow-auth-env", "Work"])
        .env("ANTHROPIC_API_KEY", "fake-key")
        .output()
        .unwrap();
    assert!(out.status.success());
    let probe = parsed(&out);
    assert_eq!(probe["env"]["ANTHROPIC_API_KEY"], "fake-key");
    assert!(probe["env"].get("CLAUDE_CODE_OAUTH_TOKEN").is_none());
    let native = parsed(&f.ok(&["run", "Work"]));
    assert_eq!(
        native["env"]["CLAUDE_CODE_OAUTH_TOKEN"],
        "fake-stored-token"
    );
}

#[test]
fn upstream_is_borrowed_and_unsafe_token_blocks_even_override() {
    let f = Fixture::new();
    let upstream = f.upstream("original");
    let token = upstream.join(".ccm-oauth-token");
    fs::write(&token, "fake-borrowed-token").unwrap();
    fs::set_permissions(&token, fs::Permissions::from_mode(0o600)).unwrap();
    f.ok(&["register", "Work", "--path", upstream.to_str().unwrap()]);
    let probe = parsed(&f.ok(&["run", "Work"]));
    assert_eq!(
        probe["env"]["CLAUDE_CONFIG_DIR"],
        upstream.to_str().unwrap()
    );
    assert_eq!(
        probe["env"]["CLAUDE_CODE_OAUTH_TOKEN"],
        "fake-borrowed-token"
    );
    assert!(!upstream.join(".roost-profile.json").exists());
    assert!(!f.run(&["token", "Work", "--clear"]).status.success());
    assert!(
        !f.run(&["remove", "Work", "--purge", "--yes"])
            .status
            .success()
    );
    fs::set_permissions(&token, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(!f.run(&["run", "--allow-auth-env", "Work"]).status.success());
    assert_eq!(
        fs::metadata(&token).unwrap().permissions().mode() & 0o777,
        0o644
    );
    f.ok(&["remove", "Work"]);
    assert_eq!(fs::read_to_string(&token).unwrap(), "fake-borrowed-token");
}

#[test]
fn status_is_allowlisted_and_logged_out_differs_from_unknown() {
    let f = Fixture::new();
    f.add("Work");
    let out = f.ok(&["status", "Work", "--json"]);
    let value = parsed(&out);
    assert_eq!(value["data"]["reported_logged_in"], true);
    assert_eq!(value["data"].as_object().unwrap().len(), 6);
    assert!(
        !String::from_utf8(out.stdout)
            .unwrap()
            .contains("fake-secret")
    );
    let out = f
        .command()
        .args(["status", "Work", "--json"])
        .env("FAKE_CLAUDE_MODE", "logout")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(parsed(&out)["data"]["reported_logged_in"], false);
    for mode in ["malformed", "unknown", "overflow"] {
        let out = f
            .command()
            .args(["status", "Work", "--json"])
            .env("FAKE_CLAUDE_MODE", mode)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        let value = parsed(&out);
        assert_eq!(value["data"]["reported_logged_in"], Value::Null);
        assert_eq!(value["error"]["code"], "auth_status");
        assert!(
            !String::from_utf8(out.stdout)
                .unwrap()
                .contains("fake-secret")
        );
    }
}

#[test]
fn safe_copy_excludes_credentials_and_never_traverses_links() {
    let f = Fixture::new();
    let source = f.path.join("seed");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("settings.json"), "{}").unwrap();
    fs::write(source.join(".credentials.json"), "fake-secret").unwrap();
    fs::create_dir(source.join("nested")).unwrap();
    fs::write(source.join("nested/.claude.json.backup"), "fake-secret").unwrap();
    fs::create_dir(source.join("cache")).unwrap();
    fs::write(source.join("cache/skip"), "skip").unwrap();
    std::os::unix::fs::symlink(&f.home, source.join("linked")).unwrap();
    fs::hard_link(source.join("settings.json"), source.join("hardlinked")).unwrap();
    f.ok(&[
        "add",
        "Work",
        "--copy-default",
        "--source",
        source.to_str().unwrap(),
        "--yes",
    ]);
    let copied = f.root.join("profiles/Work");
    assert!(!copied.join(".credentials.json").exists());
    assert!(!copied.join("nested/.claude.json.backup").exists());
    assert!(!copied.join("cache").exists());
    assert!(!copied.join("linked").exists());
    assert!(!copied.join("hardlinked").exists());
}

#[test]
fn launcher_collision_foreign_storage_and_symlinks_fail_closed() {
    let f = Fixture::new();
    f.add("Work");
    let launcher = f.root.join("bin/roost-Work");
    fs::write(&launcher, "foreign-launcher\n").unwrap();
    assert!(!f.run(&["reuse", "Work"]).status.success());
    assert!(!f.run(&["remove", "Work"]).status.success());
    assert_eq!(fs::read_to_string(&launcher).unwrap(), "foreign-launcher\n");
    let other = Fixture::new();
    fs::create_dir(&other.root).unwrap();
    private(&other.root);
    fs::write(other.root.join("foreign"), "preserve").unwrap();
    assert!(!other.run(&["add", "Work"]).status.success());
    assert_eq!(
        fs::read_to_string(other.root.join("foreign")).unwrap(),
        "preserve"
    );
    let link = Fixture::new();
    std::os::unix::fs::symlink(&link.home, &link.root).unwrap();
    assert!(!link.run(&["add", "Work"]).status.success());
    assert!(!link.home.join(".roost-root.json").exists());
}

#[test]
fn purge_unlinks_contained_symlink_without_deleting_target() {
    let f = Fixture::new();
    f.add("Work");
    let outside = f.path.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep"), "preserve").unwrap();
    std::os::unix::fs::symlink(&outside, f.root.join("profiles/Work/linked")).unwrap();
    f.ok(&["remove", "Work", "--purge", "--yes"]);
    assert_eq!(
        fs::read_to_string(outside.join("keep")).unwrap(),
        "preserve"
    );
}

#[test]
fn fish_path_apply_is_scoped_idempotent_and_refuses_foreign_snippet() {
    let f = Fixture::new();
    let config = f.home.join(".config/fish/conf.d");
    fs::create_dir_all(&config).unwrap();
    private(&f.home.join(".config"));
    private(&f.home.join(".config/fish"));
    private(&config);
    let out = f
        .command()
        .args(["setup-path", "--shell", "fish", "--apply"])
        .env("XDG_CONFIG_HOME", f.home.join(".config"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let snippet = config.join("roost-path.fish");
    let before = fs::read(&snippet).unwrap();
    assert!(
        String::from_utf8(before.clone())
            .unwrap()
            .contains("fish_add_path --path")
    );
    let out = f
        .command()
        .args(["setup-path", "--shell", "fish", "--apply"])
        .env("XDG_CONFIG_HOME", f.home.join(".config"))
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(fs::read(&snippet).unwrap(), before);
    fs::write(&snippet, "# foreign\n").unwrap();
    let out = f
        .command()
        .args(["setup-path", "--shell", "fish", "--apply"])
        .env("XDG_CONFIG_HOME", f.home.join(".config"))
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert_eq!(fs::read_to_string(&snippet).unwrap(), "# foreign\n");
    assert!(!f.root.exists());
}

#[test]
fn child_numeric_result_is_preserved_and_old_claude_refused() {
    let f = Fixture::new();
    f.add("Work");
    let out = f
        .command()
        .args(["run", "Work"])
        .env("FAKE_CLAUDE_EXIT", "42")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(42));
    let out = f
        .command()
        .args(["run", "Work"])
        .env("FAKE_CLAUDE_VERSION", "2.1.279 (Claude Code)")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("claude_unsupported"));
    assert!(stderr.contains("Claude Code 2.1.280 or later"));
}

#[test]
fn override_checks_token_metadata_without_reading_unused_contents() {
    let f = Fixture::new();
    f.add("Work");
    let token = f.root.join("profiles/Work/.roost-token");
    fs::write(&token, "not a valid token").unwrap();
    fs::set_permissions(&token, fs::Permissions::from_mode(0o600)).unwrap();
    let out = f.ok(&["run", "--allow-auth-env", "Work"]);
    assert!(parsed(&out)["env"].get("CLAUDE_CODE_OAUTH_TOKEN").is_none());
    assert!(!f.run(&["run", "Work"]).status.success());
    let listed = parsed(&f.ok(&["list", "--json"]));
    assert_eq!(listed["data"]["profiles"][0]["token_present"], true);
    fs::set_permissions(&token, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(!f.run(&["run", "--allow-auth-env", "Work"]).status.success());
}

#[test]
fn long_running_native_child_releases_lock_and_receives_direct_signal() {
    use std::{
        os::unix::process::ExitStatusExt,
        time::{Duration, Instant},
    };
    let f = Fixture::new();
    f.add("Work");
    let ready = f.path.join("child-ready");
    let mut child = f
        .command()
        .args(["run", "Work"])
        .env("FAKE_CLAUDE_MODE", "slow")
        .env("FAKE_CLAUDE_READY", &ready)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    if !ready.exists() {
        let _ = child.kill();
        panic!("fake native child did not reach handoff");
    }
    let started = Instant::now();
    f.add("Personal");
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(
        Command::new("/bin/kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(child.wait().unwrap().signal(), Some(15));
}

#[test]
fn native_pty_preserves_tty_and_hidden_token_input() {
    let f = Fixture::new();
    f.add("Work");
    let script = r#"
import errno,json,os,pty,select,sys,time
exe,root,binpath,home=sys.argv[1:]
def run(arguments,secret=None):
    pid,fd=pty.fork()
    if pid==0:
        os.execve(exe,[exe]+arguments,{'ROOST_DIR':root,'HOME':home,'PATH':binpath+':/usr/bin:/bin'})
    data=b'';sent=False;deadline=time.monotonic()+10
    while time.monotonic()<deadline:
        if not select.select([fd],[],[],0.1)[0]:continue
        try:chunk=os.read(fd,65536)
        except OSError as e:
            if e.errno==errno.EIO:break
            raise
        if not chunk:break
        data+=chunk
        if secret and not sent and b'Token: ' in data:
            os.write(fd,secret+b'\n');sent=True
    else:
        os.kill(pid,9);raise RuntimeError('PTY deadline exceeded')
    os.close(fd);_,status=os.waitpid(pid,0)
    assert os.waitstatus_to_exitcode(status)==0,data.decode(errors='replace')
    return data
probe=json.loads(run(['run','Work','--pty-test']).decode())
assert probe['stdin_tty'] is True
secret=b'fake-hidden-pty-token'
assert secret not in run(['token','Work'],secret)
print(json.dumps({'tty':True,'hidden':True}))
"#;
    let out = Command::new("/usr/bin/python3")
        .args([
            "-c",
            script,
            env!("CARGO_BIN_EXE_roost"),
            f.root.to_str().unwrap(),
            f.bin.to_str().unwrap(),
            f.home.to_str().unwrap(),
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(parsed(&out), json!({"tty":true,"hidden":true}));
    assert_eq!(
        fs::read_to_string(f.root.join("profiles/Work/.roost-token")).unwrap(),
        "fake-hidden-pty-token"
    );
}

#[test]
fn prompts_refuse_replacement_profiles_before_mutation() {
    let script = r#"
import errno,json,os,pathlib,pty,select,signal,subprocess,sys,tempfile,time
exe=sys.argv[1]
results=[]
for action in ('purge','token'):
    with tempfile.TemporaryDirectory(prefix='roost-prompt-race-') as folder:
        root=pathlib.Path(folder)/'root';env={'HOME':folder,'ROOST_DIR':str(root),'PATH':'/usr/bin:/bin'}
        def manager(*args):
            result=subprocess.run([exe,*args],env=env,capture_output=True,timeout=5)
            assert result.returncode==0,result.stderr
        manager('add','Work')
        old_id=json.loads((root/'registry.json').read_text())['registrations'][0]['registration_id']
        pid,fd=pty.fork()
        if pid==0:os.execve(exe,[exe,'remove','Work','--purge'] if action=='purge' else [exe,'token','Work'],env)
        output=b'';deadline=time.monotonic()+10
        marker=b'[y/N]' if action=='purge' else b'Token: '
        while marker not in output and time.monotonic()<deadline:
            if select.select([fd],[],[],.1)[0]:output+=os.read(fd,65536)
        assert marker in output,output
        manager('remove','Work','--purge','--yes');manager('add','Work')
        new_id=json.loads((root/'registry.json').read_text())['registrations'][0]['registration_id'];assert old_id!=new_id
        os.write(fd,b'y\n' if action=='purge' else b'fake-race-token\n')
        while time.monotonic()<deadline:
            if not select.select([fd],[],[],.1)[0]:continue
            try:chunk=os.read(fd,65536)
            except OSError as e:
                if e.errno==errno.EIO:break
                raise
            if not chunk:break
            output+=chunk
        else:os.kill(pid,signal.SIGKILL);raise RuntimeError('deadline')
        os.close(fd);_,status=os.waitpid(pid,0)
        exitcode=os.waitstatus_to_exitcode(status)
        results.append({'action':action,'exit':exitcode,'replacement_preserved':(root/'profiles/Work').exists(),'replacement_token_present':(root/'profiles/Work/.roost-token').exists()})
print(json.dumps(results))
"#;
    let out = Command::new("/usr/bin/python3")
        .args(["-c", script, env!("CARGO_BIN_EXE_roost")])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        parsed(&out),
        json!([
            {"action":"purge","exit":1,"replacement_preserved":true,"replacement_token_present":false},
            {"action":"token","exit":1,"replacement_preserved":true,"replacement_token_present":false}
        ])
    );
}

/// A separate project: its own `.git` directory, so its key never depends on
/// whatever repository the temporary directory happens to sit in.
fn repo(f: &Fixture, name: &str) -> PathBuf {
    let path = f.path.join(name);
    fs::create_dir_all(path.join(".git")).unwrap();
    path
}

fn launched(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    parsed(output)
}

#[test]
fn switch_selects_per_project_and_bare_roost_launches_it() {
    let f = Fixture::new();
    f.add("Work");
    f.add("Home");
    let project = repo(&f, "app");
    let other = repo(&f, "other");
    let selected = f
        .command()
        .current_dir(&project)
        .args(["switch", "--no-launch", "work"])
        .output()
        .unwrap();
    assert!(selected.status.success(), "{selected:?}");
    let probe = launched(
        &f.command()
            .current_dir(&project)
            .args(["--", "--help", "-p", ""])
            .output()
            .unwrap(),
    );
    assert_eq!(probe["arguments"], json!(["--help", "-p", ""]));
    assert_eq!(
        probe["env"]["CLAUDE_CONFIG_DIR"],
        json!(f.root.join("profiles/Work").to_str().unwrap())
    );
    // Another directory has no selection and no terminal: usage, never a picker.
    let other = f.command().current_dir(&other).output().unwrap();
    assert_eq!(other.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&other.stderr);
    assert!(
        stderr.contains("roost switch NAME") && stderr.contains("roost run NAME"),
        "{stderr}"
    );
    // switch NAME launches like run and moves the selection.
    let probe = launched(
        &f.command()
            .current_dir(&project)
            .args(["switch", "Home", "--", "--", "x"])
            .output()
            .unwrap(),
    );
    assert_eq!(probe["arguments"], json!(["--", "x"]));
    let probe = launched(&f.command().current_dir(&project).output().unwrap());
    assert_eq!(
        probe["env"]["CLAUDE_CONFIG_DIR"],
        json!(f.root.join("profiles/Home").to_str().unwrap())
    );
    // Forget clears; forgetting again still succeeds.
    for _ in 0..2 {
        let out = f
            .command()
            .current_dir(&project)
            .args(["switch", "--forget"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
    }
    assert_eq!(
        f.command()
            .current_dir(&project)
            .output()
            .unwrap()
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn desktop_gives_owned_and_upstream_profiles_their_own_data_folder() {
    let f = Fixture::new();
    f.add("Work");
    let upstream = f.upstream("Up");
    f.ok(&["register", "Up", "--path", upstream.to_str().unwrap()]);
    let out = f.ok(&["desktop", "--foreground", "work"]);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "chromium console noise\n"
    );
    let folder = f.desktop_folder("Work");
    let launch = &f.desktop_launches()[0];
    assert_eq!(
        launch["arguments"],
        json!([format!("--user-data-dir={}", folder.display())])
    );
    assert_eq!(
        launch["env"]["CLAUDE_CONFIG_DIR"],
        f.root.join("profiles/Work").to_str().unwrap()
    );
    assert_eq!(launch["env"]["DISABLE_AUTOUPDATER"], "1");
    assert_eq!(
        fs::metadata(&folder).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let marker: Value =
        serde_json::from_slice(&fs::read(folder.join(".roost-desktop.json")).unwrap()).unwrap();
    assert_eq!(marker["schema_version"], 1);
    assert_eq!(marker["registration_id"], f.registration_id("Work"));
    let state: Value =
        serde_json::from_slice(&fs::read(f.root.join("state.json")).unwrap()).unwrap();
    assert_eq!(
        state["desktop_launched"][0]["registration_id"],
        f.registration_id("Work")
    );
    assert_eq!(
        state["last_used"][0]["registration_id"],
        f.registration_id("Work")
    );
    // The folder is created once and reused.
    f.ok(&["desktop", "--foreground", "Work"]);
    assert_eq!(f.desktop_launches()[1]["arguments"], launch["arguments"]);

    let before: Vec<_> = fs::read_dir(&upstream).unwrap().collect();
    f.ok(&["desktop", "--foreground", "Up"]);
    let launch = &f.desktop_launches()[2];
    assert_eq!(
        launch["arguments"],
        json!([format!(
            "--user-data-dir={}",
            f.desktop_folder("Up").display()
        )])
    );
    assert_eq!(
        launch["env"]["CLAUDE_CONFIG_DIR"],
        upstream.to_str().unwrap()
    );
    assert_eq!(fs::read_dir(&upstream).unwrap().count(), before.len());
    // Doctor stays clean of Desktop findings and the root stays valid.
    let doctor = parsed(&f.run(&["doctor", "--json"]));
    assert!(
        !doctor.to_string().contains("desktop_data_not_isolated"),
        "{doctor}"
    );
    f.ok(&["list"]);
}

#[test]
fn desktop_default_alias_launches_plain_with_the_caller_environment() {
    let f = Fixture::new();
    f.ok(&["add", "personal", "--link-default"]);
    let out = f
        .command()
        .args(["desktop", "--foreground", "personal"])
        .env("CLAUDE_CONFIG_DIR", "/caller/choice")
        .env("ANTHROPIC_API_KEY", "fake-caller-key")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let launch = &f.desktop_launches()[0];
    assert_eq!(launch["arguments"], json!([]));
    assert_eq!(launch["env"]["CLAUDE_CONFIG_DIR"], "/caller/choice");
    assert_eq!(launch["env"]["ANTHROPIC_API_KEY"], "fake-caller-key");
    assert!(launch["env"].get("DISABLE_AUTOUPDATER").is_none());
    assert!(!f.root.join("desktop").exists());
    assert!(!f.root.join("state.json").exists());
}

#[test]
fn desktop_detaches_by_default_and_reports_an_early_exit() {
    let f = Fixture::new();
    f.add("Work");
    let out = f
        .command()
        .args(["desktop", "Work"])
        .env("FAKE_DESKTOP_MODE", "long")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "Started Claude Desktop for Work\n"
    );
    let launch = &f.desktop_launches()[0];
    assert_eq!(launch["session_leader"], true);
    for stream in ["stdin", "stdout", "stderr"] {
        assert_eq!(launch[stream], "/dev/null", "{stream}");
    }

    let out = f
        .command()
        .args(["desktop", "Work"])
        .env("FAKE_DESKTOP_EXIT", "3")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("claude_unavailable"), "{stderr}");
    assert!(stderr.contains("exit status 3"), "{stderr}");
    assert!(
        stderr.contains("roost desktop --foreground Work"),
        "{stderr}"
    );
    assert!(!stderr.contains("noise"), "{stderr}");

    // Foreground keeps Desktop's console and returns its result.
    let out = f
        .command()
        .args(["desktop", "--foreground", "Work"])
        .env("FAKE_DESKTOP_EXIT", "4")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&out.stderr).contains("gpu noise"));
}

fn hostname() -> String {
    fs::read_to_string("/proc/sys/kernel/hostname")
        .unwrap()
        .trim()
        .to_owned()
}

#[test]
fn desktop_refuses_a_running_profile_and_warns_about_other_instances() {
    let f = Fixture::new();
    f.add("Work");
    f.add("Other");
    f.ok(&["desktop", "--foreground", "Work"]);
    let lock = f.desktop_folder("Work").join("SingletonLock");
    let live = format!("{}-{}", hostname(), std::process::id());
    std::os::unix::fs::symlink(&live, &lock).unwrap();
    let out = f.run(&["desktop", "--foreground", "Work"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("desktop_running: this profile's Desktop is already running"),
        "{stderr}"
    );
    assert_eq!(f.desktop_launches().len(), 1);

    // Another profile still launches, with the Cowork warning.
    let out = f.ok(&["desktop", "--foreground", "Other"]);
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("only one running Desktop can use Cowork")
    );

    // A stale lock (dead PID or another host) is not running.
    let mut child = Command::new("/bin/true").spawn().unwrap();
    let dead = child.id();
    child.wait().unwrap();
    fs::remove_file(&lock).unwrap();
    std::os::unix::fs::symlink(format!("{}-{dead}", hostname()), &lock).unwrap();
    let out = f.ok(&["desktop", "--foreground", "Work"]);
    assert!(!String::from_utf8_lossy(&out.stderr).contains("Cowork"));
    fs::remove_file(&lock).unwrap();
    std::os::unix::fs::symlink(format!("elsewhere-{}", std::process::id()), &lock).unwrap();
    f.ok(&["desktop", "--foreground", "Work"]);

    // The conventional Desktop folder counts as another instance, for aliases too.
    let conventional = f.home.join(".config/Claude");
    fs::create_dir_all(&conventional).unwrap();
    std::os::unix::fs::symlink(&live, conventional.join("SingletonLock")).unwrap();
    let out = f.ok(&["desktop", "--foreground", "Work"]);
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("only one running Desktop can use Cowork")
    );
    f.ok(&["add", "personal", "--link-default"]);
    fs::remove_file(conventional.join("SingletonLock")).unwrap();
    fs::remove_file(&lock).unwrap();
    std::os::unix::fs::symlink(&live, &lock).unwrap();
    let out = f.ok(&["desktop", "--foreground", "personal"]);
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("only one running Desktop can use Cowork")
    );
}

#[test]
fn purge_and_upstream_remove_refuse_while_that_desktop_runs() {
    let f = Fixture::new();
    f.add("Work");
    let upstream = f.upstream("Up");
    f.ok(&["register", "Up", "--path", upstream.to_str().unwrap()]);
    for name in ["Work", "Up"] {
        f.ok(&["desktop", "--foreground", name]);
        let out = f
            .command()
            .args(["desktop", name])
            .env("FAKE_DESKTOP_MODE", "long")
            .env("FAKE_DESKTOP_LOCK", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let pids: Vec<i32> = ["Work", "Up"]
        .iter()
        .map(|name| {
            let target = fs::read_link(f.desktop_folder(name).join("SingletonLock")).unwrap();
            let target = target.to_str().unwrap().to_owned();
            target.rsplit_once('-').unwrap().1.parse().unwrap()
        })
        .collect();
    for args in [
        vec!["remove", "Work", "--purge", "--yes"],
        vec!["remove", "Up", "--yes"],
    ] {
        // Refused before confirmation: no terminal and no --yes still names Desktop.
        let without_yes: Vec<&str> = args.iter().copied().filter(|a| *a != "--yes").collect();
        for attempt in [args.clone(), without_yes] {
            let out = f.run(&attempt);
            assert_eq!(out.status.code(), Some(1), "{attempt:?}");
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(stderr.contains("desktop_running"), "{stderr}");
            assert!(
                stderr.contains(&format!("Quit Claude Desktop for {}", args[1])),
                "{stderr}"
            );
        }
        f.ok(&["where", args[1]]);
        assert!(f.desktop_folder(args[1]).join("Preferences").exists());
    }
    // Ordinary owned remove keeps the folder and needs no check.
    f.ok(&["remove", "Work"]);
    for pid in pids {
        unsafe { libc::kill(pid, libc::SIGTERM) };
    }
}

#[test]
fn desktop_needs_the_app_on_path_and_an_active_profile() {
    let f = Fixture::new();
    f.add("Work");
    fs::remove_file(f.bin.join("claude-desktop")).unwrap();
    // The fixture bin alone, so no real claude-desktop can ever be selected.
    let out = f
        .command()
        .env("PATH", &f.bin)
        .args(["desktop", "Work"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("claude_unavailable"));
    assert!(!f.root.join("desktop").exists());
    f.ok(&["remove", "Work"]);
    let out = f
        .command()
        .env("PATH", &f.bin)
        .args(["desktop", "Work"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("not_found"));
}

#[test]
fn desktop_folder_follows_remove_purge_and_upstream_lifecycle() {
    let f = Fixture::new();
    f.add("Work");
    f.ok(&["desktop", "--foreground", "Work"]);
    let folder = f.desktop_folder("Work");
    let outside = f.path.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep"), "preserve").unwrap();
    std::os::unix::fs::symlink(&outside, folder.join("linked")).unwrap();
    fs::create_dir(folder.join("Local Storage")).unwrap();
    fs::write(folder.join("Local Storage/leveldb"), "x").unwrap();
    // `--yes` without --purge means nothing for an owned profile.
    assert_eq!(f.run(&["remove", "Work", "--yes"]).status.code(), Some(2));
    f.ok(&["remove", "Work"]);
    assert!(folder.join("Preferences").exists());
    let out = f.ok(&["remove", "Work", "--purge", "--yes"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains(folder.to_str().unwrap()));
    assert!(!folder.exists());
    assert_eq!(
        fs::read_to_string(outside.join("keep")).unwrap(),
        "preserve"
    );

    let upstream = f.upstream("Up");
    fs::write(upstream.join("settings.json"), "{}").unwrap();
    f.ok(&["register", "Up", "--path", upstream.to_str().unwrap()]);
    f.ok(&["desktop", "--foreground", "Up"]);
    let folder = f.desktop_folder("Up");
    assert_eq!(
        f.run(&["remove", "Up", "--purge", "--yes"]).status.code(),
        Some(1)
    );
    // Deleting the Desktop sign-in needs confirmation; without a terminal, --yes.
    let mut command = f.command();
    command.args(["remove", "Up"]);
    unsafe {
        use std::os::unix::process::CommandExt;
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let out = command.output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(folder.exists());
    f.ok(&["where", "Up"]);
    let out = f.ok(&["remove", "Up", "--yes"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains(folder.to_str().unwrap()));
    assert!(!folder.exists());
    assert_eq!(
        fs::read_to_string(upstream.join("settings.json")).unwrap(),
        "{}"
    );
    assert!(!f.run(&["where", "Up"]).status.success());
    // Without a Desktop folder upstream removal needs no confirmation.
    f.ok(&["register", "Up", "--path", upstream.to_str().unwrap()]);
    f.ok(&["remove", "Up"]);
    let doctor = parsed(&f.run(&["doctor", "--json"]));
    assert!(
        doctor["data"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|x| x["severity"] == "warning"),
        "{doctor}"
    );
}

#[test]
fn doctor_warns_when_desktop_ignored_its_data_folder() {
    let f = Fixture::new();
    f.add("Work");
    let out = f
        .command()
        .args(["desktop", "--foreground", "Work"])
        .env("FAKE_DESKTOP_IGNORE_DIR", "1")
        .output()
        .unwrap();
    assert!(out.status.success());
    let doctor = parsed(&f.run(&["doctor", "--json"]));
    let finding = doctor["data"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["code"] == "desktop_data_not_isolated")
        .unwrap_or_else(|| panic!("{doctor}"))
        .clone();
    assert_eq!(finding["severity"], "warning");
    assert_eq!(finding["path"], f.desktop_folder("Work").to_str().unwrap());
    f.ok(&["desktop", "--foreground", "Work"]);
    let doctor = parsed(&f.run(&["doctor", "--json"]));
    assert!(!doctor.to_string().contains("desktop_data_not_isolated"));
}

#[test]
fn partial_desktop_deletion_keeps_the_journal_until_an_explicit_purge_retry() {
    let f = Fixture::new();
    f.add("Work");
    f.ok(&["desktop", "--foreground", "Work"]);
    let folder = f.desktop_folder("Work");
    let stuck = folder.join("Cache");
    fs::create_dir(&stuck).unwrap();
    fs::write(stuck.join("entry"), "x").unwrap();
    fs::set_permissions(&stuck, fs::Permissions::from_mode(0o500)).unwrap();
    let out = f.run(&["remove", "Work", "--purge", "--yes"]);
    fs::set_permissions(&stuck, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Partial deletion"), "{stderr}");
    assert!(folder.join(".roost-desktop.json").exists());
    assert!(f.root.join(".roost-operation.json").exists());
    assert!(!f.run(&["desktop", "--foreground", "Work"]).status.success());
    assert_eq!(f.desktop_launches().len(), 1);
    f.ok(&["remove", "Work", "--purge", "--yes"]);
    assert!(!folder.exists());
    assert!(!f.root.join(".roost-operation.json").exists());
    assert!(!f.root.join("profiles/Work").exists());
}

#[test]
fn desktop_launch_reconciles_subscribed_skill_links() {
    let f = Fixture::new();
    f.add("Work");
    let skill = f.path.join("review");
    fs::create_dir(&skill).unwrap();
    f.ok(&["set", "create", "core"]);
    f.ok(&["set", "add", "core", "--skill", skill.to_str().unwrap()]);
    f.ok(&["set", "subscribe", "Work", "core"]);
    let link = f.root.join("profiles/Work/skills/review");
    let _ = fs::remove_file(&link);
    f.ok(&["desktop", "--foreground", "Work"]);
    assert_eq!(fs::read_link(&link).unwrap(), skill);
}

#[test]
fn list_marks_selection_and_most_recent_launch_from_run_and_launchers() {
    let f = Fixture::new();
    f.add("Work");
    f.add("Home");
    f.ok(&["add", "personal", "--link-default"]);
    let old = f.upstream("Old");
    f.ok(&["register", "Old", "--path", old.to_str().unwrap()]);
    let app = repo(&f, "app");
    let in_app = |args: &[&str]| f.command().current_dir(&app).args(args).output().unwrap();
    assert!(in_app(&["switch", "--no-launch", "Work"]).status.success());
    // A launcher records the upstream registration's last use in manager storage.
    let launcher = f.root.join("bin/roost-Old");
    launched(
        &Command::new(&launcher)
            .env_clear()
            .env("HOME", &f.home)
            .env("PATH", format!("{}:/usr/bin:/bin", f.bin.display()))
            .output()
            .unwrap(),
    );
    assert!(
        fs::read_dir(&old).unwrap().next().is_none(),
        "upstream data untouched"
    );
    std::thread::sleep(std::time::Duration::from_millis(1100));
    launched(&f.run(&["run", "Home"]));
    // Default-alias launches record nothing.
    launched(&f.run(&["run", "personal"]));
    let value = parsed(&in_app(&["list", "--json"]));
    assert_eq!(
        value["data"]["project"],
        json!(app.join(".git").to_str().unwrap())
    );
    let by_name = |name: &str| {
        value["data"]["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == name)
            .unwrap()
            .clone()
    };
    let (work, home, old, personal) = (
        by_name("Work"),
        by_name("Home"),
        by_name("Old"),
        by_name("personal"),
    );
    assert_eq!(
        (&work["selected"], &work["most_recent"]),
        (&json!(true), &json!(false))
    );
    assert_eq!(work["last_used"], Value::Null);
    assert_eq!(
        (&home["selected"], &home["most_recent"]),
        (&json!(false), &json!(true))
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let at = home["last_used"].as_u64().unwrap();
    assert!(at <= now && at + 60 > now, "{at} {now}");
    assert!(old["last_used"].as_u64().unwrap() < at);
    assert_eq!(old["most_recent"], json!(false));
    assert_eq!(personal["last_used"], Value::Null);
    // Outside the project nothing is selected.
    let elsewhere = parsed(
        &f.command()
            .current_dir(repo(&f, "elsewhere"))
            .args(["list", "--json"])
            .output()
            .unwrap(),
    );
    assert!(
        elsewhere["data"]["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["selected"] == false)
    );
    let text = String::from_utf8(in_app(&["ls"]).stdout).unwrap();
    assert_eq!(
        text.lines().collect::<Vec<_>>(),
        [
            "  Profile   Kind           Token  Launchers  Sets  Last used",
            "  Home      owned          –      ready      —     now",
            "  Old       upstream       –      ready      —     now",
            "  personal  default_alias  –      ready      —     never",
            "@ Work      owned          –      ready      —     never",
            "",
            "○ 4 profiles · 1 upstream · @ selected here · hidden: Path",
        ]
    );
}

#[test]
fn remove_and_purge_clear_selections_so_a_same_name_profile_inherits_nothing() {
    let f = Fixture::new();
    let app = repo(&f, "app");
    let in_app = |args: &[&str]| f.command().current_dir(&app).args(args).output().unwrap();
    let selected_names = || -> Vec<String> {
        parsed(&in_app(&["list", "--json", "--retained"]))["data"]["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["selected"] == true)
            .map(|p| p["name"].as_str().unwrap().to_owned())
            .collect()
    };
    f.add("Work");
    assert!(in_app(&["switch", "--no-launch", "Work"]).status.success());
    launched(&in_app(&["run", "Work"]));
    assert_eq!(selected_names(), ["Work"]);
    // Ordinary remove retains the owned profile and its last use, not the selection.
    f.ok(&["remove", "Work"]);
    let state = fs::read_to_string(f.root.join("state.json")).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&state).unwrap()["selections"],
        json!([])
    );
    let record = &parsed(&in_app(&["list", "--json", "--retained"]))["data"]["profiles"][0];
    assert!(record["last_used"].is_u64(), "{record}");
    f.ok(&["reuse", "Work"]);
    assert!(selected_names().is_empty());
    let bare = in_app(&[]);
    assert_eq!(bare.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&bare.stderr).contains("warning"));
    // Purge, then a new profile with the same name starts with nothing.
    assert!(in_app(&["switch", "--no-launch", "Work"]).status.success());
    f.ok(&["remove", "Work", "--purge", "--yes"]);
    f.add("Work");
    assert!(selected_names().is_empty());
    let record = &parsed(&in_app(&["list", "--json"]))["data"]["profiles"][0];
    assert_eq!(record["last_used"], Value::Null);
    assert_eq!(in_app(&[]).status.code(), Some(2));
    // Removing a selected upstream registration drops its selection too.
    let old = f.upstream("Old");
    f.ok(&["register", "Old", "--path", old.to_str().unwrap()]);
    assert!(in_app(&["switch", "--no-launch", "Old"]).status.success());
    f.ok(&["remove", "Old"]);
    let state = fs::read_to_string(f.root.join("state.json")).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&state).unwrap()["selections"],
        json!([])
    );
}

#[test]
fn default_alias_selection_is_remembered_but_records_no_last_use() {
    let f = Fixture::new();
    f.ok(&["add", "personal", "--link-default"]);
    let app = repo(&f, "app");
    let in_app = |args: &[&str]| f.command().current_dir(&app).args(args).output().unwrap();
    assert!(
        in_app(&["switch", "--no-launch", "PERSONAL"])
            .status
            .success()
    );
    let probe = launched(
        &f.command()
            .current_dir(&app)
            .env("CLAUDE_CONFIG_DIR", "/caller/dir")
            .output()
            .unwrap(),
    );
    assert_eq!(probe["env"]["CLAUDE_CONFIG_DIR"], json!("/caller/dir"));
    let record = &parsed(&in_app(&["list", "--json"]))["data"]["profiles"][0];
    assert_eq!(record["selected"], json!(true));
    assert_eq!(record["last_used"], Value::Null);
    assert_eq!(record["most_recent"], json!(false));
}

#[test]
fn stale_selection_warns_naming_the_profile_before_asking() {
    let f = Fixture::new();
    f.add("Work");
    f.add("Gone");
    let app = repo(&f, "app");
    let registry: Value =
        serde_json::from_str(&fs::read_to_string(f.root.join("registry.json")).unwrap()).unwrap();
    let id = |name: &str| {
        registry["registrations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == name)
            .unwrap()["registration_id"]
            .clone()
    };
    f.ok(&["remove", "Gone"]);
    // A selection left naming a retained registration (for example after a failed
    // clean-up) is stale: warn, then fall through to the picker rules.
    let state = json!({"schema_version":1,"root_id":registry["root_id"],
        "selections":[{"project":app.join(".git"),"registration_id":id("Gone")}],
        "last_used":[],"links":[],"desktop_launched":[],"plugin_auto_update_at":null});
    let path = f.root.join("state.json");
    fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let out = f.command().current_dir(&app).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("warning") && stderr.contains("Gone"),
        "{stderr}"
    );
}

#[test]
fn state_write_failure_warns_and_still_launches() {
    let f = Fixture::new();
    f.add("Work");
    let app = repo(&f, "app");
    let path = f.root.join("state.json");
    fs::write(&path, b"not json").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    for args in [vec!["run", "Work"], vec!["switch", "Work"]] {
        let out = f.command().current_dir(&app).args(&args).output().unwrap();
        launched(&out);
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("warning"),
            "{args:?}"
        );
    }
    let out = f
        .command()
        .current_dir(&app)
        .args(["switch", "--no-launch", "Work"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(fs::read(&path).unwrap(), b"not json");
    // An unknown profile is refused before any state is touched.
    assert_eq!(
        f.command()
            .current_dir(&app)
            .args(["switch", "--no-launch", "Nobody"])
            .output()
            .unwrap()
            .status
            .code(),
        Some(1)
    );
}

/// Runs bare `roost` in each project under a PTY, sending keys once the picker
/// shows. Returns `[{exit, picker, launched}]`, `launched` being the fake Claude's
/// CLAUDE_CONFIG_DIR or null.
fn pick_in_pty(f: &Fixture, cases: &[(&Path, &[&str])]) -> Value {
    let cases: Vec<(&Path, &[&str], &[&str])> = cases
        .iter()
        .map(|(cwd, keys)| (*cwd, &[][..], *keys))
        .collect();
    let results = pty_session(f, &[], &cases);
    Value::Array(
        results
            .as_array()
            .unwrap()
            .iter()
            .map(|r| json!({"exit": r["exit"], "picker": r["picker"], "launched": r["launched"]}))
            .collect(),
    )
}

/// Runs `roost ARGS` in each project under a PTY with `env` added, sending keys
/// once the picker shows. Returns `[{exit, picker, launched, arguments, text}]`:
/// `launched`/`arguments` are the fake Claude's CLAUDE_CONFIG_DIR and arguments
/// (null when nothing launched) and `text` is everything the terminal received.
fn pty_session(f: &Fixture, env: &[(&str, &str)], cases: &[(&Path, &[&str], &[&str])]) -> Value {
    let script = r#"
import errno,json,os,pty,select,sys,time
exe,root,binpath,home,extra,cases=sys.argv[1:]
results=[]
for cwd,args,keys in json.loads(cases):
    pid,fd=pty.fork()
    if pid==0:
        os.chdir(cwd)
        env={'ROOST_DIR':root,'HOME':home,'PATH':binpath+':/usr/bin:/bin'}
        env.update(json.loads(extra))
        os.execve(exe,[exe]+args,env)
    data=b'';sent=False;deadline=time.monotonic()+10
    while time.monotonic()<deadline:
        if not select.select([fd],[],[],0.1)[0]:continue
        try:chunk=os.read(fd,65536)
        except OSError as e:
            if e.errno==errno.EIO:break
            raise
        if not chunk:break
        data+=chunk
        if not sent and b'Choose a profile' in data:
            time.sleep(0.1)
            for key in keys:
                os.write(fd,key.encode('latin-1'));time.sleep(0.1)
            sent=True
    else:
        os.kill(pid,9);raise RuntimeError('PTY deadline exceeded: %r'%data)
    os.close(fd);_,status=os.waitpid(pid,0)
    text=data.decode(errors='replace')
    launched=None;arguments=None
    for line in text.replace('\r','\n').split('\n'):
        if '{' in line:
            probe=json.loads(line[line.index('{'):])
            launched=probe['env'].get('CLAUDE_CONFIG_DIR');arguments=probe['arguments']
    results.append({'exit':os.waitstatus_to_exitcode(status),'picker':'Choose a profile' in text,'launched':launched,'arguments':arguments,'text':text})
print(json.dumps(results))
"#;
    let cases: Vec<Value> = cases
        .iter()
        .map(|(cwd, args, keys)| json!([cwd.to_str().unwrap(), args, keys]))
        .collect();
    let env: serde_json::Map<String, Value> = env
        .iter()
        .map(|(k, v)| ((*k).to_owned(), json!(v)))
        .collect();
    let out = Command::new("/usr/bin/python3")
        .args([
            "-c",
            script,
            env!("CARGO_BIN_EXE_roost"),
            f.root.to_str().unwrap(),
            f.bin.to_str().unwrap(),
            f.home.to_str().unwrap(),
            &serde_json::to_string(&env).unwrap(),
            &serde_json::to_string(&cases).unwrap(),
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    parsed(&out)
}

#[test]
fn picker_highlights_last_used_moves_with_keys_and_remembers_the_choice() {
    let f = Fixture::new();
    f.add("Work");
    f.add("Home");
    launched(&f.run(&["run", "Work"]));
    let work = f.root.join("profiles/Work");
    let home = f.root.join("profiles/Home");
    let projects: Vec<PathBuf> = ["a", "b", "c", "d"].iter().map(|p| repo(&f, p)).collect();
    let results = pick_in_pty(
        &f,
        &[
            // Enter takes the initial highlight: the most recently launched.
            (&projects[0], &["\r"]),
            (&projects[1], &["k", "\r"]),
            (&projects[2], &["\x1b[A", "\x1b[B", "j", "\x1bOA", "\r"]),
            (&projects[3], &["k", "j", "\n"]),
        ],
    );
    let expect = |path: &Path| json!({"exit":0,"picker":true,"launched":path.to_str().unwrap()});
    assert_eq!(
        results,
        json!([expect(&work), expect(&home), expect(&home), expect(&work)])
    );
    // The choice is remembered: no picker, no terminal needed.
    let probe = launched(&f.command().current_dir(&projects[1]).output().unwrap());
    assert_eq!(
        probe["env"]["CLAUDE_CONFIG_DIR"],
        json!(home.to_str().unwrap())
    );
}

#[test]
fn picker_cancels_with_130_and_no_state_change_and_shows_for_one_profile() {
    let f = Fixture::new();
    f.add("Work");
    let app = repo(&f, "app");
    let results = pick_in_pty(
        &f,
        &[
            (&app, &["\x1b"]),
            (&app, &["q"]),
            (&app, &["\x03"]),
            (&app, &["\x04"]),
        ],
    );
    let cancelled = json!({"exit":130,"picker":true,"launched":null});
    assert_eq!(results, json!([cancelled, cancelled, cancelled, cancelled]));
    assert!(!f.root.join("state.json").exists());
    let results = pick_in_pty(&f, &[(&app, &["\r"])]);
    assert_eq!(
        results,
        json!([{"exit":0,"picker":true,"launched":f.root.join("profiles/Work").to_str().unwrap()}])
    );
}

/// The profile directory bare `roost` launches in `cwd` without a terminal.
fn selected_dir(f: &Fixture, cwd: &Path) -> Value {
    launched(&f.command().current_dir(cwd).output().unwrap())["env"]["CLAUDE_CONFIG_DIR"].clone()
}

#[test]
fn switch_without_name_opens_the_picker_on_the_current_selection_and_launches() {
    let f = Fixture::new();
    f.add("Work");
    f.add("Home");
    let work = f.root.join("profiles/Work");
    let home = f.root.join("profiles/Home");
    let (a, b) = (repo(&f, "a"), repo(&f, "b"));
    f.command()
        .current_dir(&a)
        .args(["switch", "--no-launch", "Home"])
        .output()
        .unwrap();
    launched(&f.run(&["run", "Work"]));
    let results = pty_session(
        &f,
        &[],
        &[
            // No selection: the most recently launched profile is highlighted.
            (&b, &["switch", "--", "--resume", ""], &["\r"]),
            // A selection exists, yet the picker still shows, highlighting it over
            // the most recently launched profile.
            (&a, &["switch"], &["\r"]),
        ],
    );
    assert_eq!(results[0]["exit"], 0, "{}", results[0]["text"]);
    assert_eq!(results[0]["picker"], true);
    assert_eq!(results[0]["launched"], json!(work.to_str().unwrap()));
    assert_eq!(results[0]["arguments"], json!(["--resume", ""]));
    assert_eq!(results[1]["exit"], 0, "{}", results[1]["text"]);
    assert_eq!(results[1]["launched"], json!(home.to_str().unwrap()));
    assert_eq!(selected_dir(&f, &b), json!(work.to_str().unwrap()));
    // Keys move the highlight; --no-launch only records the choice.
    let results = pty_session(&f, &[], &[(&a, &["switch", "--no-launch"], &["j", "\r"])]);
    assert_eq!(results[0]["exit"], 0, "{}", results[0]["text"]);
    assert_eq!(results[0]["launched"], Value::Null);
    assert!(
        results[0]["text"]
            .as_str()
            .unwrap()
            .contains("Selected Work for"),
        "{}",
        results[0]["text"]
    );
    assert_eq!(selected_dir(&f, &a), json!(work.to_str().unwrap()));
}

#[test]
fn switch_picker_cancel_exits_130_and_keeps_the_selection() {
    let f = Fixture::new();
    f.add("Work");
    f.add("Home");
    let a = repo(&f, "a");
    f.command()
        .current_dir(&a)
        .args(["switch", "--no-launch", "Home"])
        .output()
        .unwrap();
    let results = pty_session(
        &f,
        &[],
        &[
            (&a, &["switch"], &["j", "\x1b"]),
            (&a, &["switch", "--no-launch"], &["j", "\x03"]),
        ],
    );
    for result in results.as_array().unwrap() {
        assert_eq!(result["exit"], 130, "{}", result["text"]);
        assert_eq!(result["picker"], true);
        assert_eq!(result["launched"], Value::Null);
    }
    assert_eq!(
        selected_dir(&f, &a),
        json!(f.root.join("profiles/Home").to_str().unwrap())
    );
}

#[test]
fn switch_without_name_and_terminal_is_usage_naming_switch_name() {
    let f = Fixture::new();
    f.add("Work");
    let a = repo(&f, "a");
    for args in [
        vec!["switch"],
        vec!["switch", "--no-launch"],
        vec!["switch", "--", "-p"],
    ] {
        let out = f.command().current_dir(&a).args(&args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("roost switch NAME"), "{args:?}: {stderr}");
    }
    assert!(!f.root.join("state.json").exists());
}

#[test]
fn picker_rows_use_list_styles_on_a_terminal_unless_no_color() {
    let f = Fixture::new();
    f.add("Work");
    f.add("Home");
    let a = repo(&f, "a");
    let colored = pty_session(&f, &[], &[(&a, &["switch"], &["\x1b"])]);
    let text = colored[0]["text"].as_str().unwrap();
    // Names are bold like `roost ls`; the highlighted row keeps reverse video
    // after every styled cell's reset.
    assert!(text.contains("\x1b[1mWork\x1b[0m"), "{text:?}");
    assert!(text.contains("\x1b[7m> "), "{text:?}");
    assert!(text.contains("\x1b[0m\x1b[7m"), "{text:?}");
    let plain = pty_session(&f, &[("NO_COLOR", "1")], &[(&a, &["switch"], &["\x1b"])]);
    let text = plain[0]["text"].as_str().unwrap();
    assert!(
        !text.contains("\x1b[1m") && !text.contains("\x1b[32m"),
        "{text:?}"
    );
    assert!(text.contains("\x1b[7m> "), "{text:?}");
}

/// Runs `roost desktop ARGS` under a PTY from the fixture directory with NO_COLOR,
/// sending `keys` once the picker shows. Detached, the fake Desktop lingers so it
/// passes the startup watch. Returns `{exit, picker, text}`.
fn desktop_picker(f: &Fixture, args: &[&str], keys: &[&str]) -> Value {
    let record = f.path.join("desktop.jsonl");
    let mut command = vec!["desktop"];
    command.extend_from_slice(args);
    let results = pty_session(
        f,
        &[
            ("FAKE_DESKTOP_RECORD", record.to_str().unwrap()),
            // Detached launches must outlive the startup watch.
            (
                "FAKE_DESKTOP_MODE",
                if args.contains(&"--foreground") {
                    ""
                } else {
                    "long"
                },
            ),
            ("NO_COLOR", "1"),
        ],
        &[(&f.path, &command, keys)],
    );
    results[0].clone()
}

/// The picker row naming `name` (the last one drawn).
fn picker_row(text: &str, name: &str) -> String {
    text.split(['\r', '\n'])
        .rfind(|line| line.contains(&format!(" {name} ")))
        .unwrap_or_else(|| panic!("no row for {name}: {text:?}"))
        .to_owned()
}

#[test]
fn desktop_without_name_highlights_the_last_desktop_launch_and_launches_detached() {
    let f = Fixture::new();
    f.add("Work");
    f.add("Home");
    f.add("Other");
    f.ok(&["desktop", "--foreground", "Work"]);
    // A later terminal launch does not move the Desktop highlight.
    launched(&f.run(&["run", "Home"]));
    let result = desktop_picker(&f, &[], &["\r"]);
    let text = result["text"].as_str().unwrap();
    assert_eq!(result["exit"], 0, "{text}");
    assert_eq!(result["picker"], true);
    assert!(text.contains("Started Claude Desktop for Work"), "{text}");
    // Desktop column: a folder means signed in, none means never launched.
    assert!(picker_row(text, "Work").contains("signed in"), "{text}");
    assert!(picker_row(text, "Home").trim_end().ends_with('—'), "{text}");
    // No last-used marker on any row.
    for name in ["Work", "Home", "Other"] {
        assert!(!picker_row(text, name).contains('^'), "{text}");
    }
    let launch = &f.desktop_launches()[1];
    assert_eq!(
        launch["arguments"],
        json!([format!(
            "--user-data-dir={}",
            f.desktop_folder("Work").display()
        )])
    );
    assert_eq!(launch["session_leader"], true);
    assert_eq!(launch["stdin"], "/dev/null");
    // Keys move the highlight; the choice is never a project selection.
    let result = desktop_picker(&f, &[], &["k", "\r"]);
    assert_eq!(result["exit"], 0, "{}", result["text"]);
    assert_eq!(
        f.desktop_launches()[2]["arguments"],
        json!([format!(
            "--user-data-dir={}",
            f.desktop_folder("Other").display()
        )])
    );
    let state: Value =
        serde_json::from_slice(&fs::read(f.root.join("state.json")).unwrap()).unwrap();
    assert_eq!(state["selections"], json!([]), "{state}");
}

#[test]
fn desktop_picker_falls_back_to_last_used_and_foreground_stays_attached() {
    let f = Fixture::new();
    f.add("Work");
    f.add("Home");
    f.ok(&["add", "personal", "--link-default"]);
    launched(&f.run(&["run", "Work"]));
    let result = desktop_picker(&f, &["--foreground"], &["\r"]);
    let text = result["text"].as_str().unwrap();
    assert_eq!(result["exit"], 0, "{text}");
    assert!(text.contains("chromium console noise"), "{text}");
    assert!(picker_row(text, "personal").contains("plain"), "{text}");
    assert_eq!(
        f.desktop_launches()[0]["arguments"],
        json!([format!(
            "--user-data-dir={}",
            f.desktop_folder("Work").display()
        )])
    );
}

#[test]
fn desktop_picker_marks_and_refuses_a_running_profile_and_cancels_with_130() {
    let f = Fixture::new();
    f.add("Work");
    f.add("Home");
    f.ok(&["desktop", "--foreground", "Work"]);
    let live = format!("{}-{}", hostname(), std::process::id());
    std::os::unix::fs::symlink(&live, f.desktop_folder("Work").join("SingletonLock")).unwrap();
    let result = desktop_picker(&f, &[], &["\r"]);
    let text = result["text"].as_str().unwrap();
    assert_eq!(result["exit"], 1, "{text}");
    assert!(picker_row(text, "Work").contains("running"), "{text}");
    assert!(
        text.contains("desktop_running: this profile's Desktop is already running"),
        "{text}"
    );
    assert_eq!(f.desktop_launches().len(), 1);
    let state = fs::read(f.root.join("state.json")).unwrap();
    for keys in [&["\x1b"][..], &["j", "\x03"], &["q"]] {
        let result = desktop_picker(&f, &[], keys);
        assert_eq!(result["exit"], 130, "{}", result["text"]);
        assert_eq!(result["picker"], true);
    }
    assert_eq!(f.desktop_launches().len(), 1);
    assert_eq!(fs::read(f.root.join("state.json")).unwrap(), state);
}

#[test]
fn desktop_without_name_and_terminal_is_usage_naming_desktop_name() {
    let f = Fixture::new();
    f.add("Work");
    for args in [vec!["desktop"], vec!["desktop", "--foreground"]] {
        let out = f.run(&args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("roost desktop NAME"), "{args:?}: {stderr}");
    }
    assert!(f.desktop_launches().is_empty());
    assert!(!f.root.join("state.json").exists());
    assert!(!f.root.join("desktop").exists());
}

/// The fixture's conventional Desktop folder, signed in and with permissive modes
/// Roost must never repair.
fn signed_in_desktop(f: &Fixture) -> PathBuf {
    let conventional = f.home.join(".config/Claude");
    fs::create_dir_all(&conventional).unwrap();
    fs::write(conventional.join("Preferences"), "signed-in").unwrap();
    fs::set_permissions(&conventional, fs::Permissions::from_mode(0o755)).unwrap();
    conventional
}

fn state_json(f: &Fixture) -> Value {
    serde_json::from_slice(&fs::read(f.root.join("state.json")).unwrap()).unwrap()
}

#[test]
fn linked_desktop_borrows_the_conventional_folder_in_place() {
    let f = Fixture::new();
    f.add("Work");
    f.add("Other");
    f.ok(&["add", "personal", "--link-default"]);
    let conventional = signed_in_desktop(&f);
    let out = f.ok(&["desktop", "--link", "Work"]);
    assert!(
        String::from_utf8_lossy(&out.stdout).contains(conventional.to_str().unwrap()),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(
        state_json(&f)["desktop_links"],
        json!([{"registration_id": f.registration_id("Work")}])
    );
    // Linking again is a no-op; aliases already use the folder.
    f.ok(&["desktop", "--link", "work"]);
    let out = f.fails(&["desktop", "--link", "personal"], "usage");
    assert_eq!(out.status.code(), Some(2));
    // Only one registration may borrow it.
    let out = f.fails(&["desktop", "--link", "Other"], "collision");
    assert!(String::from_utf8_lossy(&out.stderr).contains("Work"));

    f.ok(&["desktop", "--foreground", "Work"]);
    let launch = &f.desktop_launches()[0];
    assert_eq!(launch["arguments"], json!([]));
    assert_eq!(
        launch["env"]["CLAUDE_CONFIG_DIR"],
        f.root.join("profiles/Work").to_str().unwrap()
    );
    assert_eq!(launch["env"]["DISABLE_AUTOUPDATER"], "1");
    assert!(!f.root.join("desktop").exists());
    let state = state_json(&f);
    assert_eq!(
        state["desktop_launched"][0]["registration_id"],
        f.registration_id("Work")
    );
    assert_eq!(
        state["last_used"][0]["registration_id"],
        f.registration_id("Work")
    );
    // The borrowed folder is untouched.
    assert_eq!(
        fs::metadata(&conventional).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert_eq!(
        fs::read_to_string(conventional.join("Preferences")).unwrap(),
        "signed-in"
    );
    // Doctor's isolation check does not apply to a borrowed folder.
    let doctor = parsed(&f.run(&["doctor", "--json"]));
    assert!(
        !doctor.to_string().contains("desktop_data_not_isolated"),
        "{doctor}"
    );

    // A detached linked Desktop holds the conventional lock, so the alias sharing
    // that folder refuses to start a second time.
    let out = f
        .command()
        .args(["desktop", "Work"])
        .env("FAKE_DESKTOP_MODE", "long")
        .env("FAKE_DESKTOP_LOCK", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let target = fs::read_link(conventional.join("SingletonLock")).unwrap();
    let pid: i32 = target
        .to_str()
        .unwrap()
        .rsplit_once('-')
        .unwrap()
        .1
        .parse()
        .unwrap();
    f.fails(&["desktop", "--foreground", "personal"], "desktop_running");
    unsafe { libc::kill(pid, libc::SIGTERM) };
    fs::remove_file(conventional.join("SingletonLock")).unwrap();
    assert_eq!(f.desktop_launches().len(), 2);

    // Unlinking returns Work to its own Roost folder; unlinking twice is harmless.
    f.ok(&["desktop", "--unlink", "Work"]);
    f.ok(&["desktop", "--unlink", "Work"]);
    assert!(state_json(&f).get("desktop_links").is_none());
    f.ok(&["desktop", "--foreground", "Work"]);
    assert_eq!(
        f.desktop_launches()[2]["arguments"],
        json!([format!(
            "--user-data-dir={}",
            f.desktop_folder("Work").display()
        )])
    );
    assert_eq!(f.desktop_launches().len(), 3);
    // Another profile may borrow it now.
    f.ok(&["desktop", "--link", "Other"]);
}

#[test]
fn link_follows_xdg_config_home_from_the_caller_environment() {
    let f = Fixture::new();
    f.add("Work");
    let config = f.path.join("xdg");
    fs::create_dir(&config).unwrap();
    let out = f
        .command()
        .env("XDG_CONFIG_HOME", &config)
        .args(["desktop", "--link", "Work"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains(config.join("Claude").to_str().unwrap()));
    assert!(!config.join("Claude").exists());
    assert!(!f.home.join(".config").exists());
}

/// Runs `args` with no controlling terminal.
fn detached_from_terminal(f: &Fixture, args: &[&str]) -> Output {
    let mut command = f.command();
    command.args(args);
    unsafe {
        use std::os::unix::process::CommandExt;
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    command.output().unwrap()
}

#[test]
fn link_replaces_an_own_desktop_folder_only_after_confirmation() {
    let f = Fixture::new();
    f.add("Work");
    let conventional = signed_in_desktop(&f);
    f.ok(&["desktop", "--foreground", "Work"]);
    let own = f.desktop_folder("Work");
    let out = f.fails(&["desktop", "--link", "Work"], "collision");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains(own.to_str().unwrap()), "{stderr}");
    assert!(stderr.contains("--replace"), "{stderr}");
    // No terminal and no --yes: usage, nothing changed.
    let out = detached_from_terminal(&f, &["desktop", "--link", "Work", "--replace"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(own.join("Preferences").exists());
    assert!(state_json(&f).get("desktop_links").is_none());
    // A running own Desktop refuses before confirmation.
    let lock = own.join("SingletonLock");
    std::os::unix::fs::symlink(format!("{}-{}", hostname(), std::process::id()), &lock).unwrap();
    f.fails(
        &["desktop", "--link", "Work", "--replace", "--yes"],
        "desktop_running",
    );
    fs::remove_file(&lock).unwrap();
    assert!(own.join("Preferences").exists());

    let out = f.ok(&["desktop", "--link", "Work", "--replace", "--yes"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains(own.to_str().unwrap()));
    assert!(!own.exists());
    assert_eq!(
        state_json(&f)["desktop_links"][0]["registration_id"],
        f.registration_id("Work")
    );
    assert_eq!(
        fs::read_to_string(conventional.join("Preferences")).unwrap(),
        "signed-in"
    );
    f.ok(&["desktop", "--foreground", "Work"]);
    assert_eq!(f.desktop_launches()[1]["arguments"], json!([]));
    assert!(!own.exists());
    f.ok(&["doctor"]);
}

#[test]
fn linked_desktop_shares_one_instance_with_the_alias_and_survives_removal() {
    let f = Fixture::new();
    f.add("Work");
    f.ok(&["add", "personal", "--link-default"]);
    let conventional = signed_in_desktop(&f);
    f.ok(&["desktop", "--link", "Work"]);
    let lock = conventional.join("SingletonLock");
    std::os::unix::fs::symlink(format!("{}-{}", hostname(), std::process::id()), &lock).unwrap();
    let out = f.fails(&["desktop", "--foreground", "Work"], "desktop_running");
    assert!(String::from_utf8_lossy(&out.stderr).contains("already running"));
    let out = f.fails(&["desktop", "--foreground", "personal"], "desktop_running");
    assert!(String::from_utf8_lossy(&out.stderr).contains("shared with Work"));
    assert!(f.desktop_launches().is_empty());
    // Q51 applies to the borrowed folder: purge refuses while it runs.
    let out = f.fails(&["remove", "Work", "--purge", "--yes"], "desktop_running");
    assert!(String::from_utf8_lossy(&out.stderr).contains("Quit Claude Desktop for Work"));
    f.ok(&["where", "Work"]);
    fs::remove_file(&lock).unwrap();

    // Ordinary remove drops the link even though the registration is retained.
    f.ok(&["remove", "Work"]);
    assert!(state_json(&f).get("desktop_links").is_none());
    f.ok(&["reuse", "Work"]);
    f.ok(&["desktop", "--link", "Work"]);
    f.ok(&["remove", "Work", "--purge", "--yes"]);
    assert!(state_json(&f).get("desktop_links").is_none());
    assert_eq!(
        fs::read_to_string(conventional.join("Preferences")).unwrap(),
        "signed-in"
    );

    // Upstream: remove refuses while the borrowed Desktop runs, then drops the link
    // without asking (Roost has no folder of its own to delete).
    let upstream = f.upstream("Up");
    f.ok(&["register", "Up", "--path", upstream.to_str().unwrap()]);
    f.ok(&["desktop", "--link", "Up"]);
    std::os::unix::fs::symlink(format!("{}-{}", hostname(), std::process::id()), &lock).unwrap();
    f.fails(&["remove", "Up"], "desktop_running");
    fs::remove_file(&lock).unwrap();
    let out = detached_from_terminal(&f, &["remove", "Up"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(state_json(&f).get("desktop_links").is_none());
    assert_eq!(
        fs::read_to_string(conventional.join("Preferences")).unwrap(),
        "signed-in"
    );
    assert!(upstream.exists());
}

#[test]
fn desktop_picker_shows_the_borrowed_folder_for_the_linked_and_alias_rows() {
    let f = Fixture::new();
    f.add("Work");
    f.add("Home");
    f.ok(&["add", "personal", "--link-default"]);
    let conventional = signed_in_desktop(&f);
    f.ok(&["desktop", "--link", "Work"]);
    let result = desktop_picker(&f, &[], &["\x1b"]);
    let text = result["text"].as_str().unwrap();
    assert_eq!(result["exit"], 130, "{text}");
    assert!(picker_row(text, "Work").contains("signed in"), "{text}");
    assert!(
        picker_row(text, "personal").contains("shared with Work"),
        "{text}"
    );
    let home = picker_row(text, "Home");
    assert!(
        !home.contains("signed in") && !home.contains("shared"),
        "{text}"
    );
    let live = format!("{}-{}", hostname(), std::process::id());
    std::os::unix::fs::symlink(&live, conventional.join("SingletonLock")).unwrap();
    let result = desktop_picker(&f, &[], &["\x1b"]);
    let text = result["text"].as_str().unwrap();
    assert!(picker_row(text, "Work").contains("running"), "{text}");
    assert!(picker_row(text, "personal").contains("running"), "{text}");
    assert!(f.desktop_launches().is_empty());
}
