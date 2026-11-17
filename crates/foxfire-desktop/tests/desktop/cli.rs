use clap::{CommandFactory, Parser};
use foxfire_desktop::agents::{CommandLine, default_agents_file, project_agents_file};
use foxfire_desktop::branding::{APP_NAME, CLIENT_NAME};
use foxfire_desktop::cli::Cli;
use foxfire_desktop::registry::default_state_dir;
use std::io;
use std::path::Path;
use std::process::Command;

#[test]
fn no_agent_is_passed_by_default() {
    let cli = Cli::parse_from(["foxfire-desktop"]);
    assert_eq!(cli.agent, None);
}

#[test]
fn the_agent_accepts_a_full_command_line() {
    let cli = Cli::parse_from(["foxfire-desktop", "--agent", "cargo run -p aether-agent-cli acp"]);
    assert_eq!(cli.agent, Some(CommandLine::new("cargo").args(["run", "-p", "aether-agent-cli", "acp"])));
}

#[test]
fn an_agent_command_line_that_cannot_be_split_is_rejected() {
    assert!(Cli::try_parse_from(["foxfire-desktop", "--agent", "aether 'acp"]).is_err());
}

#[test]
fn the_desktop_client_identifies_itself_as_foxfire() {
    assert_eq!(APP_NAME, "Foxfire");
    assert_eq!(CLIENT_NAME, "foxfire-desktop");
    assert_eq!(Cli::command().get_name(), "foxfire-desktop");
}

#[test]
fn project_agents_belong_to_foxfire() {
    let project = Path::new("/project");
    assert_eq!(project_agents_file(project), project.join(".foxfire/acp.json"));
}

#[test]
fn desktop_paths_use_the_foxfire_home() -> io::Result<()> {
    const TEST: &str = "cli::desktop_paths_use_the_foxfire_home";
    const CHILD: &str = "FOXFIRE_PATHS_TEST_CHILD";
    if let Some(home) = std::env::var_os(CHILD) {
        let desktop = Path::new(&home).join("desktop");
        assert_eq!(default_state_dir(), Some(desktop.clone()));
        assert_eq!(default_agents_file(), Some(desktop.join("acp.json")));
        return Ok(());
    }

    let root = tempfile::tempdir()?;
    for override_home in [None, Some(""), Some("custom")] {
        let home = override_home
            .filter(|value| !value.is_empty())
            .map_or_else(|| root.path().join(".foxfire"), |value| root.path().join(value));
        let mut command = Command::new(std::env::current_exe()?);
        command.args(["--exact", TEST, "--nocapture"]).env(CHILD, &home).env("HOME", root.path());
        if let Some(value) = override_home {
            command.env("FOXFIRE_HOME", if value.is_empty() { Path::new("") } else { &home });
        } else {
            command.env_remove("FOXFIRE_HOME");
        }
        let output = command.output()?;
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}
