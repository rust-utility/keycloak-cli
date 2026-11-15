//! Talking to Keycloak.
//!
//! One invocation is one request: a current-thread runtime, an admin token from
//! the existing connection flags, and the generated dispatcher for the command
//! that was asked for. Nothing is cached between invocations yet; the completion
//! work (Q6/Q7) is where a session takes over the token source.

use keycloak::prelude::reqwest;
use keycloak::{KeycloakAdmin, KeycloakAdminToken};

use crate::cli::Invocation;
use crate::config::Config;
use crate::error::Error;

/// Runs the generated command `invocation` asks for and returns its response.
pub fn execute(config: &Config, invocation: &Invocation) -> Result<serde_json::Value, Error> {
    let words = invocation.words();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Error::Io)?;
    runtime.block_on(async {
        let admin = connect(config).await?;
        crate::generated::dispatch(&admin, &config.realm, &words, &invocation.args).await
    })
}

/// Authenticates with the configured user and builds the admin client.
///
/// The token is acquired the same way `keycloak`'s own examples do it, from the
/// password grant; a missing password is a usage error rather than a failed
/// request, so the message says what to set.
pub async fn connect(config: &Config) -> Result<KeycloakAdmin, Error> {
    let password = config.password.as_deref().ok_or_else(|| {
        Error::usage("a password is required: pass --password or set KEYCLOAK_PASSWORD")
    })?;
    let client = reqwest::Client::new();
    let token =
        KeycloakAdminToken::acquire(&config.server, &config.user, password, &client).await?;
    Ok(KeycloakAdmin::new(&config.server, token, client))
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
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime builds");
        let error = match runtime.block_on(connect(&config)) {
            Ok(_) => panic!("a connection without a password must fail"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::Usage(_)), "{error}");
        assert!(error.to_string().contains("KEYCLOAK_PASSWORD"), "{error}");
    }
}
