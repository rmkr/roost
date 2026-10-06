//! Shared settings fragments written into owned profiles' `settings.json` (spec
//! "Shared sets", ADR 0003, gate A16). Disposable fixtures only: fake `claude`,
//! private temp root/home, no real data.
#![cfg(unix)]
mod common;
use common::{Fixture, s};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

const FAKE: &str = r#"#!/usr/bin/python3
import json,os,sys
args=sys.argv[1:]
if args==['--version']:
    print('2.1.280 (Claude Code)');sys.exit(0)
print(json.dumps({'arguments':args,'config':os.environ.get('CLAUDE_CONFIG_DIR')}))
"#;

impl Fixture {
    fn new() -> Self {
        Fixture::with_fakes("settings", &[("claude", FAKE)])
    }
    fn sets(&self) -> Value {
        self.json(&["set", "list", "--json"])["data"]["sets"].clone()
    }
    fn profile(&self, name: &str) -> PathBuf {
        self.root.join("profiles").join(name)
    }
    fn state(&self) -> Value {
        serde_json::from_slice(&fs::read(self.root.join("state.json")).unwrap()).unwrap()
    }
    /// A user-managed fragment directory holding `(file, contents)` entries.
    fn fragments(&self, name: &str, files: &[(&str, Value)]) -> PathBuf {
        let path = self.path.join(name);
        fs::create_dir_all(&path).unwrap();
        for (file, value) in files {
            fs::write(path.join(file), serde_json::to_vec_pretty(value).unwrap()).unwrap();
        }
        path
    }
    fn settings_path(&self, name: &str) -> PathBuf {
        self.profile(name).join("settings.json")
    }
    fn settings(&self, name: &str) -> Value {
        serde_json::from_slice(&fs::read(self.settings_path(name)).unwrap()).unwrap()
    }
    fn settings_text(&self, name: &str) -> String {
        fs::read_to_string(self.settings_path(name)).unwrap()
    }
    fn write_settings(&self, name: &str, text: &str) {
        fs::write(self.settings_path(name), text).unwrap();
    }
    fn registration_id(&self, name: &str) -> Value {
        let registry: Value =
            serde_json::from_slice(&fs::read(self.root.join("registry.json")).unwrap()).unwrap();
        registry["registrations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == name)
            .unwrap()["registration_id"]
            .clone()
    }
    fn stderr_of(&self, args: &[&str]) -> String {
        String::from_utf8_lossy(&self.ok(args).stderr).into_owned()
    }
}

fn hook(command: &str) -> Value {
    json!({"type":"command","command":command})
}

#[test]
fn setting_items_are_validated_listed_and_stored_absolute() {
    let f = Fixture::new();
    f.ok(&["set", "create", "core"]);
    let dir = f.fragments(
        "settings",
        &[
            (
                "atuin.json",
                json!({"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[hook("atuin")]}]}}),
            ),
            (
                "line.json",
                json!({"statusLine":{"type":"command","command":"line.sh"},"outputStyle":"terse"}),
            ),
        ],
    );
    f.ok(&["set", "add", "core", "--setting", "settings/atuin.json"]);
    f.ok(&["set", "add", "core", "--settings-from", "settings"]);
    let base = f.path.display();
    assert_eq!(
        f.sets()[0]["items"],
        json!([
            {"kind":"setting","value":format!("{base}/settings/atuin.json")},
            {"kind":"setting_source","value":format!("{base}/settings")}
        ])
    );
    let text = String::from_utf8(f.ok(&["set", "list"]).stdout).unwrap();
    assert!(
        text.contains("setting:") && text.contains("setting_source:"),
        "{text}"
    );
    let before = fs::read(f.root.join("sets.json")).unwrap();
    // Missing or wrong-type sources.
    f.fails(
        &["set", "add", "core", "--setting", "settings/absent.json"],
        "not_found",
    );
    f.fails(
        &["set", "add", "core", "--setting", "settings"],
        "not_found",
    );
    f.fails(
        &[
            "set",
            "add",
            "core",
            "--settings-from",
            "settings/atuin.json",
        ],
        "not_found",
    );
    // Malformed, non-object, disallowed keys and ill-shaped values.
    let bad = f.path.join("bad");
    fs::create_dir(&bad).unwrap();
    for (name, text) in [
        ("syntax.json", "{".to_owned()),
        ("array.json", "[]".to_owned()),
        ("model.json", json!({"model":"opus"}).to_string()),
        (
            "perm.json",
            json!({"hooks":{},"permissions":{}}).to_string(),
        ),
        ("hooks.json", json!({"hooks":[]}).to_string()),
        ("event.json", json!({"hooks":{"Stop":{}}}).to_string()),
        (
            "group.json",
            json!({"hooks":{"Stop":[{"hooks":[{"command":"x"}]}]}}).to_string(),
        ),
        (
            "matcher.json",
            json!({"hooks":{"Stop":[{"matcher":1,"hooks":[hook("x")]}]}}).to_string(),
        ),
        ("line.json", json!({"statusLine":"line.sh"}).to_string()),
        ("style.json", json!({"outputStyle":7}).to_string()),
    ] {
        fs::write(bad.join(name), &text).unwrap();
        let out = f.fails(
            &["set", "add", "core", "--setting", s(&bad.join(name))],
            "usage",
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(name), "{name}: {stderr}");
    }
    // A source is refused when any fragment in it is invalid.
    f.fails(&["set", "add", "core", "--settings-from", s(&bad)], "usage");
    assert_eq!(fs::read(f.root.join("sets.json")).unwrap(), before);
    // Sources are never written.
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
}

#[test]
fn launch_merges_hooks_preserving_other_keys_and_records_only_its_own() {
    let f = Fixture::new();
    let dir = f.fragments(
        "settings",
        &[
            (
                "atuin.json",
                json!({"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[hook("atuin")]}]}}),
            ),
            (
                "cbm.json",
                json!({"hooks":{"Stop":[{"hooks":[hook("cbm"), hook("mine")]}]}}),
            ),
        ],
    );
    f.ok(&["set", "create", "core", "--default"]);
    f.ok(&["set", "add", "core", "--settings-from", s(&dir)]);
    f.ok(&["add", "Work"]);
    // The profile's own settings: other keys (and their order) are preserved, the
    // fragment's handlers keep their own key order (the fixture writes them sorted), and
    // the handler identical to one of its own is neither duplicated nor recorded.
    f.write_settings(
        "Work",
        r#"{"model": "opus", "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "mine"}]}]}, "env": {"Z": "1", "A": "2"}}"#,
    );
    f.ok(&["run", "Work"]);
    assert_eq!(
        f.settings_text("Work"),
        r#"{
  "model": "opus",
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "mine"
          },
          {
            "command": "cbm",
            "type": "command"
          }
        ]
      }
    ],
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "command": "atuin",
            "type": "command"
          }
        ]
      }
    ]
  },
  "env": {
    "Z": "1",
    "A": "2"
  }
}"#
    );
    let id = f.state()["settings"][0]["registration_id"].clone();
    assert_eq!(
        f.state()["settings"],
        json!([{"registration_id":id,"hooks":[
            {"event":"PreToolUse","matcher":"Bash","handler":hook("atuin")},
            {"event":"Stop","handler":hook("cbm")}
        ]}])
    );
    // A launch with nothing to change leaves the file byte-identical and untouched.
    use std::os::unix::fs::MetadataExt;
    let before = fs::metadata(f.settings_path("Work")).unwrap();
    let text = f.settings_text("Work");
    let state = f.state();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let stderr = f.stderr_of(&["run", "Work"]);
    assert!(stderr.is_empty(), "{stderr}");
    let after = fs::metadata(f.settings_path("Work")).unwrap();
    assert_eq!(f.settings_text("Work"), text);
    assert_eq!(
        (before.ino(), before.mtime(), before.mtime_nsec()),
        (after.ino(), after.mtime(), after.mtime_nsec())
    );
    assert_eq!(state["settings"], f.state()["settings"]);
    // A Roost hook removed by hand comes back: the set is the source of truth.
    let mut settings = f.settings("Work");
    settings["hooks"]
        .as_object_mut()
        .unwrap()
        .remove("PreToolUse");
    f.write_settings("Work", &serde_json::to_string_pretty(&settings).unwrap());
    f.ok(&["run", "Work"]);
    assert_eq!(
        f.settings("Work")["hooks"]["PreToolUse"],
        json!([{"matcher":"Bash","hooks":[hook("atuin")]}])
    );
    // Unsubscribing removes only Roost's recorded, unchanged entries at the next
    // launch; the profile's own identical handler and other keys stay.
    f.ok(&["set", "unsubscribe", "Work", "core"]);
    f.ok(&["run", "Work"]);
    assert_eq!(
        f.settings("Work"),
        json!({"model":"opus","hooks":{"Stop":[{"hooks":[hook("mine")]}]},"env":{"Z":"1","A":"2"}})
    );
    assert!(f.state().get("settings").is_none(), "{}", f.state());
}

#[test]
fn status_line_and_output_style_back_off_to_the_profiles_own_values() {
    let f = Fixture::new();
    let line = json!({"type":"command","command":"line.sh"});
    let dir = f.fragments(
        "settings",
        &[(
            "line.json",
            json!({"statusLine":line,"outputStyle":"terse"}),
        )],
    );
    f.ok(&["set", "create", "core", "--default"]);
    f.ok(&["set", "add", "core", "--setting", s(&dir.join("line.json"))]);
    f.ok(&["add", "Work"]);
    f.ok(&["add", "Own"]);
    // Absent settings.json: created with Roost's keys, recorded, ending with a
    // newline like the settings.json Claude writes.
    f.ok(&["run", "Work"]);
    assert_eq!(
        f.settings("Work"),
        json!({"statusLine":line,"outputStyle":"terse"})
    );
    assert!(f.settings_text("Work").ends_with("}\n"));
    // A profile with its own value keeps it; nothing is recorded for that key.
    f.write_settings("Own", r#"{"outputStyle": "mine"}"#);
    f.ok(&["run", "Own"]);
    assert_eq!(
        f.settings("Own"),
        json!({"outputStyle":"mine","statusLine":line})
    );
    let record = |name: &str| {
        let id = f.registration_id(name);
        f.state()["settings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["registration_id"] == id)
            .cloned()
            .unwrap_or(Value::Null)
    };
    assert_eq!(record("Own")["output_style"], Value::Null);
    assert_eq!(record("Own")["status_line"], line);
    // A changed fragment replaces Roost's recorded, unchanged value.
    let new_line = json!({"type":"command","command":"line2.sh","padding":0});
    fs::write(
        dir.join("line.json"),
        json!({"statusLine":new_line,"outputStyle":"terse"}).to_string(),
    )
    .unwrap();
    f.ok(&["run", "Work"]);
    assert_eq!(f.settings("Work")["statusLine"], new_line);
    // The user changing a recorded value wins: Roost warns once and lets go.
    let mut settings = f.settings("Work");
    settings["outputStyle"] = json!("explanatory");
    f.write_settings("Work", &settings.to_string());
    let stderr = f.stderr_of(&["run", "Work"]);
    assert!(stderr.contains("outputStyle"), "{stderr}");
    assert_eq!(f.settings("Work")["outputStyle"], "explanatory");
    assert_eq!(record("Work")["output_style"], Value::Null);
    let stderr = f.stderr_of(&["run", "Work"]);
    assert!(stderr.is_empty(), "{stderr}");
    // Dropping the item removes only Roost's unchanged values.
    f.ok(&[
        "set",
        "drop",
        "core",
        "--setting",
        s(&dir.join("line.json")),
    ]);
    f.ok(&["run", "Work"]);
    f.ok(&["run", "Own"]);
    assert_eq!(f.settings("Work"), json!({"outputStyle":"explanatory"}));
    assert_eq!(f.settings("Own"), json!({"outputStyle":"mine"}));
    assert!(f.state().get("settings").is_none(), "{}", f.state());
}

#[test]
fn invalid_fragments_and_unsafe_settings_warn_and_never_block_launch() {
    let f = Fixture::new();
    let dir = f.fragments(
        "settings",
        &[
            (
                "good.json",
                json!({"hooks":{"Stop":[{"hooks":[hook("good")]}]}}),
            ),
            ("later.json", json!({"outputStyle":"terse"})),
        ],
    );
    f.ok(&["set", "create", "core", "--default"]);
    f.ok(&["set", "add", "core", "--settings-from", s(&dir)]);
    f.ok(&["add", "Work"]);
    // A fragment that became invalid after `set add` is skipped with a warning.
    fs::write(dir.join("later.json"), r#"{"model":"opus"}"#).unwrap();
    let out = f.ok(&["run", "Work", "--", "hi"]);
    let child: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(child["arguments"], json!(["hi"]));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("later.json") && stderr.contains("model"),
        "{stderr}"
    );
    assert_eq!(
        f.settings("Work"),
        json!({"hooks":{"Stop":[{"hooks":[hook("good")]}]}})
    );
    // A recorded handler the user edited is theirs: never removed. The set's
    // handler comes back beside it.
    let edited = json!({"type":"command","command":"good","timeout":5});
    f.write_settings(
        "Work",
        &json!({"hooks":{"Stop":[{"hooks":[edited]}]}}).to_string(),
    );
    f.ok(&["run", "Work"]);
    assert_eq!(
        f.settings("Work"),
        json!({"hooks":{"Stop":[{"hooks":[edited, hook("good")]}]}})
    );
    f.ok(&["set", "unsubscribe", "Work", "core"]);
    f.ok(&["run", "Work"]);
    assert_eq!(
        f.settings("Work"),
        json!({"hooks":{"Stop":[{"hooks":[edited]}]}})
    );
    f.ok(&["set", "subscribe", "Work", "core"]);
    // Malformed settings.json: untouched, warning, launch proceeds, retried later.
    f.write_settings("Work", "{ not json");
    let stderr = f.stderr_of(&["run", "Work"]);
    assert!(stderr.contains("settings.json"), "{stderr}");
    assert_eq!(f.settings_text("Work"), "{ not json");
    f.write_settings("Work", "{}\n");
    f.ok(&["run", "Work"]);
    assert_eq!(
        f.settings_text("Work"),
        "{\n  \"hooks\": {\n    \"Stop\": [\n      {\n        \"hooks\": [\n          {\n            \"command\": \"good\",\n            \"type\": \"command\"\n          }\n        ]\n      }\n    ]\n  }\n}\n"
    );
    // A linked settings.json is never followed or replaced.
    let elsewhere = f.path.join("dotfiles.json");
    fs::write(&elsewhere, "{}").unwrap();
    fs::remove_file(f.settings_path("Work")).unwrap();
    std::os::unix::fs::symlink(&elsewhere, f.settings_path("Work")).unwrap();
    let stderr = f.stderr_of(&["run", "Work"]);
    assert!(stderr.contains("settings.json"), "{stderr}");
    assert_eq!(fs::read_to_string(&elsewhere).unwrap(), "{}");
    assert!(
        fs::symlink_metadata(f.settings_path("Work"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    // No Roost temporaries are left in the profile.
    let leftovers: Vec<_> = fs::read_dir(f.profile("Work"))
        .unwrap()
        .filter_map(|e| e.unwrap().file_name().into_string().ok())
        .filter(|n| n.starts_with(".roost-settings"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

const FAKE_DESKTOP: &str = "#!/usr/bin/python3\nimport sys\nsys.exit(0)\n";

#[test]
fn every_launch_path_reconciles_settings_into_owned_profiles_only() {
    let f = Fixture::with_fakes(
        "settings",
        &[("claude", FAKE), ("claude-desktop", FAKE_DESKTOP)],
    );
    let dir = f.fragments(
        "settings",
        &[(
            "all.json",
            json!({"hooks":{"Stop":[{"hooks":[hook("h")]}]},"outputStyle":"terse"}),
        )],
    );
    let wanted = json!({"hooks":{"Stop":[{"hooks":[hook("h")]}]},"outputStyle":"terse"});
    f.ok(&["set", "create", "core", "--default"]);
    f.ok(&["set", "add", "core", "--settings-from", s(&dir)]);
    f.ok(&["add", "Work"]);
    let reset = || {
        let _ = fs::remove_file(f.settings_path("Work"));
    };
    let launcher = || {
        let out = std::process::Command::new(f.root.join("bin/roost-Work"))
            .env_clear()
            .env("HOME", &f.home)
            .env("PATH", format!("{}:/usr/bin:/bin", f.bin.display()))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    let paths: [(&str, &dyn Fn()); 5] = [
        ("run", &|| drop(f.ok(&["run", "Work"]))),
        ("launcher", &launcher),
        ("switch", &|| drop(f.ok(&["switch", "Work"]))),
        ("bare", &|| drop(f.ok(&["--"]))),
        ("desktop", &|| {
            drop(f.ok(&["desktop", "--foreground", "Work"]))
        }),
    ];
    for (name, launch) in paths {
        reset();
        launch();
        assert_eq!(f.settings("Work"), wanted, "{name}");
    }
    // Upstream and default-alias data is never written.
    let upstream = f.upstream("up");
    f.ok(&["register", "up", "--path", s(&upstream)]);
    f.ok(&["set", "subscribe", "up", "core"]);
    fs::write(upstream.join("settings.json"), "{}").unwrap();
    f.ok(&["run", "up"]);
    f.ok(&["desktop", "--foreground", "up"]);
    assert_eq!(
        fs::read_to_string(upstream.join("settings.json")).unwrap(),
        "{}"
    );
    f.ok(&["add", "personal", "--link-default"]);
    f.ok(&["run", "personal"]);
    assert!(!f.home.join(".claude/settings.json").exists());
    assert_eq!(f.state()["settings"].as_array().unwrap().len(), 1);
    // status, where and list never reconcile.
    reset();
    f.run(&["status", "Work"]);
    f.ok(&["where", "Work"]);
    f.ok(&["ls"]);
    assert!(!f.settings_path("Work").exists());
}

#[test]
fn remove_keeps_settings_records_and_purge_drops_them() {
    let f = Fixture::new();
    let dir = f.fragments(
        "settings",
        &[("a.json", json!({"hooks":{"Stop":[{"hooks":[hook("a")]}]}}))],
    );
    f.ok(&["set", "create", "core", "--default"]);
    f.ok(&["set", "add", "core", "--settings-from", s(&dir)]);
    f.ok(&["add", "Work"]);
    f.ok(&["run", "Work"]);
    let recorded = f.state()["settings"].clone();
    assert_eq!(recorded.as_array().unwrap().len(), 1);
    f.ok(&["remove", "Work"]);
    assert_eq!(f.state()["settings"], recorded);
    f.ok(&["reuse", "Work"]);
    f.ok(&["run", "Work"]);
    assert_eq!(f.state()["settings"], recorded);
    f.ok(&["remove", "Work", "--purge", "--yes"]);
    assert!(!f.profile("Work").exists());
    assert!(f.state().get("settings").is_none(), "{}", f.state());
    // A new profile with the same name starts with its own, unrecorded settings.
    f.ok(&["add", "Work", "--no-sets"]);
    f.ok(&["run", "Work"]);
    assert!(!f.settings_path("Work").exists());
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
}

#[test]
fn two_fragments_setting_one_key_are_refused_at_subscribe_and_skipped_at_launch() {
    let f = Fixture::new();
    let one = f.fragments("one", &[("a.json", json!({"outputStyle":"terse"}))]);
    let two = f.fragments(
        "two",
        &[(
            "b.json",
            json!({"outputStyle":"verbose","hooks":{"Stop":[{"hooks":[hook("b")]}]}}),
        )],
    );
    f.ok(&["set", "create", "first"]);
    f.ok(&["set", "create", "second"]);
    f.ok(&["set", "add", "first", "--settings-from", s(&one)]);
    f.ok(&["set", "add", "second", "--settings-from", s(&two)]);
    f.ok(&["add", "Work", "--no-sets"]);
    f.ok(&["set", "subscribe", "Work", "first"]);
    let out = f.fails(&["set", "subscribe", "Work", "second"], "collision");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("outputStyle"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(f.sets()[1]["subscribers"], json!([]));
    f.ok(&["run", "Work"]);
    assert_eq!(f.settings("Work"), json!({"outputStyle":"terse"}));
    // A fragment added later conflicts only at launch: warn, leave the key alone,
    // and still apply everything else.
    f.ok(&["set", "add", "first", "--setting", s(&two.join("b.json"))]);
    let stderr = f.stderr_of(&["run", "Work"]);
    assert!(stderr.contains("outputStyle"), "{stderr}");
    assert_eq!(
        f.settings("Work"),
        json!({"outputStyle":"terse","hooks":{"Stop":[{"hooks":[hook("b")]}]}})
    );
}
