use crate::support::{TestError, next_document, next_result, next_review_state, run_local};
use agent_client_protocol::v2::schema::{AbsolutePath, SessionId, SessionInfo};
use clankerdiff_remote::Invocation;
use foxfire_acp::client::{AcpClientError, AcpEvent, connect_acp_client};
use foxfire_acp::testing::{Channel, FakeAgent, initialize_request, pricing_content};
use foxfire_artifacts::client::LoadError;
use foxfire_core::command::{AgentCommand, Command, CommandResult, GitReviewCommand, ReviewSource};
use foxfire_core::runtime::{ReviewOverride, SessionRuntime};
use foxfire_core::session_core::Input;
use foxfire_core::session_model::initial_workspace_status;
use foxfire_core::testing::TestWorkspace;
use foxfire_core::testing::{FakeGit, git_diff_agent};
use std::collections::BTreeMap;
use std::path::Path;
use tempfile::TempDir;

#[test]
fn a_closed_agent_connection_fails_commands_and_is_reported_once() -> Result<(), TestError> {
    run_local(async {
        let mut runtime = disconnected_runtime().await?;

        runtime.dispatch(Command::Agent(AgentCommand::Cancel { session_id: SessionId::new("session") }));

        assert!(matches!(runtime.next().await, Input::Result(CommandResult::Notified(Err(_)))));
        assert!(matches!(runtime.next().await, Input::Event(AcpEvent::ConnectionClosed)));
        runtime.shutdown().await;
        Ok(())
    })
}

#[test]
fn workspace_status_falls_back_to_the_session_path_without_an_agent() -> Result<(), TestError> {
    run_local(async {
        let mut runtime = disconnected_runtime().await?;
        let first = TempDir::new()?;
        let second = TempDir::new()?;

        for cwd in [first.path(), second.path()] {
            runtime.dispatch(Command::Agent(AgentCommand::FetchWorkspaceStatus {
                session_id: SessionId::new("s"),
                cwd: cwd.to_path_buf(),
            }));
        }

        let mut resolved = Vec::new();
        while resolved.len() < 2 {
            if let Input::Result(CommandResult::WorkspaceResolved { cwd, status }) = runtime.next().await {
                assert_eq!(status, initial_workspace_status(&cwd));
                resolved.push(cwd);
            }
        }
        assert!(resolved.contains(&second.path().to_path_buf()));
        Ok(())
    })
}

#[test]
fn listing_sessions_follows_every_page_across_every_directory() -> Result<(), TestError> {
    run_local(async {
        let all = vec![saved("one", "/alpha"), saved("two", "/beta"), saved("three", "/alpha")];
        let agent = FakeAgent::default().sessions(all.clone()).session_page_size(2);
        let client = connect_acp_client(agent.serve(), initialize_request()).await?;
        let mut runtime = SessionRuntime::new(client.handle, client.event_rx);

        runtime.dispatch(Command::Agent(AgentCommand::ListSessions));

        let Input::Result(CommandResult::SessionsListed(listed)) = runtime.next().await else {
            panic!("expected the listed sessions");
        };
        assert_eq!(listed?, all);
        runtime.shutdown().await;
        Ok(())
    })
}

#[test]
fn a_git_review_reads_the_diff_the_agent_serves() -> Result<(), TestError> {
    run_local(async {
        let git = FakeGit::new("/repo").with_untracked("changed.txt", "edited\n");
        let (agent_end, client_end) = Channel::duplex();
        tokio::task::spawn_local(git_diff_agent(FakeAgent::default(), git).connect_to(agent_end));
        let client = connect_acp_client(client_end, initialize_request()).await?;
        let mut runtime = SessionRuntime::new(client.handle, client.event_rx);

        runtime.dispatch(Command::GitReview(GitReviewCommand::Open(ReviewSource::Agent {
            session_id: SessionId::new("s1"),
        })));

        let document = next_document(&mut runtime).await;
        assert_eq!(document.files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), ["changed.txt"]);
        runtime.shutdown().await;
        Ok(())
    })
}

#[test]
fn a_git_review_of_a_local_working_tree_reads_it_through_a_diff_server_of_its_own() -> Result<(), TestError> {
    run_local(async {
        let repository = TestWorkspace::new().git_init().file("changed.txt", "edited\n");
        let mut runtime = disconnected_runtime().await?;

        runtime.dispatch(Command::GitReview(GitReviewCommand::Open(ReviewSource::Local(repository.root().into()))));

        let document = next_document(&mut runtime).await;
        assert_eq!(document.files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), ["changed.txt"]);
        runtime.shutdown().await;
        Ok(())
    })
}

#[test]
fn a_configured_stdio_review_takes_precedence_over_the_local_source() -> Result<(), TestError> {
    run_local(async {
        let repository = TestWorkspace::new().git_init().file("changed.txt", "edited\n");
        let local = TestWorkspace::new().git_init().file("local.txt", "local edit\n");
        let mut runtime = disconnected_runtime().await?;

        let args = vec![repository.root().to_string_lossy().into_owned()];
        runtime.set_review_override(Some(stdio_override(
            env!("CARGO_BIN_EXE_stdio-diff-server"),
            args,
            repository.root(),
        )));
        runtime.dispatch(Command::GitReview(GitReviewCommand::Open(ReviewSource::Local(local.root().into()))));

        let document = next_document(&mut runtime).await;
        assert_eq!(document.files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), ["changed.txt"]);
        runtime.shutdown().await;
        Ok(())
    })
}

#[test]
fn a_git_review_over_stdio_whose_command_cannot_start_or_exits_fails_naming_it() -> Result<(), TestError> {
    run_local(async {
        let dir = TempDir::new()?;
        for (program, named) in
            [("aether-test-no-such-diff-server", "aether-test-no-such-diff-server"), ("true", "`true`")]
        {
            let mut runtime = disconnected_runtime().await?;

            runtime.set_review_override(Some(stdio_override(program, Vec::new(), dir.path())));
            runtime.dispatch(Command::GitReview(GitReviewCommand::Open(ReviewSource::Local(dir.path().into()))));

            let state = next_review_state(&mut runtime).await;
            assert_eq!(state.label(), "failed");
            assert!(state.status().is_some_and(|status| status.contains(named)), "{:?}", state.status());
        }
        Ok(())
    })
}

#[test]
fn a_git_review_outside_a_repository_fails_with_the_reason() -> Result<(), TestError> {
    run_local(async {
        let outside = TempDir::new()?;
        let mut runtime = disconnected_runtime().await?;

        runtime.dispatch(Command::GitReview(GitReviewCommand::Open(ReviewSource::Local(outside.path().into()))));

        assert_eq!(next_review_state(&mut runtime).await.label(), "failed");
        Ok(())
    })
}

#[test]
fn an_unavailable_git_review_fails_with_the_reason() -> Result<(), TestError> {
    run_local(async {
        let mut runtime = disconnected_runtime().await?;

        runtime
            .dispatch(Command::GitReview(GitReviewCommand::Open(ReviewSource::Unavailable("no diff here".to_owned()))));

        let state = next_review_state(&mut runtime).await;
        assert_eq!(state.label(), "failed");
        assert!(state.status().is_some_and(|status| status.contains("no diff here")));
        Ok(())
    })
}

#[test]
fn a_configured_review_failure_is_reported_instead_of_falling_back() -> Result<(), TestError> {
    run_local(async {
        let local = TestWorkspace::new().git_init().file("local.txt", "local edit\n");
        let mut runtime = disconnected_runtime().await?;
        runtime
            .set_review_override(Some(ReviewOverride::Unavailable("unable to resolve the review endpoint".to_owned())));

        runtime.dispatch(Command::GitReview(GitReviewCommand::Open(ReviewSource::Local(local.root().into()))));

        let state = next_review_state(&mut runtime).await;
        assert_eq!(state.label(), "failed");
        assert!(state.status().is_some_and(|status| status.contains("unable to resolve the review endpoint")));
        Ok(())
    })
}

#[test]
fn an_artifact_the_session_cannot_send_is_reported_as_not_loaded() -> Result<(), TestError> {
    run_local(async {
        let client = connect_acp_client(FakeAgent::new("session").serve(), initialize_request()).await?;
        let mut runtime = SessionRuntime::new(client.handle, client.event_rx);
        let id = pricing_content(1).id();

        runtime.dispatch(Command::LoadArtifact { session_id: SessionId::new("session"), id });

        let CommandResult::ArtifactLoaded { id: loaded, result } = next_result(&mut runtime).await else {
            panic!("expected the artifact to finish loading");
        };
        assert!(loaded == id && matches!(result, Err(LoadError::Fetch(_))), "{result:?}");
        runtime.shutdown().await;
        Ok(())
    })
}

fn saved(id: &str, cwd: &str) -> SessionInfo {
    SessionInfo::new(id, AbsolutePath::new(cwd))
}

async fn disconnected_runtime() -> Result<SessionRuntime, AcpClientError> {
    let client = FakeAgent::default().build().await?;
    client.handle.disconnect().await;
    Ok(SessionRuntime::new(client.handle, client.event_rx))
}

fn stdio_override(program: &str, args: Vec<String>, dir: &Path) -> ReviewOverride {
    ReviewOverride::Stdio(Invocation { program: program.to_owned(), args, dir: dir.into(), env: BTreeMap::new() })
}
