//! Live completion values, read from the API with this terminal's session.
//!
//! Each source answers one parameter of the generated tree: realms, users, groups
//! and clients. They never prompt for a password — completion runs in a subprocess
//! the shell spawned, so a source without a session simply has nothing to offer
//! (Q6: a new terminal offers no candidates until it logs in).
//!
//! The listing endpoints are paginated and default to 100 results (F15), so the
//! requests ask for at most that many and the registry's cache and timeout do the
//! rest. Values are what the API accepts in the corresponding path parameter:
//! realm names, user names (Keycloak accepts a user name where it accepts an id),
//! and the internal ids of groups and clients — the client path parameter is
//! explicitly the id, not the `clientId` (see the help of `clients get`).

use std::sync::Arc;

use crate::live::Source;

#[cfg(any(
    feature = "tag-clients",
    feature = "tag-groups",
    feature = "tag-realms-admin",
    feature = "tag-users"
))]
use std::future::Future;

#[cfg(any(
    feature = "tag-clients",
    feature = "tag-groups",
    feature = "tag-realms-admin",
    feature = "tag-users"
))]
use keycloak::prelude::reqwest;

#[cfg(any(
    feature = "tag-clients",
    feature = "tag-groups",
    feature = "tag-realms-admin",
    feature = "tag-users"
))]
use keycloak::KeycloakAdmin;

#[cfg(any(
    feature = "tag-clients",
    feature = "tag-groups",
    feature = "tag-realms-admin",
    feature = "tag-users"
))]
use crate::error::Error;

#[cfg(any(
    feature = "tag-clients",
    feature = "tag-groups",
    feature = "tag-realms-admin",
    feature = "tag-users"
))]
use crate::session::{self, StoredToken};

/// How many values a listing asks for.
#[cfg(any(feature = "tag-clients", feature = "tag-groups", feature = "tag-users"))]
const PAGE: i32 = 100;

/// The parameters that have a live source, in command order.
///
/// A source exists only when the build enabled the tag whose method it calls, so
/// `--no-default-features --features tag-users` still compiles and completes the
/// user names it can actually list.
// Each entry is behind the feature that provides the method it calls, which is
// why the list is built by pushes instead of a literal.
#[allow(clippy::vec_init_then_push)]
pub fn sources() -> Vec<(&'static str, Arc<dyn Source>)> {
    #[allow(unused_mut)]
    let mut sources: Vec<(&'static str, Arc<dyn Source>)> = Vec::new();
    #[cfg(feature = "tag-realms-admin")]
    sources.push(("realm", Arc::new(Realms) as Arc<dyn Source>));
    #[cfg(feature = "tag-users")]
    sources.push(("user_id", Arc::new(Users) as Arc<dyn Source>));
    #[cfg(feature = "tag-groups")]
    sources.push(("group_id", Arc::new(Groups) as Arc<dyn Source>));
    #[cfg(feature = "tag-clients")]
    sources.push(("client_uuid", Arc::new(Clients) as Arc<dyn Source>));
    sources
}

/// The admin client of this terminal's session.
#[cfg(any(
    feature = "tag-clients",
    feature = "tag-groups",
    feature = "tag-realms-admin",
    feature = "tag-users"
))]
fn admin() -> Result<(KeycloakAdmin<StoredToken>, String), Error> {
    let stored = session::lookup()?.ok_or_else(|| {
        Error::usage("no session in this terminal: run keycloak-cli login for live completion")
    })?;
    let client = reqwest::Client::new();
    let admin = KeycloakAdmin::new(&stored.server, StoredToken::new(stored.token), client);
    Ok((admin, stored.realm))
}

/// Runs `future` on a runtime of its own (completion has none).
#[cfg(any(
    feature = "tag-clients",
    feature = "tag-groups",
    feature = "tag-realms-admin",
    feature = "tag-users"
))]
fn block_on<T>(future: impl Future<Output = T>) -> Result<T, Error> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Error::Io)?;
    Ok(runtime.block_on(future))
}

/// Realm names, from `GET /admin/realms`.
#[cfg(feature = "tag-realms-admin")]
struct Realms;

#[cfg(feature = "tag-realms-admin")]
impl Source for Realms {
    fn values(&self) -> Result<Vec<String>, Error> {
        let (admin, _) = admin()?;
        let realms = block_on(admin.get(Some(true)))??;
        Ok(realms.into_iter().filter_map(|realm| realm.realm).collect())
    }
}

/// User names of the session's realm, from `GET /admin/realms/{realm}/users`.
#[cfg(feature = "tag-users")]
struct Users;

#[cfg(feature = "tag-users")]
impl Source for Users {
    fn values(&self) -> Result<Vec<String>, Error> {
        let (admin, realm) = admin()?;
        let users = block_on(admin.realm_users_get(
            &realm,
            Some(true),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(0),
            None,
            None,
            None,
            None,
            Some(PAGE),
            None,
            None,
            None,
        ))??;
        Ok(users.into_iter().filter_map(|user| user.username).collect())
    }
}

/// Group ids of the session's realm, from `GET /admin/realms/{realm}/groups`.
#[cfg(feature = "tag-groups")]
struct Groups;

#[cfg(feature = "tag-groups")]
impl Source for Groups {
    fn values(&self) -> Result<Vec<String>, Error> {
        let (admin, realm) = admin()?;
        let groups = block_on(admin.realm_groups_get(
            &realm,
            Some(true),
            None,
            Some(0),
            Some(PAGE),
            None,
            None,
            None,
            None,
        ))??;
        Ok(groups.into_iter().filter_map(|group| group.id).collect())
    }
}

/// Client ids of the session's realm, from `GET /admin/realms/{realm}/clients`.
#[cfg(feature = "tag-clients")]
struct Clients;

#[cfg(feature = "tag-clients")]
impl Source for Clients {
    fn values(&self) -> Result<Vec<String>, Error> {
        let (admin, realm) = admin()?;
        let clients = block_on(admin.realm_clients_get(
            &realm,
            None,
            Some(0),
            Some(PAGE),
            None,
            None,
            None,
        ))??;
        Ok(clients.into_iter().filter_map(|client| client.id).collect())
    }
}

#[cfg(all(test, any(
    feature = "tag-clients",
    feature = "tag-groups",
    feature = "tag-realms-admin",
    feature = "tag-users"
)))]
mod tests {
    use super::*;

    #[test]
    #[cfg(all(
        feature = "tag-clients",
        feature = "tag-groups",
        feature = "tag-realms-admin",
        feature = "tag-users"
    ))]
    fn the_generated_parameters_have_sources() {
        let parameters: Vec<&str> = sources().iter().map(|(parameter, _)| *parameter).collect();
        assert_eq!(parameters, ["realm", "user_id", "group_id", "client_uuid"]);
    }

    #[test]
    #[cfg(any(
        feature = "tag-clients",
        feature = "tag-groups",
        feature = "tag-realms-admin",
        feature = "tag-users"
    ))]
    fn without_a_session_there_is_nothing_to_offer() {
        // The test harness has no session: every source must fail quietly rather
        // than try to prompt or panic.
        let previous = std::env::var(session::SESSION).ok();
        std::env::remove_var(session::SESSION);
        for (parameter, source) in sources() {
            assert!(
                source.values().is_err(),
                "{parameter} must not invent values"
            );
        }
        if let Some(value) = previous {
            std::env::set_var(session::SESSION, value);
        }
    }
}
