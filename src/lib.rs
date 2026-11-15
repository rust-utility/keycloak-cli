//! Wrapping the [Keycloak](https://www.keycloak.org) Admin REST API in a command line tool.
//!
//! The crate is at the skeleton stage: it provides the command line shape, the
//! connection configuration and the version scheme, but no API calls yet. See
//! `doc/ROADMAP.md` in the repository for the planned scope.
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
pub mod config;
pub mod error;
pub mod generate;
pub mod generated;
pub mod version;

pub use cli::Cli;
pub use config::Config;
pub use error::Error;

/// Runs the parsed command line.
///
/// The skeleton only knows [`cli::Command::Info`], which echoes the effective
/// connection configuration with secrets redacted.
pub fn run(cli: Cli) -> Result<(), Error> {
    use std::io::Write;

    match cli.command.unwrap_or(cli::Command::Info) {
        cli::Command::Info => {
            let config = Config::from(&cli.connection);
            let mut stdout = std::io::stdout();
            write!(stdout, "{}", config.describe())?;
            stdout.flush()?;
            Ok(())
        }
    }
}
