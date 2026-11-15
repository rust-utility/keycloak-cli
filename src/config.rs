//! The effective connection configuration.
//!
//! Values are resolved from command line flags and `KEYCLOAK_*` environment
//! variables (flags win). Nothing touches the network yet.

use std::fmt;

use crate::cli::ConnectionArgs;
use crate::version;

/// Where and how to reach Keycloak, after flag and environment resolution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Base URL of the Keycloak server, e.g. `http://localhost:8080`.
    pub server: String,
    /// Realm to operate on.
    pub realm: String,
    /// Administrator user name.
    pub user: String,
    /// Administrator password, when one was provided.
    pub password: Option<String>,
}

impl Config {
    /// Whether a password was supplied, by flag or environment.
    pub fn has_password(&self) -> bool {
        self.password.is_some()
    }

    /// A copy safe to print or log: the password is replaced by `***`.
    pub fn redacted(&self) -> Self {
        Self {
            password: self.password.as_ref().map(|_| "***".to_owned()),
            ..self.clone()
        }
    }

    /// The report printed by `keycloak-cli info`.
    pub fn describe(&self) -> String {
        let version = format!(
            "{} (wraps Keycloak Admin REST API {})",
            version::CRATE_VERSION,
            version::keycloak_version()
        );
        let password = if self.has_password() {
            "set (hidden)"
        } else {
            "not set"
        };

        let rows = [
            ("version", version.as_str()),
            ("server", self.server.as_str()),
            ("realm", self.realm.as_str()),
            ("user", self.user.as_str()),
            ("password", password),
        ];
        let width = rows.iter().map(|(name, _)| name.len()).max().unwrap_or(0);

        let mut report = String::new();
        for (name, value) in rows {
            report.push_str(&format!("{name:<width$}  {value}\n"));
        }
        report
    }
}

impl From<&ConnectionArgs> for Config {
    fn from(args: &ConnectionArgs) -> Self {
        Self {
            server: args.server.clone(),
            realm: args.realm.clone(),
            user: args.user.clone(),
            password: args.password.clone(),
        }
    }
}

impl fmt::Display for Config {
    /// Prints the redacted form.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let redacted = self.redacted();
        write!(
            f,
            "{} realm {:?} as {:?} (password {})",
            redacted.server,
            redacted.realm,
            redacted.user,
            redacted.password.as_deref().unwrap_or("not set")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        Config {
            server: "http://keycloak.example.com".to_owned(),
            realm: "master".to_owned(),
            user: "admin".to_owned(),
            password: Some("s3cret".to_owned()),
        }
    }

    #[test]
    fn describe_never_prints_the_password() {
        let report = config().describe();
        assert!(!report.contains("s3cret"), "{report}");
        assert!(report.contains("set (hidden)"), "{report}");
    }

    #[test]
    fn display_never_prints_the_password() {
        let displayed = config().to_string();
        assert!(!displayed.contains("s3cret"), "{displayed}");
        assert!(displayed.contains("***"), "{displayed}");
    }

    #[test]
    fn redacted_keeps_everything_else() {
        let config = config();
        let redacted = config.redacted();
        assert_eq!(redacted.password.as_deref(), Some("***"));
        assert_eq!(redacted.server, config.server);
        assert_eq!(redacted.realm, config.realm);
        assert_eq!(redacted.user, config.user);
    }

    #[test]
    fn missing_password_is_reported() {
        let mut config = config();
        config.password = None;
        assert!(!config.has_password());
        assert!(config.describe().contains("not set"));
    }
}
