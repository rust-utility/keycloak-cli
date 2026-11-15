//! Runtime support for the generated command tree.
//!
//! Generated modules carry data ([`CommandSpec`]) and the call to the `keycloak`
//! crate method; this module owns everything they must not repeat: building the
//! clap tree from the data, reading argument values back and turning the crate's
//! responses into JSON.
//!
//! The data is hand-shaped from the OpenAPI description by
//! `keycloak-cli-generate`, so a command is described once — words, help, args —
//! and clap, help output and (later) completion all read the same description.

use std::collections::BTreeMap;
use std::io::Read as _;

use clap::parser::ValueSource;
use clap::{Arg, ArgAction, ArgMatches, Command};
use clap_complete::{ArgValueCompleter, CompletionCandidate};
use serde::de::DeserializeOwned;

use crate::error::Error;

/// The value type of a generated flag.
///
/// Everything the API takes outside the body fits in these four shapes (F: the
/// crate's methods only have `Option<String>`, `Option<bool>`, `Option<i32>`,
/// `Option<Vec<String>>` and `&str` parameters).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind {
    /// A switch: present means `true`, absent means "not sent".
    Flag,
    /// A signed integer, e.g. `--max 50`.
    Integer,
    /// A string, e.g. `--search foo`.
    Text,
    /// One or more strings, e.g. `--fields id --fields name`.
    Texts,
}

/// One argument of a generated command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParamSpec {
    /// The argument id, i.e. the crate's parameter name (`client_uuid`).
    pub id: &'static str,
    /// The value name shown in help (`client-uuid`).
    pub value_name: &'static str,
    /// The long flag without dashes; empty for positionals.
    pub long: &'static str,
    /// Help text: the parameter description of the API.
    pub help: &'static str,
    pub kind: ValueKind,
}

/// One generated command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    /// The command words, e.g. `["clients", "roles", "get"]`.
    pub words: &'static [&'static str],
    /// One line summary, from the operation's `summary`.
    pub about: &'static str,
    /// Longer help, from the operation's `description`.
    pub long_about: &'static str,
    /// Path parameters, in path order, i.e. the positionals.
    pub positionals: &'static [ParamSpec],
    /// Query parameters, i.e. the flags.
    pub flags: &'static [ParamSpec],
    /// Whether the operation takes a JSON body (`--body <FILE|->`).
    pub body: bool,
    /// Whether the operation is deprecated in the description.
    pub deprecated: bool,
}

/// Builds the top level commands from generated specs.
///
/// Specs may come from several generated modules (one per tag); they are merged
/// by their command words, so a group can be fed by more than one tag.
pub fn tree<'a>(specs: impl IntoIterator<Item = &'a CommandSpec>) -> Vec<Command> {
    let mut by_first: BTreeMap<&'static str, Vec<&'a CommandSpec>> = BTreeMap::new();
    for spec in specs {
        if let Some(first) = spec.words.first() {
            by_first.entry(first).or_default().push(spec);
        }
    }
    by_first
        .into_iter()
        .map(|(name, group)| node(&[name], &group))
        .collect()
}

/// Builds one command and, recursively, its subcommands.
fn node(prefix: &[&'static str], specs: &[&CommandSpec]) -> Command {
    let name = *prefix.last().expect("a command has at least one word");
    let mut command = Command::new(name);

    if let Some(leaf) = specs.iter().find(|spec| spec.words.len() == prefix.len()) {
        command = describe(command, leaf);
    }

    let mut children: BTreeMap<&'static str, Vec<&CommandSpec>> = BTreeMap::new();
    for spec in specs {
        if spec.words.len() > prefix.len() {
            children
                .entry(spec.words[prefix.len()])
                .or_default()
                .push(*spec);
        }
    }
    for (word, group) in children {
        let mut child_prefix = prefix.to_vec();
        child_prefix.push(word);
        command = command.subcommand(node(&child_prefix, &group));
    }
    command
}

/// Adds the help and arguments of a leaf command.
fn describe(mut command: Command, spec: &CommandSpec) -> Command {
    if !spec.about.is_empty() {
        command = command.about(spec.about);
    }
    if !spec.long_about.is_empty() {
        command = command.long_about(spec.long_about);
    }
    if spec.deprecated {
        command = command.about(format!(
            "{} (deprecated)",
            if spec.about.is_empty() {
                "Deprecated operation"
            } else {
                spec.about
            }
        ));
    }

    for (index, positional) in spec.positionals.iter().enumerate() {
        let arg = Arg::new(positional.id)
            .index(index + 1)
            .required(true)
            .value_name(positional.value_name)
            .help(positional.help);
        command = command.arg(complete(arg, positional.id));
    }
    for flag in spec.flags {
        let arg = Arg::new(flag.id)
            .long(flag.long)
            .value_name(flag.value_name)
            .help(flag.help);
        let arg = match flag.kind {
            ValueKind::Flag => arg.action(ArgAction::SetTrue),
            ValueKind::Integer => arg.value_parser(clap::value_parser!(i32)),
            ValueKind::Text => arg.value_parser(clap::value_parser!(String)),
            ValueKind::Texts => arg
                .action(ArgAction::Append)
                .num_args(1..)
                .value_parser(clap::value_parser!(String)),
        };
        command = command.arg(complete(arg, flag.id));
    }
    if spec.body {
        command = command.arg(
            Arg::new("body")
                .long("body")
                .value_name("FILE|-")
                .help("request body: a JSON file, or - for standard input"),
        );
    }
    command
}

/// Attaches live completion to `arg` when its parameter has a source.
///
/// The completer only sees the word being completed (F12), so the sources get the
/// session (server, realm, token) from the environment, and a terminal without a
/// session offers no values at all.
pub(crate) fn complete(arg: Arg, parameter: &'static str) -> Arg {
    if !crate::live::global().has(parameter) {
        return arg;
    }
    arg.add(ArgValueCompleter::new(move |current: &std::ffi::OsStr| {
        crate::live::global()
            .candidates(parameter, &current.to_string_lossy())
            .into_iter()
            .map(CompletionCandidate::new)
            .collect()
    }))
}

/// Whether `words` name one of `specs`.
pub fn owns(specs: &[CommandSpec], words: &[&str]) -> bool {
    specs.iter().any(|spec| spec.words == words)
}

/// A string flag or positional that was given.
///
/// The readers never panic: an id the command does not have, or a value of
/// another type, reads as "not given". Generated code only asks for ids it
/// declares, and this keeps a mismatch a wrong result instead of a crash.
pub fn text(args: &ArgMatches, id: &str) -> Option<String> {
    args.try_get_one::<String>(id).ok().flatten().cloned()
}

/// A switch flag: `Some(true)` when it was given, `None` when it was not.
pub fn boolean(args: &ArgMatches, id: &str) -> Option<bool> {
    (args.value_source(id) == Some(ValueSource::CommandLine)).then_some(true)
}

/// An integer flag that was given.
pub fn integer(args: &ArgMatches, id: &str) -> Option<i32> {
    if args.value_source(id) != Some(ValueSource::CommandLine) {
        return None;
    }
    args.try_get_one::<i32>(id).ok().flatten().copied()
}

/// A multi-value flag that was given.
pub fn texts(args: &ArgMatches, id: &str) -> Option<Vec<String>> {
    args.try_get_many::<String>(id)
        .ok()
        .flatten()
        .map(|values| values.cloned().collect())
}

/// A required positional.
pub fn positional(args: &ArgMatches, id: &str) -> Result<String, Error> {
    text(args, id).ok_or_else(|| Error::usage(format!("missing <{id}>")))
}

/// Reads `--body <FILE|->` and deserialises it into the type the crate expects.
pub fn body<T: DeserializeOwned>(args: &ArgMatches) -> Result<T, Error> {
    let source = text(args, "body").ok_or_else(|| Error::usage("--body is required"))?;
    let text = if source == "-" {
        let mut text = String::new();
        std::io::stdin().read_to_string(&mut text)?;
        text
    } else {
        std::fs::read_to_string(&source)?
    };
    Ok(serde_json::from_str(&text)?)
}

/// A crate response as JSON.
pub fn to_json<T: serde::Serialize>(value: T) -> Result<serde_json::Value, Error> {
    Ok(serde_json::to_value(value)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const POSITIONALS: &[ParamSpec] = &[ParamSpec {
        id: "client_uuid",
        value_name: "client-uuid",
        long: "",
        help: "id of client (not client-id!)",
        kind: ValueKind::Text,
    }];

    const FLAGS: &[ParamSpec] = &[
        ParamSpec {
            id: "first",
            value_name: "first",
            long: "first",
            help: "the first result",
            kind: ValueKind::Integer,
        },
        ParamSpec {
            id: "search",
            value_name: "search",
            long: "search",
            help: "whether this is a search query",
            kind: ValueKind::Flag,
        },
        ParamSpec {
            id: "fields",
            value_name: "fields",
            long: "fields",
            help: "fields to return",
            kind: ValueKind::Texts,
        },
    ];

    const SPECS: &[CommandSpec] = &[
        CommandSpec {
            words: &["clients", "list"],
            about: "Get clients belonging to the realm.",
            long_about: "",
            positionals: &[],
            flags: FLAGS,
            body: false,
            deprecated: false,
        },
        CommandSpec {
            words: &["clients", "get"],
            about: "Get representation of the client.",
            long_about: "The long description.",
            positionals: POSITIONALS,
            flags: &[],
            body: false,
            deprecated: false,
        },
        CommandSpec {
            words: &["clients", "roles", "get"],
            about: "",
            long_about: "",
            positionals: POSITIONALS,
            flags: &[],
            body: true,
            deprecated: true,
        },
    ];

    #[test]
    fn builds_a_nested_tree() {
        let tree = tree(SPECS);
        assert_eq!(tree.len(), 1);
        let clients = &tree[0];
        assert_eq!(clients.get_name(), "clients");
        assert_eq!(clients.get_about(), None);
        let names: Vec<&str> = clients.get_subcommands().map(Command::get_name).collect();
        assert_eq!(names, ["get", "list", "roles"]);

        let get = clients
            .get_subcommands()
            .find(|command| command.get_name() == "get")
            .expect("get exists");
        assert_eq!(
            get.get_about().map(|about| about.to_string()),
            Some("Get representation of the client.".to_owned())
        );
        assert_eq!(
            get.get_long_about()
                .map(|about| about.to_string())
                .as_deref(),
            Some("The long description.")
        );
        assert_eq!(get.get_positionals().count(), 1);
        assert_eq!(get.get_arguments().count(), 1);

        let roles = clients
            .get_subcommands()
            .find(|command| command.get_name() == "roles")
            .expect("roles exists");
        let leaf = roles
            .get_subcommands()
            .find(|command| command.get_name() == "get")
            .expect("roles get exists");
        assert!(leaf
            .get_about()
            .expect("deprecated commands keep a description")
            .to_string()
            .contains("deprecated"));
        assert!(leaf.get_arguments().any(|arg| arg.get_id() == "body"));
    }

    #[test]
    fn parses_flags_back_into_values() {
        let tree = tree(SPECS);
        let matches = Command::new("keycloak-cli")
            .subcommands(tree)
            .try_get_matches_from([
                "keycloak-cli",
                "clients",
                "list",
                "--first",
                "5",
                "--search",
                "--fields",
                "id",
                "--fields",
                "name",
            ])
            .expect("arguments parse");

        let args = matches
            .subcommand_matches("clients")
            .and_then(|clients| clients.subcommand_matches("list"))
            .expect("clients list matches");
        assert_eq!(integer(args, "first"), Some(5));
        assert_eq!(boolean(args, "search"), Some(true));
        assert_eq!(
            texts(args, "fields"),
            Some(vec!["id".to_owned(), "name".to_owned()])
        );
        assert_eq!(text(args, "client_id"), None);
    }

    #[test]
    fn absent_flags_are_not_sent() {
        let tree = tree(SPECS);
        let matches = Command::new("keycloak-cli")
            .subcommands(tree)
            .try_get_matches_from(["keycloak-cli", "clients", "list"])
            .expect("arguments parse");
        let args = matches
            .subcommand_matches("clients")
            .and_then(|clients| clients.subcommand_matches("list"))
            .expect("clients list matches");
        assert_eq!(boolean(args, "search"), None);
        assert_eq!(integer(args, "first"), None);
        assert_eq!(texts(args, "fields"), None);
    }

    #[test]
    fn positionals_are_required() {
        let tree = tree(SPECS);
        let error = Command::new("keycloak-cli")
            .subcommands(tree)
            .try_get_matches_from(["keycloak-cli", "clients", "get"])
            .expect_err("the client uuid is missing");
        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn owns_only_its_own_words() {
        assert!(owns(SPECS, &["clients", "list"]));
        assert!(owns(SPECS, &["clients", "roles", "get"]));
        assert!(!owns(SPECS, &["clients", "delete"]));
        assert!(!owns(SPECS, &["clients"]));
    }
}
