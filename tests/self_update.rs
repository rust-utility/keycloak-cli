//! `self-update` against a loopback release host.
//!
//! The updater takes its API and download hosts as fields, so the whole flow —
//! resolve the latest tag, read `SHA256SUMS`, download the binary, verify it and
//! rename it over the executable — runs against the stub instead of GitHub.

mod support;

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use keycloak_cli::update::{Options, Outcome, Update};
use support::Stub;

const REPOSITORY: &str = "rust-utility/keycloak-cli";
const TARGET: &str = "aarch64-apple-darwin";
const CURRENT: &str = "26.8.101";
const LATEST: &str = "26.8.102";

/// A placeholder binary; the updater only moves bytes around.
fn binary() -> String {
    "#!/bin/sh\necho keycloak-cli 26.8.102\n".to_owned()
}

fn sha256(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn latest(version: &str) -> (u16, String) {
    (200, format!(r#"{{"tag_name":"v{version}"}}"#))
}

fn checksums(text: &str) -> (u16, String) {
    (200, format!("{}  keycloak-cli-{TARGET}\n", sha256(text)))
}

/// A scratch copy of the binary that an update may replace.
struct Exe {
    dir: PathBuf,
}

impl Exe {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "keycloak-cli-self-update-{name}-{}-{}",
            std::process::id(),
            keycloak_cli::session::now()
        ));
        std::fs::create_dir_all(&dir).expect("the directory exists");
        std::fs::write(dir.join("keycloak-cli"), b"old binary").expect("the old binary exists");
        Self { dir }
    }

    fn path(&self) -> PathBuf {
        self.dir.join("keycloak-cli")
    }

    fn contents(&self) -> String {
        std::fs::read_to_string(self.path()).expect("the binary is readable")
    }
}

impl Drop for Exe {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

/// Runs the updater on a runtime of its own.
fn run(stub: &Stub, exe: &Path, options: Options) -> Result<Outcome, keycloak_cli::Error> {
    let update = Update::new(REPOSITORY, TARGET).with_hosts(&stub.url, &stub.url);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime builds");
    runtime.block_on(update.run(exe, CURRENT, &options))
}

#[test]
fn check_reports_a_newer_release_without_touching_the_binary() {
    let stub = Stub::start(vec![latest(LATEST)]);
    let exe = Exe::new("check");

    let outcome = run(
        &stub,
        &exe.path(),
        Options {
            check: true,
            ..Options::default()
        },
    )
    .expect("the check succeeds");

    assert_eq!(
        outcome,
        Outcome::Available {
            current: CURRENT.to_owned(),
            latest: LATEST.to_owned()
        }
    );
    assert_eq!(exe.contents(), "old binary");
    assert_eq!(stub.requests().len(), 1);
}

#[test]
fn the_running_version_is_up_to_date() {
    let stub = Stub::start(vec![latest(CURRENT)]);
    let exe = Exe::new("current");

    let outcome = run(&stub, &exe.path(), Options::default()).expect("the check succeeds");
    assert_eq!(
        outcome,
        Outcome::UpToDate {
            version: CURRENT.to_owned()
        }
    );
    assert_eq!(stub.requests().len(), 1);
    assert_eq!(exe.contents(), "old binary");
}

#[test]
fn an_update_verifies_and_replaces_the_binary() {
    let replacement = binary();
    let stub = Stub::start(vec![
        latest(LATEST),
        checksums(&replacement),
        (200, replacement.clone()),
    ]);
    let exe = Exe::new("update");

    let outcome = run(&stub, &exe.path(), Options::default()).expect("the update succeeds");
    match outcome {
        Outcome::Updated { from, to, path } => {
            assert_eq!(from, CURRENT);
            assert_eq!(to, LATEST);
            assert_eq!(path, exe.path());
        }
        other => panic!("expected an update, got {other:?}"),
    }
    assert_eq!(exe.contents(), replacement);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(exe.path())
            .expect("stat")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o755, "the new binary is executable");
    }
    let requests = stub.requests();
    assert_eq!(requests.len(), 3, "{requests:#?}");
    assert!(requests[0].contains("/repos/rust-utility/keycloak-cli/releases/latest"));
    assert!(
        requests[1].contains("/v26.8.102/SHA256SUMS"),
        "{requests:?}"
    );
    assert!(
        requests[2].contains("/v26.8.102/keycloak-cli-aarch64-apple-darwin"),
        "{requests:?}"
    );
    assert!(!exe.dir.join("keycloak-cli.new").exists(), "no leftovers");
}

#[test]
fn a_checksum_mismatch_is_refused() {
    let replacement = binary();
    let stub = Stub::start(vec![
        latest(LATEST),
        // The digest of something else entirely.
        (200, format!("{}  keycloak-cli-{TARGET}\n", "0".repeat(64))),
        (200, replacement),
    ]);
    let exe = Exe::new("mismatch");

    let error = run(&stub, &exe.path(), Options::default()).expect_err("the update is refused");
    assert!(error.to_string().contains("checksum mismatch"), "{error}");
    assert_eq!(exe.contents(), "old binary", "the binary is untouched");
}

#[test]
fn a_missing_release_is_reported_with_its_status() {
    let stub = Stub::start(vec![(404, r#"{"message":"Not Found"}"#.to_owned())]);
    let exe = Exe::new("missing");

    let error = run(&stub, &exe.path(), Options::default()).expect_err("the release is missing");
    let message = error.to_string();
    assert!(message.contains("404"), "{message}");
    assert!(message.contains("Not Found"), "{message}");
    assert_eq!(exe.contents(), "old binary");
}

#[test]
fn a_requested_version_skips_the_latest_lookup() {
    let replacement = binary();
    let stub = Stub::start(vec![checksums(&replacement), (200, replacement.clone())]);
    let exe = Exe::new("pinned");

    let outcome = run(
        &stub,
        &exe.path(),
        Options {
            version: Some("26.8.102".to_owned()),
            ..Options::default()
        },
    )
    .expect("the pinned update succeeds");
    assert!(matches!(outcome, Outcome::Updated { .. }));
    assert_eq!(exe.contents(), replacement);
    let requests = stub.requests();
    assert_eq!(requests.len(), 2, "{requests:#?}");
    assert!(
        requests[0].contains("/v26.8.102/SHA256SUMS"),
        "{requests:?}"
    );
}
