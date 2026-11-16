use crate::support::{replying, write_agents_file};
use aether_desktop::agents::{
    AgentCatalog, AgentTransport, Agents, CommandLine, Endpoint, EndpointSource, KNOWN_AGENTS, KnownAgent,
    project_agents_file,
};
use aether_desktop::testing::{DesktopHarness, DesktopHarnessBuilder};
use aether_desktop::threads::Thread;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use utils::fs::JsonFileError;

#[test]
fn a_project_offers_the_agents_its_file_defines_in_name_order() {
    let project = tempfile::tempdir().unwrap();
    write_agents_file(
        project.path(),
        &json!({ "agents": {
            "zed": { "type": "stdio", "command": "zed-agent" },
            "claude": { "type": "stdio", "command": "npx", "args": ["-y", "@zed-industries/claude-code-acp"] },
        }}),
    );

    let catalog = agents(None).catalog(project.path());

    assert_eq!(names(&catalog), ["claude", "zed"]);
    assert_eq!(catalog.default_agent().unwrap().name, "claude");
    assert_eq!(catalog.get("claude").unwrap().workspace, project.path());
}

#[test]
fn a_project_without_agents_has_no_default() {
    let project = tempfile::tempdir().unwrap();

    let catalog = agents(None).catalog(project.path());

    assert_eq!(names(&catalog), Vec::<String>::new());
    assert_eq!(catalog.default_agent(), None);
}

#[test]
fn the_agent_passed_on_the_command_line_comes_first_named_after_its_program() {
    let project = tempfile::tempdir().unwrap();
    write_agents_file(
        project.path(),
        &json!({ "agents": {
            "claude": { "type": "stdio", "command": "claude-code-acp" },
            "gemini": { "type": "stdio", "command": "gemini-from-file" },
        }}),
    );
    let line = CommandLine::new("/opt/bin/gemini").args(["--experimental-acp"]);

    let catalog = Agents::new(Some(line.clone()), None).catalog(project.path());

    assert_eq!(names(&catalog), ["gemini", "claude"]);
    assert_eq!(catalog.default_agent().unwrap().config.transport, AgentTransport::Stdio(line));
}

#[test]
fn a_project_agent_replaces_the_users_of_the_same_name() {
    let (home, project) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let user_file = home.path().join("acp.json");
    fs::write(
        &user_file,
        json!({ "agents": {
            "aether": { "type": "stdio", "command": "aether-dev", "args": ["acp"] },
            "claude": { "type": "stdio", "command": "user-claude" },
        }})
        .to_string(),
    )
    .unwrap();
    write_agents_file(
        project.path(),
        &json!({ "agents": { "claude": { "type": "stdio", "command": "project-claude" } } }),
    );

    let catalog = agents(Some(user_file)).catalog(project.path());

    assert_eq!(names(&catalog), ["aether", "claude"]);
    assert_eq!(
        catalog.get("aether").unwrap().config.transport,
        AgentTransport::Stdio(CommandLine::new("aether-dev").args(["acp"]))
    );
    assert_eq!(
        catalog.get("claude").unwrap().config.transport,
        AgentTransport::Stdio(CommandLine::new("project-claude"))
    );
}

#[test]
fn adding_an_agent_writes_it_to_the_users_file_beside_what_it_already_holds() {
    let (home, project) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let user_file = home.path().join("acp.json");
    fs::write(&user_file, json!({ "agents": { "zed": { "type": "stdio", "command": "zed" } }, "x": 1 }).to_string())
        .unwrap();
    let mut agents = agents(Some(user_file.clone()));
    let gemini = known("gemini");

    agents.add(gemini).unwrap();

    assert_eq!(names(&agents.catalog(project.path())), ["gemini", "zed"]);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&fs::read_to_string(&user_file).unwrap()).unwrap(),
        json!({
            "agents": {
                "gemini": { "type": "stdio", "command": "gemini", "args": ["--experimental-acp"] },
                "zed": { "type": "stdio", "command": "zed" },
            },
            "x": 1,
        })
    );
}

#[test]
fn adding_an_agent_the_user_already_defines_keeps_their_definition() {
    let (home, project) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let user_file = home.path().join("acp.json");
    fs::write(&user_file, json!({ "agents": { "gemini": { "type": "stdio", "command": "my-gemini" } } }).to_string())
        .unwrap();
    let mut agents = agents(Some(user_file));

    agents.add(known("gemini")).unwrap();

    assert_eq!(
        agents.catalog(project.path()).default_agent().unwrap().config.transport,
        AgentTransport::Stdio(CommandLine::new("my-gemini"))
    );
}

#[test]
fn an_agent_is_not_written_over_a_users_file_that_cannot_be_read() {
    let home = tempfile::tempdir().unwrap();
    let user_file = home.path().join("acp.json");
    fs::write(&user_file, "{ not json").unwrap();

    assert!(agents(Some(user_file.clone())).add(known("gemini")).is_err());
    assert_eq!(fs::read_to_string(&user_file).unwrap(), "{ not json");
}

#[test]
fn an_agent_of_either_type_may_say_where_its_working_tree_is_diffed() {
    let project = tempfile::tempdir().unwrap();
    write_agents_file(
        project.path(),
        &json!({ "agents": {
            "claude": { "type": "stdio", "command": "claude-code-acp" },
            "claude (ssh)": {
                "type": "stdio",
                "command": "ssh",
                "diff": { "url": "ws://localhost:7331/ws", "headers": { "Authorization": "Bearer ${TOKEN}" } },
            },
            "codex (VM)": {
                "type": "websocket",
                "url": "wss://vm:8765",
                "diff": { "endpoint": { "command": "./diff-endpoint.sh", "args": ["vm"] } },
            },
        }}),
    );

    let catalog = agents(None).catalog(project.path());

    assert!(catalog.problems().is_empty());
    assert_eq!(catalog.get("claude").unwrap().config.diff, None);
    assert_eq!(
        catalog.get("claude (ssh)").unwrap().config.diff,
        Some(EndpointSource::Fixed(Endpoint {
            url: "ws://localhost:7331/ws".to_owned(),
            headers: [("Authorization".to_owned(), "Bearer ${TOKEN}".to_owned())].into(),
        }))
    );
    assert_eq!(
        catalog.get("codex (VM)").unwrap().config.diff,
        Some(EndpointSource::Command(CommandLine::new("./diff-endpoint.sh").args(["vm"])))
    );
}

#[test]
fn a_file_that_cannot_be_read_is_reported_and_its_agents_left_out() {
    let project = tempfile::tempdir().unwrap();
    let file = project_agents_file(project.path());
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, "{ not json").unwrap();

    let catalog = agents(None).catalog(project.path());

    assert_eq!(names(&catalog), Vec::<String>::new());
    assert_eq!(catalog.problems().iter().map(JsonFileError::path).collect::<Vec<_>>(), [file.as_path()]);
}

#[test]
fn an_agent_whose_fields_do_not_go_together_is_reported() {
    let command = json!({ "command": "true" });
    let invalid = [
        json!({ "type": "websocket" }),
        json!({ "type": "websocket", "url": "ws://host", "endpoint": command }),
        json!({ "type": "websocket", "endpoint": command, "headers": { "Authorization": "token" } }),
        json!({ "type": "websocket", "url": "ws://host", "terminate": command }),
        json!({ "type": "websocket", "endpoint": command, "launch": command }),
        json!({ "type": "stdio", "commnd": "aether" }),
        json!({ "type": "carrier-pigeon", "command": "aether" }),
        json!({ "type": "stdio", "command": "claude", "diff": {} }),
        json!({ "type": "stdio", "command": "claude", "diff": { "url": "ws://host", "endpoint": command } }),
        json!({ "type": "stdio", "command": "claude", "diff": { "endpoint": command, "headers": { "Authorization": "token" } } }),
        json!({ "type": "stdio", "command": "claude", "diff": { "uri": "ws://host" } }),
    ];

    for config in invalid {
        let project = tempfile::tempdir().unwrap();
        write_agents_file(project.path(), &json!({ "agents": { "broken": config } }));

        let catalog = agents(None).catalog(project.path());

        assert!(catalog.get("broken").is_none(), "{config} is left out");
        assert_eq!(catalog.problems().len(), 1, "{config} is reported");
    }
}

#[test]
fn edits_to_an_agents_file_show_once_the_agents_are_reloaded() {
    let project = tempfile::tempdir().unwrap();
    let mut agents = agents(None);
    let before = names(&agents.catalog(project.path()));
    write_agents_file(project.path(), &json!({ "agents": { "claude": { "type": "stdio", "command": "claude" } } }));
    let unreloaded = names(&agents.catalog(project.path()));

    agents.reload();

    assert_eq!(before, Vec::<String>::new());
    assert_eq!(unreloaded, Vec::<String>::new(), "a catalog is read once");
    assert_eq!(names(&agents.catalog(project.path())), ["claude"]);
}

#[test]
fn a_project_with_a_single_agent_starts_a_thread_with_it_at_once() {
    let project = Project::new(&json!({ "claude": { "type": "stdio", "command": "claude-code-acp" } }));
    let mut harness = project.harness(&["s1"]);

    harness.click("project-alpha-new-thread").unwrap();

    assert_eq!(agent_of(harness.selected_thread().unwrap()), "claude");
}

#[test]
fn a_project_with_several_agents_starts_a_thread_with_the_one_chosen_from_its_menu() {
    let project = Project::new(&json!({
        "claude": { "type": "stdio", "command": "claude-code-acp" },
        "zed": { "type": "stdio", "command": "zed-agent" },
    }));
    let mut harness = project.harness(&["s1"]);

    harness.click("project-alpha-new-thread").unwrap();
    harness.click("new-thread-agent-claude").unwrap();

    let thread = harness.connected_thread();
    assert_eq!(agent_of(thread), "claude");
}

#[test]
fn the_new_thread_shortcut_starts_the_agent_last_chosen_in_the_project() {
    let project = Project::new(&json!({
        "claude": { "type": "stdio", "command": "claude-code-acp" },
        "zed": { "type": "stdio", "command": "zed-agent" },
    }));
    let mut harness = project.harness(&["s1", "s2"]);
    harness.click("project-alpha-new-thread").unwrap();
    harness.click("new-thread-agent-zed").unwrap();
    let first = harness.connected_thread();

    harness.press("secondary-n");

    let second = harness.selected_thread().unwrap();
    assert_ne!(first, second);
    assert_eq!(agent_of(second), "zed");
}

#[test]
fn an_agents_file_that_cannot_be_read_is_offered_for_fixing_in_the_new_thread_menu() {
    let project = Project::new(&json!({}));
    fs::write(project_agents_file(&project.root), "{ not json").unwrap();
    let mut harness = project.harness(&[]);

    harness.click("project-alpha-new-thread").unwrap();

    assert!(harness.bounds("new-thread-problem-0").is_some());
}

#[test]
fn aether_found_on_path_is_added_to_the_users_agents_and_becomes_the_default() {
    let project = Project::new(&json!({}));
    let user_file = project.user_file();

    let mut harness = project.builder(&["s1"]).on_path("aether").build();
    harness.click("project-alpha-new-thread").unwrap();

    assert_eq!(agent_of(harness.selected_thread().unwrap()), "aether");
    assert_eq!(
        read_json(&user_file)["agents"]["aether"],
        json!({ "type": "stdio", "command": "aether", "args": ["acp"] })
    );
}

#[test]
fn aether_removed_after_it_was_added_on_its_own_is_not_added_again() {
    let project = Project::new(&json!({}));
    let user_file = project.user_file();
    let state = tempfile::tempdir().unwrap();
    project.builder(&[]).on_path("aether").state_dir(state.path()).build().quit();
    fs::write(&user_file, json!({ "agents": {} }).to_string()).unwrap();

    project.builder(&[]).on_path("aether").state_dir(state.path()).build().quit();

    assert_eq!(read_json(&user_file), json!({ "agents": {} }));
}

#[test]
fn a_project_without_agents_offers_those_found_on_path_and_starts_a_thread_with_the_one_added() {
    let project = Project::new(&json!({}));
    let user_file = project.user_file();
    let mut harness = project.builder(&["s1"]).on_path("gemini").build();

    harness.click("project-alpha-new-thread").unwrap();
    assert!(harness.bounds("new-thread-add-aether").is_none(), "only agents on PATH are offered");
    harness.click("new-thread-add-gemini").unwrap();

    let thread = harness.connected_thread();
    assert_eq!(agent_of(thread), "gemini");
    assert_eq!(
        read_json(&user_file)["agents"]["gemini"],
        json!({ "type": "stdio", "command": "gemini", "args": ["--experimental-acp"] })
    );
}

#[test]
fn a_project_without_agents_offers_the_users_file_to_define_one_in() {
    let project = Project::new(&json!({}));
    let user_file = project.user_file();
    let mut harness = project.builder(&[]).build();

    harness.click("project-alpha-new-thread").unwrap();
    harness.click("new-thread-open-agents-file").unwrap();

    assert_eq!(harness.platform().opened(), [user_file.display().to_string()]);
    assert_eq!(read_json(&user_file), json!({ "agents": {} }));
}

struct Project {
    dir: TempDir,
    root: PathBuf,
}

impl Project {
    fn new(agents: &serde_json::Value) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("alpha");
        fs::create_dir(&root).unwrap();
        write_agents_file(&root, &json!({ "agents": agents }));
        Self { dir, root }
    }

    fn user_file(&self) -> PathBuf {
        self.dir.path().join("home").join("acp.json")
    }

    fn harness(&self, sessions: &[&str]) -> DesktopHarness {
        self.builder(sessions).build()
    }

    /// A harness with only the agents of the files, and the user's file in a home of its own.
    fn builder(&self, sessions: &[&str]) -> DesktopHarnessBuilder {
        let builder =
            DesktopHarnessBuilder::default().without_cli_agent().user_agents_file(self.user_file()).project(&self.root);
        sessions.iter().fold(builder, |builder, session| builder.script(&self.root, replying(session, "hi")))
    }
}

fn agents(user_file: Option<PathBuf>) -> Agents {
    Agents::new(None, user_file)
}

fn known(name: &str) -> KnownAgent {
    *KNOWN_AGENTS.iter().find(|agent| agent.name == name).unwrap()
}

fn read_json(file: &Path) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap()
}

fn names(catalog: &AgentCatalog) -> Vec<String> {
    catalog.agents().iter().map(|agent| agent.name.clone()).collect()
}

fn agent_of(thread: Thread) -> String {
    thread.target().label().to_owned()
}
