//! Emission of the generated command modules.
//!
//! Every operation of the pinned description becomes one command and one call to
//! its `keycloak` crate method. The modules carry data ([`crate::command`] turns
//! it into the clap tree) plus the call itself, so nothing about the API is
//! written by hand and nothing about a command lives anywhere else.
//!
//! A module is emitted per *tag* (feature `tag-…`, module `tag_…`), because the
//! tag is what a build can switch on; the commands of several tags are merged
//! into one clap tree by [`crate::command::tree`].

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use heck::ToSnakeCase;

use crate::generate::naming::{self, CommandName, Method, ParamName};
use crate::generate::signature::{rust_name, CrateApi, CrateMethod};
use crate::generate::spec::{ApiOperation, Location, Spec};

/// One generated module, ready to be written.
#[derive(Clone, Debug)]
pub struct Module {
    /// Tag in kebab-case, e.g. `client-role-mappings`.
    pub tag: String,
    /// Module name, e.g. `tag_clients`.
    pub module: String,
    /// Cargo feature, e.g. `tag-clients`.
    pub feature: String,
    /// File name, e.g. `tag_clients.rs`.
    pub file: String,
    /// The module source.
    pub source: String,
}

/// The result of a generation run.
#[derive(Clone, Debug)]
pub struct Generated {
    /// One module per selected tag, in tag order.
    pub modules: Vec<Module>,
    /// The `mod.rs` that ties the modules together.
    pub mod_source: String,
    /// How many operations the description has.
    pub operations: usize,
    /// How many commands they became.
    pub commands: usize,
    /// How many commands spell their parameters as words to stay unambiguous.
    pub shaped: usize,
}

impl Generated {
    /// Writes `mod.rs` and every module into `out`.
    pub fn write(&self, out: &Path) -> Result<(), EmitError> {
        std::fs::create_dir_all(out).map_err(|error| EmitError::Write {
            path: out.to_owned(),
            error,
        })?;
        write_file(&out.join("mod.rs"), &self.mod_source)?;
        for module in &self.modules {
            write_file(&out.join(&module.file), &module.source)?;
        }
        Ok(())
    }
}

fn write_file(path: &Path, contents: &str) -> Result<(), EmitError> {
    std::fs::write(path, contents).map_err(|error| EmitError::Write {
        path: path.to_owned(),
        error,
    })
}

/// One operation, its generated command and the crate method it calls.
struct Bound<'a> {
    tag: String,
    command: CommandName,
    operation: &'a ApiOperation<'a>,
    method: &'a CrateMethod,
}

/// Builds the modules of `selected` tags, validating the whole description.
///
/// Every tag is emitted, because a description the rules cannot cover must fail
/// generation whether or not that tag is selected right now; only the selected
/// modules are returned.
pub fn generate(
    spec: &Spec,
    api: &CrateApi,
    version: &str,
    selected: &[String],
) -> Result<Generated, EmitError> {
    let operations = spec.operations();
    let mut bound = Vec::with_capacity(operations.len());
    for operation in &operations {
        let command = CommandName::new(operation.path, operation.method).map_err(|error| {
            EmitError::Naming {
                path: operation.path.to_owned(),
                detail: error.to_string(),
            }
        })?;
        let method = api
            .resolve(operation.path, operation.method)
            .ok_or_else(|| EmitError::MissingMethod {
                path: operation.path.to_owned(),
                method: operation.method,
            })?;
        bound.push(Bound {
            tag: tag_of(operation),
            command,
            operation,
            method,
        });
    }

    // The words have to be unique across the *whole* tree, not per selected tag:
    // a command must not change its spelling depending on which tags are built.
    let mut commands: Vec<CommandName> = bound.iter().map(|entry| entry.command.clone()).collect();
    naming::make_unique(&mut commands).map_err(|collisions| {
        EmitError::Collisions(
            collisions
                .iter()
                .map(|collision| collision.to_string())
                .collect(),
        )
    })?;
    let shaped = commands
        .iter()
        .filter(|command| command.is_shaped())
        .count();
    for (entry, command) in bound.iter_mut().zip(commands) {
        entry.command = command;
    }
    bound.sort_by(|left, right| {
        left.command
            .signature()
            .cmp(&right.command.signature())
            .then_with(|| left.method.name.cmp(&right.method.name))
    });

    let all_tags: Vec<String> = {
        let mut tags: Vec<String> = bound.iter().map(|entry| entry.tag.clone()).collect();
        tags.sort();
        tags.dedup();
        tags
    };
    for tag in selected {
        if !all_tags.contains(tag) {
            return Err(EmitError::UnknownTag {
                tag: tag.clone(),
                known: all_tags,
            });
        }
    }

    let mut modules = Vec::new();
    let mut selected_modules = Vec::new();
    for tag in &all_tags {
        let entries: Vec<&Bound<'_>> = bound.iter().filter(|entry| &entry.tag == tag).collect();
        let module = Module {
            tag: tag.clone(),
            module: format!("tag_{}", tag.to_snake_case()),
            feature: format!("tag-{tag}"),
            file: format!("tag_{}.rs", tag.to_snake_case()),
            source: emit_module(tag, &entries, version)?,
        };
        if selected.is_empty() || selected.contains(tag) {
            selected_modules.push(module.clone());
        }
        modules.push(module);
    }
    let mod_source = emit_mod(&selected_modules, version, bound.len());

    Ok(Generated {
        modules: selected_modules,
        mod_source,
        operations: operations.len(),
        commands: bound.len(),
        shaped,
    })
}

/// The kebab-case tag an operation is generated under.
fn tag_of(operation: &ApiOperation<'_>) -> String {
    operation
        .tag()
        .map_or_else(|| "none".to_owned(), ToSnakeCase::to_snake_case)
        .replace('_', "-")
}

/// Emits the module of one tag.
fn emit_module(tag: &str, entries: &[&Bound<'_>], version: &str) -> Result<String, EmitError> {
    let feature = format!("tag-{tag}");
    let operations = entries.len();

    let mut source = String::new();
    writeln!(
        source,
        "//! Generated by `scripts/generate.sh` from `keycloak {version}`; do not edit."
    )
    .expect("writing to a string cannot fail");
    writeln!(source, "//!").expect("writing to a string cannot fail");
    writeln!(
        source,
        "//! {operations} commands of the `{tag}` tag (feature `{feature}`)."
    )
    .expect("writing to a string cannot fail");
    source.push('\n');
    source.push_str("use clap::ArgMatches;\n");
    source.push_str("use serde_json::Value;\n\n");
    source.push_str("#[allow(unused_imports)]\n");
    source.push_str("use keycloak::types::*;\n");
    source.push_str("use keycloak::{KeycloakAdmin, KeycloakTokenSupplier};\n\n");
    source.push_str("#[allow(unused_imports)]\n");
    source.push_str("use crate::command::{CommandSpec, ParamSpec, ValueKind};\n");
    source.push_str("use crate::error::Error;\n\n");

    source.push_str("/// The commands of this tag.\n");
    source.push_str("pub const COMMANDS: &[CommandSpec] = &[\n");
    for entry in entries {
        source.push_str(&emit_spec(entry)?);
    }
    source.push_str("];\n\n");

    source.push_str("/// Whether `words` name a command of this tag.\n");
    source.push_str("pub fn owns(words: &[&str]) -> bool {\n");
    source.push_str("    crate::command::owns(COMMANDS, words)\n");
    source.push_str("}\n\n");

    source.push_str("/// Runs the operation behind `words`.\n");
    source.push_str("///\n");
    source.push_str("/// A tag without operations that take arguments has no use for them, and\n");
    source.push_str("/// the description marks some operations deprecated: both are properties\n");
    source.push_str("/// of the API, not warnings the build should fail on.\n");
    source.push_str("#[allow(unused_variables, deprecated)]\n");
    source.push_str("pub async fn dispatch<TS: KeycloakTokenSupplier>(\n");
    source.push_str("    admin: &KeycloakAdmin<TS>,\n");
    source.push_str("    realm: &str,\n");
    source.push_str("    words: &[&str],\n");
    source.push_str("    args: &ArgMatches,\n");
    source.push_str(") -> Result<Value, Error> {\n");
    source.push_str("    match words {\n");
    for entry in entries {
        source.push_str(&emit_arm(entry)?);
    }
    source.push_str("        other => Err(Error::usage(format!(\n");
    source.push_str("            \"unknown command: {}\",\n");
    source.push_str("            other.join(\" \")\n");
    source.push_str("        ))),\n");
    source.push_str("    }\n");
    source.push_str("}\n");

    Ok(source)
}

/// Emits the `CommandSpec` literal of one command.
fn emit_spec(entry: &Bound<'_>) -> Result<String, EmitError> {
    check_parameters(entry)?;

    let mut out = String::new();
    out.push_str("    CommandSpec {\n");
    let words: Vec<String> = entry
        .command
        .words()
        .iter()
        .map(|word| format!("{word:?}"))
        .collect();
    writeln!(out, "        words: &[{}],", words.join(", "))
        .expect("writing to a string cannot fail");
    writeln!(
        out,
        "        method: {:?},",
        entry.command.method().as_str().to_ascii_uppercase()
    )
    .expect("writing to a string cannot fail");
    writeln!(out, "        path: {:?},", path_template(entry))
        .expect("writing to a string cannot fail");
    out.push_str("        query: &[\n");
    for parameter in entry.operation.query_parameters() {
        writeln!(
            out,
            "            ({:?}, {:?}),",
            rust_name(&parameter.name),
            parameter.name
        )
        .expect("writing to a string cannot fail");
    }
    out.push_str("        ],\n");
    writeln!(out, "        about: {:?},", about(entry)).expect("writing to a string cannot fail");
    writeln!(out, "        long_about: {:?},", long_about(entry))
        .expect("writing to a string cannot fail");

    out.push_str("        positionals: &[\n");
    for param in entry.command.positionals() {
        let crate_param = crate_parameter(entry, &rust_name(&param.kebab))?;
        writeln!(
            out,
            "            ParamSpec {{ id: {:?}, value_name: {:?}, long: \"\", help: {:?}, kind: ValueKind::Text }},",
            crate_param.name,
            param.kebab,
            help_of(entry, &crate_param.name)
        )
        .expect("writing to a string cannot fail");
    }
    out.push_str("        ],\n");

    out.push_str("        flags: &[\n");
    for parameter in entry.operation.query_parameters() {
        let name = rust_name(&parameter.name);
        let crate_param = crate_parameter(entry, &name)?;
        let kind = value_kind(&crate_param.ty).ok_or_else(|| EmitError::UnsupportedType {
            path: entry.operation.path.to_owned(),
            parameter: crate_param.name.clone(),
            ty: crate_param.ty.clone(),
        })?;
        let flag = ParamName::new(&parameter.name).flag();
        writeln!(
            out,
            "            ParamSpec {{ id: {:?}, value_name: {:?}, long: {:?}, help: {:?}, kind: ValueKind::{kind} }},",
            crate_param.name, flag, flag, help_of(entry, &crate_param.name)
        )
        .expect("writing to a string cannot fail");
    }
    out.push_str("        ],\n");

    writeln!(out, "        body: {},", entry.method.body().is_some())
        .expect("writing to a string cannot fail");
    writeln!(
        out,
        "        deprecated: {},",
        entry.operation.operation.deprecated
    )
    .expect("writing to a string cannot fail");
    out.push_str("    },\n");
    Ok(out)
}

/// Emits one `match` arm: the call to the crate method.
fn emit_arm(entry: &Bound<'_>) -> Result<String, EmitError> {
    check_parameters(entry)?;

    let mut out = String::new();
    let words: Vec<String> = entry
        .command
        .words()
        .iter()
        .map(|word| format!("{word:?}"))
        .collect();
    writeln!(out, "        [{}] => {{", words.join(", ")).expect("writing to a string cannot fail");

    // `&str` parameters are read into bindings first: the method takes a
    // reference, and a temporary would not outlive the call.
    let mut arguments = Vec::new();
    for parameter in &entry.method.parameters {
        if parameter.name == "realm" {
            arguments.push("realm".to_owned());
            continue;
        }
        if parameter.name == "body" {
            writeln!(
                out,
                "            let body: {} = crate::command::body(args)?;",
                parameter.ty
            )
            .expect("writing to a string cannot fail");
            arguments.push("body".to_owned());
            continue;
        }
        match parameter.ty.as_str() {
            "&str" => {
                writeln!(
                    out,
                    "            let {} = crate::command::positional(args, {:?})?;",
                    parameter.name, parameter.name
                )
                .expect("writing to a string cannot fail");
                arguments.push(format!("{}.as_str()", parameter.name));
            }
            "Option<String>" => {
                arguments.push(format!("crate::command::text(args, {:?})", parameter.name))
            }
            "Option<bool>" => arguments.push(format!(
                "crate::command::boolean(args, {:?})",
                parameter.name
            )),
            "Option<i32>" => arguments.push(format!(
                "crate::command::integer(args, {:?})",
                parameter.name
            )),
            "Option<Vec<String>>" => {
                arguments.push(format!("crate::command::texts(args, {:?})", parameter.name))
            }
            ty => {
                return Err(EmitError::UnsupportedType {
                    path: entry.operation.path.to_owned(),
                    parameter: parameter.name.clone(),
                    ty: ty.to_owned(),
                })
            }
        }
    }

    if entry.method.discards_response() {
        writeln!(
            out,
            "            admin.{}({}).await?;",
            entry.method.name,
            arguments.join(", ")
        )
        .expect("writing to a string cannot fail");
        out.push_str("            Ok(Value::Null)\n");
    } else {
        writeln!(
            out,
            "            let response = admin.{}({}).await?;",
            entry.method.name,
            arguments.join(", ")
        )
        .expect("writing to a string cannot fail");
        out.push_str("            crate::command::to_json(response)\n");
    }
    out.push_str("        }\n");
    Ok(out)
}

/// Emits `mod.rs`, which ties the modules into one tree.
fn emit_mod(modules: &[Module], version: &str, commands: usize) -> String {
    let mut source = String::new();
    writeln!(
        source,
        "//! Generated by `scripts/generate.sh` from `keycloak {version}`; do not edit."
    )
    .expect("writing to a string cannot fail");
    writeln!(source, "//!").expect("writing to a string cannot fail");
    writeln!(
        source,
        "//! The command tree of every enabled tag: {commands} commands in {} modules.",
        modules.len()
    )
    .expect("writing to a string cannot fail");
    source.push('\n');
    source.push_str("pub mod tags;\n");
    for module in modules {
        source.push('\n');
        writeln!(source, "#[cfg(feature = {:?})]", module.feature)
            .expect("writing to a string cannot fail");
        writeln!(source, "pub mod {};", module.module).expect("writing to a string cannot fail");
    }

    source.push('\n');
    source.push_str("/// Every command of every enabled tag.\n");
    source.push_str("pub fn specs() -> Vec<&'static crate::command::CommandSpec> {\n");
    source.push_str("    #[allow(unused_mut)]\n");
    source.push_str("    let mut specs: Vec<&crate::command::CommandSpec> = Vec::new();\n");
    for module in modules {
        source.push_str("    #[cfg(feature = ");
        writeln!(source, "{:?})]", module.feature).expect("writing to a string cannot fail");
        writeln!(
            source,
            "    specs.extend({}::COMMANDS.iter());",
            module.module
        )
        .expect("writing to a string cannot fail");
    }
    source.push_str("    specs\n");
    source.push_str("}\n");

    source.push('\n');
    source.push_str("/// The command named by `words`, when this build ships it.\n");
    source.push_str(
        "pub fn spec(words: &[&str]) -> Option<&'static crate::command::CommandSpec> {\n",
    );
    source.push_str("    specs().into_iter().find(|spec| spec.words == words)\n");
    source.push_str("}\n");

    source.push('\n');
    source.push_str("/// The top level commands of every enabled tag.\n");
    source.push_str("pub fn commands() -> Vec<clap::Command> {\n");
    source.push_str("    crate::command::tree(specs())\n");
    source.push_str("}\n");

    source.push('\n');
    source.push_str("/// Whether `words` name a generated command of an enabled tag.\n");
    source.push_str("pub fn owns(words: &[&str]) -> bool {\n");
    source.push_str("    commands()\n");
    source.push_str("        .iter()\n");
    source.push_str("        .any(|command| command.get_name() == words[0])\n");
    source.push_str("}\n");

    source.push('\n');
    source.push_str("/// Runs the operation behind `words`.\n");
    source.push_str("///\n");
    source.push_str("/// Without an enabled tag there is nothing to dispatch to, which is why\n");
    source.push_str("/// the arguments are allowed to be unused.\n");
    source.push_str("#[allow(unused_variables)]\n");
    source.push_str("pub async fn dispatch<TS: keycloak::KeycloakTokenSupplier>(\n");
    source.push_str("    admin: &keycloak::KeycloakAdmin<TS>,\n");
    source.push_str("    realm: &str,\n");
    source.push_str("    words: &[&str],\n");
    source.push_str("    args: &clap::ArgMatches,\n");
    source.push_str(") -> Result<serde_json::Value, crate::error::Error> {\n");
    for module in modules {
        source.push_str("    #[cfg(feature = ");
        writeln!(source, "{:?})]", module.feature).expect("writing to a string cannot fail");
        writeln!(source, "    if {}::owns(words) {{", module.module)
            .expect("writing to a string cannot fail");
        writeln!(
            source,
            "        return {}::dispatch(admin, realm, words, args).await;",
            module.module
        )
        .expect("writing to a string cannot fail");
        source.push_str("    }\n");
    }
    source.push_str("    Err(crate::error::Error::usage(format!(\n");
    source.push_str("        \"unknown command: {}\",\n");
    source.push_str("        words.join(\" \")\n");
    source.push_str("    )))\n");
    source.push_str("}\n");
    source
}

/// Checks that the operation and the crate method describe the same parameters.
fn check_parameters(entry: &Bound<'_>) -> Result<(), EmitError> {
    for parameter in &entry.method.parameters {
        if parameter.name == "realm" || parameter.name == "body" {
            continue;
        }
        let known = entry
            .operation
            .parameters
            .iter()
            .any(|spec| rust_name(&spec.name) == parameter.name);
        if !known {
            return Err(EmitError::UnmatchedParameter {
                path: entry.operation.path.to_owned(),
                parameter: parameter.name.clone(),
            });
        }
    }
    for spec in &entry.operation.parameters {
        if spec.location != Location::Path && spec.location != Location::Query {
            continue;
        }
        let name = rust_name(&spec.name);
        if name == "realm" {
            continue;
        }
        if !entry
            .method
            .parameters
            .iter()
            .any(|parameter| parameter.name == name)
        {
            return Err(EmitError::MissingParameter {
                path: entry.operation.path.to_owned(),
                parameter: name,
            });
        }
    }
    Ok(())
}

/// The path with the crate's parameter names, e.g.
/// `/admin/realms/{realm}/clients/{client_uuid}`.
fn path_template(entry: &Bound<'_>) -> String {
    let mut path = entry.operation.path.to_owned();
    for parameter in entry.operation.path_parameters() {
        path = path.replace(
            &format!("{{{}}}", parameter.name),
            &format!("{{{}}}", rust_name(&parameter.name)),
        );
    }
    path
}

/// The crate parameter called `name`.
fn crate_parameter<'a, 'b>(
    entry: &'b Bound<'a>,
    name: &str,
) -> Result<&'b crate::generate::signature::CrateParameter, EmitError> {
    entry
        .method
        .parameters
        .iter()
        .find(|parameter| parameter.name == name)
        .ok_or_else(|| EmitError::MissingParameter {
            path: entry.operation.path.to_owned(),
            parameter: name.to_owned(),
        })
}

/// The `ValueKind` for a crate parameter type.
fn value_kind(ty: &str) -> Option<&'static str> {
    match ty {
        "Option<bool>" => Some("Flag"),
        "Option<i32>" => Some("Integer"),
        "Option<String>" => Some("Text"),
        "Option<Vec<String>>" => Some("Texts"),
        _ => None,
    }
}

/// The one line summary of an operation.
fn about(entry: &Bound<'_>) -> String {
    let summary = entry.operation.operation.summary.as_deref().unwrap_or("");
    let summary = summary.lines().next().unwrap_or("").trim();
    if !summary.is_empty() {
        return summary.to_owned();
    }
    let description = entry
        .operation
        .operation
        .description
        .as_deref()
        .unwrap_or("")
        .lines()
        .next()
        .unwrap_or("")
        .trim();
    description.to_owned()
}

/// The long help of an operation, empty when it repeats the summary.
fn long_about(entry: &Bound<'_>) -> String {
    let description = entry
        .operation
        .operation
        .description
        .as_deref()
        .unwrap_or("")
        .trim();
    if description == about(entry) {
        String::new()
    } else {
        description.to_owned()
    }
}

/// The description of a parameter, empty when the API has none.
fn help_of(entry: &Bound<'_>, crate_name: &str) -> String {
    entry
        .operation
        .parameters
        .iter()
        .find(|parameter| rust_name(&parameter.name) == crate_name)
        .and_then(|parameter| parameter.description.clone())
        .map(|description| description.trim().to_owned())
        .unwrap_or_default()
}

/// Why generation failed.
#[derive(Debug)]
pub enum EmitError {
    /// A path does not follow the naming rules.
    Naming { path: String, detail: String },
    /// The crate has no method for an operation.
    MissingMethod { path: String, method: Method },
    /// The crate method has no parameter the description mentions.
    MissingParameter { path: String, parameter: String },
    /// The crate method has a parameter the description does not mention.
    UnmatchedParameter { path: String, parameter: String },
    /// A parameter type the emitter does not know how to map.
    UnsupportedType {
        path: String,
        parameter: String,
        ty: String,
    },
    /// Commands that share a clap path even with their parameters spelled out.
    Collisions(Vec<String>),
    /// A selected tag the description does not have.
    UnknownTag { tag: String, known: Vec<String> },
    /// A generated file could not be written.
    Write {
        path: PathBuf,
        error: std::io::Error,
    },
}

impl std::fmt::Display for EmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EmitError::Naming { path, detail } => write!(f, "{path}: {detail}"),
            EmitError::MissingMethod { path, method } => {
                write!(f, "{path}: the crate has no method for {method}")
            }
            EmitError::MissingParameter { path, parameter } => {
                write!(f, "{path}: the crate method has no parameter {parameter}")
            }
            EmitError::UnmatchedParameter { path, parameter } => {
                write!(f, "{path}: {parameter} is not a parameter of the operation")
            }
            EmitError::UnsupportedType {
                path,
                parameter,
                ty,
            } => write!(f, "{path}: cannot map the type {ty} of {parameter}"),
            EmitError::Collisions(collisions) => {
                for collision in collisions {
                    writeln!(f, "{collision}")?;
                }
                Ok(())
            }
            EmitError::UnknownTag { tag, known } => {
                write!(f, "unknown tag {tag}; known tags: {}", known.join(", "))
            }
            EmitError::Write { path, error } => {
                write!(f, "cannot write {}: {error}", path.display())
            }
        }
    }
}

impl std::error::Error for EmitError {}

/// Files the generator writes for one module, for the summary line.
pub fn files(modules: &[Module]) -> BTreeMap<String, usize> {
    modules
        .iter()
        .map(|module| (module.file.clone(), module.source.lines().count()))
        .collect()
}
