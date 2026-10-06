use clap::{Args, CommandFactory, Parser, Subcommand, error::ErrorKind};
use std::{
    ffi::{OsStr, OsString},
    path::PathBuf,
};

const RECIPES: &str = "First login:\n  roost add work\n  roost run work auth login\n  roost status work\n\nConcurrent sessions (separate terminals):\n  roost run work --name client-a\n  roost run work --name client-b\n  roost run work --resume client-a\n  roost run work --continue\n  roost run work --resume client-a --fork-session\n\nProfiles select configuration contexts, not proven account identities.\nDefault aliases pass through the caller environment.\nClear an owned manager token before relying on native browser login:\n  roost token work --clear\n";

#[derive(Parser)]
#[command(name="roost", about="Manage Claude profiles alongside your existing installation", disable_version_flag=true, disable_help_subcommand=true, after_help=RECIPES)]
struct Cli {
    #[arg(short = 'v', long)]
    version: bool,
    #[command(subcommand)]
    command: Option<Action>,
}

#[derive(Subcommand, Debug)]
pub enum Action {
    /// Create an owned profile or a pass-through default alias
    Add {
        name: String,
        #[arg(long, conflicts_with = "copy_default")]
        link_default: bool,
        #[arg(long)]
        copy_default: bool,
        #[arg(long, requires = "copy_default")]
        source: Option<PathBuf>,
        #[arg(long, requires = "copy_default")]
        yes: bool,
        /// Do not subscribe the new profile to default sets
        #[arg(long, conflicts_with = "link_default")]
        no_sets: bool,
    },
    /// Register an upstream profile at its unchanged real directory
    Register {
        name: String,
        #[arg(long)]
        path: PathBuf,
    },
    /// Restore retained owned data or refresh verified launchers
    Reuse { name: String },
    /// List registration metadata; token presence is not login validity
    #[command(alias = "ls")]
    List {
        #[arg(long)]
        retained: bool,
        /// Add bounded status probes (login, auth method, Claude directory)
        #[arg(long)]
        full: bool,
        #[arg(long)]
        json: bool,
    },
    /// Print the selected absolute configuration directory
    #[command(alias = "path")]
    Where { name: String },
    /// Start Claude; manager options precede NAME, the rest belongs to Claude
    Run {
        #[arg(long)]
        allow_auth_env: bool,
        name: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        arguments: Vec<OsString>,
    },
    /// Ask native Claude for supported login status
    Status {
        #[arg(long)]
        allow_auth_env: bool,
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Set a manager token using hidden input, explicit stdin, or clear it
    Token {
        name: String,
        #[arg(long, conflicts_with = "clear")]
        stdin: bool,
        #[arg(long)]
        clear: bool,
    },
    /// Remove owned launchers/registration; retain data unless explicitly purged
    #[command(alias = "rm")]
    Remove {
        name: String,
        #[arg(long)]
        purge: bool,
        /// Skip the confirmation (purge, or deleting an upstream profile's Desktop folder)
        #[arg(long)]
        yes: bool,
    },
    /// Delegate to the existing shared Claude updater
    Update,
    /// Print PATH instructions; apply edits one user destination
    SetupPath {
        #[arg(long)]
        shell: Option<String>,
        #[arg(long)]
        apply: bool,
    },
    /// Inspect installation/storage/launcher safety without repairing anything
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Manage shared sets of skills, instructions and plugins
    #[command(disable_help_subcommand = true)]
    Set {
        #[command(subcommand)]
        action: SetAction,
    },
    /// Show help for Roost or one manager command
    Help { command: Option<String> },
    /// Experimental, Linux only: start Claude Desktop with this profile's own Desktop
    /// data folder (sign-in) and configuration; detached unless --foreground
    Desktop {
        /// Stay attached with Desktop's console output, for debugging
        #[arg(long)]
        foreground: bool,
        name: String,
    },
    #[command(skip)]
    Version,
    #[command(skip)]
    Internal {
        root: PathBuf,
        root_id: String,
        registration_id: String,
        arguments: Vec<OsString>,
    },
}

/// `roost set ...` subcommands.
#[derive(Subcommand, Debug)]
pub enum SetAction {
    /// List sets, their items and subscribed profiles
    List {
        #[arg(long)]
        json: bool,
    },
    /// Create an empty set
    Create {
        set: String,
        /// Subscribe profiles created later with roost add
        #[arg(long)]
        default: bool,
    },
    /// Delete a set and its subscriptions; links leave at the next launch
    Delete { set: String },
    /// Mark a set default for new profiles, or unmark it with --off
    Default {
        set: String,
        #[arg(long)]
        off: bool,
    },
    /// Add one item to a set
    Add {
        set: String,
        #[command(flatten)]
        item: ItemArgs,
    },
    /// Drop one item from a set
    Drop {
        set: String,
        #[command(flatten)]
        item: ItemArgs,
    },
    /// Subscribe a profile to sets
    Subscribe {
        name: String,
        #[arg(required = true)]
        sets: Vec<String>,
    },
    /// Unsubscribe a profile from sets
    Unsubscribe {
        name: String,
        #[arg(required = true)]
        sets: Vec<String>,
    },
}

/// Exactly one set item.
#[derive(Args, Debug)]
#[group(required = true, multiple = false)]
pub struct ItemArgs {
    /// A skill directory, linked under skills/ by its name
    #[arg(long, value_name = "DIR")]
    pub skill: Option<PathBuf>,
    /// A directory whose every child directory is a skill
    #[arg(long, value_name = "DIR")]
    pub skills_from: Option<PathBuf>,
    /// An instruction fragment (.md file), linked under rules/
    #[arg(long, value_name = "FILE")]
    pub instruction: Option<PathBuf>,
    /// A directory whose every child .md file is an instruction fragment
    #[arg(long, value_name = "DIR")]
    pub instructions_from: Option<PathBuf>,
    /// A plugin installed in the plugin store
    #[arg(long, value_name = "PLUGIN@MARKETPLACE")]
    pub plugin: Option<String>,
}

impl Action {
    pub fn json(&self) -> bool {
        matches!(
            self,
            Self::List { json: true, .. }
                | Self::Status { json: true, .. }
                | Self::Doctor { json: true }
                | Self::Set {
                    action: SetAction::List { json: true }
                }
        )
    }
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::Add { name, .. }
            | Self::Register { name, .. }
            | Self::Reuse { name }
            | Self::Where { name }
            | Self::Run { name, .. }
            | Self::Status { name, .. }
            | Self::Token { name, .. }
            | Self::Remove { name, .. }
            | Self::Desktop { name, .. }
            | Self::Set {
                action: SetAction::Subscribe { name, .. } | SetAction::Unsubscribe { name, .. },
            } => Some(name),
            _ => None,
        }
    }
}

fn usage(message: impl Into<String>) -> clap::Error {
    Cli::command().error(ErrorKind::InvalidValue, message.into())
}

pub fn parse() -> std::result::Result<Action, clap::Error> {
    parse_from(std::env::args_os().collect())
}

fn parse_from(args: Vec<OsString>) -> std::result::Result<Action, clap::Error> {
    if args.get(1).is_some_and(|v| v == "__roost_launch_v1") {
        if args.len() < 6 || args[5] != "--" {
            return Err(usage(
                "invalid internal launcher binding; refresh with roost reuse NAME",
            ));
        }
        let id = |index: usize| -> std::result::Result<String, clap::Error> {
            let v = args[index]
                .to_str()
                .ok_or_else(|| usage("invalid launcher identity"))?;
            if v.len() != 32
                || !v
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(usage("invalid launcher identity"));
            }
            Ok(v.into())
        };
        return Ok(Action::Internal {
            root: args[2].clone().into(),
            root_id: id(3)?,
            registration_id: id(4)?,
            arguments: args[6..].to_vec(),
        });
    }
    // Public run has one boundary: only switches before NAME are manager switches.
    // Clap's normal trailing arguments can still consume a known switch after NAME.
    if args.get(1).is_some_and(|v| v == "run") {
        let mut index = 2;
        let mut allow_auth_env = false;
        loop {
            let value = args.get(index).ok_or_else(|| usage("run requires NAME"))?;
            if value == "--allow-auth-env" {
                if allow_auth_env {
                    return Err(usage("--allow-auth-env may be supplied only once"));
                }
                allow_auth_env = true;
                index += 1;
                continue;
            }
            if value == "--help" || value == "-h" {
                return Cli::try_parse_from([
                    OsString::from("roost"),
                    OsString::from("run"),
                    OsString::from("--help"),
                ])
                .map(|_| unreachable!());
            }
            let name = value
                .to_str()
                .ok_or_else(|| usage("profile NAME must be ASCII"))?;
            if name.starts_with('-') {
                return Err(usage(
                    "unknown run manager option; manager options precede NAME",
                ));
            }
            let mut tail = args[index + 1..].to_vec();
            if tail.first().is_some_and(|v| v == OsStr::new("--")) {
                tail.remove(0);
            }
            return Ok(Action::Run {
                allow_auth_env,
                name: name.into(),
                arguments: tail,
            });
        }
    }
    let cli = Cli::try_parse_from(args)?;
    if cli.version {
        if cli.command.is_some() {
            return Err(usage("--version cannot be combined with a command"));
        }
        return Ok(Action::Version);
    }
    Ok(cli.command.unwrap_or(Action::Help { command: None }))
}

pub fn help(command: Option<&str>) -> crate::Result<String> {
    let app = Cli::command();
    let mut selected = match command {
        None => app,
        Some(name) => app
            .get_subcommands()
            .find(|c| c.get_name() == name || c.get_all_aliases().any(|a| a == name))
            .cloned()
            .ok_or_else(|| crate::Error::new("usage", format!("unknown manager command {name}")))?,
    };
    let mut output = Vec::new();
    selected
        .write_long_help(&mut output)
        .map_err(|e| crate::Error::new("io", e.to_string()))?;
    let mut output = String::from_utf8(output).expect("clap help is UTF-8");
    output.push('\n');
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(values: &[&str]) -> Action {
        parse_from(values.iter().map(OsString::from).collect()).unwrap()
    }
    #[test]
    fn run_boundary_preserves_claude_switches_and_one_separator() {
        let Action::Run {
            allow_auth_env,
            arguments,
            ..
        } = parse(&[
            "roost",
            "run",
            "--allow-auth-env",
            "Work",
            "--",
            "--",
            "--help",
            "--allow-auth-env",
            "",
        ])
        else {
            panic!()
        };
        assert!(allow_auth_env);
        assert_eq!(arguments, vec!["--", "--help", "--allow-auth-env", ""]);
    }
    #[test]
    fn desktop_takes_one_name_and_no_forwarded_tail() {
        let Action::Desktop { foreground, name } = parse(&["roost", "desktop", "Work"]) else {
            panic!()
        };
        assert!(!foreground);
        assert_eq!(name, "Work");
        assert!(matches!(
            parse(&["roost", "desktop", "Work", "--foreground"]),
            Action::Desktop {
                foreground: true,
                ..
            }
        ));
        for values in [
            vec!["roost", "desktop"],
            vec!["roost", "desktop", "Work", "--", "--flag"],
            vec!["roost", "desktop", "Work", "extra"],
        ] {
            assert!(parse_from(values.iter().map(OsString::from).collect()).is_err());
        }
    }
    #[test]
    fn invalid_combinations_are_rejected() {
        for values in [
            vec!["roost", "add", "work", "--yes"],
            vec!["roost", "token", "work", "--stdin", "--clear"],
            vec!["roost", "add", "work", "--source", "."],
        ] {
            assert!(parse_from(values.iter().map(OsString::from).collect()).is_err());
        }
    }
}
