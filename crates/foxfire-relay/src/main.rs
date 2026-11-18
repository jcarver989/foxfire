use clap::Parser;
use foxfire_relay::{Cli, Relay, RelayError};
use std::env::current_dir;
use std::path::absolute;
use std::process::ExitCode;
use tokio::process::Command;
use tokio::runtime::Runtime;
use tracing_subscriber::EnvFilter;

fn main() -> ExitCode {
    if let Some(code) = clankerdiff_lspd::run_if_requested() {
        return code;
    }
    let Cli { listen, cwd, agent } = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with_writer(std::io::stderr)
        .init();
    let (program, args) = agent.split_first().expect("clap requires the agent's command");
    let mut command = Command::new(program);
    command.args(args);
    let io = |context: &str| {
        let context = context.to_owned();
        move |source| RelayError::Io { context, source }
    };
    let relayed = cwd.map_or_else(current_dir, absolute).map_err(io("find the workspace")).and_then(|cwd| {
        let runtime = Runtime::new().map_err(io("start the runtime"))?;
        runtime.block_on(Relay::new(cwd).run(listen, command))
    });
    match relayed {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "relay stopped");
            ExitCode::FAILURE
        }
    }
}
