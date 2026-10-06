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
    /// data folder (sign-in) and configuration; detached unless --foreground.
    /// Without NAME, choose the profile from a picker. --link NAME makes NAME's
    /// Desktop borrow the existing Claude Desktop data folder in place; --close NAME
    /// asks NAME's running Desktop to quit
    Desktop {
        /// Stay attached with Desktop's console output, for debugging
        #[arg(long, conflicts_with_all = ["link", "unlink"])]
        foreground: bool,
        /// Ask NAME's running Claude Desktop to quit (SIGTERM, waits up to 10 seconds)
        #[arg(long, requires = "name", conflicts_with_all = ["foreground", "link", "unlink", "replace", "yes"])]
        close: bool,
        /// Let NAME's Desktop use the existing Claude Desktop data folder in place
        #[arg(long, requires = "name", conflicts_with = "unlink")]
        link: bool,
        /// Stop NAME borrowing the existing Claude Desktop data folder
        #[arg(long, requires = "name", conflicts_with_all = ["replace", "yes"])]
        unlink: bool,
        /// With --link: delete NAME's own Roost Desktop folder (after confirmation)
        #[arg(long, requires = "link")]
        replace: bool,
        /// With --replace: skip the confirmation
        #[arg(long, requires = "replace")]
        yes: bool,
        name: Option<String>,
    },
    /// Manage the shared plugin store and its plugins
    #[command(disable_help_subcommand = true)]
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
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
    /// Select a profile for this project (NAME, or choose with a picker) and launch it; manager options precede NAME
    Switch {
        #[arg(long)]
        allow_auth_env: bool,
        /// Only record the selection
        #[arg(long)]
        no_launch: bool,
        /// Clear this project's selection
        #[arg(long)]
        forget: bool,
        name: Option<String>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        arguments: Vec<OsString>,
    },
    /// Bare `roost`: launch the profile selected for this project.
    #[command(skip)]
    Launch {
        allow_auth_env: bool,
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

/// `roost plugin ...` subcommands.
#[derive(Subcommand, Debug)]
pub enum PluginAction {
    /// List store plugins known to Roost, their sets and the auto-update setting
    List {
        #[arg(long)]
        json: bool,
    },
    /// Manage marketplaces known to the plugin store
    #[command(disable_help_subcommand = true)]
    Marketplace {
        #[command(subcommand)]
        action: MarketplaceAction,
    },
    /// Install a plugin into the store and add it to sets
    Add {
        #[arg(value_name = "PLUGIN[@MARKETPLACE]")]
        plugin: String,
        /// Add the plugin to this set (repeatable)
        #[arg(long = "set", value_name = "SET", conflicts_with = "no_set")]
        sets: Vec<String>,
        /// Install without adding the plugin to any set
        #[arg(long)]
        no_set: bool,
    },
    /// Update one store plugin, or all of them
    Update {
        #[arg(value_name = "PLUGIN")]
        plugin: Option<String>,
    },
    /// Turn the daily launch-time plugin update on or off
    AutoUpdate {
        #[arg(long, conflicts_with = "off", required_unless_present = "off")]
        on: bool,
        #[arg(long)]
        off: bool,
    },
    /// Remove a plugin from every set, then uninstall it from the store
    Remove {
        #[arg(value_name = "PLUGIN")]
        plugin: String,
    },
}

/// `roost plugin marketplace ...` subcommands.
#[derive(Subcommand, Debug)]
pub enum MarketplaceAction {
    /// Add a marketplace to the plugin store (passed to Claude unchanged)
    Add { source: String },
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
                | Self::Plugin {
                    action: PluginAction::List { json: true }
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
            | Self::Desktop {
                name: Some(name), ..
            }
            | Self::Set {
                action: SetAction::Subscribe { name, .. } | SetAction::Unsubscribe { name, .. },
            }
            | Self::Switch {
                name: Some(name), ..
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
    if let Some(action) = parse_launch(&args)? {
        return Ok(action);
    }
    if args.get(1).is_some_and(|v| v == "switch") {
        return parse_switch(&args);
    }
    // Desktop forwards nothing, so even a lone `--` is refused.
    if args.get(1).is_some_and(|v| v == "desktop") && args[2..].iter().any(|v| v == "--") {
        return Err(usage("roost desktop takes no argument tail"));
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

/// Bare `roost`: only an optional leading `--allow-auth-env`, then nothing or `--`
/// and an opaque tail. Anything else is a command or switch for clap.
fn parse_launch(args: &[OsString]) -> std::result::Result<Option<Action>, clap::Error> {
    let allow_auth_env = args.get(1).is_some_and(|v| v == "--allow-auth-env");
    let index = if allow_auth_env { 2 } else { 1 };
    let arguments = match args.get(index) {
        None => vec![],
        Some(v) if v == "--" => args[index + 1..].to_vec(),
        Some(_) if allow_auth_env => {
            return Err(usage(
                "--allow-auth-env launches the selected profile; pass Claude arguments after --",
            ));
        }
        Some(_) => return Ok(None),
    };
    Ok(Some(Action::Launch {
        allow_auth_env,
        arguments,
    }))
}

/// `switch` shares run's single boundary: manager switches precede NAME and the
/// tail after NAME (one optional `--` consumed) belongs to Claude.
fn parse_switch(args: &[OsString]) -> std::result::Result<Action, clap::Error> {
    let (mut allow_auth_env, mut no_launch, mut forget) = (false, false, false);
    let mut index = 2;
    while let Some(value) = args.get(index) {
        let flag = if value == "--allow-auth-env" {
            &mut allow_auth_env
        } else if value == "--no-launch" {
            &mut no_launch
        } else if value == "--forget" {
            &mut forget
        } else if value == "--help" || value == "-h" {
            return Cli::try_parse_from([
                OsString::from("roost"),
                OsString::from("switch"),
                OsString::from("--help"),
            ])
            .map(|_| unreachable!());
        } else {
            break;
        };
        if *flag {
            return Err(usage(format!(
                "{} may be supplied only once",
                value.to_string_lossy()
            )));
        }
        *flag = true;
        index += 1;
    }
    if [allow_auth_env, no_launch, forget]
        .into_iter()
        .filter(|v| *v)
        .count()
        > 1
    {
        return Err(usage(
            "--allow-auth-env, --no-launch and --forget cannot be combined",
        ));
    }
    let name = match args.get(index) {
        None => None,
        Some(_) if forget => {
            return Err(usage("switch --forget takes no NAME or Claude arguments"));
        }
        // No NAME: the picker chooses; the tail after `--` belongs to Claude.
        Some(value) if value == OsStr::new("--") => None,
        Some(value) => {
            let name = value
                .to_str()
                .ok_or_else(|| usage("profile NAME must be ASCII"))?;
            if name.starts_with('-') {
                return Err(usage(
                    "unknown switch manager option; manager options precede NAME",
                ));
            }
            Some(name.to_owned())
        }
    };
    // Without NAME a `--` (if any) is at `index` and was the one separator.
    if no_launch && args.len() > index + usize::from(name.is_some()) {
        return Err(usage("switch --no-launch takes no Claude arguments"));
    }
    let mut tail = args.get(index + 1..).unwrap_or_default().to_vec();
    if name.is_some() && tail.first().is_some_and(|v| v == OsStr::new("--")) {
        tail.remove(0);
    }
    Ok(Action::Switch {
        allow_auth_env,
        no_launch,
        forget,
        name,
        arguments: tail,
    })
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
    fn desktop_takes_an_optional_name_and_no_forwarded_tail() {
        let Action::Desktop {
            foreground, name, ..
        } = parse(&["roost", "desktop", "Work"])
        else {
            panic!()
        };
        assert!(!foreground);
        assert_eq!(name.as_deref(), Some("Work"));
        assert!(matches!(
            parse(&["roost", "desktop", "Work", "--foreground"]),
            Action::Desktop {
                foreground: true,
                ..
            }
        ));
        // Without NAME the picker chooses.
        assert!(matches!(
            parse(&["roost", "desktop"]),
            Action::Desktop {
                foreground: false,
                name: None,
                ..
            }
        ));
        assert!(matches!(
            parse(&["roost", "desktop", "--foreground"]),
            Action::Desktop {
                foreground: true,
                name: None,
                ..
            }
        ));
        for values in [
            vec!["roost", "desktop", "--"],
            vec!["roost", "desktop", "Work", "--", "--flag"],
            vec!["roost", "desktop", "Work", "extra"],
        ] {
            assert!(parse_from(values.iter().map(OsString::from).collect()).is_err());
        }
    }
    #[test]
    fn desktop_link_and_unlink_need_name_and_reject_meaningless_combinations() {
        assert!(matches!(
            parse(&["roost", "desktop", "--link", "Work"]),
            Action::Desktop {
                link: true,
                unlink: false,
                replace: false,
                yes: false,
                foreground: false,
                close: false,
                name: Some(_),
            }
        ));
        assert!(matches!(
            parse(&["roost", "desktop", "Work", "--link", "--replace", "--yes"]),
            Action::Desktop {
                link: true,
                replace: true,
                yes: true,
                ..
            }
        ));
        assert!(matches!(
            parse(&["roost", "desktop", "--unlink", "Work"]),
            Action::Desktop {
                unlink: true,
                link: false,
                ..
            }
        ));
        for values in [
            vec!["roost", "desktop", "--link"],
            vec!["roost", "desktop", "--unlink"],
            vec!["roost", "desktop", "--link", "--unlink", "Work"],
            vec!["roost", "desktop", "--link", "--foreground", "Work"],
            vec!["roost", "desktop", "--unlink", "--foreground", "Work"],
            vec!["roost", "desktop", "--unlink", "--replace", "Work"],
            vec!["roost", "desktop", "--unlink", "--yes", "Work"],
            vec!["roost", "desktop", "--replace", "Work"],
            vec!["roost", "desktop", "--yes", "Work"],
            vec!["roost", "desktop", "--link", "--yes", "Work"],
            vec!["roost", "desktop", "--link", "Work", "--", "x"],
        ] {
            let error = parse_from(values.iter().map(OsString::from).collect()).unwrap_err();
            assert_eq!(error.exit_code(), 2, "{values:?}");
        }
    }
    #[test]
    fn desktop_close_needs_name_and_stands_alone() {
        assert!(matches!(
            parse(&["roost", "desktop", "--close", "Work"]),
            Action::Desktop {
                close: true,
                link: false,
                foreground: false,
                name: Some(_),
                ..
            }
        ));
        for values in [
            vec!["roost", "desktop", "--close"],
            vec!["roost", "desktop", "--close", "--link", "Work"],
            vec!["roost", "desktop", "--close", "--unlink", "Work"],
            vec!["roost", "desktop", "--close", "--replace", "Work"],
            vec!["roost", "desktop", "--close", "--foreground", "Work"],
            vec!["roost", "desktop", "--close", "--yes", "Work"],
        ] {
            let error = parse_from(values.iter().map(OsString::from).collect()).unwrap_err();
            assert_eq!(error.exit_code(), 2, "{values:?}");
        }
    }
    #[test]
    fn switch_name_is_optional_and_a_separator_starts_the_tail() {
        let switch = |values: &[&str]| match parse(values) {
            Action::Switch {
                no_launch,
                name,
                arguments,
                ..
            } => (no_launch, name, arguments),
            other => panic!("{other:?}"),
        };
        assert_eq!(switch(&["roost", "switch"]), (false, None, vec![]));
        assert_eq!(
            switch(&["roost", "switch", "--no-launch"]),
            (true, None, vec![])
        );
        assert_eq!(
            switch(&["roost", "switch", "--", "--", "-p"]),
            (false, None, vec!["--".into(), "-p".into()])
        );
        for values in [
            vec!["roost", "switch", "--no-launch", "--"],
            vec!["roost", "switch", "--forget", "--"],
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
