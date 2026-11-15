//! Safety: `--dry-run`, `--yes` on deletes, `--verbose` and `--quiet`.
//!
//! These run the real binary against the loopback stub, so they check what reaches
//! the server: a dry run must send nothing, a delete must not happen without
//! `--yes`, a verbose run must log the request and still send it, and `--quiet`
//! must keep informational messages off stderr.

mod support;

use support::{binary, token, Stub};

/// Runs the binary with the stub as the server.
fn run(stub: &Stub, args: &[&str]) -> std::process::Output {
    binary(
        args,
        &[
            ("KEYCLOAK_ADDR", stub.url.as_str()),
            ("KEYCLOAK_USER", "admin"),
            ("KEYCLOAK_PASSWORD", "pw"),
        ],
    )
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn a_dry_run_prints_the_request_and_sends_nothing() {
    let stub = Stub::start(vec![]);
    let output = run(
        &stub,
        &["--dry-run", "clients", "list", "--first", "5", "--search"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output).trim(),
        format!(
            "GET {}/admin/realms/master/clients?first=5&search=true",
            stub.url
        )
    );
    assert_eq!(
        stub.requests().len(),
        0,
        "a dry run must not reach the server"
    );
}

#[test]
fn a_dry_run_shows_the_body_of_a_mutation() {
    let stub = Stub::start(vec![]);
    let path = std::env::temp_dir().join(format!(
        "keycloak-cli-safety-body-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time moves forward")
            .as_nanos()
    ));
    std::fs::write(&path, r#"{"clientId":"demo"}"#).expect("the body is written");

    let output = run(
        &stub,
        &[
            "--dry-run",
            "clients",
            "create",
            "--body",
            path.to_str().expect("a utf-8 path"),
        ],
    );
    let printed = stdout(&output);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        printed.starts_with(&format!("POST {}/admin/realms/master/clients", stub.url)),
        "{printed}"
    );
    assert!(printed.contains(r#"{"clientId":"demo"}"#), "{printed}");
    assert_eq!(stub.requests().len(), 0);

    std::fs::remove_file(&path).ok();
}

#[test]
fn a_delete_needs_yes() {
    let stub = Stub::start(vec![]);
    let output = run(&stub, &["users", "delete", "abc"]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(stderr(&output).contains("--yes"), "{}", stderr(&output));
    assert!(stdout(&output).is_empty(), "{}", stdout(&output));
    assert_eq!(stub.requests().len(), 0, "nothing may be deleted");

    // A dry run needs no confirmation: nothing is deleted either.
    let dry = run(&stub, &["--dry-run", "users", "delete", "abc"]);
    assert!(dry.status.success(), "{}", stderr(&dry));
    assert!(stdout(&dry).starts_with("DELETE "), "{}", stdout(&dry));
    assert_eq!(stub.requests().len(), 0);
}

#[test]
fn a_delete_with_yes_reaches_the_server() {
    let stub = Stub::start(vec![token(), (204, String::new())]);
    let output = run(&stub, &["users", "delete", "abc", "--yes"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let requests = stub.requests();
    assert_eq!(requests.len(), 2, "{requests:#?}");
    assert!(
        requests[1].starts_with("DELETE /admin/realms/master/users/abc HTTP/1.1"),
        "{requests:?}"
    );
}

#[test]
fn verbose_logs_the_request_and_still_sends_it() {
    let stub = Stub::start(vec![token(), (200, "[]".to_owned())]);
    let output = run(&stub, &["--verbose", "clients", "list", "--max", "1"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stderr(&output).contains(&format!(
            "> GET {}/admin/realms/master/clients?max=1",
            stub.url
        )),
        "{}",
        stderr(&output)
    );
    assert_eq!(stdout(&output).trim(), "[]");
    assert_eq!(stub.requests().len(), 2, "the request is still sent");
}

#[test]
fn quiet_keeps_informational_messages_off() {
    let stub = Stub::start(vec![
        token(),
        (200, "0".to_owned()),
        (200, "0".to_owned()),
        (200, "0".to_owned()),
        (200, "[]".to_owned()),
        (200, "[]".to_owned()),
    ]);
    let path = std::env::temp_dir().join(format!(
        "keycloak-cli-safety-report-{}-{}.md",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time moves forward")
            .as_nanos()
    ));
    let output = run(
        &stub,
        &[
            "--quiet",
            "report",
            "--file",
            path.to_str().expect("a utf-8 path"),
        ],
    );
    assert!(stderr(&output).is_empty(), "{}", stderr(&output));
    assert!(path.exists(), "the report is still written");
    std::fs::remove_file(&path).ok();
}
