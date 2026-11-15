use std::process::ExitCode;

use keycloak_cli::Cli;

fn main() -> ExitCode {
    match keycloak_cli::run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(error.exit_code())
        }
    }
}
