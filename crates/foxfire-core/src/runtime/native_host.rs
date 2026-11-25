use super::Task;
use super::git_review::{Opening, ReviewConnection, ReviewOverride, ReviewServer};
use crate::command::CommandResult;
use crate::composer::attachment::PromptAttachment;
use crate::native::{build_attachments, local_status};
use crate::session_model::WorkspaceAccess;
use clankerdiff_client::transport::StreamTransport;
use clankerdiff_client::{ClientError, ClientOptions, ConnectionHeader, DiffClient, RemoteError, RemoteErrorCode};
use clankerdiff_remote::Invocation;
use clankerdiff_server::{DiffServer, ServerError, ServerOptions};
use std::future::ready;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;
use tokio::task::spawn_blocking;

pub(super) fn prepare_submission(attachments: Vec<PromptAttachment>, access: WorkspaceAccess) -> Task {
    blocking(move || CommandResult::SubmissionPrepared(build_attachments(&attachments, access)))
}

pub(super) fn resolve_workspace(cwd: PathBuf) -> Task {
    blocking(move || CommandResult::WorkspaceResolved { status: local_status(&cwd), cwd })
}

pub(super) fn open_review(source: ReviewOverride) -> Opening {
    match source {
        ReviewOverride::Stdio(invocation) => Box::pin(open_process(invocation)),
        ReviewOverride::WebSocket { url, headers } => Box::pin(connect(url, headers)),
        ReviewOverride::Unavailable(reason) => Box::pin(ready(Err(ClientError::Transport(reason)))),
    }
}

pub(super) fn open_local_review(path: PathBuf) -> Opening {
    Box::pin(open_local(path))
}

fn blocking(work: impl FnOnce() -> CommandResult + Send + 'static) -> Task {
    let working = spawn_blocking(work);
    Box::pin(async move { working.await.unwrap_or_else(|error| CommandResult::BackgroundFailed(error.to_string())) })
}

async fn open_local(path: PathBuf) -> Result<ReviewConnection, ClientError> {
    let unreadable =
        |error: ServerError| ClientError::Remote(RemoteError::new(RemoteErrorCode::Git, error.to_string()));
    let server = DiffServer::open(path, ServerOptions::default()).await.map_err(unreadable)?;
    let client = DiffClient::from_transport(server.connect().map_err(unreadable)?, ClientOptions::default()).await?;
    Ok(ReviewConnection::new(client, ReviewServer::Host { _opened: Box::new(server) }))
}

async fn open_process(Invocation { program, args, dir, env }: Invocation) -> Result<ReviewConnection, ClientError> {
    let mut child = Command::new(&program)
        .args(&args)
        .envs(&env)
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| ClientError::Transport(format!("failed to start `{program}`: {error}")))?;
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        unreachable!("both pipes were requested at spawn");
    };
    let client = DiffClient::from_messages(StreamTransport::new(stdout, stdin), ClientOptions::default())
        .await
        .map_err(|error| match error {
            ClientError::Disconnected => ClientError::Transport(format!("`{program}` exited before serving a diff")),
            error => error,
        })?;
    Ok(ReviewConnection::new(client, ReviewServer::Host { _opened: Box::new(child) }))
}

async fn connect(url: String, headers: Vec<ConnectionHeader>) -> Result<ReviewConnection, ClientError> {
    let client = DiffClient::connect_with_headers(&url, ClientOptions::default(), headers).await?;
    Ok(ReviewConnection::new(client, ReviewServer::Host { _opened: Box::new(()) }))
}
