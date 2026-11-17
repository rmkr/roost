use super::{Agent, Error, Result};
use crate::{
    launch, platform,
    platform::{Directory, FileIdentity},
    store::{Kind, State, Store, side::SideFile},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Repository {
    pub path: PathBuf,
    pub common: PathBuf,
    pub checkout_identity: FileIdentity,
    pub common_identity: FileIdentity,
}

impl Repository {
    pub fn resolve(path: &Path) -> Result<Self> {
        let mut git = Command::new(launch::resolve_program("git")?);
        // Ambient Git variables can redirect Git to another repository/config.
        for (key, _) in std::env::vars_os() {
            if key.as_encoded_bytes().starts_with(b"GIT_") {
                git.env_remove(key);
            }
        }
        git.arg("-C").arg(platform::absolute(path)?).args([
            "rev-parse",
            "--path-format=absolute",
            "--show-toplevel",
            "--git-common-dir",
        ]);
        let invalid = || {
            Error::new(
                "not_repository",
                format!("{} is not a verified Git checkout", path.display()),
            )
        };
        let (output, status) = launch::probe(git, "git", Duration::from_secs(3))
            .map_err(|e| if e.code == "cancelled" { e } else { invalid() })?;
        if !status.success() {
            return Err(invalid());
        }
        let output = std::str::from_utf8(&output).map_err(|_| invalid())?;
        let mut lines = output.lines();
        let checkout = canonical_directory(lines.next().ok_or_else(invalid)?)?;
        let common = canonical_directory(lines.next().ok_or_else(invalid)?)?;
        if lines.next().is_some() {
            return Err(invalid());
        }
        // Use Roost's Git-common-directory semantics for launch matching.
        let project = crate::select::project_key(&checkout.path);
        let key = project.key?;
        if !project.warnings.is_empty() || key.canonicalize().ok().as_ref() != Some(&common.path) {
            return Err(invalid());
        }
        Ok(Self {
            checkout_identity: checkout.identity()?,
            common_identity: common.identity()?,
            path: checkout.path,
            common: common.path,
        })
    }

    pub fn verify(&self) -> Result<()> {
        if Self::resolve(&self.path)? != *self {
            return Err(Error::new(
                "ownership",
                format!("Repository checkout changed at {}", self.path.display()),
            ));
        }
        Ok(())
    }

    pub fn same_repo(&self, other: &Self) -> bool {
        self.common == other.common && self.common_identity == other.common_identity
    }

    fn check(&self) -> Result<()> {
        check_path(&self.path)?;
        check_path(&self.common)
    }
}

fn canonical_directory(raw: &str) -> Result<Directory> {
    let path = Path::new(raw);
    check_path(path)?;
    let resolved = path
        .canonicalize()
        .map_err(|e| Error::io("resolve Git directory", path, e))?;
    Directory::open(&resolved, false)
}

fn check_path(path: &Path) -> Result<()> {
    if !path.is_absolute() || platform::absolute(path)? != path {
        return Err(invalid_state());
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Profile {
    pub agent: Agent,
    pub label: String,
    pub generation: String,
    // Pins explicit selections of legacy Codex homes until confirmation binds a generation.
    pub legacy_identity: Option<FileIdentity>,
}

impl Profile {
    pub fn lookup(store: &Store, selector: &str) -> Result<Self> {
        if let Some(name) = selector.strip_prefix("codex:") {
            crate::store::validate_name(name)?;
            return Self::codex(store.codex_profile(&name.to_ascii_lowercase())?);
        }
        let name = selector.strip_prefix("claude:").unwrap_or(selector);
        crate::store::validate_name(name)?;
        match store.find(name, false) {
            Ok(registration) => Self::claude(store, registration),
            Err(e) if e.code == "not_found" && !selector.contains(':') => {
                let home = store.codex_profile(&name.to_ascii_lowercase())?;
                Self::codex(home)
            }
            Err(e) => Err(e),
        }
    }

    pub fn claude(store: &Store, registration: &crate::store::Registration) -> Result<Self> {
        if registration.kind == Kind::DefaultAlias || registration.state != State::Active {
            return Err(Error::new(
                "usage",
                "Select an active isolated Claude profile",
            ));
        }
        store.validate(registration)?;
        Ok(Self {
            agent: Agent::Claude,
            label: format!("claude:{}", registration.name),
            generation: registration.registration_id.clone(),
            legacy_identity: None,
        })
    }

    pub fn codex(home: crate::store::CodexHome) -> Result<Self> {
        Ok(Self {
            agent: Agent::Codex,
            label: format!("codex:{}", home.name),
            generation: home.id,
            legacy_identity: home.unbound.as_ref().map(Directory::identity).transpose()?,
        })
    }

    pub fn bind(&mut self, store: &Store) -> Result<()> {
        let current = Self::lookup(store, &self.label)?;
        if *self != current {
            return Err(Error::new(
                "ownership",
                format!("Profile {} changed", self.label),
            ));
        }
        if let Some(name) = self.label.strip_prefix("codex:")
            && self.legacy_identity.is_some()
        {
            *self = Self::codex(store.codex_profile(name)?.bind(store)?)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Target {
    Claude,
    Codex { profile: Profile },
    Skills { repository: Repository },
}

impl Target {
    pub fn label(&self) -> &str {
        match self {
            Self::Claude => "Claude plugin store",
            Self::Codex { profile } => &profile.label,
            Self::Skills { repository } => repository.path.to_str().unwrap_or("Git checkout"),
        }
    }

    fn check(&self) -> Result<()> {
        match self {
            Self::Claude => Ok(()),
            Self::Codex { profile }
                if profile.agent == Agent::Codex && profile.label.starts_with("codex:") =>
            {
                crate::store::validate_name(&profile.label[6..]).map_err(|_| invalid_state())
            }
            Self::Codex { .. } => Err(invalid_state()),
            Self::Skills { repository } => repository.check(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Artifact {
    Native {
        id: String,
        revision: String,
        owned: bool,
        #[serde(default)]
        provenance: Option<String>,
    },
    Skills {
        receipt: crate::managed_skills::Receipt,
    },
}

impl Artifact {
    pub fn revision(&self) -> &str {
        match self {
            Self::Native { revision, .. } => revision,
            Self::Skills { receipt } => &receipt.revision,
        }
    }

    fn check(&self, target: &Target) -> Result<()> {
        if self.revision().is_empty() {
            return Err(invalid_state());
        }
        match (self, target) {
            (Self::Native { id, .. }, Target::Claude | Target::Codex { .. })
                if id
                    .split_once('@')
                    .is_some_and(|(name, market)| identifier(name) && identifier(market)) =>
            {
                Ok(())
            }
            (Self::Skills { .. }, Target::Skills { .. }) => Ok(()),
            _ => Err(invalid_state()),
        }
    }
}

pub(super) fn identifier(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && !matches!(value, "." | "..")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Entry {
    pub installed: Option<Artifact>,
    pub failure: Option<String>,
    #[serde(default)]
    pub observed_revision: Option<String>,
    #[serde(default)]
    pub local_conflict: Option<String>,
    pub candidate: Option<String>,
    pub check_failure: Option<String>,
    pub deferred: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TargetEntry {
    pub target: Target,
    pub status: Entry,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Rule {
    pub id: String,
    pub source: String,
    pub agents: Vec<Agent>,
    pub skills: Vec<String>,
    pub all_repos: bool,
    pub repositories: Vec<Repository>,
    #[serde(default)]
    pub unresolved: Vec<UnresolvedRepo>,
    pub all_profiles: bool,
    pub profiles: Vec<Profile>,
    pub targets: Vec<TargetEntry>,
    pub skill_revision: Option<String>,
    #[serde(default)]
    pub codex_revision: Option<String>,
    #[serde(default)]
    pub native_ids: BTreeMap<Agent, String>,
    pub removing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UnresolvedRepo {
    pub path: PathBuf,
    pub code: String,
    pub reason: String,
}

impl Rule {
    pub fn portable(&self) -> bool {
        !self.skills.is_empty()
    }

    pub fn same_scope(&self, other: &Self) -> bool {
        self.source == other.source
            && self.agents == other.agents
            && self.skills == other.skills
            && self.all_repos == other.all_repos
            && self.repositories == other.repositories
            && self
                .unresolved
                .iter()
                .map(|repo| &repo.path)
                .eq(other.unresolved.iter().map(|repo| &repo.path))
            && self.all_profiles == other.all_profiles
            && self.profiles == other.profiles
    }

    pub fn allows_profile(&self, agent: Agent, generation: &str) -> bool {
        self.agents.contains(&agent)
            && (self.all_profiles || self.profiles.iter().any(|p| p.generation == generation))
    }

    pub fn matching_repo(&self, key: Option<&Path>) -> bool {
        self.all_repos
            || key.is_some_and(|key| {
                self.repositories.iter().any(|repo| {
                    repo.common == key
                        && Directory::open(key, false)
                            .and_then(|dir| dir.identity())
                            .is_ok_and(|id| id == repo.common_identity)
                })
            })
    }

    fn check(&self) -> Result<()> {
        if self.agents.is_empty()
            || self.source.is_empty()
            || self.source.starts_with('-')
            || self.source.chars().any(char::is_control)
            || self.id.len() != 32
            || !self.id.bytes().all(|b| b.is_ascii_hexdigit())
            || self.skills.iter().any(|s| s != "*" && !identifier(s))
            || (!self.all_repos && self.repositories.is_empty() && self.unresolved.is_empty())
            || (self.portable() && (self.all_profiles || !self.profiles.is_empty()))
            || (!self.portable() && !self.all_profiles && self.profiles.is_empty())
        {
            return Err(invalid_state());
        }
        for repo in &self.repositories {
            repo.check()?;
        }
        for repo in &self.unresolved {
            check_path(&repo.path)?;
        }
        for (agent, id) in &self.native_ids {
            if self.portable()
                || !self.agents.contains(agent)
                || crate::managed_native::split_id(id).is_err()
            {
                return Err(invalid_state());
            }
        }
        for (index, entry) in self.targets.iter().enumerate() {
            entry.target.check()?;
            if self.targets[..index]
                .iter()
                .any(|prior| prior.target == entry.target)
                || self.portable() != matches!(entry.target, Target::Skills { .. })
            {
                return Err(invalid_state());
            }
            if let Some(artifact) = &entry.status.installed {
                artifact.check(&entry.target)?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Database {
    pub schema_version: u32,
    pub root_id: String,
    pub repositories: Vec<Repository>,
    pub rules: Vec<Rule>,
    pub last_background_check: Option<u64>,
    #[serde(default)]
    pub background_rotation: usize,
}

impl SideFile for Database {
    const NAME: &'static str = crate::store::side::MANAGED_PLUGINS;

    fn empty_for(root_id: &str) -> Self {
        Self {
            schema_version: 1,
            root_id: root_id.to_owned(),
            repositories: vec![],
            rules: vec![],
            last_background_check: None,
            background_rotation: 0,
        }
    }

    fn header(&self) -> (u32, &str) {
        (self.schema_version, &self.root_id)
    }

    fn check(&self) -> Result<()> {
        let mut paths = BTreeSet::new();
        for repo in &self.repositories {
            repo.check()?;
            if !paths.insert(&repo.path) {
                return Err(invalid_state());
            }
        }
        let mut ids = BTreeSet::new();
        for rule in &self.rules {
            rule.check()?;
            if !ids.insert(&rule.id) {
                return Err(invalid_state());
            }
            for entry in &rule.targets {
                if let Some(Artifact::Skills { receipt }) = &entry.status.installed {
                    receipt.verify_owner(&self.root_id)?;
                }
            }
        }
        Ok(())
    }

    fn retain_registered(&mut self, _: &std::collections::HashSet<&str>) {
        // Retain orphaned target receipts for reporting and ownership checks.
    }
}

fn invalid_state() -> Error {
    Error::new(
        "ownership",
        "Invalid managed-plugins.json; preserve it for inspection",
    )
}

pub(super) fn current_project_key() -> Option<PathBuf> {
    let project = crate::select::project();
    if !project.warnings.is_empty() {
        return None;
    }
    project.key.ok()?.canonicalize().ok()
}

pub(super) fn current_directory() -> Result<PathBuf> {
    std::env::current_dir().map_err(|e| Error::io("read current directory", Path::new("."), e))
}

pub(super) fn validate_source(source: &str) -> Result<()> {
    if source.is_empty() || source.starts_with('-') || source.chars().any(char::is_control) {
        return Err(Error::new("usage", "Provide a plugin or skill source"));
    }
    // Refuse URLs that would save credentials in the managed rule or display them.
    if let Some((_, rest)) = source.split_once("://")
        && (rest
            .split('/')
            .next()
            .is_some_and(|authority| authority.contains('@'))
            || rest.contains(['?', '#']))
    {
        return Err(Error::new(
            "usage",
            "Use a source URL without credentials, query, or fragment",
        ));
    }
    Ok(())
}

pub(super) fn native_source(rule: &Rule, agent: Agent) -> &str {
    match agent {
        Agent::Claude => rule.source.strip_prefix("claude:").unwrap_or(&rule.source),
        Agent::Codex => rule.source.strip_prefix("codex:").unwrap_or(&rule.source),
    }
}

pub(super) fn skill_source(source: &str) -> Result<String> {
    if Path::new(source).is_absolute() || source.starts_with("./") || source.starts_with("../") {
        let path = Path::new(source)
            .canonicalize()
            .map_err(|e| Error::io("resolve skill source", Path::new(source), e))?;
        Ok(platform::absolute(&path)?.to_string_lossy().into_owned())
    } else {
        Ok(source.to_owned())
    }
}
