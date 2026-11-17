use clap::Parser;
use foxfire_desktop::cli::Cli;
use std::process::ExitCode;
use tracing_subscriber::EnvFilter;

const DEFAULT_LOG_FILTER: &str = "warn";
const ROUGE_PROFILE: &str = "ROUGE_PROFILE";

fn main() -> ExitCode {
    if let Some(code) = clankerdiff_lspd::run_if_requested() {
        return code;
    }
    if std::env::var_os(ROUGE_PROFILE).is_none() {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| DEFAULT_LOG_FILTER.into()))
            .with_writer(std::io::stderr)
            .init();
    }
    foxfire_desktop::run(Cli::parse());
    ExitCode::SUCCESS
}
