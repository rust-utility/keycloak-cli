//! Dynamic completion, driven the way a shell drives it.
//!
//! The shell runs the binary with `COMPLETE=bash`, `_CLAP_COMPLETE_INDEX` and the
//! words so far after `--`, and reads candidates from stdout (F8, F10). These
//! tests use the real binary, so they cover the whole path: main's `complete()`
//! call, `build_cli()`, and the generated tree.

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_keycloak-cli");
const SEPARATOR: char = '\u{b}';

/// Asks the binary for the candidates of the last word.
fn candidates(words: &[&str]) -> Vec<String> {
    let mut arguments = vec!["--".to_owned()];
    arguments.extend(words.iter().map(|word| (*word).to_owned()));
    let output = Command::new(BIN)
        .env("COMPLETE", "bash")
        .env("_CLAP_COMPLETE_INDEX", (words.len() - 1).to_string())
        .env("_CLAP_COMPLETE_SPACE", "true")
        .env("_CLAP_IFS", SEPARATOR.to_string())
        .args(&arguments)
        .output()
        .expect("the binary runs");
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

#[test]
fn completes_generated_commands() {
    let candidates = candidates(&["keycloak-cli", "clients", ""]);
    for expected in ["list", "create", "get", "client-secret", "roles"] {
        assert!(
            candidates.iter().any(|candidate| candidate == expected),
            "{expected} is not among {candidates:?}"
        );
    }
}

#[test]
fn completes_the_flags_of_a_generated_command() {
    let candidates = candidates(&["keycloak-cli", "clients", "list", "--"]);
    for expected in ["--first", "--max", "--search", "--client-id"] {
        assert!(
            candidates.iter().any(|candidate| candidate == expected),
            "{expected} is not among {candidates:?}"
        );
    }
    // The connection flags are global, so a generated command completes them too.
    assert!(candidates.iter().any(|candidate| candidate == "--realm"));
}

#[test]
fn completes_the_groups_at_the_top_level() {
    let candidates = candidates(&["keycloak-cli", ""]);
    for expected in ["info", "init", "clients", "users", "realms"] {
        assert!(
            candidates.iter().any(|candidate| candidate == expected),
            "{expected} is not among {candidates:?}"
        );
    }
}

#[test]
fn completes_the_shells_of_init() {
    let candidates = candidates(&["keycloak-cli", "init", ""]);
    for expected in ["bash", "zsh", "fish"] {
        assert!(
            candidates.iter().any(|candidate| candidate == expected),
            "{expected} is not among {candidates:?}"
        );
    }
}

#[test]
fn a_positional_without_a_session_falls_back_to_flags() {
    // A path parameter has a live source, but this process has no session, so the
    // only candidates are the flags clap offers at every value position. The live
    // candidates are covered in `tests/live_completion.rs`.
    let candidates = candidates(&["keycloak-cli", "clients", "get", ""]);
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.starts_with('-')),
        "{candidates:?}"
    );
    assert!(candidates.iter().any(|candidate| candidate == "--realm"));
}

#[test]
fn prints_a_registration_that_calls_the_binary() {
    let output = Command::new(BIN)
        .env("COMPLETE", "bash")
        .arg("--")
        .output()
        .expect("the binary runs");
    assert!(output.status.success());
    let script = String::from_utf8(output.stdout).expect("the script is utf-8");
    assert!(script.contains("_clap_complete_keycloak_cli"), "{script}");
    assert!(script.contains("complete -o nospace"), "{script}");
    assert!(script.contains(BIN), "the script does not call {BIN}");
}

#[test]
fn init_prints_the_line_for_each_shell() {
    for (shell, expected) in [
        ("bash", "eval \"$(COMPLETE=bash "),
        ("zsh", "source <(COMPLETE=zsh "),
        ("fish", "COMPLETE=fish "),
    ] {
        let output = Command::new(BIN)
            .args(["init", shell])
            .output()
            .expect("the binary runs");
        assert!(
            output.status.success(),
            "init {shell}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let lines = String::from_utf8(output.stdout).expect("the lines are utf-8");
        let completion = lines.lines().last().expect("init prints a line").to_owned();
        assert!(
            completion.starts_with(expected),
            "init {shell} printed {completion:?}"
        );
        assert!(
            completion.contains(BIN),
            "init {shell} printed {completion:?}"
        );
        // The first line makes the terminal a session, before completion runs.
        assert!(
            lines.starts_with("export KEYCLOAK_CLI_SESSION=")
                || lines.starts_with("set -gx KEYCLOAK_CLI_SESSION"),
            "init {shell} printed {lines:?}"
        );
    }
}

#[test]
fn init_rejects_an_unknown_shell() {
    let output = Command::new(BIN)
        .args(["init", "nushell"])
        .output()
        .expect("the binary runs");
    assert_eq!(
        output.status.code(),
        Some(2),
        "clap usage errors exit with 2"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("bash"), "{stderr}");
}
