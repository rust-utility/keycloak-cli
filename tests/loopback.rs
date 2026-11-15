//! A generated command against a loopback HTTP stub.
//!
//! The stub answers the token request and the API request and records what it
//! saw; the tests assert the method, path, query and body a generated command
//! produces, which is what "dispatches to its crate method" means in practice.

mod support;

use serde_json::Value;

use keycloak_cli::{cli, client, Config};
use support::{token, Stub};

/// Parses `args` and runs the command against `stub`.
fn run(stub: &Stub, args: &[&str]) -> Result<Value, keycloak_cli::Error> {
    let mut argv = vec![
        "keycloak-cli",
        "--server",
        stub.url.as_str(),
        "--password",
        "pw",
    ];
    argv.extend_from_slice(args);
    let cli = cli::Cli::try_parse_from(argv).expect("the command line parses");
    let invocation = cli.command.expect("a command was given");
    client::execute(&Config::from(&cli.connection), &invocation)
}

#[test]
fn a_get_sends_the_method_path_and_query_of_its_operation() {
    let stub = Stub::start(vec![token(), (200, "[]".to_owned())]);

    let value =
        run(&stub, &["clients", "list", "--first", "5", "--search"]).expect("the command runs");
    assert_eq!(value, Value::Array(Vec::new()));

    let requests = stub.requests();
    assert_eq!(requests.len(), 2, "{requests:#?}");

    let token_request = &requests[0];
    assert!(
        token_request.starts_with("POST /realms/master/protocol/openid-connect/token HTTP/1.1"),
        "{token_request}"
    );
    assert!(
        token_request.contains("grant_type=password"),
        "{token_request}"
    );

    let api_request = &requests[1];
    assert!(
        api_request.starts_with("GET /admin/realms/master/clients?first=5&search=true HTTP/1.1"),
        "{api_request}"
    );
    assert!(
        api_request
            .to_lowercase()
            .contains("authorization: bearer stub-token"),
        "{api_request}"
    );
    // Absent flags are not sent at all, not sent as defaults.
    assert!(!api_request.contains("max="), "{api_request}");
}

#[test]
fn a_positional_reaches_the_path_of_its_operation() {
    let stub = Stub::start(vec![
        token(),
        (200, r#"{"id":"abc","clientId":"demo"}"#.to_owned()),
    ]);

    let value = run(&stub, &["clients", "get", "abc"]).expect("the command runs");
    assert_eq!(value["id"], Value::from("abc"));
    assert_eq!(value["clientId"], Value::from("demo"));

    let api_request = &stub.requests()[1];
    assert!(
        api_request.starts_with("GET /admin/realms/master/clients/abc HTTP/1.1"),
        "{api_request}"
    );
}

#[test]
fn a_body_is_sent_from_a_file() {
    let stub = Stub::start(vec![token(), (201, String::new())]);
    let path = std::env::temp_dir().join(format!(
        "keycloak-cli-body-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time moves forward")
            .as_nanos()
    ));
    std::fs::write(&path, r#"{"clientId":"demo","enabled":true}"#).expect("the body is written");

    let value = run(
        &stub,
        &[
            "clients",
            "create",
            "--body",
            path.to_str().expect("a utf-8 path"),
        ],
    )
    .expect("the command runs");
    // The crate answers a creation with a response wrapper, not a JSON body.
    assert_eq!(value, Value::Null);

    let api_request = &stub.requests()[1];
    assert!(
        api_request.starts_with("POST /admin/realms/master/clients HTTP/1.1"),
        "{api_request}"
    );
    assert!(
        api_request.contains(r#""clientId":"demo""#),
        "{api_request}"
    );

    std::fs::remove_file(&path).expect("the body file is removed");
}

#[test]
fn a_server_error_becomes_an_api_error_with_its_exit_code() {
    let stub = Stub::start(vec![
        token(),
        (404, r#"{"error":"Client not found"}"#.to_owned()),
    ]);

    let error = match run(&stub, &["clients", "get", "missing"]) {
        Ok(value) => panic!("a 404 must not succeed: {value}"),
        Err(error) => error,
    };
    assert!(matches!(error, keycloak_cli::Error::Api(_)), "{error}");
    assert!(error.to_string().contains("404"), "{error}");
    assert_eq!(error.exit_code(), 4);
}

#[test]
fn a_rejected_token_is_exit_code_3() {
    let stub = Stub::start(vec![(
        401,
        r#"{"error":"invalid_grant","error_description":"Invalid user credentials"}"#.to_owned(),
    )]);
    let error = match run(&stub, &["clients", "list"]) {
        Ok(value) => panic!("authentication must not succeed: {value}"),
        Err(error) => error,
    };
    assert_eq!(error.exit_code(), 3);
}
