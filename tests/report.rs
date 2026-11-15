//! `--output` and `--fields`: how a response is printed.
//!
//! These run the real binary against the loopback stub, so they cover the whole
//! path from the generated dispatch through the renderers: a listing as JSON when
//! piped (`auto`), as a table, YAML, markdown or HTML, and projected to the fields
//! that were asked for.

mod support;

use support::{binary, token, Stub};

const CLIENTS: &str = r#"[{"id":"c-1","clientId":"web","enabled":true},{"id":"c-2","clientId":"api","enabled":false}]"#;

/// Runs `clients list` with `output` against a stub that answers once.
fn listing(output: &[&str]) -> (String, String) {
    let stub = Stub::start(vec![token(), (200, CLIENTS.to_owned())]);
    let mut args = vec!["clients", "list"];
    args.extend_from_slice(output);
    let output = binary(
        &args,
        &[
            ("KEYCLOAK_ADDR", stub.url.as_str()),
            ("KEYCLOAK_USER", "admin"),
            ("KEYCLOAK_PASSWORD", "pw"),
        ],
    );
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn auto_is_json_when_piped() {
    let (stdout, stderr) = listing(&[]);
    assert!(stderr.is_empty(), "{stderr}");
    // No terminal here, so `auto` must choose JSON.
    assert!(stdout.trim_start().starts_with('['), "{stdout}");
    assert!(stdout.contains("\"clientId\": \"web\""), "{stdout}");
}

#[test]
fn a_table_has_borders_and_every_column() {
    let (stdout, stderr) = listing(&["--output", "table"]);
    assert!(stderr.is_empty(), "{stderr}");
    for expected in ["id", "clientId", "enabled", "web", "api"] {
        assert!(stdout.contains(expected), "{expected} missing:\n{stdout}");
    }
    assert!(stdout.contains('┌') || stdout.contains('+'), "{stdout}");
}

#[test]
fn yaml_markdown_and_html_are_available() {
    let (yaml, _) = listing(&["--output", "yaml"]);
    assert!(yaml.contains("clientId: web"), "{yaml}");

    // Columns follow the key order of the objects, which serde_json sorts;
    // `--fields` is how a caller picks its own order.
    let (markdown, _) = listing(&["--output", "markdown"]);
    assert!(
        markdown.starts_with("| clientId | enabled | id |"),
        "{markdown}"
    );
    assert!(markdown.contains("| web | true | c-1 |"), "{markdown}");

    let (html, _) = listing(&["--output", "html"]);
    assert!(html.starts_with("<table>"), "{html}");
    assert!(html.contains("<th>clientId</th>"), "{html}");
    assert!(html.contains("<td>web</td>"), "{html}");
    assert!(html.trim_end().ends_with("</table>"), "{html}");
}

#[test]
fn fields_project_the_output() {
    let (stdout, _) = listing(&["--output", "table", "--fields", "clientId"]);
    assert!(stdout.contains("clientId"), "{stdout}");
    assert!(!stdout.contains("enabled"), "{stdout}");
    assert!(!stdout.contains("\"id\""), "{stdout}");

    let (json, _) = listing(&["--fields", "clientId,id"]);
    assert!(json.contains("\"clientId\""), "{json}");
    assert!(!json.contains("\"enabled\""), "{json}");
}

#[test]
fn an_unknown_format_is_a_usage_error() {
    let stub = Stub::start(vec![]);
    let output = binary(
        &["clients", "list", "--output", "xml"],
        &[("KEYCLOAK_ADDR", stub.url.as_str())],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("table"), "{stderr}");
}

#[test]
fn a_bodyless_response_prints_nothing() {
    // `clients create` answers without a body: no output, but success.
    let stub = Stub::start(vec![token(), (201, String::new())]);
    let path = std::env::temp_dir().join(format!(
        "keycloak-cli-report-body-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time moves forward")
            .as_nanos()
    ));
    std::fs::write(&path, r#"{"clientId":"demo"}"#).expect("the body is written");
    let output = binary(
        &[
            "clients",
            "create",
            "--body",
            path.to_str().expect("a utf-8 path"),
            "--output",
            "table",
        ],
        &[
            ("KEYCLOAK_ADDR", stub.url.as_str()),
            ("KEYCLOAK_PASSWORD", "pw"),
        ],
    );
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    std::fs::remove_file(&path).ok();
}
