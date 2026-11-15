//! Terminal sessions: `login`, `logout` and what later commands use.
//!
//! These tests run the real binary, so they cover the state file, its permissions,
//! the session key and the token source the client ends up using — the loopback
//! stub records whether a second invocation asked for a new token or reused the
//! stored one.

mod support;

use std::path::PathBuf;

use support::{binary, token, Stub};

/// A state directory of its own, removed afterwards.
struct State {
    dir: PathBuf,
}

impl State {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "keycloak-cli-state-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time moves forward")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("the state directory exists");
        Self { dir }
    }

    fn sessions(&self) -> PathBuf {
        self.dir.join("sessions.json")
    }

    fn env(&self, session: &str) -> Vec<(&str, String)> {
        vec![
            ("KEYCLOAK_CLI_STATE_DIR", self.dir.display().to_string()),
            ("KEYCLOAK_CLI_SESSION", session.to_owned()),
            ("KEYCLOAK_CLI_TTY", "/dev/pts/42".to_owned()),
        ]
    }
}

impl Drop for State {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

/// Runs the binary with `env` plus the values as `(&str, &str)`.
fn run(
    state: &State,
    session: &str,
    args: &[&str],
    extra: &[(&str, &str)],
) -> std::process::Output {
    let owned = state.env(session);
    let mut env: Vec<(&str, &str)> = owned
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    env.extend_from_slice(extra);
    binary(args, &env)
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn login_stores_a_private_session_and_later_commands_use_it() {
    let state = State::new("login");
    let stub = Stub::start(vec![token(), (200, "[]".to_owned())]);

    let logged_in = run(
        &state,
        "tty:/dev/pts/42",
        &["login"],
        &[
            ("KEYCLOAK_ADDR", stub.url.as_str()),
            ("KEYCLOAK_USER", "admin"),
            ("KEYCLOAK_PASSWORD", "pw"),
        ],
    );
    assert!(
        logged_in.status.success(),
        "login failed: {}",
        stderr(&logged_in)
    );
    let report = stdout(&logged_in);
    assert!(report.contains("session"), "{report}");
    assert!(report.contains("expires"), "{report}");
    assert!(report.contains("state"), "{report}");
    assert!(!report.contains("pw"), "{report}");

    let sessions = state.sessions();
    assert!(sessions.exists(), "the state file exists");
    let stored = std::fs::read_to_string(&sessions).expect("the state file is readable");
    assert!(stored.contains("stub-token"), "{stored}");
    assert!(stored.contains("/dev/pts/42"), "{stored}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&sessions)
            .expect("stat")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "the token must not be readable by others");
    }

    // A later command in the same terminal needs no password: the stub sees the
    // GET only, with the stored token.
    let listed = run(
        &state,
        "tty:/dev/pts/42",
        &["clients", "list"],
        &[("KEYCLOAK_ADDR", stub.url.as_str())],
    );
    assert!(
        listed.status.success(),
        "the command failed: {}",
        stderr(&listed)
    );
    assert_eq!(stdout(&listed).trim(), "[]");

    let requests = stub.requests();
    assert_eq!(requests.len(), 2, "{requests:#?}");
    assert!(
        requests[0].starts_with("POST /realms/master/"),
        "{requests:?}"
    );
    assert!(
        requests[1].starts_with("GET /admin/realms/master/clients "),
        "{requests:?}"
    );
    assert!(
        requests[1]
            .to_lowercase()
            .contains("authorization: bearer stub-token"),
        "{requests:?}"
    );
}

#[test]
fn another_terminal_has_no_session() {
    let state = State::new("other");
    let stub = Stub::start(vec![token()]);

    let logged_in = run(
        &state,
        "tty:/dev/pts/42",
        &["login"],
        &[
            ("KEYCLOAK_ADDR", stub.url.as_str()),
            ("KEYCLOAK_PASSWORD", "pw"),
        ],
    );
    assert!(logged_in.status.success(), "{}", stderr(&logged_in));

    // A different terminal: no token, no password, so a usage error.
    let other = run(
        &state,
        "tty:/dev/pts/43",
        &["clients", "list"],
        &[("KEYCLOAK_ADDR", stub.url.as_str())],
    );
    assert_eq!(other.status.code(), Some(2), "{}", stderr(&other));
    assert!(
        stderr(&other).contains("keycloak-cli login"),
        "{}",
        stderr(&other)
    );
    // Nothing was sent to the server.
    assert_eq!(stub.requests().len(), 1);
}

#[test]
fn logout_forgets_the_session() {
    let state = State::new("logout");
    let stub = Stub::start(vec![token()]);

    run(
        &state,
        "tty:/dev/pts/42",
        &["login"],
        &[
            ("KEYCLOAK_ADDR", stub.url.as_str()),
            ("KEYCLOAK_PASSWORD", "pw"),
        ],
    );
    let out = run(&state, "tty:/dev/pts/42", &["logout"], &[]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("logged out"), "{}", stdout(&out));

    let stored = std::fs::read_to_string(state.sessions()).expect("the state file exists");
    assert!(!stored.contains("stub-token"), "{stored}");

    let again = run(&state, "tty:/dev/pts/42", &["logout"], &[]);
    assert!(again.status.success());
    assert!(stdout(&again).contains("no session"), "{}", stdout(&again));

    let listed = run(
        &state,
        "tty:/dev/pts/42",
        &["clients", "list"],
        &[("KEYCLOAK_ADDR", stub.url.as_str())],
    );
    assert_eq!(listed.status.code(), Some(2), "{}", stderr(&listed));
}

#[test]
fn an_expired_session_is_not_used() {
    let state = State::new("expired");
    let stub = Stub::start(vec![]);
    let sessions = state.sessions();
    std::fs::write(
        &sessions,
        format!(
            r#"{{"sessions":{{"tty:/dev/pts/42":{{"tty":"/dev/pts/42","server":"{}","realm":"master","user":"admin","token":"old","expires_at":1,"created_at":1}}}}}}"#,
            stub.url
        ),
    )
    .expect("the state file is written");

    let listed = run(
        &state,
        "tty:/dev/pts/42",
        &["clients", "list"],
        &[("KEYCLOAK_ADDR", stub.url.as_str())],
    );
    assert_eq!(listed.status.code(), Some(2), "{}", stderr(&listed));
    assert_eq!(stub.requests().len(), 0, "no request is sent");
}

#[test]
fn session_id_is_the_tty_of_the_terminal() {
    let state = State::new("id");
    let out = run(
        &state,
        "unused",
        &["session-id"],
        &[("KEYCLOAK_CLI_TTY", "/dev/pts/7")],
    );
    assert!(out.status.success());
    assert_eq!(stdout(&out).trim(), "tty:/dev/pts/7");

    // Without a terminal the id is still unique to the process.
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_keycloak-cli"))
        .arg("session-id")
        .env_remove("KEYCLOAK_CLI_TTY")
        .env_remove("KEYCLOAK_CLI_SESSION")
        .output()
        .expect("the binary runs");
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    assert!(
        value.starts_with("tty:") || value.starts_with("pid:"),
        "{value}"
    );
}

#[test]
fn init_exports_the_session_before_registering_completion() {
    let out = binary(&["init", "bash"], &[]);
    assert!(out.status.success());
    let text = stdout(&out);
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(
        lines[0].starts_with("export KEYCLOAK_CLI_SESSION="),
        "{lines:?}"
    );
    assert!(lines[0].contains("session-id"), "{lines:?}");
    assert!(lines[1].starts_with("eval \"$(COMPLETE=bash "), "{lines:?}");
}
