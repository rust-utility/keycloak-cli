//! Command line shape.
//!
//! Only the connection flags and a single `info` command exist so far; the
//! resource commands listed in `doc/ROADMAP.md` are added on top of this tree.

use clap::{Args, Parser, Subcommand};

use crate::version;

/// Command line interface for the Keycloak Admin REST API.
#[derive(Debug, Parser)]
#[command(
    name = "keycloak-cli",
    about = "Command-line interface for the Keycloak Admin REST API",
    version = version::version_string(),
    propagate_version = true
)]
pub struct Cli {
    /// Connection settings, shared by every command.
    #[command(flatten)]
    pub connection: ConnectionArgs,

    #[command(subcommand)]
    pub command: Option<Command>,
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

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Show the effective connection configuration (secrets redacted).
    Info,
}
