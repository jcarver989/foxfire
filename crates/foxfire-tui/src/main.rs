use clap::Parser;
use foxfire_tui::cli::Cli;
use foxfire_tui::run_tui;
use foxfire_tui::settings::load_or_create_settings;
use std::process::ExitCode;
use tokio::runtime::Runtime;

fn main() -> ExitCode {
    if let Some(code) = clankerdiff_lspd::run_if_requested() {
        return code;
    }
    let cli = Cli::parse();
    let result = Runtime::new().map_err(|e| e.to_string()).and_then(|runtime| {
        runtime
            .block_on(run_tui(&cli.agent, load_or_create_settings(), cli.log_dir.as_deref()))
            .map_err(|e| e.to_string())
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Fatal error: {e}");
            ExitCode::FAILURE
        }
    }
}
