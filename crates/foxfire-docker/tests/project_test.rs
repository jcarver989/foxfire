use clankerdiff_git::testing::{RepoFixture, RepoFixtureBuilder};
use foxfire_docker::error::Error;
use foxfire_docker::project::{Instance, InstanceState, Project, TASK_BRANCH};
use std::io;
use std::path::Path;
use std::process::Command;
use uuid::Uuid;

#[test]
fn launch_starts_at_upstream_head_even_when_the_image_is_stale() -> Result<(), TestError> {
    let test = TestProjectBuilder::new().build()?;
    let fresh = test.advance("src/lib.rs", "pub fn answer() -> u32 {\n    43\n}\n");

    let id = test.project.launch(None)?;

    assert_eq!(exec(&id, &["git", "rev-parse", "HEAD"])?, fresh);
    assert_eq!(exec(&id, &["git", "symbolic-ref", "--short", "HEAD"])?, TASK_BRANCH);
    assert_eq!(exec(&id, &["git", "status", "--porcelain"])?, "");
    Ok(())
}

#[test]
fn launch_adds_only_the_work_ref_to_the_checkout() -> Result<(), TestError> {
    let test = TestProjectBuilder::new().build()?;
    let fresh = test.advance("README.md", "changed upstream\n");
    test.checkout.write("scratch.txt", "the developer's own uncommitted file\n");
    let refs_before =
        test.checkout(&["for-each-ref", "--exclude=refs/foxfire", "--format=%(refname) %(objectname)"])?;
    let status_before = test.checkout(&["status", "--porcelain"])?;

    let id = test.project.launch(None)?;

    assert_eq!(test.checkout(&["rev-parse", &test.project.work_ref(&id)])?, fresh);
    assert_eq!(
        test.checkout(&["for-each-ref", "--exclude=refs/foxfire", "--format=%(refname) %(objectname)"])?,
        refs_before
    );
    assert_eq!(test.checkout(&["status", "--porcelain"])?, status_before);
    Ok(())
}

#[test]
fn launch_without_an_image_fails_and_leaves_no_instance() -> Result<(), TestError> {
    let test = TestProjectBuilder::new().unwarmed().build()?;

    let launched = test.project.launch(None);

    assert!(matches!(launched, Err(Error::NoImage { .. })), "{launched:?}");
    assert_eq!(test.project.list()?, []);
    Ok(())
}

#[test]
fn instances_share_an_image_but_not_their_writes() -> Result<(), TestError> {
    let test = TestProjectBuilder::new().build()?;
    let first = test.project.launch(None)?;
    let second = test.project.launch(None)?;

    exec(&first, &["sh", "-c", "echo first > README.md"])?;

    assert_eq!(exec(&second, &["git", "status", "--porcelain"])?, "");
    Ok(())
}

#[test]
fn list_and_endpoint_follow_suspend_and_resume() -> Result<(), TestError> {
    let test = TestProjectBuilder::new().build()?;
    let id = test.project.launch(None)?;
    assert_eq!(test.project.list()?, [Instance { id: id.clone(), state: InstanceState::Running }]);
    assert!(test.project.endpoint(&id)?.url.starts_with("ws://127.0.0.1:"));

    docker(&["container", "stop", &id])?;
    assert_eq!(test.project.list()?, [Instance { id: id.clone(), state: InstanceState::Suspended }]);
    let endpoint = test.project.endpoint(&id);
    assert!(matches!(endpoint, Err(Error::NotRunning(_))), "{endpoint:?}");

    docker(&["container", "start", &id])?;
    assert_eq!(test.project.list()?, [Instance { id: id.clone(), state: InstanceState::Running }]);
    assert!(test.project.endpoint(&id)?.url.starts_with("ws://127.0.0.1:"));
    Ok(())
}

#[test]
fn stop_lands_commits_and_the_working_tree_in_the_checkout() -> Result<(), TestError> {
    let test = TestProjectBuilder::new().build()?;
    let id = test.project.launch(None)?;
    exec(&id, &["sh", "-c", "echo 'pub fn answer() -> u32 { 7 }' > src/lib.rs"])?;
    exec(
        &id,
        &[
            "git",
            "-c",
            "user.name=Agent",
            "-c",
            "user.email=agent@example.com",
            "commit",
            "--quiet",
            "--all",
            "-m",
            "agent commit",
        ],
    )?;
    let agent_commit = exec(&id, &["git", "rev-parse", "HEAD"])?;
    exec(&id, &["sh", "-c", "rm README.md && printf '\\001\\000\\377\\000' > assets/logo.bin"])?;
    exec(&id, &["sh", "-c", "mkdir notes && echo 'a new file' > notes/new.txt && echo ignored > debug.log"])?;
    let logo = exec(&id, &["git", "hash-object", "assets/logo.bin"])?;

    test.project.stop(&id)?;

    let work = test.project.work_ref(&id);
    assert_eq!(test.checkout(&["rev-parse", &format!("{work}^")])?, agent_commit);
    assert_eq!(test.checkout(&["show", &format!("{work}:src/lib.rs")])?, "pub fn answer() -> u32 { 7 }");
    assert_eq!(test.checkout(&["show", &format!("{work}:notes/new.txt")])?, "a new file");
    assert_eq!(test.checkout(&["rev-parse", &format!("{work}:assets/logo.bin")])?, logo);
    let files = test.checkout(&["ls-tree", "-r", "--name-only", &work])?;
    assert_eq!(files.lines().collect::<Vec<_>>(), [".gitignore", "assets/logo.bin", "notes/new.txt", "src/lib.rs"]);
    assert_eq!(test.project.list()?, []);
    Ok(())
}

#[test]
fn stop_without_changes_exports_the_launch_commit() -> Result<(), TestError> {
    let test = TestProjectBuilder::new().build()?;
    let id = test.project.launch(None)?;

    test.project.stop(&id)?;

    assert_eq!(test.checkout(&["rev-parse", &test.project.work_ref(&id)])?, test.upstream.head().to_string());
    Ok(())
}

#[test]
fn stop_exports_a_suspended_instance() -> Result<(), TestError> {
    let test = TestProjectBuilder::new().build()?;
    let id = test.project.launch(None)?;
    exec(&id, &["sh", "-c", "echo 'written before Suspend' > README.md"])?;
    docker(&["container", "stop", &id])?;

    test.project.stop(&id)?;

    let readme = test.checkout(&["show", &format!("{}:README.md", test.project.work_ref(&id))])?;
    assert_eq!(readme, "written before Suspend");
    assert_eq!(test.project.list()?, []);
    Ok(())
}

#[test]
fn stop_is_safe_to_repeat_once_the_work_has_landed() -> Result<(), TestError> {
    let test = TestProjectBuilder::new().build()?;
    let id = test.project.launch(None)?;
    test.project.stop(&id)?;

    test.project.stop(&id)?;

    let never_launched = test.project.stop(&"0".repeat(64));
    assert!(matches!(never_launched, Err(Error::UnknownInstance(_))), "{never_launched:?}");
    Ok(())
}

#[test]
fn stop_refuses_another_projects_instance() -> Result<(), TestError> {
    let test = TestProjectBuilder::new().build()?;
    let id = test.project.launch(None)?;
    let other = Project::new("some-other-project", test.checkout.root())?;

    let stopped = other.stop(&id);

    assert!(matches!(stopped, Err(Error::NotOwned { .. })), "{stopped:?}");
    assert_eq!(test.project.list()?, [Instance { id, state: InstanceState::Running }]);
    Ok(())
}

#[derive(Debug, thiserror::Error)]
enum TestError {
    #[error(transparent)]
    Project(#[from] Error),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("`{0}` failed: {1}")]
    Failed(String, String),
}

/// Builds a [`TestProject`]: an upstream repository, a developer checkout of
/// it, and a project with its own name and image on the real Docker engine.
struct TestProjectBuilder {
    warmed: bool,
}

struct TestProject {
    project: Project,
    name: String,
    image: String,
    upstream: RepoFixture,
    checkout: RepoFixture,
}

impl TestProjectBuilder {
    fn new() -> Self {
        Self { warmed: true }
    }

    /// Skips building the project's image.
    fn unwarmed(mut self) -> Self {
        self.warmed = false;
        self
    }

    fn build(self) -> Result<TestProject, TestError> {
        let upstream = RepoFixtureBuilder::new()
            .file("README.md", "demo\n")
            .file("src/lib.rs", "pub fn answer() -> u32 {\n    42\n}\n")
            .file("assets/logo.bin", [0_u8, 159, 146, 150, 255])
            .gitignore(&["target/", "*.log"])
            .committed()
            .build();
        let checkout = RepoFixture::init();
        checkout.git(&["remote", "add", "origin", &upstream.root().to_string_lossy()]);
        checkout.git(&["fetch", "--quiet", "origin"]);
        checkout.git(&["checkout", "--quiet", "-B", "main", "origin/main"]);
        let name = format!("test-{}", &Uuid::new_v4().simple().to_string()[..12]);
        let image = format!("foxfire-{name}:warm");
        let project = Project::new(&name, checkout.root())?.image(&image);
        let test = TestProject { project, name, image, upstream, checkout };
        if self.warmed {
            let dockerfile = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/Dockerfile");
            test.project.warm(None, Some(&dockerfile))?;
        }
        Ok(test)
    }
}

impl TestProject {
    /// Commits a change upstream and returns the new commit.
    fn advance(&self, path: &str, contents: &str) -> String {
        self.upstream.write(path, contents);
        self.upstream.commit(&format!("change {path}")).to_string()
    }

    /// Runs Git in the developer's checkout.
    fn checkout(&self, args: &[&str]) -> Result<String, TestError> {
        let mut command = Command::new("git");
        command.arg("-C").arg(self.checkout.root()).args(args);
        run(command, &format!("git {}", args.join(" ")))
    }
}

impl Drop for TestProject {
    fn drop(&mut self) {
        let filter = format!("label=dev.foxfire.project={}", self.name);
        if let Ok(ids) = docker(&["container", "ls", "--all", "--quiet", "--filter", &filter]) {
            for id in ids.lines() {
                let _ = docker(&["container", "rm", "--force", id]);
            }
        }
        let _ = docker(&["image", "rm", &self.image]);
    }
}

/// Runs a command in an instance's workspace.
fn exec(id: &str, command: &[&str]) -> Result<String, TestError> {
    let mut args = vec!["container", "exec", "--workdir", "/workspace", id];
    args.extend_from_slice(command);
    docker(&args)
}

fn docker(args: &[&str]) -> Result<String, TestError> {
    let mut command = Command::new("docker");
    command.args(args);
    run(command, &format!("docker {}", args.join(" ")))
}

fn run(mut command: Command, description: &str) -> Result<String, TestError> {
    let output = command.output()?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim_end().to_owned());
    }
    Err(TestError::Failed(description.to_owned(), String::from_utf8_lossy(&output.stderr).trim().to_owned()))
}
