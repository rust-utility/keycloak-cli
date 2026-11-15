//! Wrapping the [Keycloak](https://www.keycloak.org) Admin REST API in a command line tool.
//!
//! The command tree is generated, not written by hand: `src/generate/` turns the
//! OpenAPI description of the pinned [`keycloak`](https://crates.io/crates/keycloak)
//! crate into one command per operation (`scripts/generate.sh` does it from the
//! published tarball), and [`command`] builds the clap tree from that data while
//! the generated `dispatch` calls the crate method behind it. Shell completion,
//! the output layer and reports are the remaining scope: see `doc/ROADMAP.md`.
//!
//! # Version scheme
//!
//! `keycloak-cli` follows the same scheme as the
//! [`keycloak`](https://crates.io/crates/keycloak) crate it will be built on: a
//! crate version `x.y.(z * 100 + v)` wraps Keycloak `x.y.z` with the fix release
//! `v`. So `26.8.100` wraps Keycloak `26.8.1`, and `26.7.301` would be the second
//! fix release for Keycloak `26.7.3`. [`version::keycloak_version`] performs that
//! conversion, and `keycloak-cli --version` reports both.

pub mod cli;
pub mod client;
pub mod command;
pub mod completion;
pub mod config;
pub mod error;
pub mod generate;
pub mod generated;
pub mod live;
pub mod live_sources;
pub mod report;
pub mod report_command;
pub mod session;
pub mod update;
pub mod version;

pub use cli::{Cli, Invocation};
pub use config::Config;
pub use error::Error;

/// Runs the parsed command line.
///
/// Runs the command that was asked for: the CLI's own (`info`, `init`, `login`,
/// `logout`, `report`) or one of the generated API commands, which honours
/// `--dry-run`, `--verbose` and `--yes` for destructive operations.
pub fn run(cli: Cli) -> Result<(), Error> {
    match &cli.command {
        // No command, or the CLI's own `info`: describe the configuration.
        None => info(&cli),
        Some(invocation) if invocation.words == ["info"] => info(&cli),
        Some(invocation) if invocation.words == ["init"] => init(invocation),
        Some(invocation) if invocation.words == ["session-id"] => {
            println!("{}", session::id_for_terminal());
            Ok(())
        }
        Some(invocation) if invocation.words == ["report"] => report(&cli, invocation),
        Some(invocation) if invocation.words == ["self-update"] => self_update(invocation),
        Some(invocation) if invocation.words == ["login"] => login(&cli),
        Some(invocation) if invocation.words == ["logout"] => logout(),
        Some(invocation) => run_api(&cli, invocation),
    }
}

/// Replaces this binary with a released one.
fn self_update(invocation: &Invocation) -> Result<(), Error> {
    let options = update::Options {
        check: invocation.args.get_flag("check"),
        version: invocation.args.get_one::<String>("version").cloned(),
        force: invocation.args.get_flag("force"),
    };
    let exe = std::env::current_exe().map_err(Error::Io)?;
    let updater = update::Update::default();
    let outcome =
        client::runtime()?.block_on(updater.run(&exe, version::CRATE_VERSION, &options))?;
    match outcome {
        update::Outcome::UpToDate { version } => {
            println!("keycloak-cli {version} is up to date");
        }
        update::Outcome::Available { current, latest } => {
            println!("keycloak-cli {latest} is available (current {current})");
        }
        update::Outcome::Updated { from, to, path } => {
            println!("updated keycloak-cli {from} -> {to} ({})", path.display());
        }
    }
    Ok(())
}

/// Writes the report document, to a file or to stdout.
fn report(cli: &Cli, invocation: &Invocation) -> Result<(), Error> {
    use std::io::Write;

    let config = Config::from(&cli.connection);
    let document = client::collect_report(&config)?;
    let text = match cli.global.output.as_str() {
        // A report is a document, so `auto` means markdown rather than a table.
        "auto" | "markdown" => document.markdown(),
        "html" => document.html(),
        "json" => document.json()?,
        other => {
            return Err(Error::usage(format!(
                "a report is written as markdown, html or json, not {other:?}"
            )))
        }
    };

    match invocation.args.get_one::<String>("file") {
        Some(path) => {
            std::fs::write(path, text)?;
            if !cli.global.quiet {
                eprintln!("wrote {path}");
            }
        }
        None => {
            let mut stdout = std::io::stdout();
            stdout.write_all(text.as_bytes())?;
            stdout.flush()?;
        }
    }
    Ok(())
}

/// Logs in for this terminal and reports where the token went.
fn login(cli: &Cli) -> Result<(), Error> {
    use std::io::Write;

    let config = Config::from(&cli.connection);
    let (session, path) = client::login(&config)?;
    let rows = [
        ("session", session::current_id().unwrap_or_default()),
        ("server", session.server.clone()),
        ("realm", session.realm.clone()),
        ("user", session.user.clone()),
        (
            "expires",
            format!("in {}s", session.remaining(session::now())),
        ),
        ("state", path.display().to_string()),
    ];
    let width = rows.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
    let mut stdout = std::io::stdout();
    for (name, value) in rows {
        writeln!(stdout, "{name:<width$}  {value}")?;
    }
    stdout.flush()?;
    Ok(())
}

/// Forgets this terminal's token.
fn logout() -> Result<(), Error> {
    use std::io::Write;

    let mut stdout = std::io::stdout();
    match client::logout()? {
        Some(session) => writeln!(
            stdout,
            "logged out of {} (realm {}) as {}",
            session.server, session.realm, session.user
        )?,
        None => writeln!(stdout, "no session in this terminal")?,
    }
    stdout.flush()?;
    Ok(())
}

/// Prints the line that registers completion in a shell.
fn init(invocation: &Invocation) -> Result<(), Error> {
    let name = invocation
        .args
        .get_one::<String>("shell")
        .map(String::as_str)
        .ok_or_else(|| {
            Error::usage(format!(
                "init needs a shell: {}",
                completion::SHELLS.join(", ")
            ))
        })?;
    let shell =
        completion::shell(name).ok_or_else(|| Error::usage(format!("unknown shell {name}")))?;
    let program =
        std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("keycloak-cli"));
    println!("{}", completion::init(shell, &program));
    Ok(())
}

/// Prints the effective configuration; secrets are redacted.
fn info(cli: &Cli) -> Result<(), Error> {
    use std::io::Write;

    let config = Config::from(&cli.connection);
    let mut stdout = std::io::stdout();
    write!(stdout, "{}", config.describe())?;
    stdout.flush()?;
    Ok(())
}

/// Runs a command of the generated tree.
fn run_api(cli: &Cli, invocation: &Invocation) -> Result<(), Error> {
    use std::io::Write;

    let config = Config::from(&cli.connection);
    let spec = generated::spec(&invocation.words());

    if let Some(spec) = spec {
        // Destructive operations say so before they run.
        if spec.method == "DELETE" && !cli.global.dry_run && !invocation.args.get_flag("yes") {
            return Err(Error::usage(format!(
                "{} deletes data: pass --yes to confirm, or --dry-run to see the request",
                invocation.words.join(" ")
            )));
        }
        if cli.global.dry_run || cli.global.verbose {
            let plan = command::plan(spec, &config.server, &config.realm, &invocation.args)?;
            if cli.global.dry_run {
                let mut stdout = std::io::stdout();
                stdout.write_all(plan.render().as_bytes())?;
                stdout.flush()?;
                return Ok(());
            }
            if !cli.global.quiet {
                eprintln!("> {} {}", plan.method, plan.url);
            }
        }
    }

    let value = client::execute(&config, invocation)?;
    write_value(&value, &cli.global)
}

/// Prints a response in the requested format.
fn write_value(value: &serde_json::Value, output: &cli::GlobalArgs) -> Result<(), Error> {
    use std::io::{IsTerminal, Write};

    let format = report::Format::from_name(&output.output).ok_or_else(|| {
        Error::usage(format!(
            "unknown output format {:?}; use one of: {}",
            output.output,
            report::Format::NAMES.join(", ")
        ))
    })?;
    let text = report::render(
        value,
        &output.fields,
        format,
        std::io::stdout().is_terminal(),
    )?;
    let mut stdout = std::io::stdout();
    stdout.write_all(text.as_bytes())?;
    stdout.flush()?;
    Ok(())
}
