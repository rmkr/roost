use super::{
    Agent, Artifact, Database, Directory, Error, OpenMode, Outcome, Profile, Result, Rule, Session,
    Store, Target, TargetEntry, add_targets, confirmation, empty_db, is_interactive, model, output,
    platform, rule_header, target_preview,
};
use crate::{managed_native as native, managed_skills as skills};
use serde_json::Value;
use std::{
    borrow::Cow,
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Job {
    Check,
    Sync,
    Update,
    Remove,
}

pub(super) fn expand(session: &Session, rule: &mut Rule, warnings: &mut Vec<String>) -> Result<()> {
    let store = session.open(OpenMode::Mutate)?;
    let db = store.read_side_unfiltered()?;
    add_targets(rule, &db, Some(&store), true, warnings)
}

pub(super) fn list(data: &mut Value) -> Result<Outcome> {
    let root = platform::root()?;
    let store = Store::open_if_present(&root)?;
    let mut db = empty_db(store.as_ref())?;
    let mut warnings = vec![];
    let inventory = Database {
        rules: vec![],
        ..db.clone()
    };
    for rule in db.rules.iter_mut().filter(|rule| !rule.removing) {
        add_targets(rule, &inventory, store.as_ref(), false, &mut warnings)?;
    }
    output(db.rules, warnings, false, data)
}

fn source_matches(rule: &Rule, source: Option<&str>) -> bool {
    source.is_none_or(|source| {
        rule.id == source
            || rule.source == source
            || (rule.portable()
                && model::skill_source(source).is_ok_and(|normalized| normalized == rule.source))
    })
}

fn normalized(source: &str) -> Cow<'_, str> {
    let source = source
        .trim_start_matches("claude:")
        .trim_start_matches("codex:");
    match native::CatalogSource::remote(source) {
        Some(native::CatalogSource::Github(name)) => {
            Cow::Owned(name.strip_suffix(".git").unwrap_or(&name).to_owned())
        }
        _ => Cow::Borrowed(source),
    }
}

pub(super) fn verify_mode(rule: &Rule, db: &Database) -> Result<()> {
    let source = normalized(&rule.source);
    for other in db
        .rules
        .iter()
        .filter(|other| other.portable() != rule.portable() && !other.removing)
    {
        let rule_provenance =
            rule.targets
                .iter()
                .filter_map(|entry| match &entry.status.installed {
                    Some(Artifact::Native {
                        provenance: Some(source),
                        ..
                    }) => Some(normalized(source)),
                    _ => None,
                });
        let other_provenance =
            other
                .targets
                .iter()
                .filter_map(|entry| match &entry.status.installed {
                    Some(Artifact::Native {
                        provenance: Some(source),
                        ..
                    }) => Some(normalized(source)),
                    _ => None,
                });
        let other_source = normalized(&other.source);
        if source != other_source
            && !rule_provenance
                .into_iter()
                .any(|source| source == other_source)
            && !other_provenance.into_iter().any(|other| other == source)
        {
            continue;
        }
        let overlapping_agent = rule.agents.iter().any(|agent| {
            if !other.agents.contains(agent) {
                return false;
            }
            other.targets.iter().any(|entry| {
                entry.status.installed.is_some()
                    && match entry.target {
                        Target::Claude => *agent == Agent::Claude,
                        Target::Codex { .. } => *agent == Agent::Codex,
                        Target::Skills { .. } => true,
                    }
            })
        });
        if overlapping_agent
            && (rule.all_repos
                || other.all_repos
                || rule
                    .repositories
                    .iter()
                    .any(|repo| other.repositories.iter().any(|other| repo.same_repo(other))))
        {
            return Err(Error::new(
                "collision",
                "This source already uses the other install mode for an overlapping target",
            )
            .next("Choose a different scope or uninstall the overlapping managed rule"));
        }
    }
    Ok(())
}

fn native_agent(target: &Target) -> Option<Agent> {
    match target {
        Target::Claude => Some(Agent::Claude),
        Target::Codex { .. } => Some(Agent::Codex),
        Target::Skills { .. } => None,
    }
}

pub(super) fn resolve_native_ids(rule: &mut Rule) -> Result<()> {
    for agent in rule.agents.clone() {
        let pending = |entry: &TargetEntry| {
            native_agent(&entry.target) == Some(agent) && entry.status.installed.is_none()
        };
        if rule.native_ids.contains_key(&agent) || !rule.targets.iter().any(pending) {
            continue;
        }
        let source = model::native_source(rule, agent);
        let resolved = match agent {
            Agent::Claude => native::claude_preview(source),
            Agent::Codex => native::codex_preview(source),
        };
        match resolved {
            Ok(id) => {
                rule.native_ids.insert(agent, id);
            }
            Err(error) if error.code == "cancelled" => return Err(error),
            Err(error) => {
                let mut failure = format!("{}: {}", error.code, error.message);
                if matches!(error.code, "not_found" | "unsupported") {
                    failure.push_str(
                        ". For a portable skill source, retry with --skills --skill NAME",
                    );
                }
                for entry in rule.targets.iter_mut().filter(|entry| pending(entry)) {
                    entry.status.failure = Some(failure.clone());
                }
            }
        }
    }
    Ok(())
}

pub(super) fn keep_native_preview(rule: &mut Rule, preview: &Rule) {
    for (agent, id) in &preview.native_ids {
        rule.native_ids.entry(*agent).or_insert_with(|| id.clone());
    }
    for entry in &mut rule.targets {
        let Some(agent) = native_agent(&entry.target) else {
            continue;
        };
        if entry.status.installed.is_none()
            && !rule.native_ids.contains_key(&agent)
            && let Some(failure) = preview
                .targets
                .iter()
                .filter(|prior| native_agent(&prior.target) == Some(agent))
                .find_map(|prior| prior.status.failure.as_ref())
        {
            entry.status.failure = Some(failure.clone());
        }
    }
}

fn verify_native_mode(
    rule: &Rule,
    db: &Database,
    agent: Agent,
    provenance: Result<Option<&str>>,
) -> Result<()> {
    let overlapping = db.rules.iter().filter(|other| {
        other.portable() && other.agents.contains(&agent)
            && other.targets.iter().any(|entry| {
                entry.status.installed.is_some()
                    && matches!(&entry.target, Target::Skills { repository } if
                        rule.all_repos || rule.repositories.iter().any(|repo| repo.same_repo(repository)))
            })
    });
    let mut overlapping = overlapping.peekable();
    if overlapping.peek().is_none() {
        return Ok(());
    }
    let provenance = require_catalog(provenance?)?;
    for other in overlapping {
        verify_distinct_catalogs(&other.source, provenance)?;
    }
    Ok(())
}

fn require_catalog<T>(provenance: Option<T>) -> Result<T> {
    provenance.ok_or_else(|| {
        Error::new(
            "unverified_catalog",
            "Cannot verify the native marketplace source against existing portable skills",
        )
        .next("Verify the marketplace configuration or specify its source, then retry")
    })
}

fn verify_distinct_catalogs(portable_source: &str, native_source: &str) -> Result<()> {
    if normalized(portable_source) == normalized(native_source) {
        return Err(Error::new(
            "collision",
            "This marketplace already uses the other install mode for an overlapping target",
        ));
    }
    Ok(())
}

fn verify_portable_catalogs(
    session: &Session,
    rule: &Rule,
    db: &Database,
    repository: &model::Repository,
) -> Result<()> {
    for other in db.rules.iter().filter(|other| {
        !other.portable()
            && (other.all_repos
                || other
                    .repositories
                    .iter()
                    .any(|anchor| anchor.same_repo(repository)))
    }) {
        for entry in &other.targets {
            let Some(agent) = native_agent(&entry.target) else {
                continue;
            };
            if !rule.agents.contains(&agent) {
                continue;
            }
            if let Some(Artifact::Native { id, provenance, .. }) = &entry.status.installed {
                let discovered;
                let source = match provenance {
                    Some(source) => source.as_str(),
                    None => {
                        discovered = match &entry.target {
                            Target::Claude => native::claude_provenance(&session.root, id)?,
                            Target::Codex { profile } => {
                                let home = codex_home(session, profile)?;
                                native::codex_provenance(&home, &profile.generation, id)?
                            }
                            Target::Skills { .. } => unreachable!(),
                        };
                        require_catalog(discovered.as_deref())?
                    }
                };
                verify_distinct_catalogs(&rule.source, source)?;
            }
        }
    }
    Ok(())
}

fn selected_rules(db: &Database, source: Option<&str>, job: Job) -> Result<Vec<Rule>> {
    let rules = db
        .rules
        .iter()
        .filter(|rule| source_matches(rule, source) && (!rule.removing || job == Job::Remove))
        .cloned()
        .collect::<Vec<_>>();
    if rules.is_empty() && source.is_some() {
        return Err(Error::new(
            "not_found",
            "No matching managed plugin or skill install",
        ));
    }
    Ok(rules)
}

pub(super) fn dispatch(
    job: Job,
    source: Option<&str>,
    yes: bool,
    json: bool,
    data: &mut Value,
) -> Result<Outcome> {
    let root = platform::root()?;
    let db = empty_db(Store::open_if_present(&root)?.as_ref())?;
    let baseline = selected_rules(&db, source, job)?;
    if baseline.is_empty() {
        return output(vec![], vec![], false, data);
    }
    let mut planned = baseline.clone();
    if job == Job::Sync {
        for rule in &mut planned {
            resolve_pending_repositories(rule)?;
            let store = Store::open_if_present(&root)?;
            add_targets(rule, &db, store.as_ref(), false, &mut vec![])?;
            drop(store);
            resolve_native_ids(rule)?;
        }
    }
    let (lines, targets) =
        operation_preview(&planned, &db, job, Store::open_if_present(&root)?.as_ref())?;
    if job != Job::Check {
        if lines.is_empty() {
            return output(baseline, vec![], false, data);
        }
        let title = match job {
            Job::Sync => "Sync managed plugin and skill targets",
            Job::Update => "Apply available managed updates",
            Job::Remove => "Uninstall managed rules and their owned artifacts",
            Job::Check => unreachable!(),
        };
        confirmation(title, &lines, yes, is_interactive(json))?;
    }
    let Some(session) = Session::start(root, false, true)? else {
        return Err(Error::new("ownership", "Roost storage disappeared"));
    };
    let store = session.open(OpenMode::Read)?;
    let now = store.read_side_unfiltered::<Database>()?;
    if now.root_id != db.root_id {
        return Err(Error::new(
            "ownership",
            "Roost storage changed since the preview",
        ));
    }
    let current = selected_rules(&now, source, job)?;
    if job == Job::Check {
        planned = current;
    } else {
        if serde_json::to_value(&current).ok() != serde_json::to_value(&baseline).ok()
            || operation_preview(&planned, &now, job, Some(&store))? != (lines, targets)
        {
            return Err(
                Error::new("ownership", "Managed targets changed since the preview")
                    .next("Review the targets and retry"),
            );
        }
    }
    drop(store);
    let result = run(&session, planned, job, data)?;
    if job == Job::Check && source.is_none() {
        session.edit(|db| {
            db.last_background_check = Some(platform::unix_seconds());
            Ok(())
        })?;
    }
    Ok(result)
}

fn resolve_pending_repositories(rule: &mut Rule) -> Result<()> {
    let mut pending = vec![];
    for mut unresolved in std::mem::take(&mut rule.unresolved) {
        match model::Repository::resolve(&unresolved.path) {
            Ok(repo) => {
                if !rule.repositories.contains(&repo) {
                    rule.repositories.push(repo);
                }
            }
            Err(error) if error.code == "cancelled" => return Err(error),
            Err(error) => {
                unresolved.code = error.code.into();
                unresolved.reason = error.message;
                pending.push(unresolved);
            }
        }
    }
    rule.repositories
        .sort_by(|left, right| left.path.cmp(&right.path));
    rule.unresolved = pending;
    Ok(())
}

fn operation_preview(
    rules: &[Rule],
    db: &Database,
    job: Job,
    store: Option<&Store>,
) -> Result<(Vec<String>, Vec<Target>)> {
    if job == Job::Check || rules.is_empty() {
        return Ok((vec![], vec![]));
    }
    let store = store.ok_or_else(|| Error::new("ownership", "Roost storage disappeared"))?;
    let mut rules = rules.to_vec();
    if job == Job::Sync {
        for rule in &mut rules {
            let saved = db.rules.iter().find(|saved| saved.id == rule.id);
            let preview = rule.clone();
            if let Some(saved) = saved {
                rule.targets.clone_from(&saved.targets);
            }
            add_targets(rule, db, Some(store), false, &mut vec![])?;
            keep_native_preview(rule, &preview);
            rule.targets
                .retain(|entry| entry.status.installed.is_none());
        }
        rules.retain(|rule| !rule.targets.is_empty() || !rule.unresolved.is_empty());
    }
    let targets = rules
        .iter()
        .flat_map(|rule| rule.targets.iter().map(|entry| entry.target.clone()))
        .collect();
    Ok((preview_for(&rules, db, job, store)?, targets))
}

fn preview_for(rules: &[Rule], db: &Database, job: Job, store: &Store) -> Result<Vec<String>> {
    let mut rows = vec![];
    for rule in rules {
        if job == Job::Update
            && !rule
                .targets
                .iter()
                .any(|entry| entry.status.candidate.is_some())
        {
            continue;
        }
        rows.push(rule_header(rule));
        for entry in &rule.targets {
            if job != Job::Update {
                rows.push(target_preview(entry, rule, &store.root));
                continue;
            }
            let Some(candidate) = &entry.status.candidate else {
                continue;
            };
            let current = entry
                .status
                .observed_revision
                .as_deref()
                .or_else(|| entry.status.installed.as_ref().map(Artifact::revision))
                .unwrap_or("pending");
            rows.push(format!(
                "  {} | {current} → {candidate}",
                entry.target.label()
            ));
            for related in db.rules.iter().filter(|other| other.id != rule.id) {
                if related
                    .targets
                    .iter()
                    .any(|other| reference_matches(entry, other))
                {
                    rows.push(format!("    Also affects {}", rule_header(related)));
                }
            }
            if let (Target::Claude, Some(Artifact::Native { id, .. })) =
                (&entry.target, &entry.status.installed)
            {
                for name in crate::plugins::affected_sets(store, id)? {
                    rows.push(format!("    Also affects Claude set {name}"));
                }
            }
        }
        if job != Job::Update {
            for repo in &rule.unresolved {
                rows.push(format!(
                    "  {} | failed: {}",
                    repo.path.display(),
                    repo.reason
                ));
            }
        }
    }
    Ok(rows)
}

fn run(session: &Session, mut rules: Vec<Rule>, job: Job, data: &mut Value) -> Result<Outcome> {
    let mut warnings = vec![];
    let mut failed = false;
    for rule in &mut rules {
        if platform::cancelled() {
            return Err(Error::cancelled());
        }
        // Refresh after preceding rules may have updated shared native receipts.
        let Some(saved) = session
            .read()?
            .rules
            .into_iter()
            .find(|saved| saved.id == rule.id)
        else {
            continue;
        };
        if saved.removing && job != Job::Remove {
            continue;
        }
        if job == Job::Sync {
            let preview = rule.clone();
            let repositories = std::mem::take(&mut rule.repositories);
            let unresolved = std::mem::take(&mut rule.unresolved);
            *rule = saved;
            for repository in repositories
                .iter()
                .filter(|repo| !rule.repositories.contains(repo))
            {
                repository.verify()?;
            }
            rule.repositories = repositories;
            rule.unresolved = unresolved;
            expand(session, rule, &mut warnings)?;
            keep_native_preview(rule, &preview);
        } else {
            *rule = saved;
        }
        let result = match job {
            Job::Sync => {
                session.save(rule)?;
                sync_rule(session, rule)? || !rule.unresolved.is_empty()
            }
            Job::Update => update_rule(session, rule)?,
            Job::Check => check_rule(session, rule, 0)?,
            Job::Remove => remove_rule(session, rule)?,
        };
        failed |= result;
    }
    if job == Job::Remove {
        let remaining = session.read()?;
        rules.retain(|rule| remaining.rules.iter().any(|other| rule.id == other.id));
        if rules.is_empty() {
            let mut result = output(rules, warnings, failed, data)?;
            result.lines.push("Removed managed install rules".into());
            return Ok(result);
        }
    }
    output(rules, warnings, failed, data)
}

fn codex_home(session: &Session, profile: &Profile) -> Result<PathBuf> {
    let store = session.open(OpenMode::Read)?;
    let name = profile
        .label
        .strip_prefix("codex:")
        .ok_or_else(|| Error::new("ownership", "Invalid Codex target"))?;
    let home = store.codex_profile(name)?;
    if home.id != profile.generation || home.unbound.is_some() {
        return Err(Error::new(
            "ownership",
            format!("Codex home {} changed", profile.label),
        ));
    }
    Ok(home.path)
}

fn skill_agents(rule: &Rule) -> Vec<String> {
    rule.agents
        .iter()
        .map(|agent| match agent {
            Agent::Claude => "claude-code".into(),
            Agent::Codex => "codex".into(),
        })
        .collect()
}

fn native_artifact(install: native::NativeInstall) -> Artifact {
    Artifact::Native {
        id: install.id,
        revision: install.revision,
        owned: install.owned,
        provenance: install.provenance,
    }
}

fn prepare_skills(session: &Session, rule: &Rule) -> Result<skills::Staged> {
    let mut stage = skills::stage(&rule.source, &rule.skills, &skill_agents(rule))?;
    stage.bind_owner(&session.root_id)?;
    Ok(stage)
}

fn skills_cache(session: &Session, create: bool) -> Result<Directory> {
    let mode = if create {
        OpenMode::Mutate
    } else {
        OpenMode::Read
    };
    session
        .open(mode)?
        .reserved_dir(crate::store::side::Reserved::ManagedSkills, create)?
        .ok_or_else(|| Error::new("not_found", "The selected skill revision is not cached"))
}

pub(super) fn sync_rule(session: &Session, rule: &mut Rule) -> Result<bool> {
    if rule
        .targets
        .iter()
        .all(|entry| entry.status.installed.is_some())
    {
        return Ok(false);
    }
    let db = session.read()?;
    verify_mode(rule, &db)?;
    let staged = rule.portable().then(|| {
        if let Some(revision) = &rule.skill_revision {
            let mut stage = skills::load(&skills_cache(session, false)?, revision)?;
            stage.bind_owner(&session.root_id)?;
            Ok(stage)
        } else {
            let stage = prepare_skills(session, rule)?;
            stage.save(&skills_cache(session, true)?)?;
            Ok(stage)
        }
    });
    if let Some(Ok(staged)) = &staged
        && rule.skill_revision.is_none()
    {
        rule.skill_revision = Some(staged.revision().to_owned());
        session.save(rule)?;
    }
    let mut failed = false;
    for index in 0..rule.targets.len() {
        let entry = &rule.targets[index];
        if entry.status.installed.is_some() {
            continue;
        }
        if platform::cancelled() {
            return Err(Error::cancelled());
        }
        let approved_id = |agent| {
            rule.native_ids.get(&agent).ok_or_else(|| {
                Error::new(
                    "unresolved_plugin",
                    entry
                        .status
                        .failure
                        .clone()
                        .unwrap_or_else(|| "No plugin ID was resolved for this target".into()),
                )
            })
        };
        let result = match &entry.target {
            Target::Claude => approved_id(Agent::Claude).and_then(|id| {
                native::claude_install(
                    &session.root,
                    model::native_source(rule, Agent::Claude),
                    id,
                    |source| verify_native_mode(rule, &db, Agent::Claude, source),
                )
                .map(native_artifact)
            }),
            Target::Codex { profile } => (|| {
                let id = approved_id(Agent::Codex)?;
                let home = codex_home(session, profile)?;
                native::codex_install(
                    &home,
                    &profile.generation,
                    model::native_source(rule, Agent::Codex),
                    id,
                    rule.codex_revision.as_deref(),
                    |source| verify_native_mode(rule, &db, Agent::Codex, source),
                )
                .map(native_artifact)
            })(),
            Target::Skills { repository } => (|| {
                let staged = staged
                    .as_ref()
                    .expect("portable rule")
                    .as_ref()
                    .map_err(Clone::clone)?;
                if rule.skill_revision.as_deref() != Some(staged.revision()) {
                    return Err(Error::new(
                        "revision_changed",
                        "The skill source changed before sync",
                    )
                    .next("Review the new revision with roost plugin check, then update"));
                }
                repository.verify()?;
                verify_portable_catalogs(session, rule, &db, repository)?;
                let prior = db
                    .rules
                    .iter()
                    .filter(|other| {
                        other.id != rule.id
                            && !other.removing
                            && other.source == rule.source
                            && other.skills == rule.skills
                            && other.agents == rule.agents
                    })
                    .flat_map(|other| &other.targets)
                    .find_map(|other| match &other.status.installed {
                        Some(Artifact::Skills { receipt }) if other.target == entry.target => {
                            Some(receipt)
                        }
                        _ => None,
                    });
                if prior.is_some_and(|receipt| receipt.revision != staged.revision()) {
                    return Err(Error::new("conflict", "An overlapping install rule owns a different skill revision in this checkout")
                        .next("Update the installed rule before syncing this target"));
                }
                skills::apply_verified(
                    &repository.path,
                    &repository.checkout_identity,
                    &repository.common_identity,
                    staged,
                    prior,
                )
                .map(|receipt| Artifact::Skills { receipt })
            })(),
        };
        let codex_target = matches!(rule.targets[index].target, Target::Codex { .. });
        let status = &mut rule.targets[index].status;
        match result {
            Ok(artifact) => {
                if codex_target && rule.codex_revision.is_none() {
                    rule.codex_revision = Some(artifact.revision().to_owned());
                }
                status.installed = Some(artifact);
                status.failure = None;
            }
            Err(error) if error.code == "cancelled" => return Err(error),
            Err(error) => {
                failed = true;
                status.failure = Some(error.to_string());
            }
        }
        session.save(rule)?;
    }
    Ok(failed)
}

fn check_rule(session: &Session, rule: &mut Rule, offset: usize) -> Result<bool> {
    let staged = (rule.portable() && rule.skill_revision.is_some() && !rule.targets.is_empty())
        .then(|| prepare_skills(session, rule));
    let mut failed = false;
    for i in 0..rule.targets.len() {
        if platform::cancelled() {
            return Err(Error::cancelled());
        }
        let index = (i + offset) % rule.targets.len();
        let entry = &rule.targets[index];
        let before = entry.status.clone();
        let mut observed = None;
        let mut local_conflict = None;
        let result = match (&entry.target, &entry.status.installed) {
            (Target::Claude, Some(Artifact::Native { id, revision, .. })) => (|| {
                observed = native::claude_current(&session.root, id)?;
                verify_native_drift(revision, &observed)?;
                let candidate = native::claude_check(
                    &session.root,
                    model::native_source(rule, Agent::Claude),
                    id,
                )?;
                if observed.as_deref() != Some(revision)
                    && observed != native::claude_current(&session.root, id)?
                {
                    return Err(Error::new(
                        "conflict",
                        "Native plugin revision changed during the check",
                    ));
                }
                Ok(candidate)
            })(),
            (Target::Codex { profile }, Some(Artifact::Native { id, revision, .. })) => {
                codex_home(session, profile).and_then(|home| {
                    observed = native::codex_current(&home, &profile.generation, id)?;
                    verify_native_drift(revision, &observed)?;
                    let candidate = native::codex_check(
                        &home,
                        &profile.generation,
                        model::native_source(rule, Agent::Codex),
                        id,
                    )?;
                    if (observed.as_deref() != Some(revision) || observed != rule.codex_revision)
                        && observed != native::codex_current(&home, &profile.generation, id)?
                    {
                        return Err(Error::new(
                            "conflict",
                            "Native plugin revision changed during the check",
                        ));
                    }
                    Ok(candidate.or_else(|| {
                        observed
                            .as_ref()
                            .filter(|revision| {
                                rule.codex_revision
                                    .as_ref()
                                    .is_some_and(|pin| pin != *revision)
                            })
                            .cloned()
                    }))
                })
            }
            (Target::Skills { repository }, installed)
                if installed.is_some() || rule.skill_revision.is_some() =>
            {
                if let Some(Artifact::Skills { receipt }) = installed
                    && let Err(error) = repository
                        .verify()
                        .and_then(|()| skills::inspect(&repository.path, receipt))
                {
                    if error.code == "cancelled" {
                        return Err(error);
                    }
                    local_conflict = Some(error.to_string());
                    Ok(None)
                } else {
                    staged
                        .as_ref()
                        .ok_or_else(|| {
                            Error::new("ownership", "Skill rule has no selected revision")
                        })?
                        .as_ref()
                        .map_err(Clone::clone)
                        .map(|staged| {
                            let previous = installed
                                .as_ref()
                                .map(Artifact::revision)
                                .or(rule.skill_revision.as_deref());
                            (previous != Some(staged.revision()))
                                .then(|| staged.revision().to_owned())
                        })
                }
            }
            _ => continue,
        };
        let status = &mut rule.targets[index].status;
        status.observed_revision = observed;
        failed |= local_conflict.is_some();
        status.local_conflict = local_conflict;
        match result {
            Ok(candidate) => {
                status.candidate = candidate;
                status.check_failure = None;
            }
            Err(error) if error.code == "cancelled" => return Err(error),
            Err(error) => {
                failed = true;
                status.candidate = None;
                status.check_failure = Some(error.to_string());
            }
        }
        save_checked_status(session, rule, index, &before)?;
    }
    Ok(failed)
}

fn save_checked_status(
    session: &Session,
    rule: &Rule,
    index: usize,
    before: &super::Entry,
) -> Result<()> {
    if session.lock.is_some() {
        return session.save(rule);
    }
    let Some(_lock) = super::operation_lock(&session.root, false)? else {
        return Ok(());
    };
    let checked = &rule.targets[index];
    session.edit(|db| {
        if let Some(entry) = db
            .rules
            .iter_mut()
            .find(|saved| {
                saved.id == rule.id
                    && !saved.removing
                    && saved.skill_revision == rule.skill_revision
            })
            .and_then(|saved| {
                saved
                    .targets
                    .iter_mut()
                    .find(|entry| entry.target == checked.target && &entry.status == before)
            })
        {
            entry.status.clone_from(&checked.status);
        }
        Ok(())
    })
}

fn verify_native_drift(recorded: &str, observed: &Option<String>) -> Result<()> {
    if let Some(observed) = observed
        && observed != recorded
        && native::reported_version(observed) == native::reported_version(recorded)
    {
        return Err(Error::new(
            "conflict",
            "Installed native plugin content changed without a version change",
        ));
    }
    Ok(())
}

fn same_native(left: &Target, right: &Target) -> bool {
    match (left, right) {
        (Target::Claude, Target::Claude) => true,
        (Target::Codex { profile: left }, Target::Codex { profile: right }) => {
            left.generation == right.generation
        }
        _ => false,
    }
}

fn reference_matches(left: &TargetEntry, right: &TargetEntry) -> bool {
    match (&left.status.installed, &right.status.installed) {
        (
            Some(Artifact::Native { id: left_id, .. }),
            Some(Artifact::Native { id: right_id, .. }),
        ) => left_id == right_id && same_native(&left.target, &right.target),
        (
            Some(Artifact::Skills {
                receipt: left_receipt,
            }),
            Some(Artifact::Skills {
                receipt: right_receipt,
            }),
        ) => left.target == right.target && left_receipt == right_receipt,
        _ => false,
    }
}

fn save_updated_rule(
    session: &Session,
    rule: &Rule,
    previous_entry: &TargetEntry,
    index: usize,
) -> Result<()> {
    let updated = &rule.targets[index].status.installed;
    session.edit(|db| {
        for other in &mut db.rules {
            if other.id == rule.id {
                *other = rule.clone();
                continue;
            }
            for entry in other
                .targets
                .iter_mut()
                .filter(|entry| reference_matches(previous_entry, entry))
            {
                match (&mut entry.status.installed, updated) {
                    (
                        Some(Artifact::Native { revision, .. }),
                        Some(Artifact::Native { revision: new, .. }),
                    ) => {
                        revision.clone_from(new);
                    }
                    (_, Some(Artifact::Skills { .. })) => {
                        entry.status.installed.clone_from(updated)
                    }
                    _ => {}
                }
                entry.status.candidate = None;
                entry.status.deferred = None;
                entry.status.check_failure = None;
                entry.status.local_conflict = None;
                entry.status.observed_revision = None;
                entry.status.failure = None;
            }
        }
        Ok(())
    })
}

fn update_rule(session: &Session, rule: &mut Rule) -> Result<bool> {
    let selected = rule
        .targets
        .iter()
        .find_map(|entry| entry.status.candidate.clone());
    if selected.is_none() {
        return Ok(false);
    }
    let staged = rule.portable().then(|| {
        let stage = prepare_skills(session, rule)?;
        if selected.as_deref() == Some(stage.revision()) {
            stage.save(&skills_cache(session, true)?)?;
        }
        Ok(stage)
    });
    let mut failed = false;
    for index in 0..rule.targets.len() {
        if platform::cancelled() {
            return Err(Error::cancelled());
        }
        let entry = rule.targets[index].clone();
        let Some(candidate) = &entry.status.candidate else {
            continue;
        };
        let result = match (&entry.target, &entry.status.installed) {
            (
                Target::Claude,
                Some(Artifact::Native {
                    id,
                    owned,
                    revision,
                    provenance,
                }),
            ) => native::claude_update(
                &session.root,
                model::native_source(rule, Agent::Claude),
                id,
                entry
                    .status
                    .observed_revision
                    .as_deref()
                    .unwrap_or(revision),
                Some(candidate),
            )
            .map(|mut update| {
                update.owned = *owned;
                update.provenance = update.provenance.or_else(|| provenance.clone());
                native_artifact(update)
            }),
            (
                Target::Codex { profile },
                Some(Artifact::Native {
                    id,
                    owned,
                    revision,
                    provenance,
                }),
            ) => codex_home(session, profile).and_then(|home| {
                native::codex_update(
                    &home,
                    &profile.generation,
                    model::native_source(rule, Agent::Codex),
                    id,
                    entry
                        .status
                        .observed_revision
                        .as_deref()
                        .unwrap_or(revision),
                    Some(candidate),
                )
                .map(|mut update| {
                    update.owned = *owned;
                    update.provenance = update.provenance.or_else(|| provenance.clone());
                    native_artifact(update)
                })
            }),
            (Target::Skills { repository }, previous) => (|| {
                let staged = staged
                    .as_ref()
                    .expect("portable rule")
                    .as_ref()
                    .map_err(Clone::clone)?;
                if candidate != staged.revision() {
                    return Err(Error::new(
                        "revision_changed",
                        "The checked skill revision changed",
                    )
                    .next("Run roost plugin check again"));
                }
                repository.verify()?;
                verify_portable_catalogs(session, rule, &session.read()?, repository)?;
                let prior = match previous {
                    Some(Artifact::Skills { receipt }) => Some(receipt),
                    None => None,
                    _ => unreachable!("skills target has native artifact"),
                };
                skills::apply_verified(
                    &repository.path,
                    &repository.checkout_identity,
                    &repository.common_identity,
                    staged,
                    prior,
                )
                .map(|receipt| Artifact::Skills { receipt })
            })(),
            _ => Err(Error::new(
                "ownership",
                "Update target has no verified install",
            )),
        };
        let status = &mut rule.targets[index].status;
        let updated = result.is_ok();
        match result {
            Ok(artifact) => {
                match entry.target {
                    Target::Skills { .. } => {
                        rule.skill_revision = Some(artifact.revision().to_owned())
                    }
                    Target::Codex { .. } => {
                        rule.codex_revision = Some(artifact.revision().to_owned())
                    }
                    Target::Claude => {}
                }
                status.installed = Some(artifact);
                status.candidate = None;
                status.deferred = None;
                status.failure = None;
                status.check_failure = None;
                status.observed_revision = None;
                status.local_conflict = None;
            }
            Err(error) if error.code == "cancelled" => return Err(error),
            Err(error) => {
                failed = true;
                status.failure = Some(error.to_string());
            }
        }
        if updated {
            save_updated_rule(session, rule, &entry, index)?;
        } else {
            session.save(rule)?;
        }
    }
    Ok(failed)
}

fn keep_or_transfer_native(
    session: &Session,
    rule: &Rule,
    entry: &TargetEntry,
    id: &str,
    revision: &str,
) -> Result<bool> {
    let store = session.open(OpenMode::Read)?;
    let db: Database = store.read_side_unfiltered()?;
    if let Some((other, index)) = db
        .rules
        .iter()
        .filter(|other| other.id != rule.id)
        .find_map(|other| {
            other
                .targets
                .iter()
                .position(|target| reference_matches(entry, target))
                .map(|index| (other, index))
        })
    {
        let mut other = other.clone();
        if let Some(Artifact::Native {
            owned,
            revision: recorded,
            ..
        }) = &mut other.targets[index].status.installed
            && recorded == revision
        {
            *owned = true;
            drop(store);
            session.save(&other)?;
        }
        return Ok(true);
    }
    if matches!(entry.target, Target::Claude) {
        return Ok(store.read_sets()?.sets.iter().any(|set| {
            set.items
                .iter()
                .any(|item| item.kind == crate::store::side::ItemKind::Plugin && item.value == id)
        }));
    }
    Ok(false)
}

fn remove_rule(session: &Session, rule: &mut Rule) -> Result<bool> {
    rule.removing = true;
    session.save(rule)?;
    let mut failed = false;
    for index in 0..rule.targets.len() {
        if platform::cancelled() {
            return Err(Error::cancelled());
        }
        let entry = &rule.targets[index];
        let result = match (&entry.target, &entry.status.installed) {
            (
                target,
                Some(Artifact::Native {
                    id,
                    revision,
                    owned: true,
                    ..
                }),
            ) => (|| {
                if keep_or_transfer_native(session, rule, entry, id, revision)? {
                    return Ok(());
                }
                let expected =
                    if entry.status.candidate.is_none() && entry.status.check_failure.is_none() {
                        entry
                            .status
                            .observed_revision
                            .as_deref()
                            .unwrap_or(revision)
                    } else {
                        revision
                    };
                match target {
                    Target::Claude => native::claude_remove(&session.root, id, expected),
                    Target::Codex { profile } => codex_home(session, profile).and_then(|home| {
                        native::codex_remove(&home, &profile.generation, id, expected)
                    }),
                    _ => unreachable!("native artifact on skills target"),
                }
            })(),
            (Target::Skills { repository }, Some(Artifact::Skills { receipt })) => (|| {
                let refs = session.read()?.rules.into_iter().any(|other| {
                    other.id != rule.id
                        && other
                            .targets
                            .iter()
                            .any(|target| reference_matches(entry, target))
                });
                if refs {
                    return Ok(());
                }
                repository.verify()?;
                skills::remove_verified(
                    &repository.path,
                    &repository.checkout_identity,
                    &repository.common_identity,
                    receipt,
                )
            })(),
            _ => Ok(()),
        };
        let status = &mut rule.targets[index].status;
        match result {
            Ok(()) => *status = Default::default(),
            Err(error) if error.code == "cancelled" => return Err(error),
            Err(error) => {
                failed = true;
                status.failure = Some(error.to_string());
            }
        }
        session.save(rule)?;
    }
    if !failed {
        session.edit(|db| {
            db.rules.retain(|saved| saved.id != rule.id);
            Ok(())
        })?;
    }
    Ok(failed)
}

pub(crate) fn background_check(root: PathBuf, root_id: &str) -> Result<Outcome> {
    // The foreground executable installed Roost's SIGINT handler before
    // dispatch. Cancel all bounded probes in this disposable checker at its
    // overall deadline, including a single slow target.
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(45));
        // SAFETY: SIGINT is handled by Roost's process-local cancel handler.
        unsafe {
            libc::raise(libc::SIGINT);
        }
    });
    let session = Session {
        root,
        root_id: root_id.to_owned(),
        lock: None,
    };
    let db = session.read()?;
    let mut rules: Vec<_> = db.rules.into_iter().filter(|rule| !rule.removing).collect();
    if !rules.is_empty() {
        let offset = db.background_rotation % rules.len();
        rules.rotate_left(offset);
    }
    for mut rule in rules {
        let offset = db
            .background_rotation
            .checked_rem(rule.targets.len())
            .unwrap_or(0);
        check_rule(&session, &mut rule, offset)?;
    }
    Ok(Outcome::quiet())
}

const CHECK_INTERVAL: u64 = 24 * 60 * 60;

fn entry_relevant(
    rule: &Rule,
    entry: &TargetEntry,
    agent: Agent,
    generation: &str,
    project: Option<&Path>,
    cwd: &Path,
) -> bool {
    if rule.removing || !rule.agents.contains(&agent) {
        return false;
    }
    match &entry.target {
        Target::Claude => {
            agent == Agent::Claude
                && rule.allows_profile(agent, generation)
                && rule.matching_repo(project)
        }
        Target::Codex { profile } => agent == Agent::Codex && profile.generation == generation,
        Target::Skills { repository } => {
            project == Some(repository.common.as_path())
                && cwd.starts_with(&repository.path)
                && Directory::open(&repository.path, false)
                    .and_then(|dir| dir.identity())
                    .is_ok_and(|id| id == repository.checkout_identity)
        }
    }
}

pub(crate) fn on_launch(
    root: &Path,
    root_id: &str,
    agent: Agent,
    generation: &str,
    arguments: &[OsString],
) {
    if !is_interactive(false) || !interactive_session(agent, arguments) {
        return;
    }
    if let Err(error) = launch_updates(root, root_id, agent, generation)
        && error.code != "cancelled"
    {
        eprintln!("warning: Managed plugin updates: {}", error.message);
    }
}

fn interactive_session(agent: Agent, arguments: &[OsString]) -> bool {
    let head = arguments
        .first()
        .and_then(|argument| argument.to_str())
        .unwrap_or("");
    if arguments.iter().any(|argument| {
        matches!(
            argument.to_str(),
            Some("--help" | "-h" | "--version" | "-V")
        )
    }) {
        return false;
    }
    match agent {
        Agent::Claude => {
            !crate::cli::CLAUDE_SUBCOMMANDS.contains(&head)
                && !matches!(head, "plugin" | "doctor" | "update" | "install")
                && !arguments
                    .iter()
                    .any(|argument| matches!(argument.to_str(), Some("-p" | "--print")))
        }
        Agent::Codex => !matches!(
            head,
            "exec"
                | "e"
                | "login"
                | "logout"
                | "plugin"
                | "mcp"
                | "app-server"
                | "debug"
                | "completion"
                | "sandbox"
        ),
    }
}

fn launch_updates(root: &Path, root_id: &str, agent: Agent, generation: &str) -> Result<()> {
    let project = model::current_project_key();
    let cwd = model::current_directory()?;
    let store = Store::open(root, false, OpenMode::Launch)?;
    if store.root_id != root_id {
        return Ok(());
    }
    let db: Database = store.read_side_unfiltered()?;
    let relevant = |rule: &Rule, entry: &TargetEntry| {
        entry_relevant(rule, entry, agent, generation, project.as_deref(), &cwd)
    };
    if !db.rules.iter().any(|rule| {
        rule.targets
            .iter()
            .any(|entry| relevant(rule, entry) && entry.status.installed.is_some())
    }) {
        return Ok(());
    }
    let now = platform::unix_seconds();
    let candidate = db.rules.iter().find(|rule| {
        rule.targets.iter().any(|entry| {
            relevant(rule, entry)
                && entry.status.candidate.is_some()
                && entry.status.candidate != entry.status.deferred
        })
    });
    // Give the update prompt priority so a background checker cannot take the
    // operation lock while the user is answering.
    let due = candidate.is_none()
        && db
            .last_background_check
            .is_none_or(|last| now.saturating_sub(last) >= CHECK_INTERVAL);
    if due {
        store.update_side::<Database>(|db| {
            db.last_background_check = Some(now);
            db.background_rotation = db.background_rotation.wrapping_add(1);
            Ok(())
        })?;
    }
    let prompt = candidate
        .map(|rule| preview_for(std::slice::from_ref(rule), &db, Job::Update, &store))
        .transpose()?;
    drop(store);
    if due {
        let exe =
            std::env::current_exe().map_err(|e| Error::io("resolve", Path::new("roost"), e))?;
        Command::new(exe)
            .arg("__managed-plugin-check")
            .arg(root)
            .arg(root_id)
            .current_dir("/")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| Error::io("start update check in", root, e))?;
    }
    let (Some(rule), Some(lines)) = (candidate, prompt) else {
        return Ok(());
    };
    let answer = platform::prompt_line(&format!(
        "Managed plugin update available\n{}\nUpdate now? [y/N] ",
        lines.join("\n")
    ))?;
    let Some(session) = Session::start(root.to_owned(), false, false)? else {
        return Err(Error::new(
            "busy",
            "Another managed plugin operation is running",
        ));
    };
    if session.root_id != root_id {
        return Ok(());
    }
    let store = session.open(OpenMode::Read)?;
    let latest = store.read_side_unfiltered::<Database>()?;
    let Some(mut saved) = latest
        .rules
        .iter()
        .find(|saved| saved.id == rule.id)
        .cloned()
    else {
        return Ok(());
    };
    if saved.removing
        || preview_for(std::slice::from_ref(&saved), &latest, Job::Update, &store)? != lines
    {
        return Err(
            Error::new("revision_changed", "Update changed since the prompt")
                .next("Run roost plugin check"),
        );
    }
    drop(store);
    if answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes") {
        if update_rule(&session, &mut saved)? {
            return Err(Error::new("managed_plugin_failed", "Update was incomplete")
                .next("Run roost plugin list --managed"));
        }
    } else {
        for entry in saved
            .targets
            .iter_mut()
            .filter(|entry| entry.status.candidate.is_some())
        {
            entry.status.deferred.clone_from(&entry.status.candidate);
        }
        session.save(&saved)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::normalized;

    #[test]
    fn catalog_identity_normalizes_github_but_preserves_filesystem_and_url_case() {
        assert_eq!(
            normalized("Owner/Repo"),
            normalized("https://github.com/owner/repo.git")
        );
        assert_ne!(normalized("/Repo/Skills"), normalized("/Repo/skills"));
        assert_ne!(
            normalized("https://example.org/Repo"),
            normalized("https://example.org/repo")
        );
    }
}
