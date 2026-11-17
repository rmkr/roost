use super::{
    Agent, Artifact, Database, Directory, Error, OpenMode, Outcome, Profile, Result, Rule, Session,
    Store, Target, TargetEntry, add_targets, confirmation, empty_db, is_interactive, model, output,
    platform, rule_header, target_preview,
};
use crate::{managed_native as native, managed_skills as skills};
use serde_json::Value;
use std::{
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

pub(super) fn verify_mode(rule: &Rule, db: &Database) -> Result<()> {
    let source = |rule: &Rule| {
        rule.source
            .trim_start_matches("claude:")
            .trim_start_matches("codex:")
            .trim_start_matches("https://github.com/")
            .trim_end_matches(".git")
            .to_owned()
    };
    let rule_source = source(rule);
    for other in db.rules.iter().filter(|other| {
        other.portable() != rule.portable() && !other.removing && source(other) == rule_source
    }) {
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
    let store = Store::open_if_present(&root)?;
    let db = empty_db(store.as_ref())?;
    let preview = selected_rules(&db, source, job)?;
    let plan = operation_preview(&preview, &db, job, store.as_ref())?;
    drop(store);
    if preview.is_empty() {
        return output(vec![], vec![], false, data);
    }
    if job != Job::Check {
        if plan.is_empty() {
            return output(preview, vec![], false, data);
        }
        let title = match job {
            Job::Sync => "Sync managed plugin and skill targets",
            Job::Update => "Apply available managed updates",
            Job::Remove => "Uninstall managed rules and their owned artifacts",
            Job::Check => unreachable!(),
        };
        confirmation(title, &plan, yes, is_interactive(json))?;
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
    if job != Job::Check
        && serde_json::to_vec(&selected_rules(&now, source, job)?).ok()
            != serde_json::to_vec(&preview).ok()
    {
        return Err(
            Error::new("ownership", "Managed targets changed since the preview")
                .next("Review the targets and retry"),
        );
    }
    if job != Job::Check && operation_preview(&preview, &now, job, Some(&store))? != plan {
        return Err(Error::new(
            "ownership",
            "Affected targets changed since the preview",
        ));
    }
    drop(store);
    let result = run(&session, preview, job, data)?;
    if job == Job::Check && source.is_none() {
        session.edit(|db| {
            db.last_background_check = Some(platform::unix_seconds());
            Ok(())
        })?;
    }
    Ok(result)
}

fn operation_preview(
    rules: &[Rule],
    db: &Database,
    job: Job,
    store: Option<&Store>,
) -> Result<Vec<String>> {
    if job == Job::Check {
        return Ok(vec![]);
    }
    if job == Job::Sync {
        let mut rules = rules.to_vec();
        for rule in &mut rules {
            add_targets(rule, db, store, false, &mut vec![])?;
            rule.targets
                .retain(|entry| entry.status.installed.is_none());
        }
        rules.retain(|rule| !rule.targets.is_empty());
        return Ok(preview_for(&rules, db, job));
    }
    Ok(preview_for(rules, db, job))
}

fn preview_for(rules: &[Rule], db: &Database, job: Job) -> Vec<String> {
    rules
        .iter()
        .filter(|rule| {
            job != Job::Update
                || rule
                    .targets
                    .iter()
                    .any(|entry| entry.status.candidate.is_some())
        })
        .flat_map(|rule| {
            let mut rows = vec![rule_header(rule)];
            for entry in &rule.targets {
                if job == Job::Update {
                    let Some(candidate) = &entry.status.candidate else {
                        continue;
                    };
                    rows.push(format!("  {} → {candidate}", entry.target.label()));
                    for related in db.rules.iter().filter(|other| other.id != rule.id) {
                        if related
                            .targets
                            .iter()
                            .any(|other| reference_matches(entry, other))
                        {
                            rows.push(format!("    Also affects {}", rule_header(related)));
                        }
                    }
                    if matches!(entry.target, Target::Claude) {
                        rows.push(
                            "    Also updates any existing shared sets using this Claude plugin"
                                .into(),
                        );
                    }
                } else {
                    rows.push(target_preview(entry, &rule.agents));
                }
            }
            rows
        })
        .collect()
}

fn run(session: &Session, mut rules: Vec<Rule>, job: Job, data: &mut Value) -> Result<Outcome> {
    let mut warnings = vec![];
    let mut failed = false;
    for rule in &mut rules {
        if platform::cancelled() {
            return Err(Error::cancelled());
        }
        // Refresh after preceding rules may have updated shared native receipts.
        if let Some(saved) = session
            .read()?
            .rules
            .into_iter()
            .find(|saved| saved.id == rule.id)
        {
            *rule = saved;
        }
        let result = match job {
            Job::Sync => {
                expand(session, rule, &mut warnings)?;
                session.save(rule)?;
                sync_rule(session, rule)?
            }
            Job::Update => update_rule(session, rule)?,
            Job::Check => check_rule(session, rule)?,
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
    }
}

fn prepare_skills(rule: &Rule) -> Result<skills::Staged> {
    skills::stage(&rule.source, &rule.skills, &skill_agents(rule))
}

pub(super) fn sync_rule(session: &Session, rule: &mut Rule) -> Result<bool> {
    if rule
        .targets
        .iter()
        .all(|entry| entry.status.installed.is_some())
    {
        return Ok(false);
    }
    verify_mode(rule, &session.read()?)?;
    let staged = rule.portable().then(|| prepare_skills(rule));
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
        let result = match &entry.target {
            Target::Claude => {
                native::claude_install(&session.root, model::native_source(rule, Agent::Claude))
                    .map(native_artifact)
            }
            Target::Codex { profile } => codex_home(session, profile).and_then(|home| {
                native::codex_install(&home, model::native_source(rule, Agent::Codex))
                    .map(native_artifact)
            }),
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
                skills::apply(&repository.path, staged, None)
                    .map(|receipt| Artifact::Skills { receipt })
            })(),
        };
        let status = &mut rule.targets[index].status;
        match result {
            Ok(artifact) => {
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

fn check_rule(session: &Session, rule: &mut Rule) -> Result<bool> {
    let staged = (rule.portable() && rule.skill_revision.is_some() && !rule.targets.is_empty())
        .then(|| prepare_skills(rule));
    let mut failed = false;
    for index in 0..rule.targets.len() {
        if platform::cancelled() {
            return Err(Error::cancelled());
        }
        let entry = &rule.targets[index];
        let mut observed = None;
        let result = match (&entry.target, &entry.status.installed) {
            (Target::Claude, Some(Artifact::Native { id, .. })) => (|| {
                observed = native::claude_current(&session.root, id)?;
                native::claude_check(&session.root, model::native_source(rule, Agent::Claude), id)
            })(),
            (Target::Codex { profile }, Some(Artifact::Native { id, .. })) => {
                codex_home(session, profile).and_then(|home| {
                    observed = native::codex_current(&home, id)?;
                    native::codex_check(&home, model::native_source(rule, Agent::Codex), id)
                })
            }
            (Target::Skills { .. }, installed)
                if installed.is_some() || rule.skill_revision.is_some() =>
            {
                staged
                    .as_ref()
                    .ok_or_else(|| Error::new("ownership", "Skill rule has no selected revision"))?
                    .as_ref()
                    .map_err(Clone::clone)
                    .map(|staged| {
                        let previous = installed
                            .as_ref()
                            .map(Artifact::revision)
                            .or(rule.skill_revision.as_deref());
                        (previous != Some(staged.revision())).then(|| staged.revision().to_owned())
                    })
            }
            _ => continue,
        };
        let status = &mut rule.targets[index].status;
        status.observed_revision = observed;
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
        session.save(rule)?;
    }
    Ok(failed)
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
    matches!((&left.status.installed, &right.status.installed),
        (Some(Artifact::Native { id: left_id, .. }), Some(Artifact::Native { id: right_id, .. })) if left_id == right_id)
        && same_native(&left.target, &right.target)
}

fn update_other_native_refs(session: &Session, rule: &Rule, index: usize) -> Result<()> {
    let source_entry = &rule.targets[index];
    let Some(Artifact::Native { revision, .. }) = &source_entry.status.installed else {
        return Ok(());
    };
    session.edit(|db| {
        for other in db.rules.iter_mut().filter(|other| rule.id != other.id) {
            for entry in other
                .targets
                .iter_mut()
                .filter(|entry| reference_matches(source_entry, entry))
            {
                if let Some(Artifact::Native { revision: old, .. }) = &mut entry.status.installed {
                    old.clone_from(revision);
                }
                entry.status.candidate = None;
                entry.status.deferred = None;
                entry.status.check_failure = None;
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
    let staged = rule.portable().then(|| prepare_skills(rule));
    if let Some(Ok(staged)) = &staged
        && selected.as_deref() == Some(staged.revision())
    {
        rule.skill_revision = selected;
        session.save(rule)?;
    }
    let mut failed = false;
    for index in 0..rule.targets.len() {
        if platform::cancelled() {
            return Err(Error::cancelled());
        }
        let entry = &rule.targets[index];
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
                }),
            ) => native::claude_update(
                &session.root,
                model::native_source(rule, Agent::Claude),
                id,
                revision,
                Some(candidate),
            )
            .map(|mut update| {
                update.owned = *owned;
                native_artifact(update)
            }),
            (
                Target::Codex { profile },
                Some(Artifact::Native {
                    id,
                    owned,
                    revision,
                }),
            ) => codex_home(session, profile).and_then(|home| {
                native::codex_update(
                    &home,
                    model::native_source(rule, Agent::Codex),
                    id,
                    revision,
                    Some(candidate),
                )
                .map(|mut update| {
                    update.owned = *owned;
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
                let prior = match previous {
                    Some(Artifact::Skills { receipt }) => Some(receipt),
                    None => None,
                    _ => unreachable!("skills target has native artifact"),
                };
                skills::apply(&repository.path, staged, prior)
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
                status.installed = Some(artifact);
                status.candidate = None;
                status.deferred = None;
                status.failure = None;
                status.check_failure = None;
                status.observed_revision = None;
            }
            Err(error) if error.code == "cancelled" => return Err(error),
            Err(error) => {
                failed = true;
                status.failure = Some(error.to_string());
            }
        }
        session.save(rule)?;
        if updated {
            update_other_native_refs(session, rule, index)?;
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
                }),
            ) => (|| {
                if keep_or_transfer_native(session, rule, entry, id, revision)? {
                    return Ok(());
                }
                match target {
                    Target::Claude => native::claude_remove(&session.root, id, revision),
                    Target::Codex { profile } => codex_home(session, profile)
                        .and_then(|home| native::codex_remove(&home, id, revision)),
                    _ => unreachable!("native artifact on skills target"),
                }
            })(),
            (Target::Skills { repository }, Some(Artifact::Skills { receipt })) => repository
                .verify()
                .and_then(|()| skills::remove(&repository.path, receipt)),
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
    let Some(session) = Session::start(root, false, false)? else {
        return Ok(Outcome::quiet());
    };
    if session.root_id != root_id {
        return Ok(Outcome::quiet());
    }
    let db = session.read()?;
    let started = std::time::Instant::now();
    for mut rule in db.rules.into_iter().filter(|rule| !rule.removing) {
        if started.elapsed() >= std::time::Duration::from_secs(45) {
            break;
        }
        check_rule(&session, &mut rule)?;
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

pub(crate) fn on_launch(root: &Path, root_id: &str, agent: Agent, generation: &str) {
    if let Err(error) = launch_updates(root, root_id, agent, generation)
        && error.code != "cancelled"
    {
        eprintln!("warning: Managed plugin updates: {}", error.message);
    }
}

fn launch_updates(root: &Path, root_id: &str, agent: Agent, generation: &str) -> Result<()> {
    // Help/auth/info launches may also pass here. Never prompt or spawn a check
    // for a noninteractive launch. Outside Roost, use the explicit check command.
    if !is_interactive(false) || model::env_flag("ROOST_SKIP_PLUGIN_CHECK") {
        return Ok(());
    }
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
            Ok(())
        })?;
    }
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
    let Some(rule) = candidate else { return Ok(()) };
    let lines = preview_for(std::slice::from_ref(rule), &db, Job::Update);
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
    let latest = session.read()?;
    let Some(mut saved) = latest
        .rules
        .iter()
        .find(|saved| saved.id == rule.id)
        .cloned()
    else {
        return Ok(());
    };
    if saved.removing || preview_for(std::slice::from_ref(&saved), &latest, Job::Update) != lines {
        return Err(
            Error::new("revision_changed", "Update changed since the prompt")
                .next("Run roost plugin check"),
        );
    }
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
