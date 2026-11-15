//! Live completion: candidates that come from Keycloak, through the session.
//!
//! These tests run the real binary the way a shell does (`COMPLETE`, the index of
//! the word being completed, a separator) with a state directory holding a
//! session that points at the loopback stub. They cover what M4 and M5 ask for:
//! a logged-in terminal gets realms/users/groups/clients, a terminal without a
//! session gets nothing, and a second TAB is answered from the disk cache instead
//! of the network.

mod support;

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use support::{binary, Stub};

const SEPARATOR: char = '\u{b}';
const SESSION: &str = "tty:/dev/pts/42";

/// A state directory with a logged-in session pointing at `server`.
struct Sandbox {
    dir: PathBuf,
}

impl Sandbox {
    fn new(name: &str, server: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "keycloak-cli-live-{name}-{}-{}",
            std::process::id(),
            now()
        ));
        std::fs::create_dir_all(&dir).expect("the state directory exists");
        let sandbox = Self { dir };
        sandbox.set_session(server);
        sandbox
    }

    fn set_session(&self, server: &str) {
        let created = now();
        let session = serde_json::json!({
            "tty": "/dev/pts/42",
            "server": server,
            "realm": "master",
            "user": "admin",
            "token": "stub-token",
            "expires_at": created + 300,
            "created_at": created,
        });
        let mut sessions = serde_json::Map::new();
        sessions.insert(SESSION.to_owned(), session);
        let state = serde_json::json!({ "sessions": sessions });
        std::fs::write(self.dir.join("sessions.json"), state.to_string())
            .expect("the session is written");
    }

    fn path(&self) -> &Path {
        &self.dir
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time moves forward")
        .as_secs()
}

/// Asks the binary for the candidates of the last word, with a session.
fn complete(state: &Path, words: &[&str]) -> Vec<String> {
    complete_as(state, SESSION, words)
}

/// The same, but claiming to be another terminal.
fn complete_as(state: &Path, session: &str, words: &[&str]) -> Vec<String> {
    let mut argv = vec!["--".to_owned()];
    argv.extend(words.iter().map(|word| (*word).to_owned()));
    let index = (words.len() - 1).to_string();
    let separator = SEPARATOR.to_string();
    let state = state.display().to_string();

    let args: Vec<&str> = argv.iter().map(String::as_str).collect();
    let output = binary(
        &args,
        &[
            ("COMPLETE", "bash"),
            ("_CLAP_COMPLETE_INDEX", &index),
            ("_CLAP_IFS", &separator),
            ("KEYCLOAK_CLI_STATE_DIR", &state),
            ("KEYCLOAK_CLI_SESSION", session),
            ("KEYCLOAK_CLI_TTY", "/dev/pts/42"),
        ],
    );
    assert!(
        output.status.success(),
        "completion failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("candidates are utf-8")
        .split(SEPARATOR)
        .filter(|candidate| !candidate.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Only the candidates that are values, i.e. not flags clap offers.
fn values(candidates: &[String]) -> Vec<&str> {
    candidates
        .iter()
        .filter(|candidate| !candidate.starts_with('-'))
        .map(String::as_str)
        .collect()
}

#[test]
fn a_logged_in_terminal_completes_user_names() {
    let stub = Stub::start(vec![(
        200,
        r#"[{"id":"u1","username":"alice"},{"id":"u2","username":"bob"}]"#.to_owned(),
    )]);
    let sandbox = Sandbox::new("users", &stub.url);

    let candidates = complete(sandbox.path(), &["keycloak-cli", "users", "get", ""]);
    assert_eq!(values(&candidates), ["alice", "bob"], "{candidates:?}");

    let requests = stub.requests();
    assert_eq!(requests.len(), 1, "{requests:#?}");
    assert!(
        requests[0].starts_with("GET /admin/realms/master/users?"),
        "{requests:?}"
    );
    assert!(
        requests[0]
            .to_lowercase()
            .contains("authorization: bearer stub-token"),
        "{requests:?}"
    );
    assert!(requests[0].contains("max=100"), "{requests:?}");
}

#[test]
fn a_second_tab_is_answered_from_the_disk_cache() {
    let stub = Stub::start(vec![(
        200,
        r#"[{"id":"u1","username":"alice"}]"#.to_owned(),
    )]);
    let sandbox = Sandbox::new("warm", &stub.url);

    let first = complete(sandbox.path(), &["keycloak-cli", "users", "get", ""]);
    assert_eq!(values(&first), ["alice"], "{first:?}");
    assert_eq!(stub.requests().len(), 1);

    // The stub has no second answer: a fetch would fail, a cache hit succeeds.
    let second = complete(sandbox.path(), &["keycloak-cli", "users", "get", ""]);
    assert_eq!(values(&second), ["alice"], "{second:?}");
    assert_eq!(stub.requests().len(), 1, "the second TAB must not fetch");
}

#[test]
fn realms_complete_on_the_global_flag() {
    let stub = Stub::start(vec![(
        200,
        r#"[{"realm":"master"},{"realm":"other"}]"#.to_owned(),
    )]);
    let sandbox = Sandbox::new("realms", &stub.url);

    let candidates = complete(sandbox.path(), &["keycloak-cli", "-r", ""]);
    assert_eq!(values(&candidates), ["master", "other"], "{candidates:?}");
    assert_eq!(stub.requests().len(), 1, "{:#?}", stub.requests());
}

#[test]
fn groups_and_clients_complete_on_their_positionals() {
    let stub = Stub::start(vec![
        (
            200,
            r#"[{"id":"g-1","name":"admins"},{"id":"g-2","name":"devs"}]"#.to_owned(),
        ),
        (
            200,
            r#"[{"id":"c-1","clientId":"web"},{"id":"c-2","clientId":"api"}]"#.to_owned(),
        ),
    ]);
    let sandbox = Sandbox::new("groups", &stub.url);

    let groups = complete(sandbox.path(), &["keycloak-cli", "groups", "get", ""]);
    assert_eq!(values(&groups), ["g-1", "g-2"], "{groups:?}");

    let clients = complete(sandbox.path(), &["keycloak-cli", "clients", "get", ""]);
    assert_eq!(values(&clients), ["c-1", "c-2"], "{clients:?}");

    let requests = stub.requests();
    assert_eq!(requests.len(), 2, "{requests:#?}");
    assert!(
        requests[0].starts_with("GET /admin/realms/master/groups?"),
        "{requests:?}"
    );
    assert!(
        requests[1].starts_with("GET /admin/realms/master/clients?"),
        "{requests:?}"
    );
}

#[test]
fn a_terminal_without_a_session_offers_no_values() {
    let stub = Stub::start(vec![]);
    let dir = std::env::temp_dir().join(format!(
        "keycloak-cli-live-nosession-{}-{}",
        std::process::id(),
        now()
    ));
    std::fs::create_dir_all(&dir).expect("the state directory exists");

    let candidates = complete(&dir, &["keycloak-cli", "users", "get", ""]);
    assert!(values(&candidates).is_empty(), "{candidates:?}");
    assert_eq!(stub.requests().len(), 0, "nothing is requested");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_new_terminal_gets_nothing_until_it_logs_in() {
    let stub = Stub::start(vec![(
        200,
        r#"[{"id":"u1","username":"alice"}]"#.to_owned(),
    )]);
    let sandbox = Sandbox::new("new-terminal", &stub.url);

    // The same state directory, a different terminal: no session for it.
    let candidates = complete_as(
        sandbox.path(),
        "tty:/dev/pts/43",
        &["keycloak-cli", "users", "get", ""],
    );
    assert!(values(&candidates).is_empty(), "{candidates:?}");
    assert_eq!(stub.requests().len(), 0, "nothing is requested");
}

#[test]
fn a_slow_api_is_cut_off_by_the_budget() {
    let stub = Stub::start_after(
        std::time::Duration::from_secs(5),
        vec![(200, r#"[{"id":"u1","username":"alice"}]"#.to_owned())],
    );
    let sandbox = Sandbox::new("slow", &stub.url);

    let started = std::time::Instant::now();
    let candidates = complete(sandbox.path(), &["keycloak-cli", "users", "get", ""]);
    let elapsed = started.elapsed();
    assert!(values(&candidates).is_empty(), "{candidates:?}");
    assert!(
        elapsed < std::time::Duration::from_secs(4),
        "a 5 s API answer must be cut off, took {elapsed:?}"
    );
}

#[test]
fn a_failing_api_yields_no_candidates_and_no_noise() {
    let stub = Stub::start(vec![(500, r#"{"error":"boom"}"#.to_owned())]);
    let sandbox = Sandbox::new("failure", &stub.url);

    let mut argv = vec!["--".to_owned()];
    argv.extend(["keycloak-cli", "users", "get", ""].map(str::to_owned));
    let args: Vec<&str> = argv.iter().map(String::as_str).collect();
    let state = sandbox.path().display().to_string();
    let separator = SEPARATOR.to_string();
    let output = binary(
        &args,
        &[
            ("COMPLETE", "bash"),
            ("_CLAP_COMPLETE_INDEX", "3"),
            ("_CLAP_IFS", &separator),
            ("KEYCLOAK_CLI_STATE_DIR", &state),
            ("KEYCLOAK_CLI_SESSION", SESSION),
            ("KEYCLOAK_CLI_TTY", "/dev/pts/42"),
        ],
    );
    assert!(output.status.success(), "completion must not fail");
    assert!(
        output.stderr.is_empty(),
        "completion must stay quiet: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let candidates = String::from_utf8(output.stdout).expect("utf-8");
    assert!(
        !candidates.contains("boom"),
        "no API text may leak: {candidates:?}"
    );
    assert!(
        values(
            &candidates
                .split(SEPARATOR)
                .filter(|candidate| !candidate.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        )
        .is_empty(),
        "{candidates:?}"
    );
}
