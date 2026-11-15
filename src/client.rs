//! Talking to Keycloak.
//!
//! One invocation is one request: a current-thread runtime, an admin client and
//! the generated dispatcher for the command that was asked for. The client's token
//! comes from this terminal's session when there is one (see [`crate::session`]),
//! otherwise from the password grant with the connection flags; `login` is what
//! creates the session, so the password is only typed once per terminal.

use std::path::PathBuf;

use keycloak::prelude::reqwest;
use keycloak::{KeycloakAdmin, KeycloakAdminToken};

use crate::cli::Invocation;
use crate::config::Config;
use crate::error::Error;
use crate::session::{self, Session, StoredToken};

/// A connected admin client, with the token source it ended up using.
pub enum Admin {
    /// The token this terminal logged in with.
    Session(KeycloakAdmin<StoredToken>),
    /// A token acquired from the password grant just now.
    Password(KeycloakAdmin<KeycloakAdminToken>),
}

/// Runs the generated command `invocation` asks for and returns its response.
pub fn execute(config: &Config, invocation: &Invocation) -> Result<serde_json::Value, Error> {
    let words = invocation.words();
    runtime()?.block_on(async {
        match connect(config).await? {
            Admin::Session(admin) => {
                crate::generated::dispatch(&admin, &config.realm, &words, &invocation.args).await
            }
            Admin::Password(admin) => {
                crate::generated::dispatch(&admin, &config.realm, &words, &invocation.args).await
            }
        }
    })
}

/// Builds the admin client: the terminal's session first, the password grant
/// second.
pub async fn connect(config: &Config) -> Result<Admin, Error> {
    let client = reqwest::Client::new();
    if let Some(stored) = session::lookup()? {
        return Ok(Admin::Session(KeycloakAdmin::new(
            &config.server,
            StoredToken::new(stored.token),
            client,
        )));
    }
    let password = config.password.as_deref().ok_or_else(|| {
        let hint = if session::current_id().is_some() {
            " or log in to this terminal with `keycloak-cli login`"
        } else {
            ""
        };
        Error::usage(format!(
            "a password is required: pass --password or set KEYCLOAK_PASSWORD{hint}"
        ))
    })?;
    let token =
        KeycloakAdminToken::acquire(&config.server, &config.user, password, &client).await?;
    Ok(Admin::Password(KeycloakAdmin::new(
        &config.server,
        token,
        client,
    )))
}

/// Logs in for this terminal and stores the token.
///
/// Returns the stored session and the state file it was written to.
pub fn login(config: &Config) -> Result<(Session, PathBuf), Error> {
    let password = config.password.as_deref().ok_or_else(|| {
        Error::usage("a password is required: pass --password or set KEYCLOAK_PASSWORD")
    })?;
    runtime()?.block_on(async {
        let client = reqwest::Client::new();
        let token =
            KeycloakAdminToken::acquire(&config.server, &config.user, password, &client).await?;
        let created_at = session::now();
        let stored = Session {
            tty: session::current_tty(),
            server: config.server.clone(),
            realm: config.realm.clone(),
            user: config.user.clone(),
            token: token.access_token().to_owned(),
            expires_at: created_at + token.expires_in() as u64,
            created_at,
        };
        let path = session::store(stored.clone())?;
        Ok((stored, path))
    })
}

/// Forgets this terminal's session.
pub fn logout() -> Result<Option<Session>, Error> {
    session::remove()
}

/// The runtime every command runs on.
fn runtime() -> Result<tokio::runtime::Runtime, Error> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Error::Io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_password_is_a_usage_error() {
        let config = Config {
            server: "http://127.0.0.1:1".to_owned(),
            realm: "master".to_owned(),
            user: "admin".to_owned(),
            password: None,
        };
        let error = match runtime()
            .expect("a runtime builds")
            .block_on(connect(&config))
        {
            Ok(_) => panic!("a connection without a password must fail"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::Usage(_)), "{error}");
        assert!(error.to_string().contains("KEYCLOAK_PASSWORD"), "{error}");
    }
}
