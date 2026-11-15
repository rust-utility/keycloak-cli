//! Errors returned by the command implementations.
//!
//! The variants map to exit codes in `main`: a usage error is the caller's
//! mistake, everything else is a failure of the request or of the environment.

use std::error::Error as StdError;
use std::fmt;

/// Failure of a command.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// Writing the result to stdout or reading a file failed.
    Io(std::io::Error),
    /// The `keycloak` crate rejected the request or its response.
    Api(keycloak::KeycloakError),
    /// A response or a `--body` payload was not the expected JSON.
    Json(serde_json::Error),
    /// The command line cannot be satisfied, e.g. a missing `--body`.
    Usage(String),
    /// A response could not be rendered in the requested format.
    Format(String),
}

impl Error {
    /// A usage error, i.e. the caller has to change the command line.
    pub fn usage(message: impl Into<String>) -> Self {
        Error::Usage(message.into())
    }

    /// The exit code for this failure.
    ///
    /// `0` is success, `1` a failure of the request or the environment, `2` a
    /// command line the CLI cannot satisfy, `3` a rejected credential and `4` a
    /// resource the server does not have, so a script can tell "you asked wrong"
    /// from "the server said no" from "the server is down".
    pub fn exit_code(&self) -> u8 {
        match self {
            Error::Usage(_) => 2,
            Error::Api(keycloak::KeycloakError::HttpFailure { status, .. }) => match status {
                401 | 403 => 3,
                404 => 4,
                _ => 1,
            },
            Error::Io(_) | Error::Json(_) | Error::Api(_) | Error::Format(_) => 1,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(error) => write!(f, "i/o error: {error}"),
            Error::Api(error) => write!(f, "keycloak error: {error}"),
            Error::Json(error) => write!(f, "invalid json: {error}"),
            Error::Usage(message) => write!(f, "{message}"),
            Error::Format(message) => write!(f, "cannot render the response: {message}"),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Error::Io(error) => Some(error),
            Error::Api(error) => Some(error),
            Error::Json(error) => Some(error),
            Error::Usage(_) | Error::Format(_) => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Error::Io(error)
    }
}

impl From<keycloak::KeycloakError> for Error {
    fn from(error: keycloak::KeycloakError) -> Self {
        Error::Api(error)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Error::Json(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_errors_explain_themselves() {
        let error = Error::usage("--body is required");
        assert_eq!(error.to_string(), "--body is required");
        assert!(error.source().is_none());
    }

    #[test]
    fn io_errors_keep_their_source() {
        let error = Error::from(std::io::Error::new(std::io::ErrorKind::NotFound, "gone"));
        assert!(error.to_string().contains("gone"));
        assert!(error.source().is_some());
    }

    #[test]
    fn a_format_error_is_a_failure() {
        let error = Error::Format("no such template".to_owned());
        assert!(error.to_string().contains("cannot render"), "{error}");
        assert_eq!(error.exit_code(), 1);
    }

    #[test]
    fn exit_codes_separate_usage_from_failure() {
        assert_eq!(Error::usage("x").exit_code(), 2);
        let http = |status| {
            Error::Api(keycloak::KeycloakError::HttpFailure {
                status,
                body: None,
                text: String::new(),
            })
        };
        assert_eq!(http(401).exit_code(), 3);
        assert_eq!(http(403).exit_code(), 3);
        assert_eq!(http(404).exit_code(), 4);
        assert_eq!(http(500).exit_code(), 1);
        assert_eq!(
            Error::from(std::io::Error::new(std::io::ErrorKind::NotFound, "gone")).exit_code(),
            1
        );
    }

    #[test]
    fn json_errors_are_reported_as_such() {
        let error = Error::from(serde_json::from_str::<serde_json::Value>("{").unwrap_err());
        assert!(error.to_string().starts_with("invalid json"), "{error}");
    }
}
