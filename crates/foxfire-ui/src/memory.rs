use crate::backend::{
    BackendError, BackendFuture, ConnectFailure, HostedBackend, HostedState, HostedThread, Project, SessionEvent,
    SessionLink, ThreadId, ThreadTitle, User,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::v2::schema as acp;
use agent_client_protocol::{Error, RawJsonRpcId};
use chrono::Utc;
use clankerdiff_client::{ClientState, ConnectionState};
use clankerdiff_core::{DiffDocument, ReviewCapabilities};
use foxfire_acp::artifact::{Artifact, ArtifactContent, ArtifactId, ArtifactPayload};
use foxfire_acp::chat::{ChatChannelJoinParams, ChatChannelPostParams, FoxfireCapabilities, FoxfireMeta, Participant};
use foxfire_acp::client::{AcpClientError, AcpEvent};
use foxfire_acp::content::first_text;
use foxfire_artifacts::client::LoadError;
use foxfire_chat::{Ask, ChatChannels, ChatError, Delivery};
use foxfire_core::command::{
    AgentCommand, ChatCommand, ChatReply, Command, CommandResult, GitReviewCommand, PromptRejection, RequestId,
};
use foxfire_core::composer::attachment::AttachmentOutcome;
use foxfire_core::elicitation::{
    ArtifactReviewElicitationMeta, Elicitation, ElicitationAnswer, PendingElicitation, PermissionChoice,
};
use foxfire_core::session_core::Input;
use foxfire_core::session_core::SessionCoreConfig;
use foxfire_core::session_model::{WorkspaceAccess, WorkspaceStatus};
use rouge::reactive::{Task, spawn};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::mem::take;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::{oneshot, watch};

/// A process-local hosted backend for tests. Attachments share a transcript, not a connection.
#[derive(Clone)]
pub struct MemoryBackend {
    projects: Rc<Vec<Project>>,
    state: Rc<RefCell<Inventory>>,
    reply: Rc<str>,
    people: Option<Rc<Vec<(String, String)>>>,
    question: Option<Rc<(String, Elicitation)>>,
    lifetime: Option<Duration>,
    starts_ready: bool,
    changes: Option<Arc<ClientState>>,
}

#[derive(Default)]
struct Inventory {
    next: u64,
    failure: Option<BackendError>,
    threads: BTreeMap<ThreadId, Rc<RefCell<Session>>>,
}

struct Session {
    info: HostedThread,
    history: Vec<acp::UpdateSessionNotification>,
    clients: BTreeMap<usize, UnboundedSender<SessionEvent>>,
    next_client: usize,
    turn: Option<Turn>,
    next: u64,
    stopped: watch::Sender<bool>,
    chat: Option<ChatChannels<usize>>,
    artifacts: HashMap<ArtifactId, ArtifactContent>,
}

struct Attachment {
    session: Rc<RefCell<Session>>,
    client: usize,
}

impl Drop for Attachment {
    fn drop(&mut self) {
        self.session.borrow_mut().detach(self.client);
    }
}

struct Turn {
    request: RequestId,
    origin: UnboundedSender<SessionEvent>,
    permission: Option<RawJsonRpcId>,
    _task: Task<()>,
}

impl MemoryBackend {
    /// Builds an empty in-memory service with a configurable project catalog.
    pub fn new(projects: Vec<Project>) -> Self {
        Self {
            projects: Rc::new(projects),
            state: Rc::default(),
            reply: Rc::default(),
            people: None,
            question: None,
            lifetime: None,
            starts_ready: true,
            changes: None,
        }
    }

    /// Sets the text returned by ordinary prompts.
    pub fn with_reply(mut self, text: &str) -> Self {
        self.reply = Rc::from(text);
        self
    }

    /// Enables chat and seeds it with a first post from each named participant.
    pub fn with_chat(mut self, people: &[(&str, &str)]) -> Self {
        self.people = Some(Rc::new(people.iter().map(|&(who, body)| (who.to_owned(), body.to_owned())).collect()));
        self
    }

    /// Makes a prompt ask for permission instead of starting the usual reply.
    pub fn with_permission(mut self, prompt: &str) -> Self {
        use acp::PermissionOptionKind::{AllowOnce, RejectOnce};
        let question = Elicitation::Permission {
            title: "Allow this action?".into(),
            description: None,
            options: [("allow", "Allow once", AllowOnce), ("reject", "Reject", RejectOnce)]
                .into_iter()
                .map(|(id, name, kind)| PermissionChoice { id: id.into(), name: name.into(), kind })
                .collect(),
        };
        self.question = Some(Rc::new((prompt.into(), question)));
        self
    }

    /// Makes a prompt request review of the supplied artifact.
    pub fn with_review(mut self, prompt: &str, title: &str, markdown: &str) -> Self {
        let question = Elicitation::ArtifactReview(ArtifactReviewElicitationMeta::new(None, title, markdown));
        self.question = Some(Rc::new((prompt.into(), question)));
        self
    }

    /// Says that each thread launched from now on ends after `lifetime`.
    pub fn with_lifetime(mut self, lifetime: Duration) -> Self {
        self.lifetime = Some(lifetime);
        self
    }

    /// Leaves each thread launched from now on starting, until `set_state` says otherwise.
    pub fn with_startup(mut self) -> Self {
        self.starts_ready = false;
        self
    }

    /// Gives every thread these changed files, as a path with its old and new text.
    pub fn with_changes(mut self, files: &[(&str, &str, &str)]) -> Self {
        let document = DiffDocument::from_texts(files.iter().copied()).unwrap_or_default();
        let listed = document.files.iter().map(|file| file.path.clone()).collect();
        self.changes = Some(Arc::new(ClientState {
            snapshot: Some(Arc::new(document)),
            files: Some(listed),
            connection: ConnectionState::Connected,
            capabilities: ReviewCapabilities { submit: true, clipboard: true, ..ReviewCapabilities::NONE },
            error: None,
        }));
        self
    }

    /// Moves a thread to another state, disconnecting everyone from one that is no longer ready.
    pub fn set_state(&self, thread: &ThreadId, state: HostedState) -> Result<HostedThread, BackendError> {
        let session = self.session(thread)?;
        let mut session = session.borrow_mut();
        if state != HostedState::Ready {
            session.disconnect();
        }
        session.info.state = state;
        Ok(session.info.clone())
    }

    /// Drops every connection to a thread, as a network would.
    pub fn drop_connections(&self, thread: &ThreadId) -> Result<(), BackendError> {
        self.session(thread)?.borrow_mut().disconnect();
        Ok(())
    }

    /// Fails every call from now on with `error`, or lets them succeed again.
    pub fn fail_with(&self, error: Option<BackendError>) {
        self.state.borrow_mut().failure = error;
    }

    /// Posts to the thread's chat channel as the first person in it.
    pub fn post(&self, thread: &ThreadId, body: &str) -> Result<(), ChatError> {
        let session = self.state.borrow().threads.get(thread).cloned();
        session.ok_or(ChatError::NotJoined)?.borrow_mut().post(0, body)
    }

    pub fn submit_artifact(
        &self,
        thread: &ThreadId,
        artifact: &Artifact,
        content: ArtifactContent,
    ) -> Result<(), BackendError> {
        let session = self.session(thread)?;
        let mut session = session.borrow_mut();
        session.artifacts.insert(content.id(), content);
        for link in session.clients.values() {
            let payload = ArtifactPayload { session_id: session.session_id(), artifact: artifact.clone() };
            let _ = link.send(SessionEvent::Input(Input::Event(AcpEvent::Artifact(payload))));
        }
        Ok(())
    }

    fn check(&self) -> Result<(), BackendError> {
        self.state.borrow().failure.clone().map_or(Ok(()), Err)
    }

    fn session(&self, id: &ThreadId) -> Result<Rc<RefCell<Session>>, BackendError> {
        self.check()?;
        self.state.borrow().threads.get(id).cloned().ok_or_else(|| BackendError::UnknownThread(id.clone()))
    }

    fn attach(
        &self,
        target: &ThreadId,
        resume: Option<&acp::SessionId>,
        sender: &UnboundedSender<SessionEvent>,
    ) -> Result<Attachment, ConnectFailure> {
        let session = self.session(target)?;
        let mut attached = session.borrow_mut();
        match &attached.info.state {
            HostedState::Ready => {}
            HostedState::Suspended => attached.info.state = HostedState::Ready,
            HostedState::Starting => return Err(ConnectFailure::Retryable("The thread is still starting".to_owned())),
            HostedState::Ended => return Err(ConnectFailure::Ended("The thread has ended".to_owned())),
            HostedState::Failed(error) => return Err(ConnectFailure::Other(error.clone())),
        }
        if resume.is_some_and(|id| *id != attached.session_id()) {
            return Err(ConnectFailure::Other("This thread does not own that session".to_owned()));
        }
        let client = attached.attach(sender);
        drop(attached);
        if self.changes.is_some() {
            let _ = sender.send(SessionEvent::OpenReview);
        }
        Ok(Attachment { session, client })
    }
}

impl HostedBackend for MemoryBackend {
    fn projects(&self) -> BackendFuture<'_, Vec<Project>> {
        Box::pin(async {
            self.check()?;
            Ok(self.projects.as_ref().clone())
        })
    }

    fn list_threads(&self) -> BackendFuture<'_, Vec<HostedThread>> {
        Box::pin(async {
            self.check()?;
            Ok(self.state.borrow().threads.values().map(|session| session.borrow().info.clone()).collect())
        })
    }

    fn launch(&self, project: String) -> BackendFuture<'_, HostedThread> {
        Box::pin(async move {
            self.check()?;
            let project = self
                .projects
                .iter()
                .find(|entry| entry.id == project)
                .cloned()
                .ok_or(BackendError::UnknownProject(project))?;
            let mut inventory = self.state.borrow_mut();
            inventory.next += 1;
            let id = ThreadId(format!("{}-{}", project.id, inventory.next));
            let created_at = Utc::now();
            let info = HostedThread {
                id: id.clone(),
                project,
                state: if self.starts_ready { HostedState::Ready } else { HostedState::Starting },
                title: None,
                created_by: User { id: YOU.to_lowercase(), name: YOU.to_owned() },
                created_at,
                expires_at: self.lifetime.map(|lifetime| created_at + lifetime),
            };
            let mut session = Session {
                info: info.clone(),
                history: Vec::new(),
                clients: BTreeMap::new(),
                next_client: self.people.as_ref().map_or(0, |people| people.len()),
                turn: None,
                next: 0,
                stopped: watch::Sender::new(false),
                chat: self.people.is_some().then(ChatChannels::in_memory),
                artifacts: HashMap::new(),
            };
            if let Some(people) = &self.people {
                session.seed_people(people).map_err(|error| BackendError::Unavailable(error.to_string()))?;
            }
            inventory.threads.insert(id, Rc::new(RefCell::new(session)));
            Ok(info)
        })
    }

    fn rename(&self, id: ThreadId, title: ThreadTitle) -> BackendFuture<'_, HostedThread> {
        Box::pin(async move {
            let session = self.session(&id)?;
            let mut session = session.borrow_mut();
            let current = &mut session.info.title;
            match title {
                ThreadTitle::Suggested(title) => {
                    current.get_or_insert(title);
                }
                ThreadTitle::Chosen(title) => *current = Some(title),
            }
            Ok(session.info.clone())
        })
    }

    fn suspend(&self, id: ThreadId) -> BackendFuture<'_, HostedThread> {
        Box::pin(async move { self.set_state(&id, HostedState::Suspended) })
    }

    fn stop(&self, id: ThreadId) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            self.session(&id)?.borrow_mut().disconnect();
            self.state.borrow_mut().threads.remove(&id);
            Ok(())
        })
    }

    fn open(&self, target: ThreadId, resume: Option<acp::SessionId>) -> SessionLink {
        let (commands, requests) = unbounded_channel();
        let (sender, events) = unbounded_channel();
        let link = SessionLink { commands, events };
        match self.attach(&target, resume.as_ref(), &sender) {
            Ok(attachment) => {
                let stopped = attachment.session.borrow().stopped.subscribe();
                spawn(run(attachment, stopped, requests, sender, self.clone()));
            }
            Err(failure) => {
                let _ = sender.send(SessionEvent::ConnectFailed(failure));
            }
        }
        link
    }
}

impl Session {
    fn session_id(&self) -> acp::SessionId {
        self.info.id.0.clone().into()
    }

    fn attach(&mut self, sender: &UnboundedSender<SessionEvent>) -> usize {
        let config = SessionCoreConfig {
            initialize_response: initialize(self.chat.is_some()),
            session_response: acp::NewSessionResponse::new(self.session_id()),
            working_dir: format!("/workspace/{}", self.info.project.id).into(),
            workspace_status: WorkspaceStatus { display_dir: self.info.project.name.clone(), git_ref: None },
            workspace_access: WorkspaceAccess::Remote,
            participant: Some(person(YOU)),
        };
        let replay = self.history.iter().map(|update| Input::Event(update.clone().into())).collect();
        let _ = sender.send(SessionEvent::Connected(Box::new(config), replay));
        let client = self.next_client;
        self.next_client += 1;
        self.clients.insert(client, sender.clone());
        client
    }

    fn detach(&mut self, client: usize) {
        if self.clients.remove(&client).is_none() {
            return;
        }
        let session = self.session_id();
        if let Some(left) = self.chat.as_mut().and_then(|chat| chat.leave(client, &session)) {
            self.deliver([left]);
        }
    }

    fn disconnect(&mut self) {
        self.finish(acp::StopReason::Cancelled);
        self.stopped.send_replace(true);
        self.stopped = watch::Sender::new(false);
        let session = self.session_id();
        for (client, link) in take(&mut self.clients) {
            if let Some(chat) = &mut self.chat {
                chat.leave(client, &session);
            }
            let _ = link.send(SessionEvent::Input(Input::Event(AcpEvent::ConnectionClosed)));
        }
    }

    fn update(&mut self, update: acp::SessionUpdate) {
        let update = acp::UpdateSessionNotification::new(self.session_id(), update);
        for link in self.clients.values() {
            let _ = link.send(SessionEvent::Input(Input::Event(update.clone().into())));
        }
        self.history.push(update);
    }

    fn finish(&mut self, reason: acp::StopReason) {
        if let Some(turn) = self.turn.take() {
            if let Some(id) = turn.permission {
                let _ = turn.origin.send(SessionEvent::Input(Input::Result(CommandResult::ElicitationWithdrawn(id))));
            }
            let idle = acp::IdleStateUpdate::new().stop_reason(reason);
            self.update(acp::SessionUpdate::StateUpdate(acp::StateUpdate::Idle(idle)));
            answer(&turn.origin, turn.request, Ok(()));
        }
    }
}

async fn run(
    attachment: Attachment,
    mut stopped: watch::Receiver<bool>,
    mut requests: UnboundedReceiver<Command>,
    sender: UnboundedSender<SessionEvent>,
    backend: MemoryBackend,
) {
    let session = &attachment.session;
    while !*stopped.borrow_and_update() {
        let command = tokio::select! {
            _ = stopped.changed() => break,
            () = sender.closed() => break,
            command = requests.recv() => match command {
                Some(command) => command,
                None => break,
            },
        };
        let mut state = session.borrow_mut();
        match command {
            Command::Agent(AgentCommand::Prompt { request_id, content, sources, .. }) => {
                if state.turn.is_some() {
                    answer(
                        &sender,
                        request_id,
                        Err(PromptRejection::Failed("A reply is already in progress".to_owned())),
                    );
                    continue;
                }
                state.next += 1;
                let number = state.next;
                let echo = first_text(&content).unwrap_or_default().trim().to_owned();
                let id = format!("user-{number}");
                state.update(acp::SessionUpdate::UserMessage(acp::UserMessage::new(id.clone()).content(content)));
                let session_id = state.session_id();
                if let Some(chat) = &mut state.chat {
                    let ask = Ask { session_id, author: person(YOU), body: echo.clone(), sources };
                    let recorded = chat.record_ask(ask, id.into());
                    state.deliver(recorded.unwrap_or_default());
                }
                state
                    .update(acp::SessionUpdate::StateUpdate(acp::StateUpdate::Running(acp::RunningStateUpdate::new())));
                let question =
                    backend.question.as_deref().filter(|(prompt, _)| prompt == &echo).map(|(_, q)| q.clone());
                let task = spawn(respond(session.clone(), number, sender.clone(), question, backend.reply.clone()));
                state.turn = Some(Turn {
                    request: request_id,
                    origin: sender.clone(),
                    permission: None,
                    _task: task.cancel_on_drop(),
                });
            }
            Command::Agent(AgentCommand::Cancel { .. }) => state.finish(acp::StopReason::Cancelled),
            Command::Agent(AgentCommand::Chat(command)) => state.request_chat(attachment.client, command),
            Command::GitReview(GitReviewCommand::Open(_)) => {
                if let Some(changes) = &backend.changes {
                    let result = CommandResult::GitReview(changes.clone());
                    let _ = sender.send(SessionEvent::Input(Input::Result(result)));
                }
            }
            Command::LoadArtifact { id, .. } => {
                let result = state.artifacts.get(&id).cloned().ok_or_else(|| {
                    LoadError::Fetch(AcpClientError::Protocol(Error::invalid_params().data("no such artifact")))
                });
                let _ = sender.send(SessionEvent::Input(Input::Result(CommandResult::ArtifactLoaded { id, result })));
            }
            Command::PrepareSubmission { .. } => {
                let outcome = AttachmentOutcome {
                    blocks: Vec::new(),
                    warnings: vec!["Attachments are not supported".to_owned()],
                };
                let _ = sender.send(SessionEvent::Input(Input::Result(CommandResult::SubmissionPrepared(outcome))));
            }
            _ => {}
        }
    }
}

impl Session {
    fn seed_people(&mut self, people: &[(String, String)]) -> Result<(), ChatError> {
        let params = ChatChannelJoinParams { session_id: self.session_id(), after: None };
        for (client, (who, said)) in people.iter().enumerate() {
            self.chat.as_mut().ok_or(ChatError::NotJoined)?.join(client, &person(who), &params)?;
            self.post(client, said)?;
        }
        Ok(())
    }

    fn post(&mut self, client: usize, body: &str) -> Result<(), ChatError> {
        let post = ChatChannelPostParams {
            session_id: self.session_id(),
            key: body.into(),
            body: body.into(),
            anchor: None,
            after_item: None,
        };
        let (_, deliveries) = self.chat.as_mut().ok_or(ChatError::NotJoined)?.post(client, post)?;
        self.deliver(deliveries);
        Ok(())
    }

    fn request_chat(&mut self, client: usize, command: ChatCommand) {
        let Some(chat) = &mut self.chat else { return };
        let (session_id, reply, deliveries) = match command {
            ChatCommand::Join(params) => {
                let (joined, deliveries) = answered(chat.join(client, &person(YOU), &params));
                (params.session_id, ChatReply::Joined(joined), deliveries)
            }
            ChatCommand::Post(params) => {
                let (session_id, key) = (params.session_id.clone(), params.key.clone());
                let (result, deliveries) = answered(chat.post(client, params));
                (session_id, ChatReply::Posted { key, result }, deliveries)
            }
            ChatCommand::History(params) => {
                let page = chat.history(client, &params).map_err(|error| refusal(&error));
                (params.session_id, ChatReply::History(page), Vec::new())
            }
            ChatCommand::Read(params) => {
                let deliveries = chat.read(client, &params).unwrap_or_default();
                return self.deliver(deliveries);
            }
        };
        self.send(client, Input::Result(CommandResult::Chat { session_id, reply }));
        self.deliver(deliveries);
    }

    fn deliver(&self, deliveries: impl IntoIterator<Item = Delivery<usize>>) {
        for Delivery { to, payload } in deliveries {
            for client in to {
                self.send(client, Input::Event(AcpEvent::ChatChannelEvent(payload.clone())));
            }
        }
    }

    fn send(&self, client: usize, input: Input) {
        if let Some(link) = self.clients.get(&client) {
            let _ = link.send(SessionEvent::Input(input));
        }
    }
}

async fn respond(
    session: Rc<RefCell<Session>>,
    number: u64,
    origin: UnboundedSender<SessionEvent>,
    question: Option<Elicitation>,
    reply: Rc<str>,
) {
    let reply = if let Some(question) = question {
        let request_id = RawJsonRpcId::Str(format!("question-{number}"));
        let (send, answer) = oneshot::channel();
        let pending = PendingElicitation::new(request_id.clone(), question, move |choice| {
            let allowed = matches!(&choice, ElicitationAnswer::Choose(id) if id == "allow");
            let _ = send.send(allowed || matches!(choice, ElicitationAnswer::Accept(_)));
        });
        session.borrow_mut().turn.as_mut().expect("active reply").permission = Some(request_id);
        let _ = origin.send(SessionEvent::Input(Input::Elicitation(pending)));
        let allowed = tokio::select! { choice = answer => choice.unwrap_or(false), () = origin.closed() => false };
        session.borrow_mut().turn.as_mut().expect("active reply").permission = None;
        Rc::from(if allowed { "Approved." } else { "Declined." })
    } else {
        reply
    };
    if !reply.is_empty() {
        let chunk = acp::ContentChunk::new(reply.as_ref().into(), format!("reply-{number}"));
        session.borrow_mut().update(acp::SessionUpdate::AgentMessageChunk(chunk));
    }
    session.borrow_mut().finish(acp::StopReason::EndTurn);
}

fn initialize(chat: bool) -> acp::InitializeResponse {
    let session = acp::SessionCapabilities::new().meta(Some(FoxfireCapabilities { chat }.to_meta()));
    acp::InitializeResponse::new(ProtocolVersion::V2, acp::Implementation::new("Test agent", "0.1.0"))
        .capabilities(acp::AgentCapabilities::new().session(session))
}

fn answered<T>(
    result: Result<(T, Vec<Delivery<usize>>), ChatError>,
) -> (Result<T, AcpClientError>, Vec<Delivery<usize>>) {
    match result {
        Ok((answer, deliveries)) => (Ok(answer), deliveries),
        Err(error) => (Err(refusal(&error)), Vec::new()),
    }
}

fn refusal(error: &ChatError) -> AcpClientError {
    AcpClientError::Protocol(Error::invalid_request().data(error.to_string()))
}

fn person(name: &str) -> Participant {
    Participant { id: name.to_lowercase(), name: name.to_owned() }
}

fn answer(sender: &UnboundedSender<SessionEvent>, request_id: RequestId, result: Result<(), PromptRejection>) {
    let _ = sender.send(SessionEvent::Input(Input::Result(CommandResult::Prompt { request_id, result })));
}

const YOU: &str = "You";
