#![cfg(unix)]

mod common;
use common::{Fixture, parsed, s};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const BUNX: &str = r#"#!/usr/bin/python3
import hashlib,json,os,pathlib,sys
args=sys.argv[1:]
if 'add' not in args or not any(arg.startswith('skills@') for arg in args):
    sys.exit(2)
source=args[args.index('add')+1]
revision=pathlib.Path(os.environ['FAKE_SKILL_REVISION']).read_text().strip()
skill='demo'
body='---\nname: demo\ndescription: Roost test\n---\n'+revision+'\n'
paths=[]
if 'codex' in args:
    paths.append(pathlib.Path('.agents/skills/demo'))
if 'claude-code' in args:
    paths.append(pathlib.Path('.claude/skills/demo'))
if not paths:
    sys.exit(2)
for path in paths:
    path.mkdir(parents=True,exist_ok=True)
    (path/'SKILL.md').write_text(body)
digest=hashlib.sha256(body.encode()).hexdigest()
lock={'version':2,'skills':{skill:{'source':source,'sourceType':'github','sourceUrl':'https://github.com/'+source+'.git','skillPath':'skills/demo/SKILL.md','computedHash':digest,'installMode':'copy'}}}
pathlib.Path('skills-lock.json').write_text(json.dumps(lock))
print(json.dumps([{'name':skill,'status':'installed','source':source,'hash':digest,'path':str(paths[0].resolve()),'scope':'project','agents':[agent for agent in ['Claude Code','Codex'] if agent.lower().replace(' ','-') in args],'mode':'copy','security':None}]))
"#;

fn fixture() -> Fixture {
    let fixture = Fixture::with_fakes("managed-plugins", &[("bunx", BUNX)]);
    let revision = fixture.path.join("revision");
    fs::write(&revision, "first").unwrap();
    fixture.with_var("FAKE_SKILL_REVISION", revision)
}

fn git(path: &Path, arguments: &[&str]) {
    let output = Command::new("/usr/bin/git")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", path)
        .current_dir(path)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repo(fixture: &Fixture, name: &str) -> PathBuf {
    let path = fixture.path.join(name);
    fs::create_dir(&path).unwrap();
    git(&path, &["init", "-q"]);
    path
}

#[test]
fn repository_writes_require_real_git_metadata_and_ignore_redirects() {
    let f = fixture();
    let checkout = repo(&f, "checkout");
    let foreign = f.path.join("folder");
    fs::create_dir_all(foreign.join(".git")).unwrap();
    let command = f
        .command()
        .env("GIT_DIR", checkout.join(".git"))
        .env("GIT_WORK_TREE", &foreign)
        .args(["project", "add", s(&foreign), "--json"])
        .output()
        .unwrap();
    assert_eq!(parsed(&command)["error"]["code"], "not_repository");
    assert!(!f.root.exists());

    let subdir = checkout.join("nested");
    fs::create_dir(&subdir).unwrap();
    let added = f.json(&["project", "add", s(&subdir), "--json"]);
    assert_eq!(added["data"]["repository"]["path"], s(&checkout));
    let list = f.json(&["project", "list", "--json"]);
    assert_eq!(list["data"]["repositories"].as_array().unwrap().len(), 1);
}

#[test]
fn portable_install_is_scoped_and_updates_preserve_local_edits() {
    let f = fixture();
    let left = repo(&f, "left");
    let right = repo(&f, "right");
    let excluded = repo(&f, "excluded");
    let install = [
        "plugin",
        "install",
        "example/skills",
        "--skills",
        "--skill",
        "demo",
        "--repo",
        s(&left),
        "--repo",
        s(&right),
        "--agent",
        "claude",
        "--agent",
        "codex",
    ];
    let preview = f.json(&[install.as_slice(), &["--dry-run", "--json"]].concat());
    assert_eq!(
        preview["data"]["rule"]["targets"].as_array().unwrap().len(),
        2
    );
    assert!(!f.root.exists());
    f.ok(&[install.as_slice(), &["--yes"]].concat());
    f.ok(&[install.as_slice(), &["--yes"]].concat());

    let file = ".agents/skills/demo/SKILL.md";
    let claude_file = ".claude/skills/demo/SKILL.md";
    for dir in [&left, &right] {
        assert!(
            fs::read_to_string(dir.join(file))
                .unwrap()
                .contains("first")
        );
        assert!(
            fs::read_to_string(dir.join(claude_file))
                .unwrap()
                .contains("first")
        );
    }
    assert!(!excluded.join(".agents").exists());
    assert!(!excluded.join("skills-lock.json").exists());

    fs::write(f.path.join("revision"), "second").unwrap();
    let report = f.json(&["plugin", "check", "--json"]);
    let rule = &report["data"]["rules"][0];
    assert!(rule["targets"][0]["status"]["candidate"].is_string());
    assert!(
        fs::read_to_string(left.join(file))
            .unwrap()
            .contains("first")
    );
    fs::write(right.join(file), "local edit").unwrap();
    let report = f.run(&["plugin", "update", "--managed", "--yes", "--json"]);
    assert!(!report.status.success());
    assert_eq!(parsed(&report)["error"]["code"], "managed_plugin_failed");
    assert!(
        fs::read_to_string(left.join(file))
            .unwrap()
            .contains("second")
    );
    assert_eq!(fs::read_to_string(right.join(file)).unwrap(), "local edit");
    assert!(!excluded.join("skills-lock.json").exists());
}

#[test]
fn sync_enrolls_registered_worktree_without_exposing_claude_skill_to_codex() {
    let f = fixture();
    let checkout = repo(&f, "checkout");
    git(
        &checkout,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "commit",
            "--allow-empty",
            "-qm",
            "fixture",
        ],
    );
    let linked = f.path.join("linked");
    git(
        &checkout,
        &["worktree", "add", "--detach", "-q", s(&linked)],
    );
    f.ok(&["project", "add", s(&checkout)]);
    f.ok(&[
        "plugin",
        "install",
        "example/skills",
        "--all-repos",
        "--skills",
        "--skill",
        "demo",
        "--agent",
        "claude",
        "--yes",
    ]);
    f.ok(&["project", "add", s(&linked)]);
    let statuses = f.json(&["plugin", "list", "--managed", "--json"]);
    let targets = statuses["data"]["rules"][0]["targets"].as_array().unwrap();
    assert_eq!(targets.len(), 2);
    assert!(targets[1]["status"]["installed"].is_null());
    assert!(!linked.join(".claude").exists());

    f.ok(&["plugin", "sync", "--yes"]);
    for dir in [&checkout, &linked] {
        assert!(dir.join(".claude/skills/demo/SKILL.md").exists());
        assert!(!dir.join(".agents/skills/demo").exists());
    }
    f.ok(&["plugin", "uninstall", "example/skills", "--yes"]);
    assert!(!checkout.join(".claude/skills/demo").exists());
    assert!(!linked.join(".claude/skills/demo").exists());
}

#[test]
fn native_codex_refuses_selected_repositories_and_preview_is_read_only() {
    let f = fixture();
    let checkout = repo(&f, "checkout");
    f.ok(&["codex", "add", "work"]);
    let generation = f.root.join("codex/work/.roost-codex-generation");
    fs::remove_file(&generation).unwrap();
    f.fails(
        &[
            "plugin",
            "install",
            "codex:demo@tools",
            "--repo",
            s(&checkout),
            "--profile",
            "codex:work",
            "--dry-run",
        ],
        "unsupported_scope",
    );
    f.json(&[
        "plugin",
        "install",
        "codex:demo@tools",
        "--all-repos",
        "--profile",
        "codex:work",
        "--dry-run",
        "--json",
    ]);
    assert!(!generation.exists());
    assert!(!f.root.join("managed-plugins.json").exists());
}

const CLAUDE: &str = r#"#!/bin/sh
if [ "${1:-}" = --version ]; then
    printf '%s\n' '2.1.280 (Claude Code)'
else
    printf '%s\n' started-claude
fi
"#;

#[test]
fn an_update_prompt_can_be_deferred_for_the_current_revision() {
    let f = Fixture::with_fakes("managed-prompt", &[("bunx", BUNX), ("claude", CLAUDE)]);
    let revision = f.path.join("revision");
    fs::write(&revision, "first").unwrap();
    let f = f.with_var("FAKE_SKILL_REVISION", revision.clone());
    let checkout = repo(&f, "checkout");
    f.ok(&["add", "work"]);
    f.ok(&[
        "plugin",
        "install",
        "example/skills",
        "--skills",
        "--skill",
        "demo",
        "--repo",
        s(&checkout),
        "--agent",
        "claude",
        "--yes",
    ]);
    fs::write(revision, "second").unwrap();
    f.ok(&["plugin", "check"]);

    // Roost opens /dev/tty for prompts, so attach a controlling terminal.
    let run = |defer: bool| {
        let fixture_env = f.command();
        let out = Command::new("/usr/bin/python3")
            .env_clear()
            .envs(
                fixture_env
                    .get_envs()
                    .filter_map(|(name, value)| value.map(|value| (name, value))),
            )
            .current_dir(&checkout)
            .args([
                "-c",
                r#"
import errno,os,pty,select,sys,time
pid, fd = pty.fork()
if pid == 0:
    os.execv(sys.argv[1], [sys.argv[1], 'run', 'work'])
deadline=time.monotonic()+8
output=b''
answered=False
while time.monotonic()<deadline:
    if not select.select([fd],[],[],0.1)[0]:
        continue
    try:
        data=os.read(fd,8192)
    except OSError as exc:
        if exc.errno!=errno.EIO:
            raise
        break
    if not data:
        break
    output+=data
    if b'Update now? [y/N]' in output and not answered:
        os.write(fd,b'n\n')
        answered=True
else:
    os.kill(pid,9)
_, status=os.waitpid(pid,0)
sys.stdout.buffer.write(output)
sys.exit(os.waitstatus_to_exitcode(status))
"#,
                env!("CARGO_BIN_EXE_roost"),
            ])
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success(),
            "{text}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(text.contains("started-claude"), "{text}");
        assert_eq!(
            text.contains("Managed plugin update available"),
            defer,
            "{text}"
        );
    };

    run(true);
    run(false);
    let skill = checkout.join(".claude/skills/demo/SKILL.md");
    assert!(fs::read_to_string(&skill).unwrap().contains("first"));
    f.ok(&["plugin", "update", "--managed", "--yes"]);
    assert!(fs::read_to_string(&skill).unwrap().contains("second"));
}

#[test]
fn overlap_preserves_the_shared_checkout_and_sync_uses_the_selected_revision() {
    let f = fixture();
    let checkout = repo(&f, "checkout");
    let later = repo(&f, "later");
    f.ok(&["project", "add", s(&checkout)]);
    for scope in [vec!["--repo", s(&checkout)], vec!["--all-repos"]] {
        f.ok(&[
            &[
                "plugin",
                "install",
                "example/skills",
                "--skills",
                "--skill",
                "*",
                "--agent",
                "claude",
                "--yes",
            ][..],
            &scope,
        ]
        .concat());
    }
    fs::write(f.path.join("revision"), "second").unwrap();
    let report = f.json(&["plugin", "list", "--managed", "--json"]);
    let first = report["data"]["rules"][0]["id"].as_str().unwrap();
    let second = report["data"]["rules"][1]["id"].as_str().unwrap();
    f.ok(&["plugin", "uninstall", first, "--yes"]);
    let location = ".claude/skills/demo/SKILL.md";
    assert!(
        fs::read_to_string(checkout.join(location))
            .unwrap()
            .contains("first")
    );

    f.ok(&["project", "add", s(&later)]);
    f.ok(&["plugin", "sync", "--yes"]);
    assert!(
        fs::read_to_string(later.join(location))
            .unwrap()
            .contains("first")
    );
    f.ok(&["plugin", "check"]);
    f.ok(&["plugin", "update", "--managed", second, "--yes"]);
    for repo in [&checkout, &later] {
        assert!(
            fs::read_to_string(repo.join(location))
                .unwrap()
                .contains("second")
        );
    }
    f.ok(&["plugin", "uninstall", second, "--yes"]);
    assert!(!checkout.join(location).exists());
    assert!(!later.join(location).exists());
}

#[test]
fn an_unavailable_repo_reports_partial_results_and_retries_when_registered_as_git() {
    let f = fixture();
    let checkout = repo(&f, "valid");
    let missing = f.path.join("missing");
    let install = [
        "plugin",
        "install",
        "example/skills",
        "--skills",
        "--skill",
        "demo",
        "--agent",
        "codex",
        "--repo",
        s(&checkout),
        "--repo",
        s(&missing),
    ];
    let dry_run = f.run(&[&install[..], &["--dry-run", "--json"]].concat());
    assert!(!dry_run.status.success());
    let preview = parsed(&dry_run);
    assert_eq!(
        preview["data"]["rule"]["unresolved"][0]["path"],
        s(&missing)
    );
    assert_eq!(
        preview["data"]["rule"]["targets"].as_array().unwrap().len(),
        1
    );
    assert!(!f.root.exists());

    let run = f.run(&[&install[..], &["--yes", "--json"]].concat());
    assert!(!run.status.success());
    let state = parsed(&run);
    assert_eq!(
        state["data"]["rules"][0]["unresolved"][0]["code"],
        "not_repository"
    );
    let location = ".agents/skills/demo/SKILL.md";
    assert!(checkout.join(location).is_file());
    assert!(!missing.exists());
    repo(&f, "missing");
    f.ok(&["plugin", "sync", "--yes"]);
    assert!(missing.join(location).is_file());
}

#[test]
fn check_reports_local_edits_even_when_upstream_is_current() {
    let f = fixture();
    let checkout = repo(&f, "checkout");
    f.ok(&[
        "plugin",
        "install",
        "example/skills",
        "--skills",
        "--skill",
        "demo",
        "--agent",
        "codex",
        "--repo",
        s(&checkout),
        "--yes",
    ]);
    let location = checkout.join(".agents/skills/demo/SKILL.md");
    fs::write(&location, "local edit").unwrap();
    let run = f.run(&["plugin", "check", "--json"]);
    let data = parsed(&run);
    assert!(!run.status.success());
    let status = &data["data"]["rules"][0]["targets"][0]["status"];
    assert!(status["local_conflict"].is_string());
    assert!(status["candidate"].is_null());
    assert!(status["check_failure"].is_null());
    assert_eq!(fs::read_to_string(location).unwrap(), "local edit");
}
