#![cfg(unix)]
mod common;

use common::Fixture;
use std::{fs, fs::OpenOptions};

fn complete(fixture: &Fixture, words: &[&str]) -> Vec<String> {
    let output = fixture
        .command()
        .args(["__roost_complete", "roost"])
        .args(words)
        .output()
        .unwrap();
    assert!(output.status.success(), "{words:?}: {output:?}");
    assert!(output.stderr.is_empty(), "{words:?}: {output:?}");
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn shell_scripts_and_cli_completions_need_no_storage() {
    let fixture = Fixture::with_fakes("completion-empty", &[]);
    for shell in ["bash", "zsh", "fish"] {
        let script = String::from_utf8(fixture.ok(&["completions", shell]).stdout).unwrap();
        assert!(script.contains("roost __roost_complete"));
    }
    for (words, expected) in [
        (&["pl"][..], "plugin"),
        (&["plugin", "marketplace", "re"], "remove"),
        (&["ls", "--fu"], "--full"),
        (&["switch", "--no-"], "--no-launch"),
        (&["switch", "--forget", "--h"], "--help"),
        (&["set", "add", "team", "--skills-"], "--skills-from"),
        (&["completions", "fi"], "fish"),
        (&["--allow-auth-env", "mc"], "mcp"),
    ] {
        assert_eq!(complete(&fixture, words), [expected], "{words:?}");
    }
    assert!(complete(&fixture, &["__"]).is_empty());
    assert!(complete(&fixture, &["run", "work"]).is_empty());
    assert!(!fixture.root.exists());
}

#[test]
fn completion_omits_conflicting_options_and_keeps_incomplete_commands() {
    let fixture = Fixture::with_fakes("completion-options", &[]);
    for words in [
        &["ls", "--full", "--fu"][..],
        &["token", "work", "--stdin", "--cl"],
        &["plugin", "add", "pkg", "--no-set", "--se"],
        &["set", "add", "team", "--skill", "./one", "--ins"],
    ] {
        assert!(complete(&fixture, words).is_empty(), "{words:?}");
    }
    for (words, expected) in [
        (&["desktop", "--cl"][..], "--close"),
        (&["plugin", "add", "pkg", "--set", "team", "--se"], "--set"),
    ] {
        assert_eq!(complete(&fixture, words), [expected], "{words:?}");
    }
}

#[test]
fn profiles_follow_the_active_root_and_leave_storage_untouched() {
    let fixture = Fixture::with_fakes("completion-profiles", &[]);
    fixture.ok(&["add", "Work"]);
    fixture.ok(&["codex", "add", "work"]);
    let generation = fixture.root.join("codex/work/.roost-codex-generation");
    fs::remove_file(&generation).unwrap();
    let state = fixture.root.join("state.json");
    let before = fs::read(&state).ok();

    assert_eq!(complete(&fixture, &["run", "W"]), ["Work"]);
    assert_eq!(
        complete(&fixture, &["switch", "--no-launch", "claude:w"]),
        ["claude:work"]
    );
    assert_eq!(
        complete(&fixture, &["run", "--allow-auth-env", "codex:w"]),
        ["codex:work"]
    );
    assert!(!generation.exists());
    assert_eq!(fs::read(&state).ok(), before);

    fixture.ok(&["remove", "Work"]);
    assert!(complete(&fixture, &["run", "claude:"]).is_empty());
    assert_eq!(complete(&fixture, &["run", "w"]), ["work"]);
    fixture.ok(&["add", "Weekend"]);
    assert_eq!(complete(&fixture, &["run", "claude:W"]), ["claude:Weekend"]);

    let other = Fixture::with_fakes("completion-other", &[]);
    other.ok(&["add", "Personal"]);
    assert_eq!(complete(&other, &["run", "claude:"]), ["claude:Personal"]);

    let lock = OpenOptions::new()
        .write(true)
        .open(fixture.root.join(".roost-lock"))
        .unwrap();
    lock.try_lock().unwrap();
    assert!(complete(&fixture, &["run", "w"]).is_empty());
    assert_eq!(complete(&fixture, &["run", "--a"]), ["--allow-auth-env"]);
}

#[test]
fn completion_leaves_forwarded_arguments_to_the_shell() {
    let fixture = Fixture::with_fakes("completion-forwarding", &[]);
    fixture.ok(&["add", "work"]);
    for words in [
        &["run", "work", "--a"][..],
        &["run", "codex:work", "--a"],
        &["switch", "work", "--no-"],
        &["switch", "--", "w"],
        &["switch", "--forget", "w"],
        &["switch", "--no-launch", "--allow-"],
        &["agents", "--h"],
        &["--allow-auth-env", "agents", "--h"],
        &["--", "pl"],
    ] {
        assert!(complete(&fixture, words).is_empty(), "{words:?}");
    }
}
