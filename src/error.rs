//! Errors returned by the command implementations.

use std::error::Error as StdError;
use std::fmt;

/// Failure of a command.
///
/// Only I/O can fail in the skeleton (writing the report); the variants grow
/// with the commands in `doc/ROADMAP.md`.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// Writing the result to stdout or reading a file failed.
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(error) => write!(f, "i/o error: {error}"),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Error::Io(error) => Some(error),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Error::Io(error)
    }
}
