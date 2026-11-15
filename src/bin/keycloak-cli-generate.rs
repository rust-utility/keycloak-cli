//! Generator for the command tree of `keycloak-cli`.
//!
//! This binary is internal: `scripts/generate.sh` runs it against the
//! `api/openapi.json` of the pinned `keycloak` crate and writes what it emits into
//! `src/generated/`. It is part of the package so that generation needs neither a
//! second crate nor a checkout of the `keycloak` repository, but nothing in the
//! CLI calls it at runtime.
//!
//! `names` is the self-check: it derives a command for every operation, makes the
//! names unique and fails if the rules cannot cover the description.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use keycloak_cli::generate::emit;
use keycloak_cli::generate::naming::{self, CommandName};
use keycloak_cli::generate::signature::CrateApi;
use keycloak_cli::generate::spec::Spec;

/// Generates the command tree of `keycloak-cli` from the Admin REST API description.
#[derive(Debug, Parser)]
#[command(name = "keycloak-cli-generate", version, about, long_about = None)]
struct Cli {
    /// The `api/openapi.json` of the pinned `keycloak` crate.
    #[arg(long, value_name = "FILE")]
    spec: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print the command of every operation, failing if two would collide.
    Names,
    /// Write the generated command modules.
    Emit {
        /// The unpacked `keycloak` crate root, which has `src/rest/generated_rest`.
        #[arg(long, value_name = "DIR")]
        crate_dir: PathBuf,
        /// Directory to write `mod.rs` and the tag modules into.
        #[arg(long, value_name = "DIR")]
        out: PathBuf,
        /// Version of the crate the sources come from, for the file headers.
        #[arg(long, value_name = "VERSION")]
        version: String,
        /// Tags to emit (kebab-case), every tag when empty.
        #[arg(long, value_name = "TAG", value_delimiter = ',')]
        tags: Vec<String>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<(), String> {
    let bytes = std::fs::read(&cli.spec)
        .map_err(|error| format!("cannot read {}: {error}", cli.spec.display()))?;
    let spec = Spec::from_json(&bytes).map_err(|error| {
        format!(
            "{} is not an OpenAPI description: {error}",
            cli.spec.display()
        )
    })?;

    match &cli.command {
        Command::Names => names(&spec),
        Command::Emit {
            crate_dir,
            out,
            version,
            tags,
        } => emit(&spec, crate_dir, out, version, tags),
    }
}

/// Emits the command modules of `tags` into `out`.
fn emit(
    spec: &Spec,
    crate_dir: &std::path::Path,
    out: &std::path::Path,
    version: &str,
    tags: &[String],
) -> Result<(), String> {
    let rest = crate_dir.join("src").join("rest").join("generated_rest");
    let api = CrateApi::read(&rest).map_err(|error| error.to_string())?;
    let generated = emit::generate(spec, &api, version, tags).map_err(|error| error.to_string())?;
    generated.write(out).map_err(|error| error.to_string())?;
    eprintln!(
        "{} operations, {} commands ({} spelled with their parameters), {} crate methods, {} modules",
        generated.operations,
        generated.commands,
        generated.shaped,
        api.len(),
        generated.modules.len()
    );
    Ok(())
}

/// Derives every command, makes the words unique and prints the tree.
fn names(spec: &Spec) -> Result<(), String> {
    let operations = spec.operations();
    let mut commands = Vec::with_capacity(operations.len());
    for operation in &operations {
        let command = CommandName::new(operation.path, operation.method)
            .map_err(|error| error.to_string())?;
        commands.push(command);
    }

    naming::make_unique(&mut commands).map_err(|collisions| {
        collisions
            .iter()
            .map(|collision| collision.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    })?;

    let shaped = commands
        .iter()
        .filter(|command| command.is_shaped())
        .count();
    let mut sorted: Vec<&CommandName> = commands.iter().collect();
    sorted.sort_by_key(|command| command.signature());

    let mut report = String::new();
    for command in &sorted {
        writeln!(
            report,
            "{}\t{} {}",
            command.usage(),
            command.method(),
            command.path()
        )
        .expect("writing to a string cannot fail");
    }
    print!("{report}");

    let mut groups: Vec<&str> = commands.iter().map(CommandName::group).collect();
    groups.sort_unstable();
    groups.dedup();
    eprintln!(
        "{} operations, {} commands in {} groups, {} spelled with their parameters",
        operations.len(),
        commands.len(),
        groups.len(),
        shaped
    );
    Ok(())
}
