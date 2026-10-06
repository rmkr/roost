//! Plugin store and launch-time injection (spec "Plugin store", gate A17).
//! Disposable fixtures only: a fake `claude` that emulates `plugin` subcommands in
//! its CLAUDE_CONFIG_DIR, private temp root/home, no real Claude, account or network.
#![cfg(unix)]
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

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
    install(pid);save();print('Installed '+pid);sys.exit(0)
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

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    path: PathBuf,
    root: PathBuf,
    home: PathBuf,
    catalog: Value,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "roost-plugins-{}-{}",
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
        fs::write(bin.join("claude"), FAKE).unwrap();
        fs::set_permissions(bin.join("claude"), fs::Permissions::from_mode(0o700)).unwrap();
        let desktop = bin.join("claude-desktop");
        fs::write(
            &desktop,
            "#!/usr/bin/python3\nimport json,os,sys\nopen(os.environ['FAKE_DESKTOP_RECORD'],'a').write(json.dumps({'arguments':sys.argv[1:],'env':dict(os.environ)})+'\\n')\n",
        )
        .unwrap();
        fs::set_permissions(&desktop, fs::Permissions::from_mode(0o700)).unwrap();
        let root = path.join("root");
        Self {
            path,
            root,
            home,
            catalog: json!({
                "tools": {"lint": {}, "fmt": {"deps": ["core"]}, "core": {}},
                "other": {"lint": {}, "fmt2": {"manifest": "fmt"}}
            }),
        }
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_roost"));
        command
            .env_clear()
            .env("ROOST_DIR", &self.root)
            .env("HOME", &self.home)
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.path.join("bin").display()),
            )
            .env("SHELL", "/bin/bash")
            .env("FAKE_LOG", self.path.join("claude.jsonl"))
            .env("FAKE_CATALOG", self.catalog.to_string())
            .env("FAKE_DESKTOP_RECORD", self.path.join("desktop.jsonl"))
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
    fn fails(&self, args: &[&str], code: &str) -> Output {
        let out = self.run(args);
        assert!(!out.status.success(), "{args:?} unexpectedly succeeded");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(code), "{args:?}: {stderr}");
        out
    }
    fn json(&self, args: &[&str]) -> Value {
        parsed(&self.ok(args))
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
fn s(path: &Path) -> &str {
    path.to_str().unwrap()
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
