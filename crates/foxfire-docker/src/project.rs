use crate::error::Error;
use crate::process::{docker, docker_shown, git};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};

/// The label naming the project a container or image belongs to.
pub const PROJECT_LABEL: &str = "dev.foxfire.project";
/// The label naming the commit an image's workspace was warmed at.
pub const WARMED_LABEL: &str = "dev.foxfire.warmed";
/// The branch an instance's agent works on.
pub const TASK_BRANCH: &str = "foxfire/task";

const DOCKERFILE: &str = include_str!("../Dockerfile");
const ENTRYPOINT: &str = include_str!("../entrypoint.sh");
const WORKSPACE: &str = "/workspace";
const RELAY_PORT: u16 = 7777;
/// The file `entrypoint.sh` looks for.
const EXPORT_MARKER: &str = "/run/foxfire-exporting";
const EXPORT_REF: &str = "refs/foxfire/export";

/// Records the work as one commit: `HEAD` if the working tree is clean,
/// otherwise a commit on top of it holding the working tree, ignored files
/// excluded.
const SNAPSHOT: &str = r#"git add --all
tree=$(git write-tree)
if [ "$tree" = "$(git rev-parse 'HEAD^{tree}')" ]; then
    commit=$(git rev-parse HEAD)
else
    commit=$(git -c user.name=Foxfire -c user.email=foxfire@localhost \
        commit-tree "$tree" -p HEAD -m 'Uncommitted work at Stop')
fi
git update-ref refs/foxfire/export "$commit"
"#;

/// An instance's state, in the vocabulary of the desktop's remotes contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InstanceState {
    Starting,
    Running,
    Suspended,
    Stopping,
    Ended,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Instance {
    pub id: String,
    pub state: InstanceState,
}

/// Where the desktop connects to an instance's relay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Endpoint {
    pub url: String,
    pub headers: BTreeMap<String, String>,
}

/// A project whose instances run as containers. Its host checkout supplies
/// the upstream URL and credentials and receives each instance's work under
/// `refs/foxfire/<project>/`. The checkout is never mounted into a container.
#[derive(Debug, Clone)]
pub struct Project {
    name: String,
    checkout: PathBuf,
    remote: String,
    branch: String,
    image: String,
}

impl Project {
    /// A project launching from `origin`'s `main` on the image
    /// `<name>-dev:warm`.
    pub fn new(name: &str, checkout: impl Into<PathBuf>) -> Result<Self, Error> {
        let valid = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-';
        if name.is_empty() || !name.bytes().all(valid) {
            return Err(Error::InvalidName(name.to_owned()));
        }
        Ok(Self {
            name: name.to_owned(),
            checkout: checkout.into(),
            remote: "origin".to_owned(),
            branch: "main".to_owned(),
            image: format!("{name}-dev:warm"),
        })
    }

    pub fn upstream(mut self, remote: &str, branch: &str) -> Self {
        remote.clone_into(&mut self.remote);
        branch.clone_into(&mut self.branch);
        self
    }

    pub fn image(mut self, image: &str) -> Self {
        image.clone_into(&mut self.image);
        self
    }

    /// The ref in the checkout that holds an instance's launch commit while
    /// it runs, and its work once it is stopped.
    pub fn work_ref(&self, id: &str) -> String {
        format!("refs/foxfire/{}/{}", self.name, &id[..id.len().min(12)])
    }

    /// Builds the project's image from its freshly fetched upstream branch
    /// and returns the commit it was warmed at. The image tag only moves once
    /// the build succeeds.
    ///
    /// `relay_source` is the project whose upstream supplies `foxfire-relay`,
    /// when that is not this project. `dockerfile` replaces the built-in one.
    pub fn warm(&self, relay_source: Option<&Self>, dockerfile: Option<&Path>) -> Result<String, Error> {
        let scratch = TempDir::new().map_err(|source| Error::Io { path: std::env::temp_dir(), source })?;
        let context = scratch.path().join("context");
        let project_src = scratch.path().join("project");
        let sha = self.bundle(&project_src)?;
        let (relay_src, relay_sha) = match relay_source {
            Some(source) => {
                let dir = scratch.path().join("relay");
                let sha = source.bundle(&dir)?;
                (dir, sha)
            }
            None => (project_src.clone(), sha.clone()),
        };
        write(&context.join("entrypoint.sh"), ENTRYPOINT)?;
        let built_in = context.join("Dockerfile");
        write(&built_in, DOCKERFILE)?;

        docker_shown(&[
            "build",
            "--file",
            &text(dockerfile.unwrap_or(&built_in)),
            "--tag",
            &self.image,
            "--build-context",
            &format!("project-src={}", text(&project_src)),
            "--build-context",
            &format!("foxfire-src={}", text(&relay_src)),
            "--build-arg",
            &format!("SOURCE_SHA={sha}"),
            "--build-arg",
            &format!("TOOLS_SHA={relay_sha}"),
            "--label",
            &self.label(),
            "--label",
            &format!("{WARMED_LABEL}={sha}"),
            &text(&context),
        ])?;
        Ok(sha)
    }

    /// Starts an instance at the commit just fetched from upstream, on the
    /// project's warm image, and returns its container ID.
    pub fn launch(&self, env_file: Option<&Path>) -> Result<String, Error> {
        let image = docker(&["image", "inspect", "--format", "{{.Id}}", &self.image]).map_err(|error| {
            if error.is_missing() {
                Error::NoImage { image: self.image.clone(), project: self.name.clone() }
            } else {
                error
            }
        })?;
        let label = self.label();
        let publish = format!("127.0.0.1::{RELAY_PORT}");
        let env_file = env_file.map(text);
        let mut args = vec!["container", "create", "--init", "--pull", "never"];
        args.extend(["--log-driver", "local", "--log-opt", "max-size=10m", "--log-opt", "max-file=3"]);
        args.extend(["--publish", &publish, "--label", &label]);
        if let Some(env_file) = &env_file {
            args.extend(["--env-file", env_file]);
        }
        args.push(&image);
        let id = docker(&args)?;
        if let Err(error) = self.start_at_upstream(&id) {
            let _ = docker(&["container", "rm", "--force", &id]);
            let _ = git(&self.checkout, &["update-ref", "-d", &self.work_ref(&id)]);
            return Err(error);
        }
        Ok(id)
    }

    pub fn list(&self) -> Result<Vec<Instance>, Error> {
        let filter = format!("label={}", self.label());
        let listed =
            docker(&["container", "ls", "--all", "--no-trunc", "--filter", &filter, "--format", "{{.ID}} {{.State}}"])?;
        Ok(listed
            .lines()
            .filter_map(|line| line.split_once(' '))
            .map(|(id, state)| Instance { id: id.to_owned(), state: instance_state(state) })
            .collect())
    }

    /// The loopback address a running instance's relay is published on.
    pub fn endpoint(&self, id: &str) -> Result<Endpoint, Error> {
        if !self.owns(id)? {
            return Err(Error::UnknownInstance(id.to_owned()));
        }
        let published = docker(&["container", "port", id, &format!("{RELAY_PORT}/tcp")]).unwrap_or_default();
        let address = published
            .lines()
            .filter_map(|line| line.trim().parse::<SocketAddr>().ok())
            .find(SocketAddr::is_ipv4)
            .ok_or_else(|| Error::NotRunning(id.to_owned()))?;
        Ok(Endpoint { url: format!("ws://{address}"), headers: BTreeMap::new() })
    }

    /// Finishes an instance: exports its commits and working tree to
    /// [`Project::work_ref`] in the checkout, and only then removes the
    /// container. Safe to repeat after a failure, which leaves the container
    /// in place.
    pub fn stop(&self, id: &str) -> Result<(), Error> {
        let work_ref = self.work_ref(id);
        if !self.owns(id)? {
            return match git(&self.checkout, &["rev-parse", "--verify", "--quiet", &work_ref]) {
                Ok(_) => Ok(()),
                Err(_) => Err(Error::UnknownInstance(id.to_owned())),
            };
        }
        // Stopping the container is what quiesces the agent: no process
        // survives it, whatever the agent started.
        docker(&["container", "stop", id])?;
        let marker = NamedTempFile::new().map_err(|source| Error::Io { path: std::env::temp_dir(), source })?;
        docker(&["container", "cp", &text(marker.path()), &format!("{id}:{EXPORT_MARKER}")])?;
        docker(&["container", "start", id])?;
        docker(&["container", "exec", "--workdir", WORKSPACE, id, "sh", "-ec", SNAPSHOT])?;
        let refspec = format!("+{EXPORT_REF}:{work_ref}");
        self.transfer(&["fetch", "--quiet", "--no-write-fetch-head", "--no-tags", &transport(id), &refspec])?;
        docker(&["container", "rm", "--force", id]).map(drop)
    }

    fn start_at_upstream(&self, id: &str) -> Result<(), Error> {
        let sha = self.fetch(&self.work_ref(id))?;
        docker(&["container", "start", id])?;
        self.transfer(&["push", "--quiet", &transport(id), &format!("+{sha}:refs/foxfire/base")])?;
        docker(&[
            "container",
            "exec",
            "--workdir",
            WORKSPACE,
            id,
            "git",
            "checkout",
            "--quiet",
            "-B",
            TASK_BRANCH,
            &sha,
        ])
        .map(drop)
    }

    /// Fetches the upstream branch into `reference` in the checkout and
    /// returns its commit.
    fn fetch(&self, reference: &str) -> Result<String, Error> {
        // Fetching by URL keeps Git from also moving the checkout's
        // remote-tracking branch, as it would for a named remote.
        let url = git(&self.checkout, &["remote", "get-url", &self.remote])?;
        let refspec = format!("+refs/heads/{}:{reference}", self.branch);
        git(&self.checkout, &["fetch", "--quiet", "--no-write-fetch-head", "--no-tags", "--", &url, &refspec])?;
        git(&self.checkout, &["rev-parse", reference])
    }

    /// Writes the freshly fetched upstream branch to `dir/repo.bundle`, as a
    /// build context, and returns its commit.
    fn bundle(&self, dir: &Path) -> Result<String, Error> {
        let reference = format!("refs/foxfire/{}/warm", self.name);
        let sha = self.fetch(&reference)?;
        fs::create_dir_all(dir).map_err(|source| Error::Io { path: dir.to_owned(), source })?;
        git(&self.checkout, &["bundle", "create", "--quiet", &text(&dir.join("repo.bundle")), &reference])?;
        Ok(sha)
    }

    /// Runs a Git command in the checkout that talks to a container.
    fn transfer(&self, args: &[&str]) -> Result<String, Error> {
        let mut allowed = vec!["-c", "protocol.ext.allow=always"];
        allowed.extend_from_slice(args);
        git(&self.checkout, &allowed)
    }

    /// Whether `id` is a container of this project. Refuses anyone else's.
    fn owns(&self, id: &str) -> Result<bool, Error> {
        let hex = |byte: u8| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte);
        if !(12..=64).contains(&id.len()) || !id.bytes().all(hex) {
            return Err(Error::InvalidId(id.to_owned()));
        }
        let format = format!("{{{{index .Config.Labels \"{PROJECT_LABEL}\"}}}}");
        match docker(&["container", "inspect", "--format", &format, id]) {
            Ok(project) if project == self.name => Ok(true),
            Ok(_) => Err(Error::NotOwned { id: id.to_owned(), project: self.name.clone() }),
            Err(error) if error.is_missing() => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn label(&self) -> String {
        format!("{PROJECT_LABEL}={}", self.name)
    }
}

/// A Git remote that reaches the workspace of a running container through
/// `docker exec`. Git negotiates what to send, so nothing is mounted or
/// copied by hand.
fn transport(id: &str) -> String {
    format!("ext::docker container exec --interactive {id} %S {WORKSPACE}")
}

fn instance_state(docker_state: &str) -> InstanceState {
    match docker_state {
        "created" | "restarting" => InstanceState::Starting,
        "running" => InstanceState::Running,
        "paused" | "exited" => InstanceState::Suspended,
        "removing" => InstanceState::Stopping,
        _ => InstanceState::Ended,
    }
}

fn write(file: &Path, contents: &str) -> Result<(), Error> {
    let io_error = |source| Error::Io { path: file.to_owned(), source };
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir).map_err(io_error)?;
    }
    fs::write(file, contents).map_err(io_error)
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
