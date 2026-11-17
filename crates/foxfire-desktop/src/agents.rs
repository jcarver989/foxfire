pub use agent_client_protocol::AcpAgentConfig as CommandLine;

use crate::variables::{VarError, Vars};
use agent_client_protocol::AcpAgent;
use client_core::fs::{JsonFileError, read_json, write_json_atomic};
use rouge::reactive::{Signal, Stored, signal, stored};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::str::FromStr;

pub const AGENTS_FILE: &str = "acp.json";

/// Added to the user's agents file the first time it is found on `PATH`.
pub const AUTO_ADDED_AGENT: KnownAgent = KnownAgent { name: "aether", command: "aether", args: &["acp"] };

/// ACP agents offered for adding when a project has none.
pub const KNOWN_AGENTS: &[KnownAgent] = &[
    AUTO_ADDED_AGENT,
    KnownAgent { name: "claude-code-acp", command: "claude-code-acp", args: &[] },
    KnownAgent { name: "gemini", command: "gemini", args: &["--experimental-acp"] },
    KnownAgent { name: "codex-acp", command: "codex-acp", args: &[] },
];

/// The agents each project can start threads with: the one passed with `--agent`, then those of the user's file,
/// then those of the project's own file.
pub struct Agents {
    extra: Option<CommandLine>,
    user_file: Option<PathBuf>,
    catalogs: HashMap<PathBuf, Rc<AgentCatalog>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownAgent {
    pub name: &'static str,
    pub command: &'static str,
    pub args: &'static [&'static str],
}

#[derive(Debug)]
pub struct AgentCatalog {
    agents: Vec<AgentProfile>,
    problems: Vec<JsonFileError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentProfile {
    pub name: String,
    pub workspace: PathBuf,
    pub config: AgentConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AgentConfig {
    #[serde(flatten)]
    pub transport: AgentTransport,
    #[serde(default)]
    pub diff: Option<EndpointSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum AgentTransport {
    Stdio(CommandLine),
    WebSocket(EndpointSource),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "EndpointFields")]
pub enum EndpointSource {
    Fixed(Endpoint),
    Command(CommandLine),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Endpoint {
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AgentConfigError {
    #[error("a websocket agent or `diff` needs a `url` or an `endpoint` command")]
    MissingEndpoint,
    #[error("a websocket agent or `diff` takes a `url` or an `endpoint` command, not both")]
    ConflictingEndpoints,
    #[error("`headers` go with a `url`, since an `endpoint` command prints its own")]
    HeadersWithoutUrl,
}

#[derive(Debug, thiserror::Error)]
pub enum AddAgentError {
    #[error("there is no home directory to keep the agents file in")]
    NoUserFile,
    #[error(transparent)]
    File(#[from] JsonFileError),
}

pub fn default_agents_file() -> Option<PathBuf> {
    client_core::path::app_home("AETHER_HOME", ".aether").map(|home| home.join("desktop").join(AGENTS_FILE))
}

pub fn project_agents_file(project: &Path) -> PathBuf {
    project.join(".aether").join(AGENTS_FILE)
}

impl Agents {
    /// `extra` is offered first in every project, under its program's name.
    pub fn new(extra: Option<CommandLine>, user_file: Option<PathBuf>) -> Self {
        Self { extra, user_file, catalogs: HashMap::new() }
    }

    pub fn catalog(&mut self, project: &Path) -> Rc<AgentCatalog> {
        if let Some(catalog) = self.catalogs.get(project) {
            return catalog.clone();
        }
        let catalog = Rc::new(read_catalog(self.extra.as_ref(), self.user_file.as_deref(), project));
        self.catalogs.insert(project.to_path_buf(), catalog.clone());
        catalog
    }

    pub fn user_file(&self) -> Option<&Path> {
        self.user_file.as_deref()
    }

    /// Writes `agent` into the user's agents file, keeping an entry of the same name the user already has.
    pub fn add(&mut self, agent: KnownAgent) -> Result<(), AddAgentError> {
        let file = self.user_file.as_deref().ok_or(AddAgentError::NoUserFile)?;
        let mut config = read_json::<UserFile>(file)?.unwrap_or_default();
        if !config.agents.contains_key(agent.name) {
            let entry = json!({ "type": "stdio", "command": agent.command, "args": agent.args });
            config.agents.insert(agent.name.to_owned(), entry);
            write_json_atomic(file, &config)?;
        }
        self.reload();
        Ok(())
    }

    pub fn reload(&mut self) {
        self.catalogs.clear();
    }
}

#[derive(Clone, Copy)]
pub struct Catalogs {
    agents: Stored<Agents>,
    revision: Signal<u64>,
}

impl Catalogs {
    pub fn new(agents: Agents) -> Self {
        Self { agents: stored(agents), revision: signal(0) }
    }

    pub fn catalog(&self, project: &Path) -> Rc<AgentCatalog> {
        self.revision.get();
        self.agents.update(|agents| agents.catalog(project))
    }

    pub fn revision(&self) -> u64 {
        self.revision.get()
    }

    pub fn user_file(&self) -> Option<PathBuf> {
        self.agents.with(|agents| agents.user_file().map(Path::to_path_buf))
    }

    pub fn add(&self, agent: KnownAgent) -> Result<(), AddAgentError> {
        self.agents.update(|agents| agents.add(agent))?;
        self.revision.update(|revision| *revision += 1);
        Ok(())
    }

    pub fn reload(&self) {
        self.agents.update(Agents::reload);
        self.revision.update(|revision| *revision += 1);
    }
}

impl AgentCatalog {
    pub fn agents(&self) -> &[AgentProfile] {
        &self.agents
    }

    pub fn get(&self, name: &str) -> Option<&AgentProfile> {
        self.agents.iter().find(|agent| agent.name == name)
    }

    pub fn default_agent(&self) -> Option<&AgentProfile> {
        self.agents.first()
    }

    pub fn problems(&self) -> &[JsonFileError] {
        &self.problems
    }
}

impl AgentProfile {
    pub(crate) fn vars(&self) -> Vars {
        Vars::new(self.workspace.to_string_lossy())
    }
}

pub fn parse_command_line(command_line: &str) -> Result<CommandLine, agent_client_protocol::Error> {
    Ok(AcpAgent::from_str(command_line)?.into_config())
}

pub(crate) fn expand_command_line(line: &CommandLine, vars: &Vars) -> Result<CommandLine, VarError> {
    let command = vars.expand(&line.command().to_string_lossy())?;
    let args = line.arguments().iter().map(|arg| vars.expand(arg)).collect::<Result<Vec<_>, _>>()?;
    Ok(CommandLine::new(command).args(args).envs(expand_values(line.environment(), vars)?))
}

impl Endpoint {
    pub(crate) fn expand(&self, vars: &Vars) -> Result<Self, VarError> {
        Ok(Self { url: vars.expand(&self.url)?, headers: expand_values(&self.headers, vars)? })
    }
}

impl TryFrom<EndpointFields> for EndpointSource {
    type Error = AgentConfigError;

    fn try_from(fields: EndpointFields) -> Result<Self, Self::Error> {
        match (fields.url, fields.endpoint, fields.headers) {
            (Some(url), None, headers) => Ok(Self::Fixed(Endpoint { url, headers: headers.unwrap_or_default() })),
            (None, Some(command), None) => Ok(Self::Command(command)),
            (None, Some(_), Some(_)) => Err(AgentConfigError::HeadersWithoutUrl),
            (Some(_), Some(_), _) => Err(AgentConfigError::ConflictingEndpoints),
            (None, None, _) => Err(AgentConfigError::MissingEndpoint),
        }
    }
}

#[derive(Default, Deserialize)]
struct AcpConfig {
    #[serde(default)]
    agents: BTreeMap<String, AgentConfig>,
}

/// The user's agents file as written back, keeping whatever else it holds.
#[derive(Default, Serialize, Deserialize)]
struct UserFile {
    #[serde(default)]
    agents: Map<String, Value>,
    #[serde(flatten)]
    rest: Map<String, Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EndpointFields {
    url: Option<String>,
    headers: Option<BTreeMap<String, String>>,
    endpoint: Option<CommandLine>,
}

fn read_catalog(extra: Option<&CommandLine>, user_file: Option<&Path>, project: &Path) -> AgentCatalog {
    let mut configs = BTreeMap::new();
    let mut problems = Vec::new();
    for file in user_file.map(Path::to_path_buf).into_iter().chain([project_agents_file(project)]) {
        match read_json::<AcpConfig>(&file) {
            Ok(config) => configs.extend(config.unwrap_or_default().agents),
            Err(error) => {
                tracing::warn!(%error, "leaving out the agents of a file that cannot be read");
                problems.push(error);
            }
        }
    }
    let extra = extra.map(|line| {
        let name =
            line.command().file_name().map_or_else(|| line.command().to_string_lossy(), |name| name.to_string_lossy());
        configs.remove(name.as_ref());
        (name.into_owned(), AgentConfig { transport: AgentTransport::Stdio(line.clone()), diff: None })
    });
    let agents = extra
        .into_iter()
        .chain(configs)
        .map(|(name, config)| AgentProfile { name, workspace: project.to_path_buf(), config })
        .collect();
    AgentCatalog { agents, problems }
}

fn expand_values(values: &BTreeMap<String, String>, vars: &Vars) -> Result<BTreeMap<String, String>, VarError> {
    values.iter().map(|(name, value)| Ok((name.clone(), vars.expand(value)?))).collect()
}
