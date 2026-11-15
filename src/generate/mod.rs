//! Generation of the command tree from the pinned `keycloak` crate.
//!
//! The command tree is not written by hand: it is derived from the OpenAPI
//! description that ships in the `keycloak` crate tarball, so every operation of
//! the wrapped version has exactly one command and the two cannot drift apart.
//!
//! [`naming`] turns an OpenAPI path and HTTP method into a command name, [`spec`]
//! reads the subset of the description the generator needs. The emitting code
//! lives in the `keycloak-cli-generate` binary, which `scripts/generate.sh` runs
//! against the unpacked crate.
//!
//! Generating inside this crate (rather than from a `cli` subcommand of the
//! `keycloak` crate) is deliberate: the pinned release `26.8.100` is the newest
//! published one, so a generator change upstream could not be used by
//! `scripts/generate.sh` until a new crate release exists, while the script has
//! to reproduce the committed sources from a *published* crate.

pub mod naming;
pub mod spec;

pub use naming::{Action, CommandName, Method, ParamName};
pub use spec::{ApiOperation, Spec};
