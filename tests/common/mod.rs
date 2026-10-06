//! The disposable fixture shared by the integration tests: a private temp directory
//! holding fake executables in `bin/`, a fake `HOME` and a `ROOST_DIR` that does not
//! exist yet. Commands run with a cleared environment and `PATH=bin:/usr/bin:/bin`,
//! so the real Claude, account, credentials and startup files are never reached.
//! Each test crate adds its own constructor and helpers in an `impl Fixture` block.
#![allow(dead_code, reason = "each test crate uses a different subset")]
use serde_json::Value;
use std::{
    ffi::OsString,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub struct Fixture {
    pub path: PathBuf,
    pub root: PathBuf,
    pub bin: PathBuf,
    pub home: PathBuf,
    env: Vec<(String, OsString)>,
}

impl Fixture {
    /// A fresh fixture under `roost-<prefix>-<pid>-<n>` with each `(name, script)`
    /// of `fakes` written to `bin/` as an owner-only executable.
    pub fn with_fakes(prefix: &str, fakes: &[(&str, &str)]) -> Self {
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "roost-{prefix}-{}-{}",
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
        for (name, script) in fakes {
            let fake = bin.join(name);
            fs::write(&fake, script).unwrap();
            fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let root = path.join("root");
        Self {
            path,
            root,
            bin,
            home,
            env: vec![],
        }
    }

    /// Adds a variable every `command()` sets.
    pub fn with_var(mut self, key: &str, value: impl Into<OsString>) -> Self {
        self.env.push((key.to_owned(), value.into()));
        self
    }

    /// The Roost binary under test with only the fixture's environment.
    pub fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_roost"));
        command
            .env_clear()
            .env("ROOST_DIR", &self.root)
            .env("HOME", &self.home)
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("SHELL", "/bin/bash")
            .current_dir(&self.path);
        for (key, value) in &self.env {
            command.env(key, value);
        }
        command
    }

    pub fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }

    pub fn ok(&self, args: &[&str]) -> Output {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }

    /// Runs `args`, expecting failure with `code` on stderr.
    pub fn fails(&self, args: &[&str], code: &str) -> Output {
        let out = self.run(args);
        assert!(!out.status.success(), "{args:?} unexpectedly succeeded");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(code), "{args:?}: {stderr}");
        out
    }

    /// Stdout of a successful run, parsed as JSON.
    pub fn json(&self, args: &[&str]) -> Value {
        parsed(&self.ok(args))
    }

    /// An upstream profile directory under the fake home.
    pub fn upstream(&self, name: &str) -> PathBuf {
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

pub fn private(path: &Path) {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

pub fn parsed(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&output.stdout)))
}

pub fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}
