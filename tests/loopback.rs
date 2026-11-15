//! A generated command against a loopback HTTP stub.
//!
//! The `keycloak` crate builds and sends its requests internally, so the only way
//! to see what a command does on the wire is to point it at a local server. The
//! stub below answers the token request and the API request and records what it
//! saw; the tests then assert the method, path, query and body a generated
//! command produces, which is what "dispatches to its crate method" means in
//! practice.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::Value;

use keycloak_cli::{cli, client, Config};

/// A one-shot HTTP server that answers with canned responses in order.
struct Stub {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Stub {
    /// Starts a stub that answers `responses` (status, body) in order.
    fn start(responses: Vec<(u16, String)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("the stub binds a port");
        let address = listener.local_addr().expect("the stub has an address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&requests);
        thread::spawn(move || {
            for (status, body) in responses {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                answer(stream, status, &body, &seen);
            }
        });
        Self {
            url: format!("http://{address}"),
            requests,
        }
    }

    /// The requests the stub answered: head and body, in order.
    fn requests(&self) -> Vec<String> {
        self.requests
            .lock()
            .expect("the request log is not poisoned")
            .clone()
    }
}

/// Reads one request, records it and answers it.
fn answer(mut stream: TcpStream, status: u16, body: &str, seen: &Arc<Mutex<Vec<String>>>) {
    let mut raw = Vec::new();
    let mut buffer = [0u8; 4096];

    let head_end = loop {
        if let Some(position) = find(&raw, b"\r\n\r\n") {
            break position;
        }
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => raw.extend_from_slice(&buffer[..read]),
        }
    };
    let head = String::from_utf8_lossy(&raw[..head_end]).into_owned();

    let length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    let mut content = raw[head_end + 4..].to_vec();
    while content.len() < length {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => content.extend_from_slice(&buffer[..read]),
        }
    }
    seen.lock()
        .expect("the request log is not poisoned")
        .push(format!(
            "{head}\r\n\r\n{}",
            String::from_utf8_lossy(&content)
        ));

    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "Error",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

/// The index of `needle` in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// What Keycloak answers to a password grant.
fn token() -> (u16, String) {
    (
        200,
        r#"{"access_token":"stub-token","expires_in":300,"refresh_expires_in":0,"refresh_token":"stub-refresh","scope":"profile","token_type":"Bearer"}"#.to_owned(),
    )
}

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
