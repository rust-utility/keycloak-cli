//! Naming rules for the generated command tree.
//!
//! Every operation becomes one command, and its name follows from the OpenAPI
//! path and the HTTP method alone — never from the Rust method name, which loses
//! the path shape (`GET /admin/realms` is `KeycloakAdmin::get()`, not
//! `realms_get()`).
//!
//! # Rules
//!
//! 1. The path must be `/admin/realms`, `/admin/realms/{realm}` or start with
//!    `/admin/realms/{realm}/`; anything else is rejected.
//! 2. `{realm}` is not a positional. It is the global `-r/--realm` flag of the
//!    CLI, so a realm-level command never repeats it.
//! 3. Every remaining literal path segment is a command word, in path order. The
//!    first one is the command group (`clients …`, `users …`); a path without
//!    literals (the two realm-level ones) belongs to the `realms` group.
//! 4. Every remaining path parameter is a positional, in path order
//!    (`clients roles {client-uuid} {role-name}` → `clients roles … <client-uuid>
//!    <role-name>`).
//! 5. The last word is the action, derived from the method and from what the
//!    path addresses. A path addresses a single object when it ends with a
//!    parameter, or when its last literal is not a collection (does not end with
//!    `s`); everything else addresses a collection:
//!
//!    | method | single object | collection |
//!    |--------|---------------|------------|
//!    | GET    | `get`         | `list`     |
//!    | POST   | `create`      | `create`   |
//!    | PUT    | `update`      | `update`   |
//!    | DELETE | `delete`      | `delete`   |
//!    | PATCH  | `patch`       | `patch`    |
//!
//!    So `/admin/realms` is `realms list|create` and `/admin/realms/{realm}` is
//!    `realms get|update|delete`: without the rule both would have no word after
//!    the group and collide.
//! 6. Words must be unique across the whole tree, but rule 3 drops parameter
//!    segments, so `GET /localization`, `/localization/{locale}` and
//!    `/localization/{locale}/{key}` all read `localization get`. Clap cannot
//!    host two commands with one path, so [`make_unique`] spells the parameters of
//!    such a group as words too (`localization locale get`), iterating until the
//!    tree is unambiguous and failing loudly if it cannot be.
//!
//! Query parameters become flags with the crate's snake_case spelling in kebab
//! form: the spec's `client-uuid`/`userId` both read `--client-uuid`/`--user-id`,
//! matching what the generated crate calls them. A flag that would shadow a
//! global connection flag is prefixed with `param-` instead ([`ParamName::flag`]).

use std::collections::BTreeMap;
use std::fmt;

use heck::{ToKebabCase, ToSnakeCase};

/// Every path of the Admin REST API starts with this prefix.
const ADMIN_PREFIX: &str = "/admin/realms";

/// The realm parameter of [`ADMIN_PREFIX`], not a positional.
const REALM_SEGMENT: &str = "/{realm}";

/// Group of the operations that have no path segment after the realm.
const FALLBACK_GROUP: &str = "realms";

/// Long names the generated commands must not reuse, because the CLI defines
/// them globally: connection settings and output controls.
pub const RESERVED_FLAGS: &[&str] = &[
    "server", "realm", "user", "password", "body", "output", "help", "version",
];

/// HTTP method of an operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
    Patch,
}

impl Method {
    /// Every method the Admin REST API uses, in a stable order.
    pub const ALL: [Method; 5] = [
        Method::Get,
        Method::Post,
        Method::Put,
        Method::Delete,
        Method::Patch,
    ];

    /// Parses the method name as it appears in the description, e.g. `get`.
    pub fn from_http(name: &str) -> Option<Self> {
        match name {
            "get" => Some(Method::Get),
            "post" => Some(Method::Post),
            "put" => Some(Method::Put),
            "delete" => Some(Method::Delete),
            "patch" => Some(Method::Patch),
            _ => None,
        }
    }

    /// The method name as the description writes it.
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "get",
            Method::Post => "post",
            Method::Put => "put",
            Method::Delete => "delete",
            Method::Patch => "patch",
        }
    }

    /// The action word this method performs on its target.
    pub fn action(self, target: Target) -> Action {
        match (self, target) {
            (Method::Get, Target::Single) => Action::Get,
            (Method::Get, Target::Collection) => Action::List,
            (Method::Post, _) => Action::Create,
            (Method::Put, _) => Action::Update,
            (Method::Delete, _) => Action::Delete,
            (Method::Patch, _) => Action::Patch,
        }
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether an operation addresses a single object or a collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Single,
    Collection,
}

/// The last word of a command: what it does to [`Target`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    List,
    Get,
    Create,
    Update,
    Delete,
    Patch,
}

impl Action {
    /// The word as it appears on the command line.
    pub fn as_str(self) -> &'static str {
        match self {
            Action::List => "list",
            Action::Get => "get",
            Action::Create => "create",
            Action::Update => "update",
            Action::Delete => "delete",
            Action::Patch => "patch",
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A parameter name in the two spellings the generator needs.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ParamName {
    /// Command line spelling, e.g. `client-uuid` (a positional value name).
    pub kebab: String,
    /// Rust spelling, e.g. `client_uuid`, as the `keycloak` crate names it.
    pub snake: String,
}

impl ParamName {
    /// Normalises a parameter name from the description, which is usually already
    /// kebab-case (`user-id`) but not always (`clientScopeId`).
    pub fn new(spec_name: &str) -> Self {
        let kebab = spec_name.to_kebab_case();
        let snake = kebab.to_snake_case();
        Self { kebab, snake }
    }

    /// The long flag for this parameter, without the leading dashes.
    ///
    /// A name that would shadow a global flag is prefixed with `param-`: the
    /// events endpoint filters by `user`, and `--user` is the administrator.
    pub fn flag(&self) -> String {
        if RESERVED_FLAGS.contains(&self.kebab.as_str()) {
            format!("param-{}", self.kebab)
        } else {
            self.kebab.clone()
        }
    }
}

impl fmt::Display for ParamName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.kebab)
    }
}

/// One segment of the path after the realm.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Segment {
    Literal(String),
    Param(ParamName),
}

/// A command derived from one operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandName {
    path: String,
    method: Method,
    segments: Vec<Segment>,
    action: Action,
    shaped: bool,
}

impl CommandName {
    /// Derives the command name of `path` for `method`.
    pub fn new(path: &str, method: Method) -> Result<Self, NamingError> {
        let segments = parse_segments(path)?;
        let target = target_of(path, &segments);
        Ok(Self {
            action: method.action(target),
            path: path.to_owned(),
            method,
            segments,
            shaped: false,
        })
    }

    /// The OpenAPI path this command came from.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The HTTP method this command came from.
    pub fn method(&self) -> Method {
        self.method
    }

    /// The command group, i.e. the first word.
    pub fn group(&self) -> &str {
        self.segments
            .iter()
            .find_map(|segment| match segment {
                Segment::Literal(name) => Some(name.as_str()),
                Segment::Param(_) => None,
            })
            .unwrap_or(FALLBACK_GROUP)
    }

    /// What the command does to its target.
    pub fn action(&self) -> Action {
        self.action
    }

    /// The path parameters, in path order, i.e. the positionals of the command.
    pub fn positionals(&self) -> Vec<&ParamName> {
        self.segments
            .iter()
            .filter_map(|segment| match segment {
                Segment::Param(param) => Some(param),
                Segment::Literal(_) => None,
            })
            .collect()
    }

    /// Whether the parameters are spelled as words (see [`make_unique`]).
    pub fn is_shaped(&self) -> bool {
        self.shaped
    }

    /// The command words, i.e. the clap subcommand path.
    pub fn words(&self) -> Vec<&str> {
        let mut words = Vec::new();
        for segment in &self.segments {
            match segment {
                Segment::Literal(name) => words.push(name.as_str()),
                Segment::Param(param) if self.shaped => words.push(param.kebab.as_str()),
                Segment::Param(_) => {}
            }
        }
        if words.is_empty() {
            words.push(FALLBACK_GROUP);
        }
        words.push(self.action.as_str());
        words
    }

    /// The words joined by spaces: the key that must be unique in the tree.
    pub fn signature(&self) -> String {
        self.words().join(" ")
    }

    /// The command with its positionals, e.g. `clients roles get <client-uuid>`.
    pub fn usage(&self) -> String {
        let mut usage = self.signature();
        for param in self.positionals() {
            usage.push_str(&format!(" <{}>", param.kebab));
        }
        usage
    }
}

impl fmt::Display for CommandName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.usage())
    }
}

/// Two or more commands that would share one clap command path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Collision {
    /// The shared words, e.g. `localization get`.
    pub signature: String,
    /// The operations behind them, e.g. `get /admin/realms/{realm}/localization`.
    pub operations: Vec<String>,
}

impl fmt::Display for Collision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} is ambiguous:", self.signature)?;
        for operation in &self.operations {
            write!(f, "\n  {operation}")?;
        }
        Ok(())
    }
}

/// Makes the command words of a whole operation set unique.
///
/// Operations that share their words but differ in the path parameters they take
/// are spelled with their parameters as words, because clap cannot host two
/// commands at one path. The iteration stops when the tree is unambiguous, and a
/// set that stays ambiguous is reported instead of being renamed silently.
pub fn make_unique(commands: &mut [CommandName]) -> Result<(), Vec<Collision>> {
    loop {
        let mut by_signature: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (index, command) in commands.iter().enumerate() {
            by_signature
                .entry(command.signature())
                .or_default()
                .push(index);
        }

        let collisions: Vec<&Vec<usize>> = by_signature
            .values()
            .filter(|indexes| indexes.len() > 1)
            .collect();
        if collisions.is_empty() {
            return Ok(());
        }

        let mut changed = false;
        for indexes in &collisions {
            for &index in indexes.iter() {
                if !commands[index].shaped {
                    commands[index].shaped = true;
                    changed = true;
                }
            }
        }
        if !changed {
            return Err(collisions
                .iter()
                .map(|indexes| Collision {
                    signature: commands[indexes[0]].signature(),
                    operations: indexes
                        .iter()
                        .map(|&index| {
                            format!("{} {}", commands[index].method, commands[index].path)
                        })
                        .collect(),
                })
                .collect());
        }
    }
}

/// Why a path cannot become a command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NamingError {
    /// The path does not belong to the Admin REST API under `/admin/realms`.
    OutsideAdminApi { path: String },
}

impl fmt::Display for NamingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NamingError::OutsideAdminApi { path } => {
                write!(f, "{path} is not under /admin/realms")
            }
        }
    }
}

impl std::error::Error for NamingError {}

/// Splits a path into the segments that become command words and positionals.
fn parse_segments(path: &str) -> Result<Vec<Segment>, NamingError> {
    let trimmed = path.trim_end_matches('/');
    let rest = match trimmed.strip_prefix(ADMIN_PREFIX) {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => rest,
        _ => {
            return Err(NamingError::OutsideAdminApi {
                path: path.to_owned(),
            })
        }
    };
    let rest = rest.strip_prefix(REALM_SEGMENT).unwrap_or(rest);

    let mut segments = Vec::new();
    for raw in rest.split('/').filter(|segment| !segment.is_empty()) {
        match raw
            .strip_prefix('{')
            .and_then(|name| name.strip_suffix('}'))
        {
            Some(name) => segments.push(Segment::Param(ParamName::new(name))),
            None => segments.push(Segment::Literal(raw.to_kebab_case())),
        }
    }
    Ok(segments)
}

/// Whether the path addresses a single object or a collection.
///
/// A trailing parameter is always a single object; otherwise a last literal that
/// does not end with `s` is a single resource too (`…/client-secret`,
/// `…/groups/count`), and everything else is a collection.
fn target_of(path: &str, segments: &[Segment]) -> Target {
    if path.trim_end_matches('/').ends_with('}') {
        return Target::Single;
    }
    match segments.iter().rev().find_map(|segment| match segment {
        Segment::Literal(name) => Some(name),
        Segment::Param(_) => None,
    }) {
        Some(name) if !name.ends_with('s') => Target::Single,
        _ => Target::Collection,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(path: &str, method: Method) -> CommandName {
        CommandName::new(path, method).expect("path is part of the admin api")
    }

    fn words(path: &str, method: Method) -> Vec<String> {
        name(path, method)
            .words()
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    fn positionals(path: &str, method: Method) -> Vec<String> {
        name(path, method)
            .positionals()
            .into_iter()
            .map(|param| param.kebab.clone())
            .collect()
    }

    #[test]
    fn realm_collection_and_realm_object_do_not_collide() {
        // The case the rules exist for: both paths have nothing after the realm,
        // so only the target distinguishes them.
        assert_eq!(words("/admin/realms", Method::Get), ["realms", "list"]);
        assert_eq!(words("/admin/realms", Method::Post), ["realms", "create"]);

        assert_eq!(
            words("/admin/realms/{realm}", Method::Get),
            ["realms", "get"]
        );
        assert_eq!(
            words("/admin/realms/{realm}", Method::Put),
            ["realms", "update"]
        );
        assert_eq!(
            words("/admin/realms/{realm}", Method::Delete),
            ["realms", "delete"]
        );

        let all = [
            name("/admin/realms", Method::Get),
            name("/admin/realms", Method::Post),
            name("/admin/realms/{realm}", Method::Get),
            name("/admin/realms/{realm}", Method::Put),
            name("/admin/realms/{realm}", Method::Delete),
        ];
        let mut signatures: Vec<String> = all.iter().map(CommandName::signature).collect();
        signatures.sort();
        signatures.dedup();
        assert_eq!(signatures.len(), all.len(), "{signatures:?}");

        // The realm is the global `--realm` flag, never a positional.
        for command in &all {
            assert!(command.positionals().is_empty(), "{command}");
        }
    }

    #[test]
    fn collections_are_listed_and_objects_are_got() {
        assert_eq!(
            words("/admin/realms/{realm}/clients", Method::Get),
            ["clients", "list"]
        );
        assert_eq!(
            words("/admin/realms/{realm}/clients", Method::Post),
            ["clients", "create"]
        );
        assert_eq!(
            words("/admin/realms/{realm}/clients/{client-uuid}", Method::Get),
            ["clients", "get"]
        );
        assert_eq!(
            words("/admin/realms/{realm}/clients/{client-uuid}", Method::Put),
            ["clients", "update"]
        );
        assert_eq!(
            words(
                "/admin/realms/{realm}/clients/{client-uuid}",
                Method::Delete
            ),
            ["clients", "delete"]
        );
        assert_eq!(
            positionals("/admin/realms/{realm}/clients/{client-uuid}", Method::Get),
            ["client-uuid"]
        );
    }

    #[test]
    fn singular_leaves_are_single_objects() {
        // A leaf that is not a collection noun is read as one value, not a list.
        assert_eq!(
            words(
                "/admin/realms/{realm}/clients/{client-uuid}/client-secret",
                Method::Get
            ),
            ["clients", "client-secret", "get"]
        );
        assert_eq!(
            words(
                "/admin/realms/{realm}/clients/{client-uuid}/session-count",
                Method::Get
            ),
            ["clients", "session-count", "get"]
        );
        // `…/test-nodes-available` is singular, `…/nodes` is a collection.
        assert_eq!(
            words(
                "/admin/realms/{realm}/clients/{client-uuid}/test-nodes-available",
                Method::Get
            ),
            ["clients", "test-nodes-available", "get"]
        );
        // A DELETE of a collection of brute-force users deletes the collection.
        assert_eq!(
            words(
                "/admin/realms/{realm}/attack-detection/brute-force/users",
                Method::Delete
            ),
            ["attack-detection", "brute-force", "users", "delete"]
        );
    }

    #[test]
    fn deep_paths_keep_positional_order() {
        let path = "/admin/realms/{realm}/clients/{client-uuid}/roles/{role-name}/composites";
        assert_eq!(
            words(path, Method::Get),
            ["clients", "roles", "composites", "list"]
        );
        assert_eq!(positionals(path, Method::Get), ["client-uuid", "role-name"]);
        assert_eq!(
            name(path, Method::Get).usage(),
            "clients roles composites list <client-uuid> <role-name>"
        );
    }

    #[test]
    fn parameters_are_normalized_to_kebab_and_snake_case() {
        let command = name(
            "/admin/realms/{realm}/clients/{client-uuid}/evaluate-scopes/scope-mappings/{roleContainerId}/granted",
            Method::Get,
        );
        let params = command.positionals();
        assert_eq!(params.len(), 2);
        assert_eq!(params[1].kebab, "role-container-id");
        assert_eq!(params[1].snake, "role_container_id");
        assert_eq!(
            words(
                "/admin/realms/{realm}/users/{userId}/credentials/{credentialId}/moveToFirst",
                Method::Post
            ),
            ["users", "credentials", "move-to-first", "create"]
        );
    }

    #[test]
    fn flags_avoid_the_global_connection_flags() {
        assert_eq!(ParamName::new("client-uuid").flag(), "client-uuid");
        assert_eq!(ParamName::new("clientScopeId").flag(), "client-scope-id");
        assert_eq!(ParamName::new("user").flag(), "param-user");
        assert_eq!(ParamName::new("realm").flag(), "param-realm");
        assert_eq!(
            ParamName::new("briefRepresentation").snake,
            "brief_representation"
        );
    }

    #[test]
    fn shape_commands_that_would_share_a_clap_path() {
        let mut commands = vec![
            name("/admin/realms/{realm}/localization", Method::Get),
            name("/admin/realms/{realm}/localization/{locale}", Method::Get),
            name(
                "/admin/realms/{realm}/localization/{locale}/{key}",
                Method::Get,
            ),
            name("/admin/realms/{realm}/users", Method::Get),
        ];
        // Before: three commands want to be `localization get`.
        assert_eq!(commands[0].signature(), "localization get");
        assert_eq!(commands[1].signature(), "localization get");

        make_unique(&mut commands).expect("parameters disambiguate the group");

        let signatures: Vec<String> = commands.iter().map(CommandName::signature).collect();
        assert_eq!(
            signatures,
            [
                "localization get",
                "localization locale get",
                "localization locale key get",
                "users list",
            ]
        );
        // Shaping only changes the words; the positionals stay the same.
        assert!(commands[0].positionals().is_empty());
        assert_eq!(commands[1].positionals().len(), 1);
        assert_eq!(commands[2].positionals().len(), 2);
    }

    #[test]
    fn shape_only_the_ambiguous_group() {
        let mut commands = vec![
            name("/admin/realms/{realm}/users", Method::Get),
            name("/admin/realms/{realm}/clients", Method::Get),
            name(
                "/admin/realms/{realm}/organizations/members/{member-id}/organizations",
                Method::Get,
            ),
            name(
                "/admin/realms/{realm}/organizations/{org-id}/members/{member-id}/organizations",
                Method::Get,
            ),
        ];
        make_unique(&mut commands).expect("parameters disambiguate the group");
        assert!(!commands[0].is_shaped());
        assert!(!commands[1].is_shaped());
        assert_eq!(
            commands[2].signature(),
            "organizations members member-id organizations list"
        );
        assert_eq!(
            commands[3].signature(),
            "organizations org-id members member-id organizations list"
        );
        assert_eq!(commands[3].positionals().len(), 2);
    }

    #[test]
    fn ambiguous_operations_are_reported() {
        // The same path and method twice cannot be told apart by any rule.
        let mut commands = vec![
            name("/admin/realms/{realm}/users", Method::Get),
            name("/admin/realms/{realm}/users", Method::Get),
        ];
        let collisions = make_unique(&mut commands).expect_err("identical operations collide");
        assert_eq!(collisions.len(), 1);
        assert_eq!(collisions[0].signature, "users list");
        assert_eq!(
            collisions[0].operations,
            [
                "get /admin/realms/{realm}/users",
                "get /admin/realms/{realm}/users",
            ]
        );
        assert!(collisions[0].to_string().contains("is ambiguous"));
    }

    #[test]
    fn paths_outside_the_admin_api_are_rejected() {
        let error =
            CommandName::new("/admin/serverinfo", Method::Get).expect_err("not a realm path");
        assert_eq!(
            error,
            NamingError::OutsideAdminApi {
                path: "/admin/serverinfo".to_owned()
            }
        );
        assert!(error.to_string().contains("/admin/serverinfo"));
        // `/admin/realms-something` must not be read as `/admin/realms`.
        assert!(CommandName::new("/admin/realmsx", Method::Get).is_err());
    }

    #[test]
    fn every_method_maps_to_an_action() {
        for method in Method::ALL {
            assert_eq!(Method::from_http(method.as_str()), Some(method));
            let action = method.action(Target::Collection);
            assert!(!action.as_str().is_empty());
        }
        assert_eq!(Method::from_http("head"), None);
        assert_eq!(Method::Get.action(Target::Collection), Action::List);
        assert_eq!(Method::Get.action(Target::Single), Action::Get);
        assert_eq!(Method::Post.action(Target::Single), Action::Create);
    }
}
