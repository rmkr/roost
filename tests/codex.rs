#![cfg(unix)]

mod common;
use common::{Fixture, private, s};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    process::Stdio,
};

const FILE_AUTH: &str = "cli_auth_credentials_store = \"file\"";
const FAKE_CODEX: &str = r#"#!/bin/sh
set -eu
[ "$#" -eq 3 ]
[ "$1" = -c ]
[ "$2" = 'cli_auth_credentials_store = "file"' ]
[ "$3" = login ]
[ "$PWD" = / ]
[ -z "${OPENAI_API_KEY+x}${CODEX_API_KEY+x}${CODEX_ACCESS_TOKEN+x}" ]
[ "$CODEX_HOME" = "$ROOST_DIR/codex/work" ]
umask 077
printf '%s\n' 'fixture credential' > "$CODEX_HOME/auth.json"
printf '%s\n' 'native output'
exit 17
"#;

#[test]
fn add_and_home_print_the_absolute_private_path() {
    let f = Fixture::with_fakes("codex-path", &[]).with_var("ROOST_DIR", "state folder");
    let root = f.path.join("state folder");
    let work = root.join("codex/work");
    let path_line = format!("{}\n", work.display());
    f.fails(&["codex", "home", "work"], "not_found");
    f.fails(&["codex", "login", "work"], "not_found");
    assert_eq!(
        f.fails(&["codex", "add", "../work"], "usage").status.code(),
        Some(2)
    );
    assert!(!root.exists());

    assert_eq!(f.ok(&["codex", "add", "Work"]).stdout, path_line.as_bytes());
    assert_eq!(
        f.ok(&["codex", "home", "WORK"]).stdout,
        path_line.as_bytes()
    );
    assert_eq!(
        fs::metadata(&work).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let config = work.join("config.toml");
    assert_eq!(
        fs::metadata(&config).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::read_to_string(&config).unwrap(),
        format!("{FILE_AUTH}\n")
    );
    assert!(!work.join("auth.json").exists());
    f.fails(&["codex", "add", "work"], "collision");
    f.fails(&["codex", "home", "missing"], "not_found");
    assert!(!root.join("codex/missing").exists());

    let default = f
        .command()
        .env_remove("ROOST_DIR")
        .args(["codex", "add", "test"])
        .output()
        .unwrap();
    assert!(default.status.success());
    assert_eq!(
        default.stdout,
        format!("{}\n", f.home.join(".roost/codex/test").display()).as_bytes()
    );
}

#[test]
fn native_login_uses_only_the_requested_credential_file() {
    let f = Fixture::with_fakes("codex-login", &[("codex", FAKE_CODEX)]);
    let incoming = f.path.join("incoming-CODEX_HOME");
    let default = f.home.join(".codex");
    for dir in [&incoming, &default] {
        fs::create_dir(dir).unwrap();
        fs::write(dir.join("auth.json"), b"existing credential").unwrap();
    }
    let f = f
        .with_var("CODEX_HOME", incoming.clone())
        .with_var("OPENAI_API_KEY", "fake-key")
        .with_var("CODEX_API_KEY", "fake-codex-key")
        .with_var("CODEX_ACCESS_TOKEN", "fake-token");
    f.ok(&["codex", "add", "work"]);
    f.ok(&["codex", "add", "personal"]);
    assert!(!f.root.join("codex/work/auth.json").exists());

    let output = f.run(&["codex", "login", "work"]);
    assert_eq!(
        output.status.code(),
        Some(17),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"native output\n");
    assert_eq!(
        fs::read(f.root.join("codex/work/auth.json")).unwrap(),
        b"fixture credential\n"
    );
    f.ok(&["codex", "home", "work"]);
    assert!(!f.root.join("codex/personal/auth.json").exists());
    for dir in [incoming, default] {
        assert_eq!(
            fs::read(dir.join("auth.json")).unwrap(),
            b"existing credential"
        );
    }

    let missing = f
        .command()
        .env("PATH", &f.home)
        .args(["codex", "login", "work"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("codex_unavailable"));
}

#[test]
fn home_and_login_reject_foreign_or_redirected_state() {
    let f = Fixture::with_fakes("codex-ownership", &[("codex", FAKE_CODEX)]);
    f.ok(&["codex", "add", "work"]);
    let parent = f.root.join("codex");
    let work = parent.join("work");
    let foreign = parent.join("foreign");
    fs::create_dir(&foreign).unwrap();
    private(&foreign);
    f.fails(&["codex", "add", "foreign"], "collision");
    f.fails(&["codex", "home", "foreign"], "ownership");
    assert_eq!(fs::read_dir(&foreign).unwrap().count(), 0);

    let external_auth = f.home.join("auth.json");
    fs::write(&external_auth, b"external credential").unwrap();
    symlink(&external_auth, work.join("auth.json")).unwrap();
    f.fails(&["codex", "login", "work"], "unsafe_path");
    assert_eq!(fs::read(&external_auth).unwrap(), b"external credential");
    fs::remove_file(work.join("auth.json")).unwrap();
    fs::hard_link(&external_auth, work.join("auth.json")).unwrap();
    f.fails(&["codex", "home", "work"], "unsafe_path");

    fs::rename(&work, parent.join("moved")).unwrap();
    symlink(parent.join("moved"), &work).unwrap();
    f.fails(&["codex", "login", "work"], "unsafe_path");
    f.fails(&["codex", "home", "moved"], "ownership");
}

#[test]
fn codex_and_claude_can_use_the_same_name() {
    let f = Fixture::with_fakes("codex-coexist", &[]);
    f.ok(&["add", "work"]);
    let registry = f.root.join("registry.json");
    let before = fs::read(&registry).unwrap();
    f.ok(&["codex", "add", "work"]);
    assert_eq!(fs::read(&registry).unwrap(), before);
    assert_eq!(
        f.ok(&["where", "work"]).stdout,
        format!("{}\n", f.root.join("profiles/work").display()).as_bytes()
    );
    f.ok(&["remove", "work", "--purge", "--yes"]);
    f.ok(&["codex", "home", "work"]);
}

#[test]
fn concurrent_adds_publish_only_one_home() {
    let f = Fixture::with_fakes("codex-concurrent", &[]);
    let mut add = f.command();
    add.args(["codex", "add", "work"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let first = add.spawn().unwrap();
    let second = add.output().unwrap();
    let first = first.wait_with_output().unwrap();
    let (success, collision) = if first.status.success() {
        (first, second)
    } else {
        (second, first)
    };
    assert!(
        success.status.success(),
        "{}",
        String::from_utf8_lossy(&success.stderr)
    );
    assert_eq!(collision.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&collision.stderr).contains("collision"));
    let work = f.root.join("codex/work");
    assert_eq!(success.stdout, format!("{}\n", s(&work)).as_bytes());
    assert_eq!(fs::read_dir(f.root.join("codex")).unwrap().count(), 1);
}

const PLUGIN_CODEX: &str = r#"#!/usr/bin/python3
import json,os,pathlib,shutil,sys
assert os.getcwd()=='/'
assert all(key not in os.environ for key in ('OPENAI_API_KEY','CODEX_API_KEY','CODEX_ACCESS_TOKEN'))
assert sys.argv[1:3]==['-c','cli_auth_credentials_store = "file"']
args=sys.argv[3:]
market_root=os.environ['FAKE_MARKET_ROOT']
if args[:1]==['-c']:
    market_root=json.loads(args[1].split('source=',1)[1][:-1])
    args=args[2:]
assert args[0]=='plugin'
pid='demo@tools'
home=pathlib.Path(os.environ['CODEX_HOME'])
cache=home/'plugins/cache/tools/demo/1.0'
row={'pluginId':pid,'version':'1.0','installed':True}
with (home/'calls').open('a') as calls:
    calls.write(' '.join(args)+'\n')
if args[1]=='list':
    print(json.dumps({'installed':[row] if cache.exists() else [],'available':[]}))
elif args[1:3]==['marketplace','list']:
    print(json.dumps({'marketplaces':[{
        'name':'tools',
        'marketplaceRoot':market_root,
        'marketplaceSource':{'sourceType':'local','source':market_root},
    }]}))
elif args[1:3]==['marketplace','add']:
    print(json.dumps({'name':'tools'}))
elif args[1]=='add':
    assert args[2]==pid
    cache.mkdir(parents=True)
    (cache/'plugin.json').write_text(json.dumps({'name':'demo'}))
    print(json.dumps(row))
elif args[1]=='remove':
    assert args[2]==pid
    shutil.rmtree(cache)
    print(json.dumps({'pluginId':pid}))
else:
    sys.exit(2)
"#;

#[test]
fn managed_native_plugins_target_verified_homes_and_sync_future_profiles() {
    let f = Fixture::with_fakes("codex-plugin", &[("codex", PLUGIN_CODEX)]);
    let marketplace = f.path.join("marketplace");
    fs::create_dir(&marketplace).unwrap();
    let f = f
        .with_var("FAKE_MARKET_ROOT", marketplace)
        .with_var("OPENAI_API_KEY", "fixture")
        .with_var("CODEX_ACCESS_TOKEN", "fixture");
    for name in ["work", "personal"] {
        f.ok(&["codex", "add", name]);
    }
    let target = |name| {
        f.root
            .join("codex")
            .join(name)
            .join("plugins/cache/tools/demo/1.0")
    };
    let selected = [
        "plugin",
        "install",
        "codex:demo@tools",
        "--all-repos",
        "--profile",
        "codex:work",
        "--yes",
    ];
    f.ok(&selected);
    f.ok(&selected);
    assert!(target("work").exists());
    assert!(!target("personal").exists());
    let calls = fs::read_to_string(f.root.join("codex/work/calls")).unwrap();
    assert_eq!(
        calls
            .lines()
            .filter(|line| line.starts_with("plugin add "))
            .count(),
        1
    );

    f.ok(&[
        "plugin",
        "install",
        "codex:demo@tools",
        "--all-repos",
        "--all-profiles",
        "--yes",
    ]);
    assert!(target("personal").exists());
    f.ok(&["codex", "add", "later"]);
    let listed = f.json(&["plugin", "list", "--managed", "--json"]);
    let rules = listed["data"]["rules"].as_array().unwrap();
    assert_eq!(rules.len(), 2);
    assert!(
        rules
            .iter()
            .flat_map(|rule| rule["targets"].as_array().unwrap())
            .any(|entry| {
                entry["target"]["profile"]["label"] == "codex:later"
                    && entry["status"]["installed"].is_null()
            })
    );
    f.ok(&["plugin", "sync", "--yes"]);
    assert!(target("later").exists());

    let first = rules[0]["id"].as_str().unwrap();
    f.ok(&["plugin", "uninstall", first, "--yes"]);
    assert!(target("work").exists());
    f.ok(&["plugin", "uninstall", "codex:demo@tools", "--yes"]);
    for name in ["work", "personal", "later"] {
        assert!(!target(name).exists());
    }
}
