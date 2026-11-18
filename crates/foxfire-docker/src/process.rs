use crate::error::Error;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

/// Runs Git in `dir` and returns its standard output, trimmed.
pub(crate) fn git(dir: &Path, args: &[&str]) -> Result<String, Error> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(dir)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_TERMINAL_PROMPT", "0");
    capture(command, "git", args)
}

/// Runs Docker and returns its standard output, trimmed.
pub(crate) fn docker(args: &[&str]) -> Result<String, Error> {
    let mut command = Command::new("docker");
    command.args(args);
    capture(command, "docker", args)
}

/// Runs Docker with its output shown on standard error, for long builds.
pub(crate) fn docker_shown(args: &[&str]) -> Result<(), Error> {
    let status = Command::new("docker")
        .args(args)
        .stdin(Stdio::null())
        .stdout(io::stderr())
        .status()
        .map_err(|source| Error::Spawn { program: "docker", source })?;
    if status.success() {
        return Ok(());
    }
    Err(Error::Failed { command: describe("docker", args), message: format!("{status}; see the output above") })
}

fn capture(mut command: Command, program: &'static str, args: &[&str]) -> Result<String, Error> {
    let output = command.stdin(Stdio::null()).output().map_err(|source| Error::Spawn { program, source })?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let message = stderr.lines().map(str::trim).rfind(|line| !line.is_empty()).unwrap_or("no output").to_owned();
    Err(Error::Failed { command: describe(program, args), message })
}

/// Shows only the first line of each argument, since the desktop reports just
/// the last line of an error and one argument is a whole script.
fn describe(program: &str, args: &[&str]) -> String {
    let words: Vec<&str> = args.iter().filter_map(|arg| arg.lines().next()).collect();
    format!("{program} {}", words.join(" "))
}
