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
pub mod session;
pub mod version;

pub use cli::{Cli, Invocation};
pub use config::Config;
pub use error::Error;

/// Runs the parsed command line.
///
/// The skeleton only knows [`cli::Command::Info`], which echoes the effective
/// connection configuration with secrets redacted.
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
        Some(invocation) if invocation.words == ["login"] => login(&cli),
        Some(invocation) if invocation.words == ["logout"] => logout(),
        Some(invocation) => {
            let config = Config::from(&cli.connection);
            let value = client::execute(&config, invocation)?;
            write_value(&value)
        }
    }
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

/// Prints a response. The output layer of Q8 replaces this with `--output`.
fn write_value(value: &serde_json::Value) -> Result<(), Error> {
    use std::io::Write;

    let mut stdout = std::io::stdout();
    if !value.is_null() {
        serde_json::to_writer_pretty(&mut stdout, value)?;
        writeln!(stdout)?;
    }
    stdout.flush()?;
    Ok(())
}
