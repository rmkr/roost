//! Plugin store and launch-time injection (spec "Plugin store", gate A17).
//! Disposable fixtures only: a fake `claude` that emulates `plugin` subcommands in
//! its CLAUDE_CONFIG_DIR, private temp root/home, no real Claude, account or network.
#![cfg(unix)]
mod common;
use common::{Fixture, parsed, s};
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command};

/// Fake Claude. `plugin ...` subcommands keep `fake-state.json` in CLAUDE_CONFIG_DIR
/// and write `plugins/cache/<mkt>/<name>/<version>/` like Claude does; marketplaces
/// come from FAKE_CATALOG (`{mkt:{name:{"deps":[..],"manifest":NAME}}}`). Every call
/// is appended to FAKE_LOG with argv, environment and whether the store lock and
/// the root lock were held. Other invocations print argv/env as JSON (launches).
const FAKE: &str = r#"#!/usr/bin/python3
import fcntl,json,os,shutil,sys,time
args=sys.argv[1:]
if args==['--version']:
    print('2.1.280 (Claude Code)');sys.exit(0)
cfg=os.environ.get('CLAUDE_CONFIG_DIR')
def held(path):
    try:fd=os.open(path,os.O_RDONLY)
    except OSError:return None
    try:
        fcntl.flock(fd,fcntl.LOCK_EX|fcntl.LOCK_NB);fcntl.flock(fd,fcntl.LOCK_UN);return False
    except OSError:return True
    finally:os.close(fd)
log=os.environ.get('FAKE_LOG')
if log:
    record={'arguments':args,'config':cfg,'env':dict(os.environ),'stdin_tty':sys.stdin.isatty(),
        'store_lock_held':held(os.path.join(cfg,'.roost-store-lock')) if cfg else None,
        'root_lock_held':held(os.path.join(os.environ['ROOST_DIR'],'.roost-lock')) if os.environ.get('ROOST_DIR') else None}
    with open(log,'a') as f:f.write(json.dumps(record)+'\n')
if args[:1]!=['plugin']:
    print(json.dumps({'arguments':args,'env':dict(os.environ)}));sys.exit(int(os.environ.get('FAKE_CLAUDE_EXIT','0')))
sub=args[1:]
if os.environ.get('FAKE_PLUGIN_MODE')=='slow' and sub[0]!='list':time.sleep(30)
if os.environ.get('FAKE_PLUGIN_FAIL') in (sub[0],' '.join(sub[:2])):
    print('fake plugin failure',file=sys.stderr);sys.exit(1)
state_path=os.path.join(cfg,'fake-state.json')
st=json.load(open(state_path)) if os.path.exists(state_path) else {'marketplaces':[],'installed':{}}
catalog=json.loads(os.environ.get('FAKE_CATALOG','{}'))
def save():json.dump(st,open(state_path,'w'))
def path_for(pid,ver):
    name,m=pid.split('@');return os.path.join(cfg,'plugins','cache',m,name,ver)
def install(pid):
    name,m=pid.split('@');spec=catalog[m][name];p=path_for(pid,'v1')
    os.makedirs(os.path.join(p,'.claude-plugin'),exist_ok=True)
    json.dump({'name':spec.get('manifest',name),'dependencies':spec.get('deps',[])},open(os.path.join(p,'.claude-plugin','plugin.json'),'w'))
    st['installed'][pid]='v1'
    for d in spec.get('deps',[]):
        did=d if '@' in d else d+'@'+m
        if did not in st['installed']:install(did)
def resolve(x):
    if '@' in x:return x
    found=[n+'@'+m for m in st['marketplaces'] for n in catalog.get(m,{}) if n==x]
    if len(found)!=1:print('not found',file=sys.stderr);sys.exit(1)
    return found[0]
if sub[:2]==['marketplace','add']:
    if sub[2] not in catalog:sys.exit(1)
    st['marketplaces'].append(sub[2]);save();print('Added marketplace '+sub[2]);sys.exit(0)
if sub[0]=='install':
    pid=resolve(sub[1]);name,m=pid.split('@')
    if m not in st['marketplaces'] or name not in catalog.get(m,{}):sys.exit(1)
    install(pid);save()
    swap=os.environ.get('FAKE_SWAP_ROOT')
    if swap:
        # Replace the Roost root while the root lock is released: a new root with a
        # same-named set, created by the Roost binary under test.
        import subprocess
        root=os.environ['ROOST_DIR'];os.rename(root,root+'-old')
        env={k:v for k,v in os.environ.items() if k not in ('CLAUDE_CONFIG_DIR','FAKE_SWAP_ROOT','FAKE_LOG')}
        for argv in (['add','Other'],['set','create','dev']):
            subprocess.run([swap]+argv,env=env,check=True,stdout=subprocess.DEVNULL)
    print('Installed '+pid);sys.exit(0)
if sub[0]=='update':
    for pid in sub[1:] or list(st['installed']):
        if pid not in st['installed']:sys.exit(1)
        old=st['installed'][pid];new='v%d'%(int(old[1:])+1)
        shutil.copytree(path_for(pid,old),path_for(pid,new))
        with open(os.path.join(path_for(pid,old),'.orphaned_at'),'w') as f:f.write(os.environ.get('FAKE_ORPHANED_AT',str(int(time.time()*1000))))
        st['installed'][pid]=new
    save();sys.exit(0)
if sub[0]=='uninstall':
    pid=sub[1]
    if pid not in st['installed']:sys.exit(1)
    shutil.rmtree(path_for(pid,st['installed'].pop(pid)));save();sys.exit(0)
if sub[0]=='list':
    rows=[{'id':pid,'scope':'user','enabled':True,'version':ver,'installPath':path_for(pid,ver),'mcpServers':{'token':'fake-secret'}} for pid,ver in st['installed'].items()]
    if '--available' in sub:
        rows={'installed':rows,'available':[{'pluginId':n+'@'+m,'name':n,'marketplaceName':m} for m in st['marketplaces'] for n in catalog.get(m,{})]}
    print(json.dumps(rows));sys.exit(0)
sys.exit(2)
"#;

const FAKE_DESKTOP: &str = "#!/usr/bin/python3\nimport json,os,sys\nopen(os.environ['FAKE_DESKTOP_RECORD'],'a').write(json.dumps({'arguments':sys.argv[1:],'env':dict(os.environ)})+'\\n')\n";

impl Fixture {
    fn new() -> Self {
        let f = Fixture::with_fakes(
            "plugins",
            &[("claude", FAKE), ("claude-desktop", FAKE_DESKTOP)],
        );
        let (log, desktop) = (f.path.join("claude.jsonl"), f.path.join("desktop.jsonl"));
        let catalog = json!({
            "tools": {"lint": {}, "fmt": {"deps": ["core"]}, "core": {}},
            "other": {"lint": {}, "fmt2": {"manifest": "fmt"}}
        });
        f.with_var("FAKE_LOG", log)
            .with_var("FAKE_CATALOG", catalog.to_string())
            .with_var("FAKE_DESKTOP_RECORD", desktop)
    }
    /// Another program with the fixture's environment and working directory.
    fn with_env(&self, program: PathBuf) -> Command {
        let base = self.command();
        let mut command = Command::new(program);
        command.env_clear().current_dir(&self.path);
        for (key, value) in base.get_envs() {
            if let Some(value) = value {
                command.env(key, value);
            }
        }
        command
    }
    fn store(&self) -> PathBuf {
        self.root.join("plugin-store")
    }
    /// Every fake Claude call so far, oldest first.
    fn calls(&self) -> Vec<Value> {
        fs::read_to_string(self.path.join("claude.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
    /// Fake Claude calls that ran a `plugin` subcommand, as argument strings.
    fn plugin_calls(&self) -> Vec<String> {
        self.calls()
            .iter()
            .filter(|c| c["arguments"][0] == "plugin")
            .map(|c| {
                c["arguments"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|a| a.as_str().unwrap())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }
    fn clear_calls(&self) {
        let _ = fs::remove_file(self.path.join("claude.jsonl"));
    }
    fn cache(&self, id: &str, version: &str) -> PathBuf {
        let (name, marketplace) = id.split_once('@').unwrap();
        self.store()
            .join("plugins/cache")
            .join(marketplace)
            .join(name)
            .join(version)
    }
    /// The plugin directories a `run` of `name` receives (None when unset).
    fn injected(&self, name: &str) -> Option<String> {
        let out = self.ok(&["run", name]);
        parsed(&out)["env"]["CLAUDE_CODE_PLUGIN_DIRS"]
            .as_str()
            .map(str::to_owned)
    }
}

#[test]
fn marketplace_add_runs_claude_in_the_store_under_the_store_lock_only() {
    let f = Fixture::new();
    f.ok(&["add", "Work"]);
    let out = f
        .command()
        .env("CLAUDE_CODE_OAUTH_TOKEN", "fake-secret")
        .env("CLAUDE_CODE_PLUGIN_DIRS", "/elsewhere")
        .env("CALLER_MARK", "kept")
        .args(["plugin", "marketplace", "add", "tools"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // Claude's own output is not captured.
    assert!(String::from_utf8_lossy(&out.stdout).contains("Added marketplace tools"));
    let marker: Value =
        serde_json::from_slice(&fs::read(f.store().join(".roost-store.json")).unwrap()).unwrap();
    assert_eq!(marker["schema_version"], 1);
    assert_eq!(
        fs::metadata(f.store()).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let calls = f.calls();
    let call = calls
        .iter()
        .find(|c| c["arguments"] == json!(["plugin", "marketplace", "add", "tools"]))
        .unwrap();
    assert_eq!(call["config"], s(&f.store()));
    assert_eq!(call["env"]["DISABLE_AUTOUPDATER"], "1");
    assert_eq!(call["env"]["CALLER_MARK"], "kept");
    assert!(call["env"].get("CLAUDE_CODE_OAUTH_TOKEN").is_none());
    assert!(call["env"].get("CLAUDE_CODE_PLUGIN_DIRS").is_none());
    assert_eq!(call["store_lock_held"], true);
    assert_eq!(call["root_lock_held"], false);
    // The store is never a registration or a listed profile.
    let list = f.json(&["list", "--json"]);
    assert_eq!(list["data"]["profiles"].as_array().unwrap().len(), 1);
    // A failing child is plugin_store naming the retry command.
    let out = f.fails(&["plugin", "marketplace", "add", "missing"], "plugin_store");
    assert!(String::from_utf8_lossy(&out.stderr).contains("roost plugin marketplace add missing"));
}

#[test]
fn add_installs_records_and_injects_into_subscribed_profiles() {
    let f = Fixture::new();
    f.ok(&["add", "Work"]);
    f.ok(&["add", "Other"]);
    f.ok(&["set", "create", "dev"]);
    f.ok(&["set", "subscribe", "Work", "dev"]);
    f.ok(&["plugin", "marketplace", "add", "tools"]);
    f.clear_calls();
    let out = f.ok(&["plugin", "add", "lint@tools", "--set", "dev"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("Installed lint@tools"));
    assert_eq!(
        f.plugin_calls(),
        vec!["plugin install lint@tools", "plugin list --json"]
    );
    let install = &f.calls()[0];
    assert_eq!(install["store_lock_held"], true);
    assert_eq!(install["root_lock_held"], false);
    // Roost's own record holds what Claude's listing reported, nothing else.
    let text = fs::read_to_string(f.store().join(".roost-store-plugins.json")).unwrap();
    assert!(!text.contains("fake-secret"));
    let record: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(record["plugins"][0]["id"], "lint@tools");
    assert_eq!(
        record["plugins"][0]["install_path"],
        s(&f.cache("lint@tools", "v1"))
    );
    let sets = f.json(&["set", "list", "--json"])["data"]["sets"].clone();
    assert_eq!(
        sets[0]["items"],
        json!([{"kind":"plugin","value":"lint@tools"}])
    );
    // The subscribed profile receives the version directory; others do not.
    let expected = f.cache("lint@tools", "v1");
    assert_eq!(f.injected("Work").as_deref(), Some(s(&expected)));
    assert_eq!(f.injected("Other"), None);
    // An inherited nonempty value comes first, joined with ':'.
    let out = f
        .command()
        .env("CLAUDE_CODE_PLUGIN_DIRS", "/inherited/one")
        .args(["run", "Work"])
        .output()
        .unwrap();
    assert_eq!(
        parsed(&out)["env"]["CLAUDE_CODE_PLUGIN_DIRS"],
        format!("/inherited/one:{}", s(&expected))
    );
    // The bound launcher injects too.
    let out = f.with_env(f.root.join("bin/roost-Work")).output().unwrap();
    assert_eq!(parsed(&out)["env"]["CLAUDE_CODE_PLUGIN_DIRS"], s(&expected));
    // Status probes never receive it; launches write nothing into the store.
    let before = fs::read_dir(f.store()).unwrap().count();
    f.clear_calls();
    f.run(&["status", "Work"]);
    let status = &f.calls()[0];
    assert_eq!(status["arguments"], json!(["auth", "status"]));
    assert!(status["env"].get("CLAUDE_CODE_PLUGIN_DIRS").is_none());
    assert_eq!(fs::read_dir(f.store()).unwrap().count(), before);
    // set add --plugin accepts only installed store plugins.
    f.ok(&["set", "create", "more"]);
    f.ok(&["set", "add", "more", "--plugin", "lint@tools"]);
    f.fails(
        &["set", "add", "more", "--plugin", "fmt@tools"],
        "not_found",
    );
}

#[test]
fn bare_names_resolve_through_the_store_listing() {
    let f = Fixture::new();
    f.ok(&["plugin", "marketplace", "add", "tools"]);
    f.ok(&["plugin", "add", "fmt", "--no-set"]);
    // fmt's dependency core was installed by Claude too.
    let list = f.json(&["plugin", "list", "--json"]);
    assert_eq!(
        list["data"],
        json!({"auto_update":false,"last_auto_update":null,"plugins":[
            {"id":"core@tools","sets":[]},{"id":"fmt@tools","sets":[]}]})
    );
    f.ok(&["plugin", "marketplace", "add", "other"]);
    let out = f.fails(&["plugin", "add", "lint", "--no-set"], "not_found");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("ambiguous"));
    assert!(stderr.contains("Name one: roost plugin add lint@MARKETPLACE"));
    let out = f.fails(&["plugin", "add", "nothing", "--no-set"], "not_found");
    assert!(String::from_utf8_lossy(&out.stderr).contains("No marketplace"));
    f.fails(&["plugin", "add", "bad/name", "--no-set"], "usage");
    f.fails(&["plugin", "add", "a@b@c", "--no-set"], "usage");
}

#[test]
fn set_choice_flags_and_conflicts_are_checked_before_install() {
    let f = Fixture::new();
    f.ok(&["add", "Work"]);
    f.ok(&["plugin", "marketplace", "add", "tools"]);
    f.ok(&["plugin", "marketplace", "add", "other"]);
    // No sets at all: nothing to ask.
    f.ok(&["plugin", "add", "core@tools"]);
    f.ok(&["set", "create", "dev"]);
    f.ok(&["set", "create", "ops"]);
    f.ok(&["set", "subscribe", "Work", "dev", "ops"]);
    // Sets exist and there is no terminal: usage naming both flags.
    let out = f.run(&["plugin", "add", "lint@tools"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--set") && stderr.contains("--no-set"),
        "{stderr}"
    );
    let both = f.run(&["plugin", "add", "lint@tools", "--set", "dev", "--no-set"]);
    assert_eq!(both.status.code(), Some(2));
    f.fails(
        &["plugin", "add", "lint@tools", "--set", "missing"],
        "not_found",
    );
    f.ok(&["plugin", "add", "lint@tools", "--set", "dev"]);
    // The same plugin name from another marketplace would conflict for Work.
    f.clear_calls();
    f.fails(
        &["plugin", "add", "lint@other", "--set", "ops"],
        "collision",
    );
    assert!(!f.plugin_calls().iter().any(|c| c.contains("install")));
    // A failed install changes no sets and names the retry command.
    let out = f
        .command()
        .env("FAKE_PLUGIN_FAIL", "install")
        .args(["plugin", "add", "fmt@tools", "--set", "ops"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("plugin_store"), "{stderr}");
    assert!(
        stderr.contains("roost plugin add fmt@tools --set ops"),
        "{stderr}"
    );
    let sets = f.json(&["set", "list", "--json"])["data"]["sets"].clone();
    assert_eq!(sets[1]["name"], "ops");
    assert_eq!(sets[1]["items"], json!([]));
}

/// Runs `roost ARGS` on a pseudo-terminal, answers the first prompt containing
/// `marker` with `answer`, and returns (exit code, terminal output).
fn on_terminal(f: &Fixture, args: &[&str], marker: &str, answer: &str) -> (i32, String) {
    let script = r#"
import json,os,pty,select,sys,time
exe,marker,answer=sys.argv[1],sys.argv[2].encode(),sys.argv[3].encode()
pid,fd=pty.fork()
if pid==0:os.execv(exe,[exe,*json.loads(sys.argv[4])])
out=b'';deadline=time.monotonic()+10
while marker not in out and time.monotonic()<deadline:
    if select.select([fd],[],[],.1)[0]:out+=os.read(fd,4096)
os.write(fd,answer)
while time.monotonic()<deadline:
    if not select.select([fd],[],[],.1)[0]:continue
    try:chunk=os.read(fd,4096)
    except OSError:break
    if not chunk:break
    out+=chunk
_,status=os.waitpid(pid,0)
print(json.dumps({'exit':os.waitstatus_to_exitcode(status),'output':out.decode(errors='replace')}))
"#;
    let out = f
        .with_env(PathBuf::from("/usr/bin/python3"))
        .args([
            "-c",
            script,
            env!("CARGO_BIN_EXE_roost"),
            marker,
            answer,
            &serde_json::to_string(args).unwrap(),
        ])
        .output()
        .unwrap();
    let result = parsed(&out);
    (
        result["exit"].as_i64().unwrap() as i32,
        result["output"].as_str().unwrap().to_owned(),
    )
}

#[test]
fn a_terminal_prompt_reads_a_comma_separated_set_list() {
    let f = Fixture::new();
    f.ok(&["plugin", "marketplace", "add", "tools"]);
    f.ok(&["set", "create", "dev"]);
    f.ok(&["set", "create", "ops"]);
    let (exit, output) = on_terminal(
        &f,
        &["plugin", "add", "lint@tools"],
        "which sets",
        "dev, ops\n",
    );
    assert_eq!(exit, 0, "{output}");
    assert!(output.contains("dev, ops"), "{output}");
    let sets = f.json(&["set", "list", "--json"])["data"]["sets"].clone();
    for set in [0, 1] {
        assert_eq!(
            sets[set]["items"],
            json!([{"kind":"plugin","value":"lint@tools"}])
        );
    }
    // An empty answer installs without sets; Ctrl-C cancels before installing.
    let (exit, _) = on_terminal(&f, &["plugin", "add", "fmt@tools"], "which sets", "\n");
    assert_eq!(exit, 0);
    assert_eq!(
        f.json(&["plugin", "list", "--json"])["data"]["plugins"][1],
        json!({"id":"fmt@tools","sets":[]})
    );
    f.clear_calls();
    let (exit, _) = on_terminal(&f, &["plugin", "add", "fmt2@other"], "which sets", "\x03");
    assert_eq!(exit, 130);
    assert!(f.plugin_calls().is_empty());
}

/// A fixture with Work subscribed to set `dev` holding `lint@tools`.
fn with_lint() -> Fixture {
    let f = Fixture::new();
    f.ok(&["add", "Work"]);
    f.ok(&["set", "create", "dev"]);
    f.ok(&["set", "subscribe", "Work", "dev"]);
    f.ok(&["plugin", "marketplace", "add", "tools"]);
    f.ok(&["plugin", "add", "lint@tools", "--set", "dev"]);
    f
}

#[test]
fn add_refuses_to_record_sets_when_the_root_changed_during_install() {
    let f = Fixture::new();
    f.ok(&["add", "Work"]);
    f.ok(&["set", "create", "dev"]);
    f.ok(&["plugin", "marketplace", "add", "tools"]);
    let out = f
        .command()
        .env("FAKE_SWAP_ROOT", env!("CARGO_BIN_EXE_roost"))
        .args(["plugin", "add", "lint@tools", "--set", "dev"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("ownership"), "{stderr}");
    let sets: Value = serde_json::from_slice(&fs::read(f.root.join("sets.json")).unwrap()).unwrap();
    assert_eq!(sets["sets"][0]["name"], "dev");
    assert_eq!(sets["sets"][0]["items"], json!([]), "{sets}");
}

#[test]
fn remove_drops_the_plugin_from_sets_before_uninstalling() {
    let f = with_lint();
    let out = f
        .command()
        .env("FAKE_PLUGIN_FAIL", "uninstall")
        .args(["plugin", "remove", "lint"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr.contains("plugin_store"), "{stderr}");
    assert!(
        stderr.contains("roost plugin remove lint@tools"),
        "{stderr}"
    );
    // Sets changed first, so the profile already stops receiving it.
    let sets = f.json(&["set", "list", "--json"])["data"]["sets"].clone();
    assert_eq!(sets[0]["items"], json!([]));
    assert_eq!(f.injected("Work"), None);
    assert!(f.cache("lint@tools", "v1").exists());
    f.clear_calls();
    f.ok(&["plugin", "remove", "lint@tools"]);
    assert_eq!(
        f.plugin_calls(),
        vec!["plugin uninstall lint@tools", "plugin list --json"]
    );
    assert!(!f.cache("lint@tools", "v1").exists());
    assert_eq!(
        f.json(&["plugin", "list", "--json"])["data"]["plugins"],
        json!([])
    );
    f.fails(&["plugin", "remove", "lint"], "not_found");
}

#[test]
fn update_moves_injection_to_the_new_version_and_prunes_old_orphans() {
    let f = with_lint();
    let old = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
        - 15 * 24 * 60 * 60 * 1000)
        .to_string();
    // A stray old orphan Claude never swept, plus an old orphan from this update.
    let stray = f.store().join("plugins/cache/tools/lint/v0");
    fs::create_dir_all(&stray).unwrap();
    fs::write(stray.join(".orphaned_at"), &old).unwrap();
    f.clear_calls();
    let out = f
        .command()
        .env("FAKE_ORPHANED_AT", &old)
        .args(["plugin", "update", "lint"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        f.plugin_calls(),
        vec!["plugin update lint@tools", "plugin list --json"]
    );
    assert_eq!(f.calls()[0]["store_lock_held"], true);
    assert_eq!(f.calls()[0]["root_lock_held"], false);
    assert_eq!(
        f.injected("Work").as_deref(),
        Some(s(&f.cache("lint@tools", "v2")))
    );
    assert!(!f.cache("lint@tools", "v1").exists());
    assert!(!stray.exists());
    // A recent orphan stays inside the grace period.
    f.ok(&["plugin", "update"]);
    assert!(f.cache("lint@tools", "v2").join(".orphaned_at").exists());
    assert_eq!(
        f.injected("Work").as_deref(),
        Some(s(&f.cache("lint@tools", "v3")))
    );
    let out = f
        .command()
        .env("FAKE_PLUGIN_FAIL", "update")
        .args(["plugin", "update"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("roost plugin update"));
    f.fails(&["plugin", "update", "fmt@tools"], "not_found");
}

#[test]
fn injection_follows_dependencies_and_skips_unsafe_directories_with_warnings() {
    let f = with_lint();
    f.ok(&["plugin", "marketplace", "add", "other"]);
    f.ok(&["plugin", "add", "fmt@tools", "--set", "dev"]);
    // fmt depends on core, which reaches the profile without being in a set.
    let dirs = f.injected("Work").unwrap();
    let mut entries: Vec<&str> = dirs.split(':').collect();
    entries.sort();
    let mut expected = [
        f.cache("core@tools", "v1"),
        f.cache("fmt@tools", "v1"),
        f.cache("lint@tools", "v1"),
    ];
    expected.sort();
    assert_eq!(entries, expected.iter().map(|p| s(p)).collect::<Vec<_>>());
    // Two store plugins with the same manifest name are both refused.
    f.ok(&["set", "create", "extra"]);
    f.ok(&["plugin", "add", "fmt2@other", "--set", "extra"]);
    f.ok(&["set", "subscribe", "Work", "extra"]);
    let out = f.ok(&["run", "Work"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("share the plugin name fmt"), "{stderr}");
    let dirs = parsed(&out)["env"]["CLAUDE_CODE_PLUGIN_DIRS"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        !dirs.contains("/fmt/") && !dirs.contains("/fmt2/"),
        "{dirs}"
    );
    assert!(dirs.contains("/core/") && dirs.contains("/lint/"), "{dirs}");
    f.ok(&["set", "unsubscribe", "Work", "extra"]);
    // Orphaned and missing version directories are skipped; the launch proceeds.
    fs::write(f.cache("lint@tools", "v1").join(".orphaned_at"), "1").unwrap();
    fs::remove_dir_all(f.cache("core@tools", "v1")).unwrap();
    let out = f.ok(&["run", "Work"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Skipped plugin lint@tools"), "{stderr}");
    assert!(stderr.contains("Skipped plugin core@tools"), "{stderr}");
    assert_eq!(
        parsed(&out)["env"]["CLAUDE_CODE_PLUGIN_DIRS"],
        s(&f.cache("fmt@tools", "v1"))
    );
    // A symlinked version directory is never followed.
    fs::remove_dir_all(f.cache("fmt@tools", "v1")).unwrap();
    std::os::unix::fs::symlink(&f.path, f.cache("fmt@tools", "v1")).unwrap();
    assert_eq!(f.injected("Work"), None);
    // An unreadable record skips injection with a warning and still launches.
    fs::write(f.store().join(".roost-store-plugins.json"), b"garbage").unwrap();
    let out = f.ok(&["run", "Work"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("launching without them"));
    assert!(parsed(&out)["env"].get("CLAUDE_CODE_PLUGIN_DIRS").is_none());
}

#[test]
fn upstream_switch_bare_and_desktop_launches_inject_but_aliases_and_update_do_not() {
    let f = with_lint();
    let expected = f.cache("lint@tools", "v1");
    let upstream = f.upstream("Up");
    f.ok(&["register", "Up", "--path", s(&upstream)]);
    f.ok(&["set", "subscribe", "Up", "dev"]);
    assert_eq!(f.injected("Up").as_deref(), Some(s(&expected)));
    // Nothing was written into the borrowed upstream directory.
    assert_eq!(fs::read_dir(&upstream).unwrap().count(), 0);
    // Selected-profile launches.
    let out = f.ok(&["switch", "Work"]);
    assert_eq!(parsed(&out)["env"]["CLAUDE_CODE_PLUGIN_DIRS"], s(&expected));
    let out = f.ok(&[]);
    assert_eq!(parsed(&out)["env"]["CLAUDE_CODE_PLUGIN_DIRS"], s(&expected));
    // Desktop gets the same environment.
    #[cfg(target_os = "linux")]
    {
        f.ok(&["desktop", "--foreground", "Work"]);
        let record: Value = serde_json::from_str(
            fs::read_to_string(f.path.join("desktop.jsonl"))
                .unwrap()
                .lines()
                .last()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(record["env"]["CLAUDE_CODE_PLUGIN_DIRS"], s(&expected));
    }
    // Aliases and the shared updater keep the caller environment exactly.
    f.ok(&["add", "personal", "--link-default"]);
    f.fails(&["set", "subscribe", "personal", "dev"], "usage");
    f.ok(&["plugin", "auto-update", "--on"]);
    f.clear_calls();
    for args in [vec!["run", "personal"], vec!["update"]] {
        let out = f
            .command()
            .env("CLAUDE_CODE_PLUGIN_DIRS", "/inherited")
            .args(&args)
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_eq!(parsed(&out)["env"]["CLAUDE_CODE_PLUGIN_DIRS"], "/inherited");
    }
    assert!(f.plugin_calls().is_empty(), "{:?}", f.plugin_calls());
}

fn set_auto_update_at(f: &Fixture, at: u64) {
    let path = f.root.join("state.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    state["plugin_auto_update_at"] = json!(at);
    fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
}

#[test]
fn daily_auto_update_runs_once_per_window_and_never_blocks_the_launch() {
    let f = with_lint();
    f.clear_calls();
    f.ok(&["run", "Work"]);
    assert!(f.plugin_calls().is_empty(), "off by default");
    f.ok(&["plugin", "auto-update", "--on"]);
    let out = f.ok(&["run", "Work"]);
    // The update ran before Claude started, with stdin not a terminal, under the
    // store lock, and the launch already uses the new version.
    let calls = f.calls();
    let update = calls
        .iter()
        .find(|c| c["arguments"] == json!(["plugin", "update", "lint@tools"]))
        .unwrap();
    assert_eq!(update["store_lock_held"], true);
    assert_eq!(update["root_lock_held"], false);
    assert_eq!(update["stdin_tty"], false);
    // The record refresh after it also runs without the root lock.
    let refresh = calls.iter().find(|c| c["arguments"][1] == "list").unwrap();
    assert_eq!(refresh["root_lock_held"], false);
    assert_eq!(
        parsed(&out)["env"]["CLAUDE_CODE_PLUGIN_DIRS"],
        s(&f.cache("lint@tools", "v2"))
    );
    let list = f.json(&["plugin", "list", "--json"])["data"].clone();
    assert_eq!(list["auto_update"], true);
    assert!(list["last_auto_update"].as_u64().is_some());
    // Within 24 hours nothing runs again.
    f.clear_calls();
    f.ok(&["run", "Work"]);
    assert!(f.plugin_calls().is_empty());
    // A failing update warns and launches.
    set_auto_update_at(&f, 1);
    let out = f
        .command()
        .env("FAKE_PLUGIN_FAIL", "update")
        .args(["run", "Work"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Plugin auto-update stopped"), "{stderr}");
    assert!(parsed(&out)["arguments"] == json!([]));
    // A hanging update is stopped after 8 seconds; the launch proceeds.
    set_auto_update_at(&f, 1);
    let started = std::time::Instant::now();
    let out = f
        .command()
        .env("FAKE_PLUGIN_MODE", "slow")
        .args(["run", "Work"])
        .output()
        .unwrap();
    let elapsed = started.elapsed();
    assert!(out.status.success());
    assert!(
        elapsed >= std::time::Duration::from_secs(7)
            && elapsed < std::time::Duration::from_secs(20),
        "{elapsed:?}"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("Plugin auto-update stopped"));
    // The timestamp was written first, so the next launch does not retry.
    f.clear_calls();
    f.ok(&["run", "Work"]);
    assert!(f.plugin_calls().is_empty());
    f.ok(&["plugin", "auto-update", "--off"]);
    assert_eq!(
        f.json(&["plugin", "list", "--json"])["data"]["auto_update"],
        false
    );
    assert_eq!(f.run(&["plugin", "auto-update"]).status.code(), Some(2));
    assert_eq!(
        f.run(&["plugin", "auto-update", "--on", "--off"])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn auto_update_skips_silently_while_a_store_command_holds_the_store_lock() {
    let f = with_lint();
    f.ok(&["plugin", "auto-update", "--on"]);
    let mut busy = f
        .command()
        .env("FAKE_PLUGIN_MODE", "slow")
        .args(["plugin", "marketplace", "add", "other"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !f
        .plugin_calls()
        .iter()
        .any(|c| c.contains("marketplace add other"))
    {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let started = std::time::Instant::now();
    let out = f.ok(&["run", "Work"]);
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert!(String::from_utf8_lossy(&out.stderr).is_empty());
    assert!(!f.plugin_calls().iter().any(|c| c.contains("update")));
    busy.kill().unwrap();
    busy.wait().unwrap();
}

#[test]
fn doctor_reports_store_plugins_shadowing_native_installs() {
    let f = with_lint();
    let profile = f.root.join("profiles/Work");
    fs::write(
        profile.join("fake-state.json"),
        json!({"marketplaces":["tools"],"installed":{"lint@tools":"v9"}}).to_string(),
    )
    .unwrap();
    let out = f.run(&["doctor", "--json"]);
    let findings = parsed(&out)["data"]["findings"].clone();
    let shadowed: Vec<&Value> = findings
        .as_array()
        .unwrap()
        .iter()
        .filter(|x| x["code"] == "plugin_shadowed")
        .collect();
    assert_eq!(shadowed.len(), 1, "{findings}");
    assert_eq!(shadowed[0]["severity"], "warning");
    assert_eq!(
        shadowed[0]["next_step"],
        "roost run Work plugin uninstall lint@tools"
    );
    let probe = f
        .calls()
        .into_iter()
        .find(|c| c["config"] == s(&profile))
        .unwrap();
    assert_eq!(probe["arguments"], json!(["plugin", "list", "--json"]));
    assert_eq!(probe["root_lock_held"], false);
}
