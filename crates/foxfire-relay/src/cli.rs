use clap::Parser;
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "foxfire-relay",
    version,
    about = "Serves a stdio ACP agent, and review of the workspace it works in, to Foxfire clients over WebSocket"
)]
pub struct Cli {
    #[arg(
        long,
        env = "FOXFIRE_RELAY_LISTEN",
        default_value = "127.0.0.1:7777",
        help = "The address to accept clients on"
    )]
    pub listen: SocketAddr,

    #[arg(long, env = "FOXFIRE_RELAY_CWD", help = "The workspace the agent works in. Defaults to the current directory")]
    pub cwd: Option<PathBuf>,

    #[arg(required = true, last = true, help = "The agent's command line, after `--`")]
    pub agent: Vec<String>,
}
