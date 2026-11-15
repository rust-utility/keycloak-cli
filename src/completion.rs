//! Shell completion.
//!
//! Completion is dynamic: the shell runs the binary with `COMPLETE=<shell>` and a
//! command line to complete, the binary answers with candidates, so the tree and
//! the candidates always match the build that is installed (F8). That interface
//! is explicitly unstable on clap's side, which is why `init` prints a line that
//! re-sources the registration on every shell start instead of writing a file
//! once (F10).
//!
//! Live candidates (realms, users, groups, clients) are Q7; today the tree, the
//! flags and the help come back.

use std::path::Path;

use clap_complete::CompleteEnv;

pub use clap_complete::Shell;

/// Answers a completion request from a shell, exiting when one was served.
///
/// Must run before anything is written to stdout: the shell parses everything the
/// process prints as the answer.
pub fn complete() {
    CompleteEnv::with_factory(crate::cli::build_cli).complete();
}

/// The shell a name refers to, for `init`'s argument.
pub fn shell(name: &str) -> Option<Shell> {
    match name {
        "bash" => Some(Shell::Bash),
        "elvish" => Some(Shell::Elvish),
        "fish" => Some(Shell::Fish),
        "powershell" => Some(Shell::PowerShell),
        "zsh" => Some(Shell::Zsh),
        _ => None,
    }
}

/// The shells `init` accepts, for help and error messages.
pub const SHELLS: &[&str] = &["bash", "elvish", "fish", "powershell", "zsh"];

/// The line to add to a shell's startup file.
///
/// The binary path is written out, because the line runs before `PATH` is
/// necessarily useful; re-running `init` (or re-sourcing the line) is how a moved
/// or upgraded binary is picked up.
pub fn init(shell: Shell, program: &Path) -> String {
    let program = program.display();
    match shell {
        // `source <(…)` defines the function in a subshell on bash 3.2 (still the
        // default shell on macOS), so the registration is evaluated instead.
        Shell::Bash => format!("eval \"$(COMPLETE=bash {program})\""),
        Shell::Zsh => format!("source <(COMPLETE=zsh {program})"),
        Shell::Fish => format!("COMPLETE=fish {program} | source"),
        Shell::Elvish => format!("eval (E:COMPLETE=elvish {program} | slurp)"),
        Shell::PowerShell => format!(
            "$env:COMPLETE = \"powershell\"; {program} | Out-String | Invoke-Expression; Remove-Item Env:\\COMPLETE"
        ),
        other => format!("# {other} has no dynamic completion; use the static generator"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn names_map_to_shells() {
        for name in SHELLS {
            assert!(shell(name).is_some(), "{name}");
        }
        assert!(shell("nu").is_none());
        assert!(shell("Bash").is_none());
    }

    #[test]
    fn init_prints_a_line_that_sources_the_binary() {
        let program = PathBuf::from("/opt/keycloak-cli");
        let bash = init(Shell::Bash, &program);
        assert_eq!(bash, "eval \"$(COMPLETE=bash /opt/keycloak-cli)\"");
        assert!(init(Shell::Zsh, &program).contains("COMPLETE=zsh /opt/keycloak-cli"));
        assert!(init(Shell::Fish, &program).contains("COMPLETE=fish /opt/keycloak-cli | source"));
        assert!(init(Shell::Elvish, &program).contains("COMPLETE=elvish /opt/keycloak-cli"));
        assert!(init(Shell::PowerShell, &program).contains("COMPLETE = \"powershell\""));
    }
}
