use std::process::ExitCode;

use keycloak_cli::Cli;

fn main() -> ExitCode {
    // A shell asking for candidates must be answered before anything else runs.
    keycloak_cli::completion::complete();

    match keycloak_cli::run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(error.exit_code())
        }
    }
}
