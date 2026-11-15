//! Command line shape.
//!
//! The parser is assembled at runtime: the connection flags and `info` belong to
//! this crate, everything else is `crate::generated`, which the generator emits
//! from the pinned Admin REST API description. Building the tree instead of
//! deriving it is what makes the generated commands appear in `--help` and lets
//! completion (Q5) use the same tree as parsing.

use std::ffi::OsString;

use clap::{Arg, ArgMatches, Args, Command, FromArgMatches};

use crate::version;

/// Command line interface for the Keycloak Admin REST API.
#[derive(Debug)]
pub struct Cli {
    /// Connection settings, shared by every command.
    pub connection: ConnectionArgs,
    /// How responses are printed.
    pub output: OutputArgs,
    /// The command that was asked for, when one was.
    pub command: Option<Invocation>,
}

impl Cli {
    /// Parses the process arguments, exiting with clap's usage error handling.
    pub fn parse() -> Self {
        Self::parse_from(std::env::args_os())
    }

    /// Parses `args`, the program name first, exiting on a usage error.
    pub fn parse_from<I, T>(args: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        match Self::try_parse_from(args) {
            Ok(cli) => cli,
            Err(error) => error.exit(),
        }
    }

    /// Parses `args`, the program name first.
    pub fn try_parse_from<I, T>(args: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let matches = build_cli().try_get_matches_from(args)?;
        Self::from_matches(&matches)
    }

    /// Reads a parsed tree.
    pub fn from_matches(matches: &ArgMatches) -> Result<Self, clap::Error> {
        let connection = ConnectionArgs::from_arg_matches(matches)?;
        let output = OutputArgs::from_arg_matches(matches)?;
        let (words, args) = innermost(matches);
        Ok(Self {
            connection,
            output,
            command: (!words.is_empty()).then_some(Invocation { words, args }),
        })
    }
}

/// The command that was asked for, as clap leaves it.
#[derive(Debug)]
pub struct Invocation {
    /// The words that were typed, e.g. `["clients", "list"]`.
    pub words: Vec<String>,
    /// The matches of the innermost command, which is where its arguments are.
    pub args: ArgMatches,
}

impl Invocation {
    /// The words as `&str`, which is what the generated dispatchers take.
    pub fn words(&self) -> Vec<&str> {
        self.words.iter().map(String::as_str).collect()
    }
}

/// Builds the whole command tree: the CLI's own commands and connection flags,
/// plus every generated command of the enabled tags.
pub fn build_cli() -> Command {
    let command = Command::new("keycloak-cli")
        .version(version::version_string())
        .about("Command-line interface for the Keycloak Admin REST API")
        .subcommand(
            Command::new("info")
                .about("Show the effective connection configuration (secrets redacted)."),
        )
        .subcommand(
            Command::new("report")
                .about("Write a report of users, events and sessions with ASCII charts.")
                .long_about(
                    "Collects the realm's users, its events by type and its active sessions per \
                     client, and writes a markdown document (default), a self-contained HTML \
                     page or JSON. `--file` writes it to a file instead of stdout.",
                )
                .arg(
                    Arg::new("file")
                        .long("file")
                        .value_name("PATH")
                        .help("write the report to this file instead of stdout"),
                ),
        )
        .subcommand(
            Command::new("login")
                .about("Log in for this terminal and store the token for later commands.")
                .long_about(
                    "Logs in with the configured user and password and stores the token under this \
                     terminal's session, which `init` exports. Later commands and completion in this \
                     terminal then use it, so the password is only needed here.",
                ),
        )
        .subcommand(
            Command::new("logout").about("Forget the stored token of this terminal."),
        )
        .subcommand(
            Command::new("session-id").hide(true).about(
                "Print the id of this terminal's session (run by `init` at shell start).",
            ),
        )
        .subcommand(
            Command::new("init")
                .about("Print the line to add to a shell's startup file for completions.")
                .arg(
                    Arg::new("shell")
                        .value_name("SHELL")
                        .required(true)
                        .value_parser(clap::builder::PossibleValuesParser::new(
                            crate::completion::SHELLS,
                        )),
                ),
        );
    let command = ConnectionArgs::augment_args(command);
    let command = OutputArgs::augment_args(command);
    // `--realm` completes realm names from this terminal's session.
    let command = command.mut_arg("realm", |arg| crate::command::complete(arg, "realm"));
    crate::generated::commands()
        .into_iter()
        .fold(command, |command, generated| command.subcommand(generated))
}

/// Where and how to reach Keycloak.
///
/// Every value can also come from the environment, so the usual setup is
/// `export KEYCLOAK_ADDR=… KEYCLOAK_USER=… KEYCLOAK_PASSWORD=…`.
#[derive(Debug, Args)]
pub struct ConnectionArgs {
    /// Base URL of the Keycloak server.
    #[arg(
        short = 's',
        long,
        env = "KEYCLOAK_ADDR",
        default_value = "http://localhost:8080",
        value_name = "URL",
        global = true
    )]
    pub server: String,

    /// Realm to operate on.
    #[arg(
        short = 'r',
        long,
        env = "KEYCLOAK_REALM",
        default_value = "master",
        value_name = "REALM",
        global = true
    )]
    pub realm: String,

    /// Administrator user name.
    #[arg(
        short = 'u',
        long,
        env = "KEYCLOAK_USER",
        default_value = "admin",
        value_name = "USER",
        global = true
    )]
    pub user: String,

    /// Administrator password. Prefer `KEYCLOAK_PASSWORD` over the command line.
    #[arg(
        short = 'p',
        long,
        env = "KEYCLOAK_PASSWORD",
        value_name = "PASSWORD",
        hide_env_values = true,
        global = true
    )]
    pub password: Option<String>,
}

/// How a response is printed.
#[derive(Debug, Args)]
pub struct OutputArgs {
    /// How to print the response: `auto` is a table on a terminal and JSON when
    /// the output is piped.
    #[arg(
        long,
        value_name = "FORMAT",
        default_value = "auto",
        value_parser = clap::builder::PossibleValuesParser::new(crate::report::Format::NAMES),
        global = true
    )]
    pub output: String,

    /// Print only these fields, comma separated: `--fields id,clientId`.
    #[arg(long, value_name = "FIELDS", value_delimiter = ',', global = true)]
    pub fields: Vec<String>,
}

/// Follows the subcommands down to the innermost one.
fn innermost(matches: &ArgMatches) -> (Vec<String>, ArgMatches) {
    let mut words = Vec::new();
    let mut current = matches;
    let mut deepest = matches.clone();
    while let Some((name, subcommand)) = current.subcommand() {
        words.push(name.to_owned());
        deepest = subcommand.clone();
        current = subcommand;
    }
    (words, deepest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tree_is_valid() {
        // clap's own assertions: duplicate flags, conflicting names, bad nesting.
        build_cli().debug_assert();
    }

    #[test]
    fn generated_commands_are_in_the_help() {
        let mut help = Vec::new();
        build_cli().write_help(&mut help).expect("help renders");
        let help = String::from_utf8(help).expect("help is utf-8");
        // The CLI's own command and the generated groups.
        assert!(help.contains("info"), "{help}");
        assert!(help.contains("clients"), "{help}");
        assert!(help.contains("users"), "{help}");
        assert!(help.contains("realms"), "{help}");
    }

    #[test]
    fn parses_a_generated_command_into_words() {
        let cli = Cli::try_parse_from(["keycloak-cli", "-r", "prod", "clients", "list"])
            .expect("the command line parses");
        assert_eq!(cli.connection.realm, "prod");
        let invocation = cli.command.expect("a command was given");
        assert_eq!(invocation.words, ["clients", "list"]);
        assert_eq!(invocation.words(), ["clients", "list"]);
        assert_eq!(
            crate::command::integer(&invocation.args, "first"),
            None,
            "the innermost matches carry the command's own arguments"
        );
    }

    #[test]
    fn parses_the_built_in_command_and_no_command() {
        let info = Cli::try_parse_from(["keycloak-cli", "info"]).expect("info parses");
        assert_eq!(info.command.expect("info is a command").words, ["info"]);

        let none = Cli::try_parse_from(["keycloak-cli"]).expect("no command parses");
        assert!(none.command.is_none());
    }

    #[test]
    fn rejects_unknown_commands() {
        let error = Cli::try_parse_from(["keycloak-cli", "clients", "nonsense"])
            .expect_err("nonsense is not a command");
        assert_eq!(error.kind(), clap::error::ErrorKind::InvalidSubcommand);
    }
}
