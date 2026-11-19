use clap::Parser;
use foxfire_docker::cli::Cli;
use std::process::ExitCode;

fn main() -> ExitCode {
    match Cli::parse().run() {
        Ok(()) => ExitCode::SUCCESS,
        // The desktop reports the last line of stderr as the error.
        Err(error) => {
            eprintln!("foxfire-docker: {error}");
            ExitCode::FAILURE
        }
    }
}
