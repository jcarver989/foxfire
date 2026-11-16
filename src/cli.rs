use crate::agents::{CommandLine, parse_command_line};
use crate::branding::{ABOUT, CLIENT_NAME};
use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(name = CLIENT_NAME, about = ABOUT)]
pub struct Cli {
    #[arg(
        long = "agent",
        help = "Command line of an ACP agent (speaking over stdin/stdout) to offer first in every project, named \
                after its program",
        value_parser = parse_agent_command
    )]
    pub agent: Option<CommandLine>,
}

fn parse_agent_command(command_line: &str) -> Result<CommandLine, String> {
    parse_command_line(command_line).map_err(|error| error.to_string())
}
