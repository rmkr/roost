#![cfg(unix)]
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    path: PathBuf,
    root: PathBuf,
    bin: PathBuf,
    home: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "roost-cli-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        private(&path);
        let bin = path.join("bin");
        let home = path.join("home");
        fs::create_dir(&bin).unwrap();
        private(&bin);
        fs::create_dir(&home).unwrap();
        private(&home);
        let fake = bin.join("claude");
        fs::write(&fake,r#"#!/usr/bin/python3
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
"#).unwrap();
        fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
        let root = path.join("root");
        Self {
            path,
            root,
            bin,
            home,
        }
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_roost"));
        command
            .env_clear()
            .env("ROOST_DIR", &self.root)
            .env("HOME", &self.home)
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("SHELL", "/bin/bash")
            .current_dir(&self.path);
        command
    }
    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }
    fn ok(&self, args: &[&str]) -> Output {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }
    fn add(&self, name: &str) {
        self.ok(&["add", name]);
    }
    fn upstream(&self, name: &str) -> PathBuf {
        let path = self.home.join(".ccm/profiles").join(name);
        fs::create_dir_all(&path).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
fn private(path: &Path) {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}
fn parsed(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&output.stdout)))
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
        String::from_utf8(f.ok(&[]).stdout)
            .unwrap()
            .contains("auth login")
    );
    assert_eq!(f.ok(&["--version"]).stdout, b"0.1.0\n");
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
        vec!["remove", "Work", "--yes"],
        vec!["token", "Work", "--stdin", "--clear"],
        vec!["unknown"],
    ] {
        assert_eq!(f.run(&args).status.code(), Some(2), "{args:?}");
    }
    assert!(!f.root.exists());
    let empty = parsed(&f.ok(&["list", "--json"]));
    assert_eq!(empty["data"]["profiles"], json!([]));
    assert!(!f.root.exists());
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
