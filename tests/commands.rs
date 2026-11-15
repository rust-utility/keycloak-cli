//! The generated command tree.
//!
//! These tests read the committed generated modules, so they describe what a
//! build ships rather than what the generator meant: one command per operation of
//! the enabled tags, reachable in the clap tree, with the arguments the API takes.
//!
//! The file is gated on the `clients` tag, the smallest one that exercises every
//! argument shape; a build without it has no tree to describe.
#![cfg(feature = "tag-clients")]

use clap::Command;
use keycloak_cli::command;
use keycloak_cli::generated;

/// Finds a command by its words in a clap tree.
fn find<'a>(commands: impl Iterator<Item = &'a Command>, words: &[&str]) -> Option<&'a Command> {
    let (first, rest) = words.split_first()?;
    let command = commands
        .into_iter()
        .find(|command| command.get_name() == *first)?;
    if rest.is_empty() {
        Some(command)
    } else {
        find(command.get_subcommands(), rest)
    }
}

mod clients {
    use super::*;
    use keycloak_cli::generated::tag_clients;

    #[test]
    fn every_generated_command_is_reachable() {
        assert_eq!(
            tag_clients::COMMANDS.len(),
            35,
            "the Clients tag of keycloak 26.8.100 has 35 operations"
        );
        let tree = generated::commands();
        for spec in tag_clients::COMMANDS {
            assert!(
                tag_clients::owns(spec.words),
                "{:?} is not owned by the tag module",
                spec.words
            );
            let command = find(tree.iter(), spec.words)
                .unwrap_or_else(|| panic!("{:?} is not in the clap tree", spec.words));
            assert_eq!(
                command.get_positionals().count(),
                spec.positionals.len(),
                "{} positional count",
                spec.words.join(" ")
            );
        }
    }

    #[test]
    fn commands_of_a_tag_do_not_share_their_words() {
        let mut signatures: Vec<String> = tag_clients::COMMANDS
            .iter()
            .map(|spec| spec.words.join(" "))
            .collect();
        let generated = signatures.len();
        signatures.sort();
        signatures.dedup();
        assert_eq!(signatures.len(), generated, "{signatures:?}");
    }

    #[test]
    fn arguments_carry_the_api_names_and_help() {
        let tree = generated::commands();
        let list = find(tree.iter(), &["clients", "list"]).expect("clients list");
        let long: Vec<&str> = list
            .get_arguments()
            .filter_map(|argument| argument.get_long())
            .collect();
        // Query parameters become flags with the crate's spelling.
        assert_eq!(
            long,
            ["client-id", "first", "max", "q", "search", "viewable-only"]
        );
        let first = list
            .get_arguments()
            .find(|argument| argument.get_id() == "first")
            .expect("first exists");
        assert_eq!(
            first.get_help().map(|help| help.to_string()).as_deref(),
            Some("the first result")
        );
        assert_eq!(
            first
                .get_value_names()
                .and_then(|names| names.first())
                .map(|name| name.to_string())
                .as_deref(),
            Some("first")
        );

        let get = find(tree.iter(), &["clients", "get"]).expect("clients get");
        let positional = get
            .get_positionals()
            .next()
            .expect("clients get takes the client uuid");
        assert_eq!(positional.get_id(), "client_uuid");
        assert_eq!(
            positional
                .get_value_names()
                .and_then(|names| names.first())
                .map(|name| name.to_string())
                .as_deref(),
            Some("client-uuid")
        );
        assert!(positional.is_required_set());
    }

    #[test]
    fn bodies_reach_the_command_line() {
        let tree = generated::commands();
        let create = find(tree.iter(), &["clients", "create"]).expect("clients create");
        assert!(
            create
                .get_arguments()
                .any(|argument| argument.get_id() == "body"),
            "clients create takes a JSON body"
        );
        let list = find(tree.iter(), &["clients", "list"]).expect("clients list");
        assert!(
            !list
                .get_arguments()
                .any(|argument| argument.get_id() == "body"),
            "clients list takes no body"
        );
        // The 11 deprecated operations of the description are not in this tag; the
        // rendering itself is covered in `command.rs`.
        assert!(tag_clients::COMMANDS.iter().all(|spec| !spec.deprecated));
    }
}

#[test]
fn generated_arguments_parse_into_values() {
    let tree = generated::commands();
    let matches = Command::new("keycloak-cli")
        .subcommands(tree)
        .try_get_matches_from([
            "keycloak-cli",
            "clients",
            "list",
            "--first",
            "10",
            "--search",
            "--viewable-only",
        ])
        .expect("the command line parses");

    let args = matches
        .subcommand_matches("clients")
        .and_then(|clients| clients.subcommand_matches("list"))
        .expect("clients list matches");
    assert_eq!(command::integer(args, "first"), Some(10));
    assert_eq!(command::boolean(args, "search"), Some(true));
    assert_eq!(command::boolean(args, "viewable_only"), Some(true));
    assert_eq!(command::text(args, "client_id"), None);
    assert_eq!(command::integer(args, "max"), None);
}

/// The whole API, when every tag is enabled.
#[cfg(feature = "tags-all")]
mod all_tags {
    use super::*;

    #[test]
    fn every_operation_of_the_pinned_crate_is_a_command() {
        let specs = generated::specs();
        assert_eq!(specs.len(), 415, "keycloak 26.8.100 has 415 operations");
        let mut words: Vec<String> = specs.iter().map(|spec| spec.words.join(" ")).collect();
        let generated = words.len();
        words.sort();
        words.dedup();
        assert_eq!(words.len(), generated, "two commands share their words");
    }

    #[test]
    fn every_command_is_reachable_and_grouped_by_its_first_word() {
        let tree = generated::commands();
        assert_eq!(tree.len(), 36, "distinct command groups");
        for spec in generated::specs() {
            let command = find(tree.iter(), spec.words)
                .unwrap_or_else(|| panic!("{:?} is not in the clap tree", spec.words));
            assert_eq!(
                command.get_name(),
                *spec.words.last().expect("a command has words")
            );
        }
    }

    #[test]
    fn the_untagged_operations_are_part_of_the_tree() {
        // The 31 operations without a tag live in group `clients` (authorization
        // services) and in `attack-detection`; they must not be dropped.
        let untagged = generated::tag_none::COMMANDS;
        assert_eq!(untagged.len(), 31);
        let tree = generated::commands();
        for spec in untagged {
            assert!(find(tree.iter(), spec.words).is_some(), "{:?}", spec.words);
        }
    }

    #[test]
    fn commands_that_share_their_wording_are_spelled_apart() {
        let tree = generated::commands();
        // `GET /localization` and `GET /localization/{locale}` would both read
        // `localization get`; the deeper one spells its parameter.
        assert!(find(tree.iter(), &["localization", "get"]).is_some());
        assert!(find(tree.iter(), &["localization", "locale", "get"]).is_some());
        assert!(find(tree.iter(), &["localization", "locale", "key", "get"]).is_some());
        assert!(find(tree.iter(), &["localization", "locale", "key", "delete"]).is_some());
    }
}
