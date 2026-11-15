//! Terminal sessions: one `login` per shell, shared with completion.
//!
//! A shell that evaluates `keycloak-cli init <shell>` exports
//! `KEYCLOAK_CLI_SESSION`, an id that identifies that terminal. `login` stores
//! the admin token under that id (plus the tty device, when one can be detected)
//! and `logout` removes it; later commands and every completion subprocess in the
//! same terminal find the token in the state file, so only `login` ever needs the
//! password (F11, F16).
//!
//! The state file is `$XDG_STATE_HOME/keycloak-cli/sessions.json`, created with
//! mode 0600 in a 0700 directory, because it holds bearer tokens.
//!
//! Ids and paths can be overridden for tests and sandboxes:
//! `KEYCLOAK_CLI_SESSION`, `KEYCLOAK_CLI_STATE_DIR` and `KEYCLOAK_CLI_TTY`.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use keycloak::KeycloakTokenSupplier;
use serde::{Deserialize, Serialize};

use crate::error::Error;

/// Environment variable that names the terminal's session (exported by `init`).
pub const SESSION: &str = "KEYCLOAK_CLI_SESSION";

/// Environment variable that overrides the state directory.
pub const STATE_DIR: &str = "KEYCLOAK_CLI_STATE_DIR";

/// Environment variable that overrides tty detection.
pub const TTY: &str = "KEYCLOAK_CLI_TTY";

/// A logged-in terminal.
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Session {
    /// The terminal device, when one could be detected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tty: Option<String>,
    /// Base URL the token was issued for.
    pub server: String,
    /// Realm the token was issued for.
    pub realm: String,
    /// Administrator the token belongs to.
    pub user: String,
    /// The bearer token; never printed.
    pub token: String,
    /// Unix seconds at which the token stops being valid.
    pub expires_at: u64,
    /// Unix seconds at which `login` ran.
    pub created_at: u64,
}

impl Session {
    /// Whether the token has expired by `now`.
    pub fn expired(&self, now: u64) -> bool {
        now >= self.expires_at
    }

    /// The seconds left before the token expires (zero once expired).
    pub fn remaining(&self, now: u64) -> u64 {
        self.expires_at.saturating_sub(now)
    }
}

impl fmt::Debug for Session {
    /// Keeps the token out of logs and test failures.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("tty", &self.tty)
            .field("server", &self.server)
            .field("realm", &self.realm)
            .field("user", &self.user)
            .field("token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("created_at", &self.created_at)
            .finish()
    }
}

/// The on-disk shape of the state file.
#[derive(Debug, Default, Deserialize, Serialize)]
struct State {
    #[serde(default)]
    sessions: BTreeMap<String, Session>,
}

/// Unix seconds now.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// The id of the session this process runs in, if the shell exported one.
pub fn current_id() -> Option<String> {
    std::env::var(SESSION)
        .ok()
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty())
}

/// The tty device of this process, when it has one.
///
/// `login` is typed in a terminal, so this is normally available; a completion
/// subprocess has only a pipe, which is why the id (not the tty) is what
/// identifies a session.
pub fn current_tty() -> Option<String> {
    if let Ok(tty) = std::env::var(TTY) {
        let tty = tty.trim().to_owned();
        return (!tty.is_empty()).then_some(tty);
    }
    // `/dev/fd/0` resolves to the terminal on both macOS and Linux.
    let path = std::fs::read_link("/dev/fd/0").ok()?;
    let path = path.display().to_string();
    let looks_like_a_terminal = path.starts_with("/dev/tty")
        || path.starts_with("/dev/pts/")
        || path.starts_with("/dev/console");
    looks_like_a_terminal.then_some(path)
}

/// The directory the state file lives in.
pub fn state_dir() -> Result<PathBuf, Error> {
    if let Some(dir) = std::env::var_os(STATE_DIR) {
        return Ok(PathBuf::from(dir));
    }
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("state"))
        });
    base.map(|base| base.join("keycloak-cli")).ok_or_else(|| {
        Error::usage(format!(
            "cannot find a state directory: set {STATE_DIR}, XDG_STATE_HOME or HOME"
        ))
    })
}

/// The state file path.
pub fn state_path() -> Result<PathBuf, Error> {
    Ok(state_dir()?.join("sessions.json"))
}

/// Reads the state file; a missing file is an empty state.
fn load() -> Result<State, Error> {
    let path = state_path()?;
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(State::default()),
        Err(error) => return Err(Error::Io(error)),
    };
    if text.trim().is_empty() {
        return Ok(State::default());
    }
    serde_json::from_str(&text).map_err(Error::Json)
}

/// Writes the state file, readable only by its owner.
fn save(state: &State) -> Result<PathBuf, Error> {
    let dir = state_dir()?;
    let path = dir.join("sessions.json");
    std::fs::create_dir_all(&dir)?;
    restrict(&dir, 0o700)?;

    let text = serde_json::to_string_pretty(state)?;
    write_private(&path, &text)?;
    Ok(path)
}

/// Creates or truncates `path` with mode 0600 (and fixes an existing file).
fn write_private(path: &Path, contents: &str) -> Result<(), Error> {
    use std::io::Write as _;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(contents.as_bytes())?;
    file.flush()?;
    drop(file);
    restrict(path, 0o600)?;
    Ok(())
}

/// Narrows the permissions of `path`, where the platform has them.
fn restrict(path: &Path, mode: u32) -> Result<(), Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)?.permissions();
        if permissions.mode() & 0o777 != mode {
            permissions.set_mode(mode);
            std::fs::set_permissions(path, permissions)?;
        }
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
    Ok(())
}

/// The session of this terminal, when it exists and has not expired.
pub fn lookup() -> Result<Option<Session>, Error> {
    let Some(id) = current_id() else {
        return Ok(None);
    };
    let state = load()?;
    Ok(state
        .sessions
        .get(&id)
        .filter(|session| !session.expired(now()))
        .cloned())
}

/// Stores `session` under this terminal's id, dropping expired ones.
pub fn store(session: Session) -> Result<PathBuf, Error> {
    let Some(id) = current_id() else {
        return Err(Error::usage(format!(
            "no terminal session: run `eval \"$(keycloak-cli init bash)\"` (or init zsh|fish) before login, or set {SESSION}"
        )));
    };
    let mut state = load()?;
    let now = now();
    state.sessions.retain(|_, session| !session.expired(now));
    state.sessions.insert(id, session);
    save(&state)
}

/// Removes this terminal's session and returns it, when there was one.
pub fn remove() -> Result<Option<Session>, Error> {
    let Some(id) = current_id() else {
        return Ok(None);
    };
    let mut state = load()?;
    let removed = state.sessions.remove(&id);
    if removed.is_some() {
        save(&state)?;
    }
    Ok(removed)
}

/// The id this terminal's session should use, derived from its tty.
///
/// `init` runs `keycloak-cli session-id` when the shell starts and exports the
/// answer, so `login` and every completion subprocess in that terminal agree on
/// it. Two shells that share a terminal share the id (and the session), which is
/// the same trade-off `sudo` makes with its tty-keyed timestamps (F16); a process
/// without a terminal gets an id of its own.
pub fn id_for_terminal() -> String {
    if let Some(tty) = current_tty() {
        return format!("tty:{tty}");
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    format!("pid:{}-{nanos}", std::process::id())
}

/// The token of a stored session, as a token supplier for the admin client.
pub struct StoredToken {
    token: String,
}

impl StoredToken {
    /// Uses `token` for every request, without asking Keycloak for a new one.
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
        }
    }
}

#[async_trait::async_trait]
impl KeycloakTokenSupplier for StoredToken {
    async fn get(&self, _url: &str) -> Result<String, keycloak::KeycloakError> {
        Ok(self.token.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// The environment is process-wide, so the tests that touch it take turns.
    static ENV: Mutex<()> = Mutex::new(());

    fn exclusive() -> std::sync::MutexGuard<'static, ()> {
        ENV.lock().unwrap_or_else(|poison| poison.into_inner())
    }

    /// A test state directory that is removed afterwards.
    struct Sandbox {
        dir: PathBuf,
    }

    impl Sandbox {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "keycloak-cli-session-{name}-{}-{}",
                std::process::id(),
                now()
            ));
            std::fs::create_dir_all(&dir).expect("the sandbox exists");
            std::env::set_var(STATE_DIR, &dir);
            Self { dir }
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.dir).ok();
            std::env::remove_var(STATE_DIR);
            std::env::remove_var(SESSION);
            std::env::remove_var(TTY);
        }
    }

    fn session(expires_in: u64) -> Session {
        Session {
            tty: Some("/dev/ttys001".to_owned()),
            server: "http://localhost:8080".to_owned(),
            realm: "master".to_owned(),
            user: "admin".to_owned(),
            token: "stub-token".to_owned(),
            expires_at: now() + expires_in,
            created_at: now(),
        }
    }

    #[test]
    fn store_lookup_and_remove() {
        let _guard = exclusive();
        let _sandbox = Sandbox::new("store");
        std::env::set_var(SESSION, "pid-1");

        assert_eq!(lookup().expect("no state file is not an error"), None);
        let path = store(session(300)).expect("the session is stored");
        let stored = lookup()
            .expect("the session is found")
            .expect("it is not expired");
        assert_eq!(stored.user, "admin");
        assert!(stored.remaining(now()) > 0);
        assert!(!stored.expired(now()));

        assert_eq!(
            remove()
                .expect("removing works")
                .map(|session| session.user),
            Some("admin".to_owned())
        );
        assert_eq!(lookup().expect("state loads"), None);
        assert!(remove().expect("removing nothing is fine").is_none());
        assert!(path.ends_with("sessions.json"));
    }

    #[test]
    fn the_state_file_is_only_readable_by_its_owner() {
        let _guard = exclusive();
        let sandbox = Sandbox::new("mode");
        std::env::set_var(SESSION, "pid-2");
        let path = store(session(300)).expect("the session is stored");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).expect("stat").permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "the token must not be readable by others");
            let dir = std::fs::metadata(&sandbox.dir)
                .expect("stat")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(dir, 0o700);
        }
        let text = std::fs::read_to_string(&path).expect("readable");
        assert!(text.contains("stub-token"));
    }

    #[test]
    fn an_expired_session_is_not_returned_and_is_pruned() {
        let _guard = exclusive();
        let _sandbox = Sandbox::new("expiry");
        std::env::set_var(SESSION, "pid-3");
        store(session(0)).expect("the session is stored");
        assert_eq!(lookup().expect("state loads"), None);

        // A new login drops it from the file instead of accumulating records.
        store(session(300)).expect("the session is stored");
        let state = load().expect("state loads");
        assert_eq!(state.sessions.len(), 1);
    }

    #[test]
    fn sessions_are_keyed_by_the_exported_id() {
        let _guard = exclusive();
        let _sandbox = Sandbox::new("keyed");
        std::env::set_var(SESSION, "pid-4");
        store(session(300)).expect("the session is stored");

        // Another terminal (another id) sees nothing.
        std::env::set_var(SESSION, "pid-5");
        assert_eq!(lookup().expect("state loads"), None);

        // Back in the first terminal, the session is there.
        std::env::set_var(SESSION, "pid-4");
        assert!(lookup().expect("state loads").is_some());

        // Without the variable there is no session at all.
        std::env::remove_var(SESSION);
        assert_eq!(lookup().expect("state loads"), None);
        assert!(matches!(store(session(300)), Err(Error::Usage(_))));
    }

    #[test]
    fn the_tty_is_optional() {
        let _guard = exclusive();
        let _sandbox = Sandbox::new("tty");
        std::env::set_var(SESSION, "pid-6");
        std::env::set_var(TTY, "/dev/pts/7");
        assert_eq!(current_tty().as_deref(), Some("/dev/pts/7"));

        let mut session = session(300);
        session.tty = None;
        store(session).expect("a session without a tty is fine");
        assert!(lookup().expect("state loads").is_some());

        std::env::set_var(TTY, "/dev/null");
        // An override is taken as given; detection is what filters devices.
        assert_eq!(current_tty().as_deref(), Some("/dev/null"));
    }

    #[test]
    fn a_corrupt_state_file_is_reported() {
        let _guard = exclusive();
        let sandbox = Sandbox::new("corrupt");
        std::fs::write(sandbox.dir.join("sessions.json"), "{not json").expect("write");
        std::env::set_var(SESSION, "pid-7");
        assert!(matches!(lookup(), Err(Error::Json(_))));
    }

    #[test]
    fn a_terminal_gets_a_stable_id() {
        let _guard = exclusive();
        let _sandbox = Sandbox::new("id");
        std::env::set_var(TTY, "/dev/pts/9");
        let first = id_for_terminal();
        assert_eq!(first, "tty:/dev/pts/9");
        assert_eq!(id_for_terminal(), first, "the same terminal keeps its id");

        std::env::remove_var(TTY);
        // No tty here in the test harness, so the id must at least be unique.
        let without_tty = id_for_terminal();
        assert!(without_tty.starts_with("pid:"), "{without_tty}");
    }

    #[test]
    fn the_token_is_redacted_in_debug_output() {
        let rendered = format!("{:?}", session(300));
        assert!(!rendered.contains("stub-token"), "{rendered}");
        assert!(rendered.contains("<redacted>"), "{rendered}");
    }
}
