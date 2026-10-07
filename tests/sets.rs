//! Shared sets and launch-time skill, instruction, agent, command and output-style links (spec "Shared sets", gate A16).
//! Disposable fixtures only: fake `claude`, private temp root/home, no real data.
#![cfg(unix)]
mod common;
use common::{Fixture, s};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
    process::Command,
};

const FAKE: &str = r#"#!/usr/bin/python3
import json,os,sys
args=sys.argv[1:]
if args==['--version']:
    print('2.1.280 (Claude Code)');sys.exit(0)
print(json.dumps({'arguments':args,'config':os.environ.get('CLAUDE_CONFIG_DIR')}))
"#;

impl Fixture {
    fn new() -> Self {
        Fixture::with_fakes("sets", &[("claude", FAKE)])
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
    f.source("skills", &["review"]);
    fs::create_dir(f.path.join("notes")).unwrap();
    fs::write(f.path.join("notes/style.md"), "# style").unwrap();
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
    // Case-insensitive file systems would alias Claude's own skills/synced.
    f.fails(&["set", "add", "core", "--skill", "skills/Synced"], "usage");
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
    assert_eq!(fs::read_dir(f.path.join("skills")).unwrap().count(), 1);
    assert_eq!(fs::read_dir(f.path.join("notes")).unwrap().count(), 1);
    assert_eq!(
        fs::read_to_string(f.path.join("notes/style.md")).unwrap(),
        "# style"
    );
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
    f.ok(&["add", "Work"]);
    let skills = f.profile("Work").join("skills");
    fs::create_dir(&skills).unwrap();
    // Claude creates skills/ group-writable under umask 002; Roost tightens it.
    fs::set_permissions(&skills, fs::Permissions::from_mode(0o775)).unwrap();
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
    assert_eq!(
        fs::metadata(&skills).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert!(fs::symlink_metadata(skills.join("beta")).unwrap().is_dir());
    assert!(
        fs::symlink_metadata(skills.join("synced"))
            .unwrap()
            .is_dir()
    );
    assert!(!skills.join(".hidden").exists() && !skills.join("notes.md").exists());
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
    // Borrowed data keeps its permissions, even a group-writable skills/.
    fs::create_dir(upstream.join("skills")).unwrap();
    fs::set_permissions(upstream.join("skills"), fs::Permissions::from_mode(0o775)).unwrap();
    f.ok(&["run", "up"]);
    assert_eq!(
        fs::metadata(upstream.join("skills"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o775
    );
    assert_eq!(fs::read_dir(upstream.join("skills")).unwrap().count(), 0);
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

/// A user-managed directory of instruction fragments (never written by Roost).
fn fragments(f: &Fixture, name: &str, files: &[&str]) -> PathBuf {
    let path = f.path.join(name);
    fs::create_dir_all(&path).unwrap();
    for file in files {
        fs::write(path.join(file), format!("# {file}")).unwrap();
    }
    path
}

#[test]
fn launch_links_instruction_fragments_into_rules_and_unsubscribe_removes_them() {
    let f = Fixture::new();
    let notes = fragments(
        &f,
        "notes",
        &["style.md", "git.md", ".hidden.md", "todo.txt"],
    );
    // Only immediate regular .md files: no directories, links or nested files.
    fs::create_dir(notes.join("nested.md")).unwrap();
    fs::write(notes.join("nested.md/deep.md"), "deep").unwrap();
    symlink(notes.join("style.md"), notes.join("alias.md")).unwrap();
    let solo = fragments(&f, "solo", &["review.md"]).join("review.md");
    f.ok(&["set", "create", "core"]);
    f.ok(&["set", "add", "core", "--instructions-from", s(&notes)]);
    f.ok(&["set", "add", "core", "--instruction", s(&solo)]);
    f.ok(&["add", "Work", "--no-sets"]);
    f.ok(&["set", "subscribe", "Work", "core"]);
    let profile = f.profile("Work");
    fs::write(profile.join("CLAUDE.md"), "mine").unwrap();
    let rules = profile.join("rules");
    fs::create_dir(&rules).unwrap();
    fs::set_permissions(&rules, fs::Permissions::from_mode(0o775)).unwrap();
    fs::write(rules.join("git.md"), "own rule").unwrap();
    let out = f.ok(&["run", "Work"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("rules/git.md") && stderr.contains("existing content"),
        "{stderr}"
    );
    assert_eq!(
        fs::read_link(rules.join("style.md")).unwrap(),
        notes.join("style.md")
    );
    assert_eq!(fs::read_link(rules.join("review.md")).unwrap(), solo);
    assert_eq!(
        fs::metadata(&rules).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::read_to_string(rules.join("git.md")).unwrap(),
        "own rule"
    );
    for skipped in [".hidden.md", "todo.txt", "nested.md", "alias.md"] {
        assert!(
            fs::symlink_metadata(rules.join(skipped)).is_err(),
            "{skipped}"
        );
    }
    assert_eq!(
        fs::read_to_string(profile.join("CLAUDE.md")).unwrap(),
        "mine"
    );
    let paths: Vec<_> = f.state()["links"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["path"].clone())
        .collect();
    assert_eq!(
        paths,
        vec![json!("rules/review.md"), json!("rules/style.md")]
    );
    // Unsubscribing removes exactly the recorded links at the next launch.
    f.ok(&["set", "unsubscribe", "Work", "core"]);
    f.ok(&["run", "Work"]);
    assert!(fs::symlink_metadata(rules.join("style.md")).is_err());
    assert!(fs::symlink_metadata(rules.join("review.md")).is_err());
    assert_eq!(
        fs::read_to_string(rules.join("git.md")).unwrap(),
        "own rule"
    );
    assert_eq!(
        fs::read_to_string(notes.join("style.md")).unwrap(),
        "# style.md"
    );
    assert!(solo.exists());
    assert_eq!(f.state()["links"], json!([]));
}

#[test]
fn instruction_items_are_listed_and_same_named_fragments_conflict() {
    let f = Fixture::new();
    let one = fragments(&f, "one", &["style.md"]);
    let two = fragments(&f, "two", &["style.md"]);
    f.ok(&["set", "create", "core"]);
    f.ok(&["set", "create", "other"]);
    f.ok(&["set", "add", "core", "--instructions-from", s(&one)]);
    f.ok(&[
        "set",
        "add",
        "other",
        "--instruction",
        s(&two.join("style.md")),
    ]);
    f.ok(&["add", "Work", "--no-sets"]);
    f.ok(&["set", "subscribe", "Work", "core"]);
    f.fails(&["set", "subscribe", "Work", "other"], "collision");
    assert_eq!(
        f.sets(),
        json!([
            {"name":"core","default":false,
             "items":[{"kind":"instruction_source","value":s(&one)}],
             "subscribers":["Work"]},
            {"name":"other","default":false,
             "items":[{"kind":"instruction","value":s(&two.join("style.md"))}],
             "subscribers":[]}
        ])
    );
    let text = String::from_utf8(f.ok(&["set", "list"]).stdout).unwrap();
    assert!(
        text.contains(&format!("instruction_source:{}", one.display())),
        "{text}"
    );
    assert_eq!(
        f.json(&["ls", "--json"])["data"]["profiles"][0]["sets"],
        json!(["core"])
    );
    // Launch creates a private rules/ when the profile has none.
    f.ok(&["run", "Work"]);
    let rules = f.profile("Work").join("rules");
    assert_eq!(
        fs::metadata(&rules).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::read_link(rules.join("style.md")).unwrap(),
        one.join("style.md")
    );
}

#[test]
fn upstream_and_alias_receive_no_instructions_and_purge_unlinks_without_following() {
    let f = Fixture::new();
    let notes = fragments(&f, "notes", &["style.md"]);
    f.ok(&["set", "create", "core", "--default"]);
    f.ok(&["set", "add", "core", "--instructions-from", s(&notes)]);
    let upstream = f.home.join(".ccm/profiles/up");
    fs::create_dir_all(&upstream).unwrap();
    f.ok(&["register", "up", "--path", s(&upstream)]);
    f.ok(&["set", "subscribe", "up", "core"]);
    f.ok(&["run", "up"]);
    assert!(!upstream.join("rules").exists());
    f.ok(&["add", "personal", "--link-default"]);
    f.ok(&["run", "personal"]);
    assert!(!f.home.join(".claude/rules").exists());
    f.ok(&["add", "Work"]);
    f.ok(&["run", "Work"]);
    let link = f.profile("Work").join("rules/style.md");
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    f.ok(&["remove", "Work", "--purge", "--yes"]);
    assert!(!f.profile("Work").exists());
    assert_eq!(
        fs::read_to_string(notes.join("style.md")).unwrap(),
        "# style.md"
    );
    assert_eq!(f.state()["links"], json!([]));
}

#[test]
fn add_refuses_missing_or_wrong_type_sources_and_leaves_sets_unchanged() {
    let f = Fixture::new();
    f.ok(&["set", "create", "core"]);
    let library = f.source("library", &["alpha"]);
    fs::write(library.join("note.md"), "a note").unwrap();
    let before = fs::read(f.root.join("sets.json")).unwrap();
    let base = f.path.display().to_string();
    let refused = |args: &[&str], path: &str| {
        let mut full = vec!["set", "add", "core"];
        full.extend_from_slice(args);
        let out = f.fails(&full, "not_found");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(&format!("{base}/{path}")), "{stderr}");
    };
    // Missing sources of every kind.
    refused(&["--skill", "library/absent"], "library/absent");
    refused(&["--skills-from", "nowhere"], "nowhere");
    refused(&["--instruction", "notes/style.md"], "notes/style.md");
    refused(&["--instructions-from", "notes"], "notes");
    // Wrong types: skills and sources are directories, instructions regular files.
    refused(&["--skill", "library/note.md"], "library/note.md");
    refused(&["--skills-from", "library/note.md"], "library/note.md");
    fs::create_dir(library.join("dir.md")).unwrap();
    refused(&["--instruction", "library/dir.md"], "library/dir.md");
    refused(
        &["--instructions-from", "library/note.md"],
        "library/note.md",
    );
    // A dangling symlink is missing too.
    symlink(f.path.join("gone"), library.join("dangling")).unwrap();
    refused(&["--skill", "library/dangling"], "library/dangling");
    assert_eq!(fs::read(f.root.join("sets.json")).unwrap(), before);
    // Existing sources are accepted, and dropping never requires the source.
    f.ok(&["set", "add", "core", "--skill", "library/alpha"]);
    f.ok(&["set", "add", "core", "--instruction", "library/note.md"]);
    f.ok(&["set", "drop", "core", "--skill", "library/absent"]);
    assert_eq!(f.sets()[0]["items"].as_array().unwrap().len(), 2);
}

#[test]
fn launch_skips_vanished_sources_with_one_warning_and_drops_their_links() {
    let f = Fixture::new();
    let library = f.source("library", &["alpha"]);
    let solo = f.source("solo", &["gamma", "delta"]);
    let notes = fragments(&f, "notes", &["style.md"]);
    f.ok(&["set", "create", "core", "--default"]);
    f.ok(&["set", "create", "extra", "--default"]);
    f.ok(&["set", "add", "core", "--skills-from", s(&library)]);
    f.ok(&["set", "add", "core", "--skill", s(&solo.join("gamma"))]);
    f.ok(&["set", "add", "extra", "--skill", s(&solo.join("delta"))]);
    f.ok(&[
        "set",
        "add",
        "extra",
        "--instruction",
        s(&notes.join("style.md")),
    ]);
    f.ok(&["add", "Work"]);
    let profile = f.profile("Work");
    // The delta skill vanishes before the first launch: no dangling link.
    fs::remove_dir_all(solo.join("delta")).unwrap();
    let out = f.ok(&["run", "Work"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let delta = solo.join("delta");
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(
        stderr.contains("extra") && stderr.contains(s(&delta)),
        "{stderr}"
    );
    assert!(fs::symlink_metadata(profile.join("skills/delta")).is_err());
    assert!(fs::read_link(profile.join("skills/alpha")).is_ok());
    assert!(fs::read_link(profile.join("skills/gamma")).is_ok());
    assert!(fs::read_link(profile.join("rules/style.md")).is_ok());
    // Sources vanishing after their links exist: one warning per item, naming its
    // set and path; the recorded links leave by normal recorded-link cleanup.
    fs::remove_dir_all(&library).unwrap();
    fs::remove_dir_all(solo.join("gamma")).unwrap();
    // A source replaced by the wrong type counts as vanished too.
    fs::remove_file(notes.join("style.md")).unwrap();
    fs::create_dir(notes.join("style.md")).unwrap();
    let out = f.ok(&["run", "Work"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(stderr.lines().count(), 4, "{stderr}");
    for (set, path) in [
        ("core", library.clone()),
        ("core", solo.join("gamma")),
        ("extra", delta.clone()),
        ("extra", notes.join("style.md")),
    ] {
        assert!(
            stderr
                .lines()
                .any(|l| l.contains(&format!("set {set}")) && l.contains(s(&path))),
            "{set} {}: {stderr}",
            path.display()
        );
    }
    for link in [
        "skills/alpha",
        "skills/gamma",
        "skills/delta",
        "rules/style.md",
    ] {
        assert!(fs::symlink_metadata(profile.join(link)).is_err(), "{link}");
    }
    assert_eq!(f.state()["links"], json!([]));
    // Items stay in their sets; a restored source links again at the next launch.
    assert_eq!(f.sets()[0]["items"].as_array().unwrap().len(), 2);
    fs::create_dir(&delta).unwrap();
    f.ok(&["run", "Work"]);
    assert_eq!(fs::read_link(profile.join("skills/delta")).unwrap(), delta);
}

/// Agents, commands and output styles: (CLI flag for one file, flag for a whole
/// directory, profile subdirectory, item kind names).
const MARKDOWN_KINDS: [(&str, &str, &str, &str, &str); 3] = [
    (
        "--agent",
        "--agents-from",
        "agents",
        "agent",
        "agent_source",
    ),
    (
        "--command",
        "--commands-from",
        "commands",
        "command",
        "command_source",
    ),
    (
        "--output-style",
        "--output-styles-from",
        "output-styles",
        "output_style",
        "output_style_source",
    ),
];

#[test]
fn launch_links_agents_commands_and_output_styles_and_unsubscribe_removes_them() {
    for (one, many, directory, _, _) in MARKDOWN_KINDS {
        let f = Fixture::new();
        let library = fragments(
            &f,
            "library",
            &["review.md", "git.md", ".hidden.md", "notes.txt"],
        );
        fs::create_dir(library.join("nested.md")).unwrap();
        symlink(library.join("review.md"), library.join("alias.md")).unwrap();
        let solo = fragments(&f, "solo", &["deploy.md"]).join("deploy.md");
        f.ok(&["set", "create", "core"]);
        f.ok(&["set", "add", "core", many, s(&library)]);
        f.ok(&["set", "add", "core", one, s(&solo)]);
        f.ok(&["add", "Work", "--no-sets"]);
        f.ok(&["set", "subscribe", "Work", "core"]);
        let target = f.profile("Work").join(directory);
        fs::create_dir(&target).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o775)).unwrap();
        fs::write(target.join("git.md"), "own").unwrap();
        let out = f.ok(&["run", "Work"]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(&format!("{directory}/git.md")) && stderr.contains("existing content"),
            "{directory}: {stderr}"
        );
        assert_eq!(
            fs::read_link(target.join("review.md")).unwrap(),
            library.join("review.md")
        );
        assert_eq!(fs::read_link(target.join("deploy.md")).unwrap(), solo);
        assert_eq!(fs::read_to_string(target.join("git.md")).unwrap(), "own");
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o700
        );
        for skipped in [".hidden.md", "notes.txt", "nested.md", "alias.md"] {
            assert!(
                fs::symlink_metadata(target.join(skipped)).is_err(),
                "{directory}/{skipped}"
            );
        }
        let paths: Vec<_> = f.state()["links"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["path"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(
            paths,
            [
                format!("{directory}/deploy.md"),
                format!("{directory}/review.md")
            ]
        );
        f.ok(&["set", "unsubscribe", "Work", "core"]);
        f.ok(&["run", "Work"]);
        assert!(fs::symlink_metadata(target.join("review.md")).is_err());
        assert!(fs::symlink_metadata(target.join("deploy.md")).is_err());
        assert_eq!(fs::read_to_string(target.join("git.md")).unwrap(), "own");
        assert_eq!(
            fs::read_to_string(library.join("review.md")).unwrap(),
            "# review.md"
        );
        assert!(solo.exists());
        assert_eq!(f.state()["links"], json!([]));
    }
}

#[test]
fn agent_command_and_style_items_are_listed_validated_and_conflict() {
    for (one, many, directory, kind, source_kind) in MARKDOWN_KINDS {
        let f = Fixture::new();
        let library = fragments(&f, "library", &["review.md", "notes.txt"]);
        let other = fragments(&f, "other", &["review.md"]);
        f.ok(&["set", "create", "core"]);
        f.ok(&["set", "create", "extra"]);
        // Single items must be existing regular .md files; sources directories.
        f.fails(
            &["set", "add", "core", one, s(&library.join("notes.txt"))],
            "usage",
        );
        f.fails(&["set", "add", "core", one, s(&library)], "usage");
        fs::create_dir(library.join("dir.md")).unwrap();
        f.fails(
            &["set", "add", "core", one, s(&library.join("dir.md"))],
            "not_found",
        );
        f.fails(
            &["set", "add", "core", one, s(&library.join("absent.md"))],
            "not_found",
        );
        f.fails(
            &["set", "add", "core", many, s(&library.join("review.md"))],
            "not_found",
        );
        f.ok(&["set", "add", "core", many, s(&library)]);
        f.ok(&["set", "add", "extra", one, s(&other.join("review.md"))]);
        assert_eq!(
            f.sets(),
            json!([
                {"name":"core","default":false,
                 "items":[{"kind":source_kind,"value":s(&library)}],
                 "subscribers":[]},
                {"name":"extra","default":false,
                 "items":[{"kind":kind,"value":s(&other.join("review.md"))}],
                 "subscribers":[]}
            ])
        );
        let text = String::from_utf8(f.ok(&["set", "list"]).stdout).unwrap();
        assert!(
            text.contains(&format!("{source_kind}:{}", library.display()))
                && text.contains(&format!("{kind}:{}", other.join("review.md").display())),
            "{text}"
        );
        f.ok(&["add", "Work", "--no-sets"]);
        f.ok(&["set", "subscribe", "Work", "core"]);
        let out = f.fails(&["set", "subscribe", "Work", "extra"], "collision");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(&format!("{directory}/review.md")),
            "{directory}"
        );
        f.ok(&["set", "drop", "core", many, s(&library)]);
        assert_eq!(f.sets()[0]["items"], json!([]));
    }
}

#[test]
fn upstream_and_alias_get_no_agents_commands_or_styles_and_purge_unlinks_without_following() {
    let f = Fixture::new();
    let library = fragments(&f, "library", &["review.md"]);
    f.ok(&["set", "create", "core", "--default"]);
    for (_, many, _, _, _) in MARKDOWN_KINDS {
        f.ok(&["set", "add", "core", many, s(&library)]);
    }
    let upstream = f.home.join(".ccm/profiles/up");
    fs::create_dir_all(&upstream).unwrap();
    f.ok(&["register", "up", "--path", s(&upstream)]);
    f.ok(&["set", "subscribe", "up", "core"]);
    f.ok(&["run", "up"]);
    f.ok(&["add", "personal", "--link-default"]);
    f.ok(&["run", "personal"]);
    f.ok(&["add", "Work"]);
    f.ok(&["run", "Work"]);
    for (_, _, directory, _, _) in MARKDOWN_KINDS {
        assert!(!upstream.join(directory).exists(), "{directory}");
        assert!(
            !f.home.join(".claude").join(directory).exists(),
            "{directory}"
        );
        let link = f.profile("Work").join(directory).join("review.md");
        assert_eq!(fs::read_link(&link).unwrap(), library.join("review.md"));
    }
    assert_eq!(f.state()["links"].as_array().unwrap().len(), 3);
    f.ok(&["remove", "Work", "--purge", "--yes"]);
    assert!(!f.profile("Work").exists());
    assert_eq!(
        fs::read_to_string(library.join("review.md")).unwrap(),
        "# review.md"
    );
    assert_eq!(f.state()["links"], json!([]));
}
