use crate::backend::{
    BackendError, BackendFuture, HostedBackend, HostedState, HostedThread, Project, SessionEvent, SessionLink, ThreadId,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::v2::schema as acp;
use agent_client_protocol::{Error, RawJsonRpcId};
use foxfire_acp::chat::{ChatChannelJoinParams, ChatChannelPostParams, FoxfireCapabilities, FoxfireMeta, Participant};
use foxfire_acp::client::{AcpClientError, AcpEvent};
use foxfire_acp::content::first_text;
use foxfire_chat::{Ask, ChatChannels, ChatError, Delivery};
use foxfire_core::command::{AgentCommand, ChatCommand, ChatReply, Command, CommandResult, PromptRejection, RequestId};
use foxfire_core::composer::attachment::AttachmentOutcome;
use foxfire_core::elicitation::{Elicitation, ElicitationAnswer, PendingElicitation, PermissionChoice};
use foxfire_core::session_core::Input;
use foxfire_core::session_core::SessionCoreConfig;
use foxfire_core::session_model::{WorkspaceAccess, WorkspaceStatus};
use rouge::reactive::{Task, sleep, spawn};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::{oneshot, watch};

/// A process-local hosted backend for demos and tests. Attachments share a transcript, not a connection.
#[derive(Clone)]
pub struct MemoryBackend {
    projects: Rc<Vec<Project>>,
    state: Rc<RefCell<Inventory>>,
    reply: Rc<Vec<String>>,
    delay: Duration,
    chat: bool,
}

#[derive(Default)]
struct Inventory {
    next: u64,
    threads: BTreeMap<ThreadId, Rc<RefCell<Session>>>,
}

struct Session {
    info: HostedThread,
    history: Vec<acp::UpdateSessionNotification>,
    clients: Vec<UnboundedSender<SessionEvent>>,
    turn: Option<Turn>,
    next: u64,
    stopped: watch::Sender<bool>,
    chat: Option<Chat>,
}

/// The session's channel, where `PEOPLE` came before any link did.
struct Chat {
    channels: ChatChannels<usize>,
    links: Vec<UnboundedSender<SessionEvent>>,
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
            reply: Rc::new(vec![
                "This reply is running entirely on your device. ".to_owned(),
                "No agent or server was contacted.".to_owned(),
            ]),
            delay: Duration::from_millis(220),
            chat: false,
        }
    }

    /// Sets the chunks returned by each prompt and the time between chunks.
    pub fn with_reply(mut self, chunks: Vec<String>, delay: Duration) -> Self {
        self.reply = Rc::new(chunks);
        self.delay = delay;
        self
    }

    /// Puts a few people in the chat channel of every thread.
    pub fn with_chat(mut self) -> Self {
        self.chat = true;
        self
    }

    /// Posts to the thread's chat channel as the first person in it.
    pub fn post(&self, thread: &ThreadId, body: &str) -> Result<(), ChatError> {
        let session = self.state.borrow().threads.get(thread).cloned();
        match session.and_then(|session| session.borrow_mut().chat.as_mut().map(|chat| chat.post(0, thread, body))) {
            Some(posted) => posted,
            None => Err(ChatError::NotJoined),
        }
    }
}

impl HostedBackend for MemoryBackend {
    fn projects(&self) -> BackendFuture<'_, Vec<Project>> {
        Box::pin(async { Ok(self.projects.as_ref().clone()) })
    }

    fn list_threads(&self) -> BackendFuture<'_, Vec<HostedThread>> {
        Box::pin(async {
            Ok(self.state.borrow().threads.values().map(|session| session.borrow().info.clone()).collect())
        })
    }

    fn launch(&self, project: String) -> BackendFuture<'_, HostedThread> {
        Box::pin(async move {
            let project = self
                .projects
                .iter()
                .find(|entry| entry.id == project)
                .cloned()
                .ok_or(BackendError::UnknownProject(project))?;
            let mut inventory = self.state.borrow_mut();
            inventory.next += 1;
            let id = ThreadId(format!("demo-{}", inventory.next));
            let info = HostedThread { id: id.clone(), project, state: HostedState::Ready };
            let chat = self.chat.then(|| Chat::with_people(&id)).transpose();
            let chat = chat.map_err(|error| BackendError::Unavailable(error.to_string()))?;
            inventory.threads.insert(
                id,
                Rc::new(RefCell::new(Session {
                    info: info.clone(),
                    history: Vec::new(),
                    clients: Vec::new(),
                    turn: None,
                    next: 0,
                    stopped: watch::Sender::new(false),
                    chat,
                })),
            );
            Ok(info)
        })
    }

    fn stop(&self, id: ThreadId) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            let session = self.state.borrow_mut().threads.remove(&id).ok_or(BackendError::UnknownThread(id))?;
            let mut session = session.borrow_mut();
            session.stopped.send_replace(true);
            session.finish(acp::StopReason::Cancelled);
            for client in session.clients.drain(..) {
                let _ = client.send(SessionEvent::Input(Input::Event(AcpEvent::ConnectionClosed)));
            }
            Ok(())
        })
    }

    fn open(&self, target: ThreadId, resume: Option<acp::SessionId>) -> SessionLink {
        let (commands, requests) = unbounded_channel();
        let (sender, events) = unbounded_channel();
        let link = SessionLink { commands, events };
        let Some(session) = self.state.borrow().threads.get(&target).cloned() else {
            let _ = sender.send(SessionEvent::ConnectFailed(BackendError::UnknownThread(target).to_string()));
            return link;
        };
        if resume.is_some_and(|id| id != session.borrow().session_id()) {
            let _ = sender.send(SessionEvent::ConnectFailed("This thread does not own that session".to_owned()));
            return link;
        }
        session.borrow_mut().attach(&sender);
        spawn(run(session, requests, sender, self.reply.clone(), self.delay));
        link
    }
}

impl Session {
    fn session_id(&self) -> acp::SessionId {
        self.info.id.0.clone().into()
    }

    fn attach(&mut self, sender: &UnboundedSender<SessionEvent>) {
        let config = SessionCoreConfig {
            initialize_response: demo_initialize(self.chat.is_some()),
            session_response: acp::NewSessionResponse::new(self.session_id()),
            working_dir: format!("/workspace/{}", self.info.project.id).into(),
            workspace_status: WorkspaceStatus { display_dir: self.info.project.name.clone(), git_ref: None },
            workspace_access: WorkspaceAccess::Remote,
            participant: Some(person(YOU)),
        };
        let _ = sender.send(SessionEvent::Connected(Box::new(config)));
        for update in &self.history {
            let _ = sender.send(SessionEvent::Input(Input::Event(update.clone().into())));
        }
        self.clients.retain(|client| !client.is_closed());
        self.clients.push(sender.clone());
    }

    fn update(&mut self, update: acp::SessionUpdate) {
        let update = acp::UpdateSessionNotification::new(self.session_id(), update);
        self.clients.retain(|client| client.send(SessionEvent::Input(Input::Event(update.clone().into()))).is_ok());
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
    session: Rc<RefCell<Session>>,
    mut requests: UnboundedReceiver<Command>,
    sender: UnboundedSender<SessionEvent>,
    reply: Rc<Vec<String>>,
    delay: Duration,
) {
    let mut stopped = session.borrow().stopped.subscribe();
    let client = session.borrow_mut().chat.as_mut().map(|chat| chat.connect(&sender));
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
                        Err(PromptRejection::Failed("The demo agent is already replying".to_owned())),
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
                    let recorded = chat.channels.record_ask(ask, id.into());
                    chat.deliver(recorded.unwrap_or_default());
                }
                state
                    .update(acp::SessionUpdate::StateUpdate(acp::StateUpdate::Running(acp::RunningStateUpdate::new())));
                let task = spawn(respond(session.clone(), number, sender.clone(), echo, reply.clone(), delay));
                state.turn = Some(Turn {
                    request: request_id,
                    origin: sender.clone(),
                    permission: None,
                    _task: task.cancel_on_drop(),
                });
            }
            Command::Agent(AgentCommand::Cancel { .. }) => state.finish(acp::StopReason::Cancelled),
            Command::Agent(AgentCommand::Chat(command)) => {
                if let Some((chat, client)) = state.chat.as_mut().zip(client) {
                    chat.request(client, command);
                }
            }
            Command::PrepareSubmission { .. } => {
                let outcome = AttachmentOutcome {
                    blocks: Vec::new(),
                    warnings: vec!["The demo does not support attachments".to_owned()],
                };
                let _ = sender.send(SessionEvent::Input(Input::Result(CommandResult::SubmissionPrepared(outcome))));
            }
            _ => {}
        }
    }
}

impl Chat {
    fn with_people(thread: &ThreadId) -> Result<Self, ChatError> {
        let mut chat = Self { channels: ChatChannels::in_memory(), links: Vec::new() };
        for (client, (who, said)) in PEOPLE.into_iter().enumerate() {
            let params = ChatChannelJoinParams { session_id: thread.0.clone().into(), after: None };
            chat.channels.join(client, &person(who), &params)?;
            chat.post(client, thread, said)?;
        }
        Ok(chat)
    }

    fn connect(&mut self, link: &UnboundedSender<SessionEvent>) -> usize {
        self.links.push(link.clone());
        PEOPLE.len() + self.links.len() - 1
    }

    fn post(&mut self, client: usize, thread: &ThreadId, body: &str) -> Result<(), ChatError> {
        let (session_id, key, body) = (thread.0.clone().into(), body.to_owned(), body.to_owned());
        let post = ChatChannelPostParams { session_id, key, body, anchor: None, after_item: None };
        let (_, deliveries) = self.channels.post(client, post)?;
        self.deliver(deliveries);
        Ok(())
    }

    fn request(&mut self, client: usize, command: ChatCommand) {
        let (session_id, reply, deliveries) = match command {
            ChatCommand::Join(params) => {
                let (joined, deliveries) = answered(self.channels.join(client, &person(YOU), &params));
                (params.session_id, ChatReply::Joined(joined), deliveries)
            }
            ChatCommand::Post(params) => {
                let (session_id, key) = (params.session_id.clone(), params.key.clone());
                let (result, deliveries) = answered(self.channels.post(client, params));
                (session_id, ChatReply::Posted { key, result }, deliveries)
            }
            ChatCommand::History(params) => {
                let page = self.channels.history(client, &params).map_err(|error| refusal(&error));
                (params.session_id, ChatReply::History(page), Vec::new())
            }
            ChatCommand::Read(params) => {
                let deliveries = self.channels.read(client, &params).unwrap_or_default();
                return self.deliver(deliveries);
            }
        };
        self.send(client, Input::Result(CommandResult::Chat { session_id, reply }));
        self.deliver(deliveries);
    }

    fn deliver(&self, deliveries: Vec<Delivery<usize>>) {
        for Delivery { to, payload } in deliveries {
            for client in to {
                self.send(client, Input::Event(AcpEvent::ChatChannelEvent(payload.clone())));
            }
        }
    }

    fn send(&self, client: usize, input: Input) {
        if let Some(link) = client.checked_sub(PEOPLE.len()).and_then(|link| self.links.get(link)) {
            let _ = link.send(SessionEvent::Input(input));
        }
    }
}

async fn respond(
    session: Rc<RefCell<Session>>,
    number: u64,
    origin: UnboundedSender<SessionEvent>,
    echo: String,
    reply: Rc<Vec<String>>,
    delay: Duration,
) {
    let prefix = if echo == "/permission" {
        let request_id = RawJsonRpcId::Str(format!("permission-{number}"));
        let (send, answer) = oneshot::channel();
        let pending = PendingElicitation::new(request_id.clone(), demo_permission(), move |choice| {
            let _ = send.send(matches!(choice, ElicitationAnswer::Choose(id) if id == "allow"));
        });
        session.borrow_mut().turn.as_mut().expect("active reply").permission = Some(request_id);
        let _ = origin.send(SessionEvent::Input(Input::Elicitation(pending)));
        let allowed = tokio::select! { choice = answer => choice.unwrap_or(false), () = origin.closed() => false };
        session.borrow_mut().turn.as_mut().expect("active reply").permission = None;
        if allowed { "Permission granted." } else { "Permission not granted." }.to_owned()
    } else {
        format!("**Demo agent** received: {echo}\n\n")
    };
    let rest = (echo != "/permission").then(|| reply.iter()).into_iter().flatten();
    for text in std::iter::once(&prefix).chain(rest) {
        if !delay.is_zero() {
            sleep(delay).await;
        }
        let chunk = acp::ContentChunk::new(text.as_str().into(), format!("reply-{number}"));
        session.borrow_mut().update(acp::SessionUpdate::AgentMessageChunk(chunk));
    }
    session.borrow_mut().finish(acp::StopReason::EndTurn);
}

fn demo_permission() -> Elicitation {
    use acp::PermissionOptionKind::{AllowOnce, RejectOnce};
    Elicitation::Permission {
        title: "Show a demo permission response?".into(),
        description: None,
        options: [("allow", "Allow once", AllowOnce), ("reject", "Reject", RejectOnce)]
            .into_iter()
            .map(|(id, name, kind)| PermissionChoice { id: id.into(), name: name.into(), kind })
            .collect(),
    }
}

fn demo_initialize(chat: bool) -> acp::InitializeResponse {
    let session = acp::SessionCapabilities::new().meta(Some(FoxfireCapabilities { chat }.to_meta()));
    acp::InitializeResponse::new(ProtocolVersion::V2, acp::Implementation::new("Demo agent", "0.1.0"))
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
const PEOPLE: [(&str, &str); 2] = [
    ("Maya", "Resuming a sandbox is dropping everyone again."),
    ("Theo", "Only after a suspend for me. First boot is always fine."),
];
