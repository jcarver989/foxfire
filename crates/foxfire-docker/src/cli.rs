use crate::error::Error;
use crate::project::Project;
use clap::{Args, Parser, Subcommand};
use serde_json::json;
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "foxfire-docker",
    version,
    about = "Runs isolated, warm-built Foxfire Cloud workspaces on a local Docker engine",
    long_about = "Runs isolated, warm-built Foxfire Cloud workspaces on a local Docker engine.\n\n\
        Run it from the project's checkout, or pass --checkout. \
        Only protocol output goes to stdout; errors go to stderr."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Build the project's image from its upstream branch
    Warm {
        #[command(flatten)]
        target: Target,
        /// A Foxfire checkout to build `foxfire-relay` from, when the project is not Foxfire
        #[arg(long)]
        foxfire: Option<PathBuf>,
        /// A Dockerfile to use instead of the built-in one
        #[arg(long)]
        dockerfile: Option<PathBuf>,
    },
    /// Fetch the project's upstream branch and start an instance at it; prints `{"id": …}`
    Launch {
        #[command(flatten)]
        target: Target,
        /// A Docker env file holding the agent's model credentials
        #[arg(long)]
        env_file: Option<PathBuf>,
    },
    /// List the project's instances; prints `{"instances": [{"id": …, "state": …}]}`
    List {
        #[command(flatten)]
        target: Target,
    },
    /// Print the WebSocket address of a running instance's relay
    Endpoint {
        #[command(flatten)]
        target: Target,
        id: String,
    },
    /// Export an instance's work to `refs/foxfire/<project>/<id>` in the checkout, then remove it
    Stop {
        #[command(flatten)]
        target: Target,
        id: String,
    },
}

#[derive(Debug, Args)]
pub struct Target {
    /// The project's name, which labels its containers and names its refs
    pub project: String,
    /// The project's host checkout
    #[arg(long, default_value = ".")]
    pub checkout: PathBuf,
    #[arg(long, default_value = "origin")]
    pub remote: String,
    #[arg(long, default_value = "main")]
    pub branch: String,
    /// The project's image. Defaults to `<project>-dev:warm`
    #[arg(long)]
    pub image: Option<String>,
}

impl Cli {
    pub fn run(self) -> Result<(), Error> {
        let mut stdout = io::stdout();
        match self.command {
            Command::Warm { target, foxfire, dockerfile } => {
                let relay_source = foxfire.map(|checkout| Project::new("foxfire", checkout)).transpose()?;
                let sha = target.project()?.warm(relay_source.as_ref(), dockerfile.as_deref())?;
                writeln!(stdout, "{}", json!({ "sha": sha }))?;
            }
            Command::Launch { target, env_file } => {
                let id = target.project()?.launch(env_file.as_deref())?;
                writeln!(stdout, "{}", json!({ "id": id }))?;
            }
            Command::List { target } => {
                writeln!(stdout, "{}", json!({ "instances": target.project()?.list()? }))?;
            }
            Command::Endpoint { target, id } => {
                writeln!(stdout, "{}", json!(target.project()?.endpoint(&id)?))?;
            }
            Command::Stop { target, id } => target.project()?.stop(&id)?,
        }
        Ok(())
    }
}

impl Target {
    fn project(self) -> Result<Project, Error> {
        let project = Project::new(&self.project, self.checkout)?.upstream(&self.remote, &self.branch);
        Ok(match self.image {
            Some(image) => project.image(&image),
            None => project,
        })
    }
}
