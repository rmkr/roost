use crate::{
    cli::{self, CLAUDE_SUBCOMMANDS, CompletionShell, RUN_FLAGS, SWITCH_FLAGS},
    platform,
    store::{State, Store},
};
use clap::{Arg, ArgAction, Command};
use clap_complete::engine::{self, CompletionCandidate};
use std::{
    collections::{BTreeSet, HashSet},
    ffi::{OsStr, OsString},
};

pub(crate) fn script(shell: CompletionShell) -> &'static str {
    match shell {
        CompletionShell::Bash => include_str!("completion/bash.sh"),
        CompletionShell::Zsh => include_str!("completion/zsh.zsh"),
        CompletionShell::Fish => include_str!("completion/fish.fish"),
    }
    .trim_end()
}

pub(crate) fn candidates(words: Vec<OsString>) -> Vec<String> {
    let Some((current, before)) = words.split_last() else {
        return vec![];
    };
    let Some(command) = before.get(1) else {
        return if before.is_empty() {
            vec![]
        } else {
            complete(words, false)
        };
    };
    let manager_flag_used = match command.to_str() {
        Some("--allow-auth-env") if before.len() == 2 => {
            return CLAUDE_SUBCOMMANDS
                .iter()
                .filter(|name| {
                    current
                        .to_str()
                        .is_some_and(|prefix| name.starts_with(prefix))
                })
                .map(|name| (*name).into())
                .collect();
        }
        Some("run") => match cli::manager_prefix(before, RUN_FLAGS) {
            Ok(([used], index)) if index == before.len() => used,
            _ => return vec![],
        },
        Some("switch") => match cli::manager_prefix(before, SWITCH_FLAGS) {
            Ok(([auth, no_launch, false], index))
                if index == before.len() && !(auth && no_launch) =>
            {
                auth || no_launch
            }
            _ => return vec![],
        },
        Some("--" | "--allow-auth-env" | "--help" | "-h" | "--version" | "-v") => return vec![],
        Some(name) if CLAUDE_SUBCOMMANDS.contains(&name) => return vec![],
        _ => false,
    };
    complete(words, manager_flag_used)
}

fn complete(words: Vec<OsString>, manager_flag_used: bool) -> Vec<String> {
    let mut command = cli::command().arg(
        Arg::new("allow_auth_env")
            .long("allow-auth-env")
            .action(ArgAction::SetTrue),
    );
    for name in CLAUDE_SUBCOMMANDS {
        command = command.subcommand(Command::new(name));
    }
    let cursor = words.len() - 1;
    engine::complete(&mut command, words, cursor, None)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|candidate| {
            let value = candidate.get_value().to_str()?;
            let is_manager_flag = RUN_FLAGS.contains(&value) || SWITCH_FLAGS.contains(&value);
            (!value.contains(['\r', '\n', '\t']) && !(manager_flag_used && is_manager_flag))
                .then(|| value.to_owned())
        })
        .collect()
}

pub(crate) fn profile_names(current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(prefix) = current.to_str().filter(|prefix| {
        prefix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-:".contains(&b))
            && !prefix.starts_with('-')
    }) else {
        return vec![];
    };
    let Ok(store) = platform::root().and_then(|root| Store::open_for_completion(&root)) else {
        return vec![];
    };

    let mut candidates = BTreeSet::new();
    let mut claude_names = HashSet::new();
    for profile in &store.registry.registrations {
        if profile.state == State::Active {
            claude_names.insert(profile.name.to_ascii_lowercase());
            add_name(&mut candidates, prefix, "", &profile.name);
            add_name(&mut candidates, prefix, "claude:", &profile.name);
        }
    }
    for home in store.codex_profiles(&mut vec![]).unwrap_or_default() {
        if !claude_names.contains(&home.name) {
            add_name(&mut candidates, prefix, "", &home.name);
        }
        add_name(&mut candidates, prefix, "codex:", &home.name);
    }
    candidates
        .into_iter()
        .map(CompletionCandidate::new)
        .collect()
}

fn add_name(candidates: &mut BTreeSet<String>, prefix: &str, qualifier: &str, name: &str) {
    if let Some(name_prefix) = prefix.strip_prefix(qualifier) {
        if let Some(head) = name.get(..name_prefix.len())
            && head.eq_ignore_ascii_case(name_prefix)
        {
            // Keep the text the shell is matching, even if it uses different case.
            candidates.insert(format!("{prefix}{}", &name[head.len()..]));
        }
    } else if qualifier.starts_with(prefix) {
        candidates.insert(format!("{qualifier}{name}"));
    }
}
