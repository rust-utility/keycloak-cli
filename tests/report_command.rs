//! The `report` command: a document with sections and ASCII charts.
//!
//! The loopback stub answers the three endpoints the report reads (users count,
//! events, session stats) plus the token request, so these tests cover the whole
//! path: collect → render → stdout or `--file`.

mod support;

use support::{binary, token, Stub};

/// Runs `report` with `args` against a stub that serves a fixed realm.
fn run(args: &[&str]) -> (String, String, Option<i32>) {
    let stub = Stub::start(vec![
        token(),
        (200, "12".to_owned()),
        (200, "10".to_owned()),
        (200, "2".to_owned()),
        (
            200,
            r#"[{"type":"LOGIN","time":1},{"type":"LOGIN","time":2},{"type":"LOGOUT","time":3},{"type":"CODE_TO_TOKEN","time":4}]"#.to_owned(),
        ),
        (
            200,
            r#"[{"clientId":"web","active":"3","offline":"1"},{"clientId":"api","active":"1","offline":"0"}]"#.to_owned(),
        ),
    ]);
    let mut arguments = vec!["report"];
    arguments.extend_from_slice(args);
    let output = binary(
        &arguments,
        &[
            ("KEYCLOAK_ADDR", stub.url.as_str()),
            ("KEYCLOAK_USER", "admin"),
            ("KEYCLOAK_PASSWORD", "pw"),
        ],
    );
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code(),
    )
}

#[test]
fn markdown_is_the_default_document() {
    let (stdout, stderr, code) = run(&[]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(
        stdout.starts_with("# keycloak-cli report — realm `master`"),
        "{stdout}"
    );
    assert!(stdout.contains("## Users"), "{stdout}");
    assert!(
        stdout.contains("12 users in total: 10 enabled, 2 disabled."),
        "{stdout}"
    );
    assert!(stdout.contains("## Events by type"), "{stdout}");
    assert!(stdout.contains("LOGIN"), "{stdout}");
    assert!(stdout.contains("## Sessions per client"), "{stdout}");
    assert!(stdout.contains("web"), "{stdout}");
    // Charts are fenced blocks with bars.
    assert!(stdout.contains("```\n"), "{stdout}");
    assert!(stdout.contains('█'), "{stdout}");
}

#[test]
fn html_is_self_contained() {
    let (stdout, stderr, code) = run(&["--output", "html"]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.starts_with("<!doctype html>"), "{stdout}");
    assert!(stdout.contains("<style>"), "{stdout}");
    assert!(stdout.contains("<h2>Users</h2>"), "{stdout}");
    assert!(stdout.contains("<pre>"), "{stdout}");
    assert!(stdout.trim_end().ends_with("</html>"), "{stdout}");
    assert!(!stdout.contains("<link"), "{stdout}");
}

#[test]
fn json_is_available_for_tooling() {
    let (stdout, stderr, code) = run(&["--output", "json"]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.contains("\"realm\": \"master\""), "{stdout}");
    assert!(stdout.contains("\"title\": \"Users\""), "{stdout}");
    assert!(stdout.contains("\"label\": \"enabled\""), "{stdout}");
}

#[test]
fn a_report_can_be_written_to_a_file() {
    let path = std::env::temp_dir().join(format!(
        "keycloak-cli-report-{}-{}.md",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time moves forward")
            .as_nanos()
    ));
    let (stdout, stderr, code) = run(&["--file", path.to_str().expect("a utf-8 path")]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.is_empty(), "{stdout}");
    assert!(stderr.contains("wrote"), "{stderr}");
    let document = std::fs::read_to_string(&path).expect("the report is written");
    assert!(document.contains("# keycloak-cli report"), "{document}");
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_table_is_not_a_report() {
    let (_, stderr, code) = run(&["--output", "table"]);
    assert_eq!(code, Some(2), "{stderr}");
    assert!(stderr.contains("markdown"), "{stderr}");
}
