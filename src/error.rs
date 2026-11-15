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
}

impl Error {
    /// A usage error, i.e. the caller has to change the command line.
    pub fn usage(message: impl Into<String>) -> Self {
        Error::Usage(message.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(error) => write!(f, "i/o error: {error}"),
            Error::Api(error) => write!(f, "keycloak error: {error}"),
            Error::Json(error) => write!(f, "invalid json: {error}"),
            Error::Usage(message) => write!(f, "{message}"),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Error::Io(error) => Some(error),
            Error::Api(error) => Some(error),
            Error::Json(error) => Some(error),
            Error::Usage(_) => None,
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
    fn json_errors_are_reported_as_such() {
        let error = Error::from(serde_json::from_str::<serde_json::Value>("{").unwrap_err());
        assert!(error.to_string().starts_with("invalid json"), "{error}");
    }
}
