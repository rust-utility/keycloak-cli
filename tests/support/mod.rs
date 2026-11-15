//! Shared test support: a loopback HTTP stub and a way to run the binary.
//!
//! The `keycloak` crate builds and sends its requests internally, so the only way
//! to see what a command does on the wire is to point it at a local server. The
//! stub answers canned responses in order and records what it saw.
//!
//! This file is a module for the integration tests, not a test itself.

#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

/// A one-shot HTTP server that answers with canned responses in order.
pub struct Stub {
    pub url: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Stub {
    /// Starts a stub that answers `responses` (status, body) in order.
    pub fn start(responses: Vec<(u16, String)>) -> Self {
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
    pub fn requests(&self) -> Vec<String> {
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
pub fn token() -> (u16, String) {
    (
        200,
        r#"{"access_token":"stub-token","expires_in":300,"refresh_expires_in":0,"refresh_token":"stub-refresh","scope":"profile","token_type":"Bearer"}"#.to_owned(),
    )
}

/// Runs the CLI binary with `args` and `env`, returning its output.
pub fn binary(args: &[&str], env: &[(&str, &str)]) -> std::process::Output {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_keycloak-cli"));
    command.args(args);
    for (name, value) in env {
        command.env(name, value);
    }
    // A developer's real session must not leak into the tests.
    command.env_remove("KEYCLOAK_PASSWORD");
    command.env_remove("KEYCLOAK_CLI_SESSION");
    for (name, value) in env {
        command.env(name, value);
    }
    command.output().expect("the binary runs")
}
