//! Shared sets and launch-time skill links (spec "Shared sets", gate A16).
//! Disposable fixtures only: fake `claude`, private temp root/home, no real data.
#![cfg(unix)]
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    path: PathBuf,
    root: PathBuf,
    home: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "roost-sets-{}-{}",
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
        fs::write(
            &fake,
            r#"#!/usr/bin/python3
import json,os,sys
args=sys.argv[1:]
if args==['--version']:
    print('2.1.280 (Claude Code)');sys.exit(0)
print(json.dumps({'arguments':args,'config':os.environ.get('CLAUDE_CONFIG_DIR')}))
"#,
        )
        .unwrap();
        fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
        let root = path.join("root");
        Self { path, root, home }
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
        let out = self.ok(args);
        serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)))
    }
    fn sets(&self) -> Value {
        self.json(&["set", "list", "--json"])["data"]["sets"].clone()
    }
    /// A user-managed source directory of skills (never written by Roost).
    fn source(&self, name: &str, skills: &[&str]) -> PathBuf {
        let path = self.path.join(name);
        fs::create_dir_all(&path).unwrap();
        for skill in skills {
            fs::create_dir(path.join(skill)).unwrap();
            fs::write(path.join(skill).join("SKILL.md"), skill).unwrap();
        }
        path
    }
    fn profile(&self, name: &str) -> PathBuf {
        self.root.join("profiles").join(name)
    }
    fn state(&self) -> Value {
        serde_json::from_slice(&fs::read(self.root.join("state.json")).unwrap()).unwrap()
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
fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn sets_are_created_marked_default_listed_and_deleted() {
    let f = Fixture::new();
    f.ok(&["add", "Work"]);
    f.ok(&["set", "create", "Core", "--default"]);
    f.ok(&["set", "create", "writing"]);
    f.fails(&["set", "create", "core"], "collision");
    f.fails(&["set", "create", "bad name"], "usage");
    f.ok(&["set", "default", "writing"]);
    f.ok(&["set", "default", "CORE", "--off"]);
    assert_eq!(
        f.sets(),
        json!([
            {"name":"Core","default":false,"items":[],"subscribers":[]},
            {"name":"writing","default":true,"items":[],"subscribers":[]}
        ])
    );
    f.ok(&["set", "delete", "core"]);
    f.fails(&["set", "delete", "core"], "not_found");
    f.fails(&["set", "default", "core"], "not_found");
    assert_eq!(f.sets().as_array().unwrap().len(), 1);
    f.ok(&["set", "subscribe", "work", "writing"]);
    let text = String::from_utf8(f.ok(&["set", "list"]).stdout).unwrap();
    assert_eq!(
        text.lines().collect::<Vec<_>>(),
        [
            "Set      Default  Subscribers  Items",
            "writing  yes      Work         —",
        ]
    );
    f.fails(&["set", "help"], "unrecognized");
}

#[test]
fn items_are_stored_absolute_and_add_or_drop_is_idempotent() {
    let f = Fixture::new();
    f.ok(&["set", "create", "core"]);
    f.ok(&["set", "add", "core", "--skill", "skills/review"]);
    f.ok(&["set", "add", "core", "--skill", "skills/review"]);
    f.ok(&["set", "add", "core", "--skills-from", "skills"]);
    f.ok(&["set", "add", "core", "--instruction", "notes/style.md"]);
    f.ok(&["set", "add", "core", "--instructions-from", "notes"]);
    let base = f.path.display();
    assert_eq!(
        f.sets()[0]["items"],
        json!([
            {"kind":"skill","value":format!("{base}/skills/review")},
            {"kind":"skill_source","value":format!("{base}/skills")},
            {"kind":"instruction","value":format!("{base}/notes/style.md")},
            {"kind":"instruction_source","value":format!("{base}/notes")}
        ])
    );
    f.ok(&["set", "drop", "core", "--skill", "skills/review"]);
    f.ok(&["set", "drop", "core", "--skill", "skills/review"]);
    assert_eq!(f.sets()[0]["items"].as_array().unwrap().len(), 3);
    // Exactly one item switch; synced/hidden/non-.md names never become links.
    f.fails(&["set", "add", "core"], "--skill");
    f.fails(
        &["set", "add", "core", "--skill", "a", "--skills-from", "b"],
        "cannot be used",
    );
    f.fails(&["set", "add", "core", "--skill", "skills/synced"], "usage");
    f.fails(
        &["set", "add", "core", "--skill", "skills/.hidden"],
        "usage",
    );
    f.fails(
        &["set", "add", "core", "--instruction", "notes/style.txt"],
        "usage",
    );
    f.fails(&["set", "add", "core", "--plugin", "bad id"], "usage");
    f.fails(&["set", "add", "core", "--plugin", "a@b@c"], "usage");
    // Plugins must be installed in the plugin store.
    f.fails(
        &["set", "add", "core", "--plugin", "lint@tools"],
        "not_found",
    );
    f.fails(&["set", "add", "absent", "--skill", "x"], "not_found");
    // Sources are never created or written.
    assert!(!f.path.join("skills").exists() && !f.path.join("notes").exists());
}

#[test]
fn add_subscribes_default_sets_unless_no_sets_and_ls_shows_them() {
    let f = Fixture::new();
    f.ok(&["set", "create", "writing", "--default"]);
    f.ok(&["set", "create", "Core", "--default"]);
    f.ok(&["set", "create", "extra"]);
    f.ok(&["add", "Work"]);
    f.ok(&["add", "bare", "--no-sets"]);
    f.ok(&["add", "personal", "--link-default"]);
    assert_eq!(
        f.run(&["add", "x", "--link-default", "--no-sets"])
            .status
            .code(),
        Some(2)
    );
    let profiles = f.json(&["ls", "--json"])["data"]["profiles"].clone();
    let sets: Vec<_> = profiles
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["name"].clone(), p["sets"].clone()))
        .collect();
    assert_eq!(
        sets,
        vec![
            (json!("bare"), json!([])),
            (json!("personal"), json!([])),
            (json!("Work"), json!(["Core", "writing"])),
        ]
    );
    let mut command = f.command();
    command.env("NO_COLOR", "1");
    let text = String::from_utf8(command.arg("ls").output().unwrap().stdout).unwrap();
    assert!(text.lines().next().unwrap().contains("Sets"), "{text}");
    let work = text.lines().find(|l| l.contains("Work")).unwrap();
    assert!(work.contains("Core,writing"), "{text}");
    let bare = text.lines().find(|l| l.contains("bare")).unwrap();
    assert!(bare.contains("—"), "{text}");
    assert_eq!(f.sets()[0]["subscribers"], json!(["Work"]));
    // Aliases receive nothing; upstream subscriptions are allowed.
    f.fails(&["set", "subscribe", "personal", "extra"], "usage");
    let upstream = f.home.join(".ccm/profiles/up");
    fs::create_dir_all(&upstream).unwrap();
    f.ok(&["register", "up", "--path", s(&upstream)]);
    f.ok(&["set", "subscribe", "up", "extra"]);
    f.ok(&["set", "subscribe", "bare", "extra", "core"]);
    f.ok(&["set", "unsubscribe", "bare", "core"]);
    f.fails(&["set", "subscribe", "bare", "absent"], "not_found");
    f.fails(&["set", "subscribe", "bare"], "SET");
    assert_eq!(f.sets()[1]["subscribers"], json!(["bare", "up"]));
}

#[test]
fn launch_links_subscribed_skills_and_removes_only_recorded_links() {
    let f = Fixture::new();
    let source = f.source("library", &["alpha", "beta", "synced", ".hidden"]);
    fs::write(source.join("notes.md"), "not a skill").unwrap();
    let single = f.source("solo", &["gamma"]).join("gamma");
    f.ok(&["set", "create", "core", "--default"]);
    f.ok(&["set", "add", "core", "--skills-from", s(&source)]);
    f.ok(&["set", "add", "core", "--skill", s(&single)]);
    // Instruction items are stored but not linked yet.
    f.ok(&[
        "set",
        "add",
        "core",
        "--instruction",
        s(&f.path.join("rule.md")),
    ]);
    f.ok(&["add", "Work"]);
    let skills = f.profile("Work").join("skills");
    fs::create_dir(&skills).unwrap();
    fs::set_permissions(&skills, fs::Permissions::from_mode(0o755)).unwrap();
    // Existing content wins; unrecorded foreign links are never touched.
    fs::create_dir(skills.join("beta")).unwrap();
    symlink(&source, skills.join("foreign")).unwrap();
    fs::create_dir(skills.join("synced")).unwrap();
    let out = f.ok(&["run", "Work", "--", "hello"]);
    let child: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(child["arguments"], json!(["hello"]));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("skills/beta") && stderr.contains("existing content"),
        "{stderr}"
    );
    assert_eq!(
        fs::read_link(skills.join("alpha")).unwrap(),
        source.join("alpha")
    );
    assert_eq!(fs::read_link(skills.join("gamma")).unwrap(), single);
    assert!(fs::symlink_metadata(skills.join("beta")).unwrap().is_dir());
    assert!(
        fs::symlink_metadata(skills.join("synced"))
            .unwrap()
            .is_dir()
    );
    assert!(!skills.join(".hidden").exists() && !skills.join("notes.md").exists());
    assert!(!f.profile("Work").join("rules").exists());
    let links = f.state()["links"].clone();
    let paths: Vec<_> = links
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["path"].clone())
        .collect();
    assert_eq!(paths, vec![json!("skills/alpha"), json!("skills/gamma")]);
    // A second launch changes nothing and warns only about existing content.
    let again = f.ok(&["run", "Work"]);
    let stderr = String::from_utf8_lossy(&again.stderr);
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    // Dropping items removes their recorded links; a replaced recorded link is left
    // alone and forgotten; unrecorded links stay.
    fs::remove_file(skills.join("gamma")).unwrap();
    symlink(&source, skills.join("gamma")).unwrap();
    f.ok(&["set", "drop", "core", "--skills-from", s(&source)]);
    f.ok(&["set", "drop", "core", "--skill", s(&single)]);
    let out = f.ok(&["run", "Work"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("skills/gamma") && stderr.contains("replaced"),
        "{stderr}"
    );
    assert!(fs::symlink_metadata(skills.join("alpha")).is_err());
    assert_eq!(fs::read_link(skills.join("gamma")).unwrap(), source);
    assert_eq!(fs::read_link(skills.join("foreign")).unwrap(), source);
    assert!(source.join("alpha/SKILL.md").exists() && single.join("SKILL.md").exists());
    assert_eq!(f.state()["links"], json!([]));
}

#[test]
fn subscribe_refuses_conflicts_and_launch_skips_them() {
    let f = Fixture::new();
    let one = f.source("one", &["shared", "a"]);
    let two = f.source("two", &["shared", "b"]);
    f.ok(&["set", "create", "first"]);
    f.ok(&["set", "create", "second"]);
    f.ok(&["set", "add", "first", "--skills-from", s(&one)]);
    f.ok(&["set", "add", "second", "--skills-from", s(&two)]);
    f.ok(&["add", "Work", "--no-sets"]);
    f.ok(&["set", "subscribe", "Work", "first"]);
    f.fails(&["set", "subscribe", "Work", "second"], "collision");
    assert_eq!(f.sets()[1]["subscribers"], json!([]));
    // An item added later conflicts only at launch: warn and skip that path.
    f.ok(&["set", "add", "first", "--skill", s(&two.join("shared"))]);
    let out = f.ok(&["run", "Work"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("skills/shared") && stderr.contains("more than one"),
        "{stderr}"
    );
    let skills = f.profile("Work").join("skills");
    assert!(fs::symlink_metadata(skills.join("shared")).is_err());
    assert_eq!(fs::read_link(skills.join("a")).unwrap(), one.join("a"));
    assert!(fs::symlink_metadata(skills.join("b")).is_err());
    assert_eq!(
        fs::metadata(&skills).unwrap().permissions().mode() & 0o777,
        0o700
    );
}

#[test]
fn upstream_and_alias_launches_receive_no_links() {
    let f = Fixture::new();
    let source = f.source("library", &["alpha"]);
    f.ok(&["set", "create", "core"]);
    f.ok(&["set", "add", "core", "--skills-from", s(&source)]);
    let upstream = f.home.join(".ccm/profiles/up");
    fs::create_dir_all(&upstream).unwrap();
    f.ok(&["register", "up", "--path", s(&upstream)]);
    f.ok(&["set", "subscribe", "up", "core"]);
    f.ok(&["run", "up"]);
    assert!(!upstream.join("skills").exists());
    f.ok(&["add", "personal", "--link-default"]);
    f.ok(&["run", "personal"]);
    assert!(!f.home.join(".claude/skills").exists());
    let links = fs::read(f.root.join("state.json"))
        .map(|b| serde_json::from_slice::<Value>(&b).unwrap()["links"].clone())
        .unwrap_or(json!([]));
    assert_eq!(links, json!([]));
}

#[test]
fn copy_from_owned_profile_copies_subscriptions_and_launcher_reconciles() {
    let f = Fixture::new();
    let source = f.source("library", &["alpha"]);
    f.ok(&["set", "create", "core", "--default"]);
    f.ok(&["set", "create", "work"]);
    f.ok(&["set", "add", "work", "--skills-from", s(&source)]);
    f.ok(&["add", "Work", "--no-sets"]);
    f.ok(&["set", "subscribe", "Work", "work"]);
    f.ok(&["run", "Work"]);
    let original = f.profile("Work");
    f.ok(&[
        "add",
        "Clone",
        "--copy-default",
        "--source",
        s(&original),
        "--yes",
    ]);
    let profiles = f.json(&["ls", "--json"])["data"]["profiles"].clone();
    assert_eq!(profiles[0]["name"], "Clone");
    assert_eq!(profiles[0]["sets"], json!(["work"]));
    let clone_skills = f.profile("Clone").join("skills");
    assert!(fs::symlink_metadata(clone_skills.join("alpha")).is_err());
    // A non-profile copy source gets the default sets instead.
    let plain = f.path.join("plain");
    fs::create_dir(&plain).unwrap();
    f.ok(&[
        "add",
        "Other",
        "--copy-default",
        "--source",
        s(&plain),
        "--yes",
    ]);
    let profiles = f.json(&["ls", "--json"])["data"]["profiles"].clone();
    assert_eq!(profiles[1]["sets"], json!(["core"]));
    // The bound launcher reconciles like run.
    let out = Command::new(f.root.join("bin/roost-Clone"))
        .env_clear()
        .env("HOME", &f.home)
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", f.path.join("bin").display()),
        )
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        fs::read_link(clone_skills.join("alpha")).unwrap(),
        source.join("alpha")
    );
}

#[test]
fn remove_keeps_records_and_purge_unlinks_without_following() {
    let f = Fixture::new();
    let source = f.source("library", &["alpha"]);
    f.ok(&["set", "create", "core", "--default"]);
    f.ok(&["set", "add", "core", "--skills-from", s(&source)]);
    f.ok(&["add", "Work"]);
    f.ok(&["run", "Work"]);
    let link = f.profile("Work").join("skills/alpha");
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    f.ok(&["remove", "Work"]);
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(f.state()["links"].as_array().unwrap().len(), 1);
    assert_eq!(f.sets()[0]["subscribers"], json!(["Work"]));
    f.ok(&["reuse", "Work"]);
    f.ok(&["remove", "Work", "--purge", "--yes"]);
    assert!(!f.profile("Work").exists());
    assert_eq!(
        fs::read_to_string(source.join("alpha/SKILL.md")).unwrap(),
        "alpha"
    );
    assert_eq!(f.state()["links"], json!([]));
    let sets: Value = serde_json::from_slice(&fs::read(f.root.join("sets.json")).unwrap()).unwrap();
    assert_eq!(sets["subscriptions"], json!([]));
    // A later profile with the same name inherits no subscriptions.
    f.ok(&["add", "Work", "--no-sets"]);
    assert_eq!(f.sets()[0]["subscribers"], json!([]));
}

#[test]
fn deleting_a_set_drops_subscriptions_and_links_leave_at_next_launch() {
    let f = Fixture::new();
    let source = f.source("library", &["alpha"]);
    f.ok(&["set", "create", "core", "--default"]);
    f.ok(&["set", "add", "core", "--skills-from", s(&source)]);
    f.ok(&["add", "Work"]);
    f.ok(&["run", "Work"]);
    f.ok(&["set", "delete", "core"]);
    let link = f.profile("Work").join("skills/alpha");
    assert!(fs::symlink_metadata(&link).is_ok());
    f.ok(&["run", "Work"]);
    assert!(fs::symlink_metadata(&link).is_err());
    assert_eq!(
        f.json(&["ls", "--json"])["data"]["profiles"][0]["sets"],
        json!([])
    );
}
