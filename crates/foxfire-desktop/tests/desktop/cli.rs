use clap::Parser;
use foxfire_desktop::agents::CommandLine;
use foxfire_desktop::cli::Cli;

#[test]
fn no_agent_is_passed_by_default() {
    let cli = Cli::parse_from(["aether-desktop"]);
    assert_eq!(cli.agent, None);
}

#[test]
fn the_agent_accepts_a_full_command_line() {
    let cli = Cli::parse_from(["aether-desktop", "--agent", "cargo run -p aether-agent-cli acp"]);
    assert_eq!(cli.agent, Some(CommandLine::new("cargo").args(["run", "-p", "aether-agent-cli", "acp"])));
}

#[test]
fn an_agent_command_line_that_cannot_be_split_is_rejected() {
    assert!(Cli::try_parse_from(["aether-desktop", "--agent", "aether 'acp"]).is_err());
}
