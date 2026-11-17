//! Repository and profile install rules, stored independently of profile selections.
mod model;
mod work;

pub(crate) use crate::cli::Agent;
use crate::{
    Error, Outcome, Result,
    cli::{InstallArgs, PluginAction, ProjectAction},
    platform::{self, Directory},
    store::{Kind, OpenMode, Registration, State, Store},
};
use model::{
    Artifact, Database, Entry, Profile, Repository, Rule, Target, TargetEntry, UnresolvedRepo,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs::File,
    path::{Path, PathBuf},
};

/// Take this lock before the root lock. It serializes managed writes across
/// unlocked provider calls, including legacy Claude removals.
fn operation_lock(root: &Path, wait: bool) -> Result<Option<File>> {
    let directory = Directory::open(root, true)?;
    let lock = crate::plugins::provider_lock(
        &directory,
        crate::store::side::MANAGED_PLUGIN_LOCK,
        "managed plugin",
        wait,
    )?;
    directory.verify_namespace()?;
    Ok(lock)
}

pub(crate) fn legacy_removal_lock(action: &PluginAction) -> Result<Option<File>> {
    use crate::cli::MarketplaceAction;
    if !matches!(
        action,
        PluginAction::Remove { .. }
            | PluginAction::Marketplace {
                action: MarketplaceAction::Remove { .. }
            }
    ) {
        return Ok(None);
    }
    let root = platform::root()?;
    if Store::open_if_present(&root)?.is_none() {
        return Ok(None);
    }
    operation_lock(&root, true)
}

pub(crate) fn claude_references(store: &Store) -> Result<BTreeSet<String>> {
    let db: Database = store.read_side_unfiltered()?;
    Ok(db
        .rules
        .into_iter()
        .flat_map(|rule| rule.targets)
        .filter_map(|entry| match (entry.target, entry.status.installed) {
            (Target::Claude, Some(Artifact::Native { id, .. })) => Some(id),
            _ => None,
        })
        .collect())
}

pub(crate) fn claude_for_launch(store: &Store, registration: &Registration) -> Vec<String> {
    if registration.kind == Kind::DefaultAlias {
        return vec![];
    }
    let db: Database = match store.read_side_unfiltered() {
        Ok(db) => db,
        Err(error) => {
            eprintln!(
                "warning: Could not read managed plugin rules: {}",
                error.message
            );
            return vec![];
        }
    };
    let key = model::current_project_key();
    db.rules
        .into_iter()
        .filter(|rule| {
            !rule.removing
                && !rule.portable()
                && rule.allows_profile(Agent::Claude, &registration.registration_id)
                && rule.matching_repo(key.as_deref())
        })
        .flat_map(|rule| rule.targets)
        .filter_map(|entry| match (entry.target, entry.status.installed) {
            (Target::Claude, Some(Artifact::Native { id, .. })) => Some(id),
            _ => None,
        })
        .collect()
}

struct Session {
    root: PathBuf,
    root_id: String,
    lock: Option<File>,
}

impl Session {
    fn start(root: PathBuf, create: bool, wait: bool) -> Result<Option<Self>> {
        if create {
            drop(Store::open(&root, true, OpenMode::Mutate)?);
        } else if Store::open_if_present(&root)?.is_none() {
            return Ok(None);
        }
        let Some(lock) = operation_lock(&root, wait)? else {
            return Ok(None);
        };
        let root_id = Store::open(&root, false, OpenMode::Read)?.root_id.clone();
        Ok(Some(Self {
            root,
            root_id,
            lock: Some(lock),
        }))
    }

    fn open(&self, mode: OpenMode) -> Result<Store> {
        let store = Store::open(&self.root, false, mode)?;
        if store.root_id != self.root_id {
            return Err(Error::new(
                "ownership",
                "Roost storage changed during managed plugin operation",
            ));
        }
        Ok(store)
    }

    fn read(&self) -> Result<Database> {
        self.open(OpenMode::Read)?.read_side_unfiltered()
    }

    fn edit(&self, edit: impl FnOnce(&mut Database) -> Result<()>) -> Result<()> {
        self.open(OpenMode::Mutate)?.update_side(edit)
    }

    fn save(&self, rule: &Rule) -> Result<()> {
        self.edit(|db| {
            if let Some(saved) = db.rules.iter_mut().find(|saved| saved.id == rule.id) {
                *saved = rule.clone();
            } else {
                db.rules.push(rule.clone());
            }
            Ok(())
        })
    }
}

fn target_present(rule: &Rule, target: &Target) -> bool {
    rule.targets.iter().any(|entry| &entry.target == target)
}

fn add_targets(
    rule: &mut Rule,
    db: &Database,
    store: Option<&Store>,
    bind: bool,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let mut targets = Vec::new();
    if rule.portable() {
        if rule.all_repos {
            targets.extend(db.repositories.iter().cloned());
        } else {
            targets.extend(rule.repositories.iter().cloned());
            targets.extend(
                db.repositories
                    .iter()
                    .filter(|repo| {
                        rule.repositories
                            .iter()
                            .any(|anchor| anchor.same_repo(repo))
                    })
                    .cloned(),
            );
        }
    }
    for repository in targets {
        let target = Target::Skills { repository };
        if !target_present(rule, &target) {
            rule.targets.push(TargetEntry {
                target,
                status: Entry::default(),
            });
        }
    }
    if rule.portable() {
        return Ok(());
    }

    let has_claude_target = if rule.all_profiles {
        crate::launch::resolve_program("claude").is_ok()
            || store.is_some_and(|store| {
                store.registry.registrations.iter().any(|registration| {
                    registration.kind != Kind::DefaultAlias && registration.state == State::Active
                })
            })
    } else {
        rule.profiles
            .iter()
            .any(|profile| profile.agent == Agent::Claude)
    };
    if rule.agents.contains(&Agent::Claude)
        && has_claude_target
        && (rule.all_repos || !rule.repositories.is_empty())
        && !target_present(rule, &Target::Claude)
    {
        rule.targets.push(TargetEntry {
            target: Target::Claude,
            status: Entry::default(),
        });
    }
    if !rule.agents.contains(&Agent::Codex) {
        return Ok(());
    }
    let mut profiles = if rule.all_profiles {
        let mut profiles = vec![];
        if let Some(store) = store {
            for home in store.codex_profiles(warnings)? {
                profiles.push(Profile::codex(home)?);
            }
        }
        profiles
    } else {
        rule.profiles
            .iter()
            .filter(|p| p.agent == Agent::Codex)
            .cloned()
            .collect()
    };
    for profile in &mut profiles {
        if bind && profile.legacy_identity.is_some() {
            profile.bind(store.ok_or_else(|| {
                Error::new("not_found", "Set up the selected Codex home first")
            })?)?;
        }
        let target = Target::Codex {
            profile: profile.clone(),
        };
        if !target_present(rule, &target) {
            // Keep the old receipt if a path/name is reused. Its generation is
            // still checked before cleanup and a new generation is a new target.
            rule.targets.push(TargetEntry {
                target,
                status: Entry::default(),
            });
        }
    }
    Ok(())
}

fn choose_scope(title: &str, rows: &[String], selected: usize) -> Result<usize> {
    use crate::platform::Picked;
    match platform::pick_with(
        title,
        "↑/↓ move, Enter choose, Esc cancel",
        "Target",
        rows,
        selected,
        &[],
    )? {
        Picked::Chosen(index) => Ok(index),
        Picked::Action(_) => Err(Error::cancelled()),
    }
}

fn is_interactive(json: bool) -> bool {
    !json && crate::select::terminal()
}

fn confirmation(title: &str, lines: &[String], yes: bool, interactive: bool) -> Result<()> {
    if yes {
        return Ok(());
    }
    if !interactive {
        return Err(Error::new(
            "usage",
            "Review the targets with --dry-run and pass --yes to apply",
        ));
    }
    let prompt = format!("{title}\n{}\nContinue? [y/N] ", lines.join("\n"));
    let answer = platform::prompt_line(&prompt)?;
    if answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes") {
        Ok(())
    } else {
        Err(Error::cancelled())
    }
}

fn install_rule(args: &InstallArgs, root: &Path, db: &Database) -> Result<Rule> {
    model::validate_source(&args.source)?;
    let interactive = !args.yes && is_interactive(args.json);
    let mut agents = args.agent.clone();
    let provider_prefix = if args.source.starts_with("claude:") {
        Some(Agent::Claude)
    } else if args.source.starts_with("codex:") {
        Some(Agent::Codex)
    } else {
        None
    };
    if let Some(agent) = provider_prefix {
        if args.skills || agents.iter().any(|selected| *selected != agent) {
            return Err(Error::new(
                "usage",
                "A provider-qualified plugin requires its matching agent",
            ));
        }
        agents = vec![agent];
    }
    let mut profiles = vec![];
    if !args.profile.is_empty() {
        let store = Store::open_if_present(root)?
            .ok_or_else(|| Error::new("not_found", "Create the selected profiles first"))?;
        for selector in &args.profile {
            let profile = Profile::lookup(&store, selector)?;
            if !profiles.contains(&profile) {
                profiles.push(profile);
            }
        }
        if agents.is_empty() {
            agents.extend(profiles.iter().map(|p| p.agent));
        } else if profiles.iter().any(|p| !agents.contains(&p.agent)) {
            return Err(Error::new(
                "usage",
                "Each selected profile must match a selected agent",
            ));
        }
    }
    let mut all_repos = args.all_repos;
    let mut paths = args.repo.clone();
    if args.here {
        paths.push(model::current_directory()?);
    } else if !all_repos && paths.is_empty() {
        if !interactive {
            return Err(Error::new(
                "usage",
                "Choose --here, --repo PATH, or --all-repos",
            ));
        }
        let mut options = vec!["Current repository".into(), "All repositories".into()];
        options.extend(
            db.repositories
                .iter()
                .map(|repo| repo.path.display().to_string()),
        );
        match choose_scope("Choose a repository scope", &options, 0)? {
            0 => paths.push(model::current_directory()?),
            1 => all_repos = true,
            n => paths.push(db.repositories[n - 2].path.clone()),
        }
    }
    if agents.is_empty() {
        if !args.skills && !all_repos {
            agents.push(Agent::Claude);
        } else if interactive {
            let options = [
                "Claude and Codex".into(),
                "Claude only".into(),
                "Codex only".into(),
            ];
            agents = match choose_scope("Choose coding agents", &options, 0)? {
                1 => vec![Agent::Claude],
                2 => vec![Agent::Codex],
                _ => vec![Agent::Claude, Agent::Codex],
            };
        } else {
            agents = vec![Agent::Claude, Agent::Codex];
        }
    }
    agents.sort();
    agents.dedup();
    if !args.skills && !all_repos && agents.contains(&Agent::Codex) {
        return Err(Error::new(
            "unsupported_scope",
            "Native Codex plugins apply across repositories in a Codex home",
        )
        .next(
            "Use --skills --skill NAME for selected repositories, or --all-repos for native Codex",
        ));
    }
    let mut all_profiles = args.all_profiles;
    if !args.skills && profiles.is_empty() && !all_profiles {
        if !interactive {
            return Err(Error::new(
                "usage",
                "Choose --all-profiles or --profile NAME",
            ));
        }
        all_profiles = true;
    }
    let mut repositories = vec![];
    let mut unresolved = vec![];
    for path in &paths {
        let path = platform::absolute(path)?;
        match Repository::resolve(&path) {
            Ok(repo) => repositories.push(repo),
            Err(error) if error.code == "cancelled" => return Err(error),
            Err(error) => unresolved.push(UnresolvedRepo {
                path,
                code: error.code.into(),
                reason: error.message,
            }),
        }
    }
    repositories.sort_by(|left, right| left.path.cmp(&right.path));
    repositories.dedup();
    unresolved.sort_by(|left, right| left.path.cmp(&right.path));
    unresolved.dedup_by(|left, right| left.path == right.path);
    profiles.sort_by(|left, right| left.label.cmp(&right.label));
    let mut skills = args.skill.clone();
    skills.sort();
    skills.dedup();
    if skills
        .iter()
        .any(|skill| skill != "*" && !model::identifier(skill))
        || skills.len() > 1 && skills.iter().any(|skill| skill == "*")
    {
        return Err(Error::new(
            "usage",
            "Select skill names or use '*' by itself",
        ));
    }
    Ok(Rule {
        id: platform::random_id()?,
        source: if args.skills {
            model::skill_source(&args.source)?
        } else {
            args.source.clone()
        },
        agents,
        skills,
        all_repos,
        repositories,
        unresolved,
        all_profiles,
        profiles,
        targets: vec![],
        skill_revision: None,
        codex_revision: None,
        native_ids: Default::default(),
        removing: false,
    })
}

fn empty_db(store: Option<&Store>) -> Result<Database> {
    use crate::store::side::SideFile;
    store.map_or_else(|| Ok(Database::empty_for("")), Store::read_side_unfiltered)
}

fn rule_header(rule: &Rule) -> String {
    let repos = if rule.all_repos {
        "all repositories".to_owned()
    } else {
        rule.repositories
            .iter()
            .map(|repo| &repo.path)
            .chain(rule.unresolved.iter().map(|repo| &repo.path))
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let profiles = if rule.portable() {
        "all profiles in each checkout".to_owned()
    } else if rule.all_profiles {
        "all compatible profiles".to_owned()
    } else {
        rule.profiles
            .iter()
            .map(|p| p.label.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let agents = rule
        .agents
        .iter()
        .map(|agent| match agent {
            Agent::Claude => "Claude",
            Agent::Codex => "Codex",
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{} | {agents} | {repos} | {profiles}", rule.source)
}

fn target_preview(entry: &TargetEntry, rule: &Rule, root: &Path) -> String {
    let target = entry.target.label();
    if matches!(entry.target, Target::Skills { .. }) {
        let mut paths = vec!["skills-lock.json"];
        if rule.agents.contains(&Agent::Codex) {
            paths.push(".agents/skills/");
        }
        if rule.agents.contains(&Agent::Claude) {
            paths.push(".claude/skills/");
        }
        return format!("  {target} ({})", paths.join(", "));
    }
    let (agent, directory) = match &entry.target {
        Target::Claude => (Agent::Claude, root.join("plugin-store")),
        Target::Codex { profile } => (
            Agent::Codex,
            root.join("codex")
                .join(profile.label.trim_start_matches("codex:")),
        ),
        Target::Skills { .. } => unreachable!(),
    };
    let id = rule
        .native_ids
        .get(&agent)
        .map_or("unresolved plugin", String::as_str);
    let location = format!("  {target} | {id} | {}", directory.display());
    match &entry.status.failure {
        Some(failure) if !rule.native_ids.contains_key(&agent) => format!("{location} | {failure}"),
        _ => location,
    }
}

fn install(args: InstallArgs, data: &mut Value) -> Result<Outcome> {
    let root = platform::root()?;
    let candidate_store = Store::open_if_present(&root)?;
    let db = empty_db(candidate_store.as_ref())?;
    let root_id = candidate_store.as_ref().map(|store| store.root_id.clone());
    // No root lock while interacting with the terminal or invoking Git.
    drop(candidate_store);
    let mut rule = install_rule(&args, &root, &db)?;
    if let Some(existing) = db
        .rules
        .iter()
        .find(|existing| !existing.removing && existing.same_scope(&rule))
    {
        rule.native_ids.clone_from(&existing.native_ids);
    }
    work::verify_mode(&rule, &db)?;
    let mut warnings = vec![];
    add_targets(
        &mut rule,
        &db,
        Store::open_if_present(&root)?.as_ref(),
        false,
        &mut warnings,
    )?;
    work::resolve_native_ids(&mut rule)?;
    let mut preview = vec![rule_header(&rule)];
    preview.extend(
        rule.targets
            .iter()
            .map(|entry| target_preview(entry, &rule, &root)),
    );
    preview.extend(
        rule.unresolved
            .iter()
            .map(|repo| format!("  {} | failed: {}", repo.path.display(), repo.reason)),
    );
    if rule.portable() && rule.targets.is_empty() {
        preview.push(
            "  No checkouts registered. Add one with roost project add PATH, then sync.".into(),
        );
    }
    if args.dry_run {
        *data = json!({"dry_run":true,"rule":rule,"targets":preview});
        let failed = !rule.unresolved.is_empty()
            || rule
                .targets
                .iter()
                .any(|entry| entry.status.failure.is_some());
        let error = failed.then(|| {
            Error::new(
                "managed_plugin_failed",
                "One or more targets could not be resolved",
            )
        });
        return Ok(Outcome {
            warnings,
            exit: i32::from(failed),
            error,
            ..Outcome::lines(preview)
        });
    }
    if !rule.all_repos && rule.repositories.is_empty() {
        return output(vec![rule], warnings, true, data);
    }
    confirmation(
        "Install managed plugins or skills",
        &preview,
        args.yes,
        is_interactive(args.json),
    )?;
    let mut verified = vec![];
    for repo in std::mem::take(&mut rule.repositories) {
        match repo.verify() {
            Ok(()) => verified.push(repo),
            Err(error) if error.code == "cancelled" => return Err(error),
            Err(error) => rule.unresolved.push(UnresolvedRepo {
                path: repo.path,
                code: error.code.into(),
                reason: error.message,
            }),
        }
    }
    let resolved = rule.clone();
    rule.repositories = verified;
    if !rule.all_repos && rule.repositories.is_empty() {
        return output(vec![rule], warnings, true, data);
    }
    if !rule.all_repos {
        rule.targets.retain(|entry| match &entry.target {
            Target::Skills { repository } => rule
                .repositories
                .iter()
                .any(|repo| repo.same_repo(repository)),
            _ => true,
        });
    }
    let session = Session::start(root, true, true)?.expect("initialized storage");
    if root_id.as_ref().is_some_and(|id| id != &session.root_id) {
        return Err(Error::new(
            "ownership",
            "Roost storage changed since the preview",
        ));
    }
    // Adapter writes validate the checkout identity under their own lock.
    let db = session.read()?;
    work::verify_mode(&rule, &db)?;
    {
        let store = session.open(OpenMode::Mutate)?;
        let mut current = rule.clone();
        current.targets.clear();
        add_targets(&mut current, &db, Some(&store), false, &mut vec![])?;
        if current
            .targets
            .iter()
            .map(|entry| &entry.target)
            .ne(rule.targets.iter().map(|entry| &entry.target))
        {
            return Err(Error::new(
                "ownership",
                "Managed targets changed since the preview",
            ));
        }
        for profile in &mut rule.profiles {
            profile.bind(&store)?;
        }
        rule.targets.clear();
        add_targets(&mut rule, &db, Some(&store), true, &mut warnings)?;
    }
    if let Some(existing) = db
        .rules
        .into_iter()
        .find(|existing| !existing.removing && existing.same_scope(&rule))
    {
        if existing.native_ids.iter().any(|(agent, id)| {
            resolved
                .native_ids
                .get(agent)
                .is_some_and(|approved| approved != id)
        }) {
            return Err(Error::new(
                "conflict",
                "A managed plugin ID changed since preview",
            ));
        }
        rule = existing;
        work::expand(&session, &mut rule, &mut warnings)?;
    }
    work::keep_native_preview(&mut rule, &resolved);
    session.save(&rule)?;
    let result = work::sync_rule(&session, &mut rule)? || !rule.unresolved.is_empty();
    output(vec![rule], warnings, result, data)
}

pub(crate) fn project(action: ProjectAction, data: &mut Value) -> Result<Outcome> {
    let root = platform::root()?;
    match action {
        ProjectAction::Add { path, .. } => {
            let repo = Repository::resolve(&path)?;
            let session = Session::start(root, true, true)?.expect("initialized storage");
            repo.verify()?;
            session.edit(|db| {
                if let Some(existing) = db
                    .repositories
                    .iter()
                    .find(|existing| existing.path == repo.path)
                {
                    if *existing != repo {
                        return Err(Error::new(
                            "ownership",
                            "A different checkout already uses this registered path",
                        ));
                    }
                } else {
                    db.repositories.push(repo.clone());
                }
                Ok(())
            })?;
            *data = json!({"repository":repo});
            Ok(Outcome::lines(vec![format!(
                "Registered {}",
                repo.path.display()
            )]))
        }
        ProjectAction::List { .. } => {
            let db = empty_db(Store::open_if_present(&root)?.as_ref())?;
            *data = json!({"repositories":db.repositories});
            Ok(Outcome::lines(
                db.repositories
                    .into_iter()
                    .map(|repo| format!("{} | {}", repo.path.display(), repo.common.display()))
                    .collect(),
            ))
        }
    }
}

pub(crate) fn command(action: PluginAction, data: &mut Value) -> Result<Outcome> {
    match action {
        PluginAction::Install(args) => return install(args, data),
        PluginAction::List { managed: true, .. } => return work::list(data),
        _ => {}
    }
    let (job, source, yes, json) = match action {
        PluginAction::Sync { source, yes, json } => (work::Job::Sync, source, yes, json),
        PluginAction::Update {
            managed: true,
            plugin,
            yes,
            json,
        } => (work::Job::Update, plugin, yes, json),
        PluginAction::Uninstall { source, yes, json } => {
            (work::Job::Remove, Some(source), yes, json)
        }
        PluginAction::Check { source, json } => (work::Job::Check, source, true, json),
        _ => unreachable!("legacy command sent to managed plugins"),
    };
    work::dispatch(job, source.as_deref(), yes, json, data)
}

fn output(
    rules: Vec<Rule>,
    warnings: Vec<String>,
    failed: bool,
    data: &mut Value,
) -> Result<Outcome> {
    let mut lines = vec![];
    for rule in &rules {
        lines.push(format!("{} | {}", rule.id, rule_header(rule)));
        for entry in &rule.targets {
            let status = &entry.status;
            let description = if let Some(conflict) = &status.local_conflict {
                format!("local conflict: {conflict}")
            } else if let Some(error) = &status.failure {
                format!("failed: {error}")
            } else if let Some(candidate) = &status.candidate {
                let deferred = if status.deferred.as_ref() == Some(candidate) {
                    " (deferred)"
                } else {
                    ""
                };
                format!("update available: {candidate}{deferred}")
            } else if let Some(artifact) = &status.installed {
                if let Some(revision) = &status.observed_revision
                    && revision != artifact.revision()
                {
                    format!("installed {revision} (recorded {})", artifact.revision())
                } else {
                    format!("installed {}", artifact.revision())
                }
            } else {
                "pending".into()
            };
            lines.push(format!("  {} | {description}", entry.target.label()));
            if let Some(error) = &status.check_failure {
                lines.push(format!("    update check failed: {error}"));
            }
        }
        for repo in &rule.unresolved {
            lines.push(format!(
                "  {} | failed: {}",
                repo.path.display(),
                repo.reason
            ));
        }
    }
    *data = json!({"rules":rules});
    let error = failed.then(|| {
        Error::new(
            "managed_plugin_failed",
            "One or more managed plugin targets failed",
        )
    });
    Ok(Outcome {
        warnings,
        exit: i32::from(failed),
        error,
        ..Outcome::lines(lines)
    })
}

pub(crate) use work::{background_check, on_launch};
