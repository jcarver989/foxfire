use crate::chat::{self, Caller};
use crate::wire::{self, field, session_of};
use crate::workspace::{DiffBridge, Workspace};
use agent_client_protocol::v2::schema::{InitializeResponse, ReplayFrom, ReplayFromStart, SessionId};
use agent_client_protocol::{
    Error, ErrorCode, RawJsonRpcId, RawJsonRpcMessage, RawJsonRpcNotification, RawJsonRpcParams, RawJsonRpcRequest,
    RawJsonRpcResponse, TransportFrame,
};
use foxfire_acp::aether::{GitDiffCommandPayload, RemoteServerInfo};
use foxfire_acp::chat::{FOXFIRE_METHOD_PREFIX, Participant};
use foxfire_chat::{Ask, ChatChannels, Delivery};
use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use serde_json::{Map, Value, json};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::mem;
use std::path::PathBuf;
use tokio::sync::mpsc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct ClientId(pub(crate) u64);

pub(crate) enum ClientEvent {
    Connected { client: ClientId, outgoing: UnboundedSender<TransportFrame> },
    Received { client: ClientId, frame: TransportFrame },
    Disconnected(ClientId),
}

pub(crate) struct Hub {
    agent: UnboundedSender<TransportFrame>,
    initialize: InitializeResponse,
    workspace: Workspace,
    chat: Option<ChatChannels<ClientId>>,
    live: Option<LiveSession>,
    clients: HashMap<ClientId, Client>,
    sessions: HashMap<String, Session>,
    session_parents: HashMap<String, String>,
    forwarded_client_requests: HashMap<i64, ForwardedClientRequest>,
    waiting_opens: VecDeque<WaitingOpen>,
    pending_agent_requests: HashMap<i64, PendingAgentRequest>,
    next_request_id: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LiveSession {
    pub(crate) id: String,
    pub(crate) cwd: PathBuf,
}

struct Client {
    outgoing: UnboundedSender<TransportFrame>,
    initialized: bool,
    participant: Option<Participant>,
    reviews: HashMap<String, DiffBridge>,
}

#[derive(Default)]
struct Session {
    subscribers: BTreeSet<ClientId>,
    history: Vec<TransportFrame>,
}

struct ForwardedClientRequest {
    client: ClientId,
    id: RawJsonRpcId,
    kind: ClientRequestKind,
}

struct WaitingOpen {
    client: ClientId,
    request: RawJsonRpcRequest,
}

enum ClientRequestKind {
    Other,
    CreateSession { cwd: PathBuf },
    ResumeSession { session: LiveSession, answers_new: bool },
    CloseSession(String),
    SetSessionConfigOption(String),
    Prompt(Ask),
}

struct PendingAgentRequest {
    agent_request_id: RawJsonRpcId,
    request: RawJsonRpcRequest,
    session: Option<String>,
    recipients: BTreeSet<ClientId>,
}

impl Hub {
    pub(crate) fn create(
        agent: UnboundedSender<TransportFrame>,
        initialize: InitializeResponse,
        workspace: Workspace,
        chat: Option<ChatChannels<ClientId>>,
        live: Option<LiveSession>,
    ) -> Self {
        Self {
            agent,
            initialize,
            workspace,
            chat,
            live,
            clients: HashMap::new(),
            sessions: HashMap::new(),
            session_parents: HashMap::new(),
            forwarded_client_requests: HashMap::new(),
            waiting_opens: VecDeque::new(),
            pending_agent_requests: HashMap::new(),
            next_request_id: 1,
        }
    }

    pub(crate) async fn run(
        mut self,
        mut from_agent: UnboundedReceiver<TransportFrame>,
        mut events: mpsc::UnboundedReceiver<ClientEvent>,
    ) {
        loop {
            tokio::select! {
                frame = from_agent.next() => {
                    let Some(frame) = frame else { return };
                    self.on_agent_frame(frame);
                }
                Some(event) = events.recv() => self.on_client_event(event),
            }
        }
    }

    fn on_client_event(&mut self, event: ClientEvent) {
        match event {
            ClientEvent::Connected { client, outgoing } => self.on_client_connected(client, outgoing),
            ClientEvent::Received { client, frame } => self.on_client_frame(client, frame),
            ClientEvent::Disconnected(client) => self.on_client_disconnected(client),
        }
    }

    fn on_client_connected(&mut self, client: ClientId, outgoing: UnboundedSender<TransportFrame>) {
        self.clients
            .insert(client, Client { outgoing, initialized: false, participant: None, reviews: HashMap::new() });
    }

    fn on_client_frame(&mut self, client: ClientId, frame: TransportFrame) {
        let TransportFrame::Single(message) = frame else {
            tracing::warn!(?client, "dropping a malformed client message");
            return;
        };
        if !self.clients.contains_key(&client) {
            return;
        }
        match message {
            RawJsonRpcMessage::Request(request) => self.on_client_request(client, request),
            RawJsonRpcMessage::Notification(notification) => self.on_client_notification(client, notification),
            RawJsonRpcMessage::Response(response) => self.on_client_response(client, response),
        }
    }

    fn on_client_request(&mut self, client: ClientId, mut request: RawJsonRpcRequest) {
        let session = session_of(request.params.as_ref());
        match (&*request.method, session) {
            ("initialize", _) => {
                if let Some(connected) = self.clients.get_mut(&client) {
                    connected.initialized = true;
                    connected.participant = chat::introduced(&request);
                }
                let mut advertised = self.workspace.advertise(self.initialize.clone(), &self.remote_info());
                if self.chat.is_some() {
                    wire::advertise(&mut advertised, chat::capabilities());
                }
                self.send_to_client(client, wire::result(request.id, advertised));
            }
            ("session/new", _) | ("session/resume", Some(_)) => {
                let open = WaitingOpen { client, request };
                if self.must_wait(&open) {
                    self.waiting_opens.push_back(open);
                } else {
                    self.open_session(open);
                }
            }
            ("session/close", Some(session)) => self.on_client_close_session(client, request, session),
            ("session/set_config_option", Some(session)) => {
                self.forward_client_request(client, request, ClientRequestKind::SetSessionConfigOption(session));
            }
            ("_aether/workspace_status", _) => {
                if let Some(connected) = self.clients.get(&client) {
                    self.workspace.report_status(request.id, connected.outgoing.clone());
                }
            }
            ("_aether/session_preview", session) => {
                let preview = self.workspace.preview(session.unwrap_or_default());
                self.send_to_client(client, wire::result(request.id, preview));
            }
            ("session/prompt", Some(session)) => {
                let author = self.clients.get(&client).and_then(|connected| connected.participant.clone());
                let ask = chat::take_ask(&mut request, session, author);
                let kind = ask.map_or(ClientRequestKind::Other, ClientRequestKind::Prompt);
                self.forward_client_request(client, request, kind);
            }
            (method, session) if method.starts_with(FOXFIRE_METHOD_PREFIX) => {
                let RawJsonRpcRequest { id, method, params } = request;
                let frame = match self.answer_chat(client, &method, params, &session.unwrap_or_default()) {
                    Ok(answer) => wire::result(id, answer),
                    Err(error) => wire::failure(id, error),
                };
                self.send_to_client(client, frame);
            }
            _ => self.forward_client_request(client, request, ClientRequestKind::Other),
        }
    }

    fn answer_chat(
        &mut self,
        client: ClientId,
        method: &str,
        params: Option<RawJsonRpcParams>,
        session: &str,
    ) -> Result<Value, Error> {
        let (Some(chat_channels), Some(connected)) = (&mut self.chat, self.clients.get(&client)) else {
            return Err(Error::method_not_found());
        };
        let caller = Caller {
            client,
            participant: connected.participant.as_ref(),
            watching: self.sessions.get(session).is_some_and(|watched| watched.subscribers.contains(&client)),
        };
        let (answer, deliveries) = chat::answer(chat_channels, &caller, method, params)?;
        self.deliver(deliveries);
        Ok(answer)
    }

    fn must_wait(&self, open: &WaitingOpen) -> bool {
        let target = match session_of(open.request.params.as_ref()) {
            Some(session) if &*open.request.method == "session/resume" => Some(session),
            _ => self.live.as_ref().map(|live| live.id.clone()),
        };
        self.forwarded_client_requests.values().any(|forwarded| match &forwarded.kind {
            ClientRequestKind::CreateSession { .. } | ClientRequestKind::CloseSession(_) => true,
            ClientRequestKind::ResumeSession { session, .. } => Some(&session.id) != target.as_ref(),
            ClientRequestKind::SetSessionConfigOption(_) | ClientRequestKind::Prompt(_) | ClientRequestKind::Other => {
                false
            }
        })
    }

    fn open_waiting_sessions(&mut self) {
        while let Some(open) = self.waiting_opens.pop_front() {
            if self.must_wait(&open) {
                self.waiting_opens.push_front(open);
                return;
            }
            self.open_session(open);
        }
    }

    fn open_session(&mut self, WaitingOpen { client, request }: WaitingOpen) {
        let cwd = field(request.params.as_ref(), "cwd").and_then(Value::as_str).map(PathBuf::from);
        let cwd = cwd.unwrap_or_else(|| self.workspace.cwd().to_path_buf());
        let resumed = session_of(request.params.as_ref()).filter(|_| &*request.method == "session/resume");
        match (resumed, self.live.clone()) {
            (Some(id), _) => self.on_client_resume_session(client, request, LiveSession { id, cwd }),
            (None, Some(live)) => self.join_live_session(client, request, live),
            (None, None) => self.forward_client_request(client, request, ClientRequestKind::CreateSession { cwd }),
        }
    }

    fn on_client_resume_session(&mut self, client: ClientId, request: RawJsonRpcRequest, session: LiveSession) {
        let watched_elsewhere = self.sessions.iter().find(|(id, watched)| {
            **id != session.id && watched.subscribers.iter().any(|subscriber| *subscriber != client)
        });
        if let Some((watched, _)) = watched_elsewhere {
            let reason = format!("other clients are using session {watched}, so {} cannot be opened", session.id);
            self.send_to_client(client, wire::failure(request.id, Error::invalid_request().data(reason)));
            return;
        }
        self.subscribe_and_resume(client, request, session, false);
    }

    fn join_live_session(&mut self, client: ClientId, mut request: RawJsonRpcRequest, live: LiveSession) {
        let mut params = match request.params.take() {
            Some(RawJsonRpcParams::Object(params)) => params,
            _ => Map::new(),
        };
        params.insert("sessionId".to_owned(), json!(live.id));
        params.insert("cwd".to_owned(), json!(live.cwd));
        params.insert("replayFrom".to_owned(), json!(ReplayFrom::Start(ReplayFromStart::new())));
        request.method = "session/resume".into();
        request.params = Some(RawJsonRpcParams::Object(params));
        self.subscribe_and_resume(client, request, live, true);
    }

    fn subscribe_and_resume(
        &mut self,
        client: ClientId,
        mut request: RawJsonRpcRequest,
        session: LiveSession,
        answers_new: bool,
    ) {
        let joined = self.sessions.entry(session.id.clone()).or_default();
        let mut history = Vec::new();
        if joined.subscribers.is_empty() {
            joined.history.clear();
        } else if let Some(RawJsonRpcParams::Object(params)) = &mut request.params
            && params.remove("replayFrom").is_some_and(|replay_from| !replay_from.is_null())
        {
            history.clone_from(&joined.history);
        }
        joined.subscribers.insert(client);
        self.send_frames_to_client(client, history);
        self.forward_client_request(client, request, ClientRequestKind::ResumeSession { session, answers_new });
    }

    fn on_client_close_session(&mut self, client: ClientId, request: RawJsonRpcRequest, session: String) {
        if let Some(connected) = self.clients.get_mut(&client) {
            connected.reviews.remove(&session);
        }
        if self.unsubscribe(client, &session) {
            self.send_to_client(client, wire::result(request.id, json!({})));
        } else {
            self.forward_client_request(client, request, ClientRequestKind::CloseSession(session));
        }
    }

    fn on_client_notification(&mut self, client: ClientId, notification: RawJsonRpcNotification) {
        let session = session_of(notification.params.as_ref());
        match (&*notification.method, session) {
            ("_aether/git_diff", Some(_)) => self.on_client_git_diff(client, notification.params.as_ref()),
            ("_aether/git_diff_close", Some(session)) => {
                if let Some(connected) = self.clients.get_mut(&client) {
                    connected.reviews.remove(&session);
                }
            }
            ("$/cancel_request", _) => {
                let requested = field(notification.params.as_ref(), "requestId").cloned();
                let requested = requested.and_then(|id| serde_json::from_value::<RawJsonRpcId>(id).ok());
                let waiting = self
                    .waiting_opens
                    .iter()
                    .position(|open| open.client == client && Some(&open.request.id) == requested.as_ref());
                if let Some(open) = waiting.and_then(|index| self.waiting_opens.remove(index)) {
                    self.send_to_client(client, wire::failure(open.request.id, Error::request_cancelled()));
                    return;
                }
                let matching_request = self
                    .forwarded_client_requests
                    .iter()
                    .find(|(_, entry)| entry.client == client && Some(&entry.id) == requested.as_ref());
                if let Some((id, _)) = matching_request {
                    self.send_to_agent(wire::cancel(*id));
                }
            }
            ("_foxfire/channel_read", _) => {
                if let (Some(chat_channels), Ok(params)) = (&mut self.chat, wire::parse(notification.params)) {
                    match chat_channels.read(client, &params) {
                        Ok(deliveries) => self.deliver(deliveries),
                        Err(error) => {
                            tracing::debug!(error = %chat::refusal(&error), ?client, "ignoring a read mark");
                        }
                    }
                }
            }
            (method, _) if method.starts_with(FOXFIRE_METHOD_PREFIX) => {}
            _ => self.send_to_agent(TransportFrame::Single(notification.into())),
        }
    }

    fn on_client_response(&mut self, client: ClientId, response: RawJsonRpcResponse) {
        let RawJsonRpcId::Number(id) = *response.id() else {
            return;
        };
        if !self.pending_agent_requests.get(&id).is_some_and(|pending| pending.recipients.contains(&client)) {
            return;
        }
        let Some(pending) = self.pending_agent_requests.remove(&id) else {
            return;
        };
        self.send_to_agent(TransportFrame::Single(wire::readdress(response, pending.agent_request_id).into()));
        for other in pending.recipients.into_iter().filter(|other| *other != client) {
            self.send_to_client(other, wire::cancel(id));
        }
    }

    fn on_client_git_diff(&mut self, client: ClientId, params: Option<&RawJsonRpcParams>) {
        let payload = serde_json::to_value(params).ok().and_then(|params| serde_json::from_value(params).ok());
        let (Some(GitDiffCommandPayload { session_id, mut command }), Some(connected)) =
            (payload, self.clients.get_mut(&client))
        else {
            return;
        };
        if let Some(bridge) = connected.reviews.get(&session_id) {
            match bridge.send(command) {
                Ok(()) => return,
                Err(unsent) => command = unsent.0,
            }
        }
        if let Some(bridge) = self.workspace.bridge(&session_id, connected.outgoing.clone()) {
            let _ = bridge.send(command);
            connected.reviews.insert(session_id, bridge);
        }
    }

    fn forward_client_request(&mut self, client: ClientId, mut request: RawJsonRpcRequest, kind: ClientRequestKind) {
        let id = self.allocate_request_id();
        let original = mem::replace(&mut request.id, RawJsonRpcId::Number(id));
        self.forwarded_client_requests.insert(id, ForwardedClientRequest { client, id: original, kind });
        self.send_to_agent(TransportFrame::Single(request.into()));
    }

    fn on_agent_frame(&mut self, frame: TransportFrame) {
        let TransportFrame::Single(message) = frame else {
            tracing::warn!("dropping a malformed agent message");
            return;
        };
        match message {
            RawJsonRpcMessage::Request(request) => self.on_agent_request(request),
            RawJsonRpcMessage::Notification(notification) => self.on_agent_notification(notification),
            RawJsonRpcMessage::Response(response) => self.on_agent_response(response),
        }
    }

    fn on_agent_request(&mut self, mut request: RawJsonRpcRequest) {
        let session = session_of(request.params.as_ref()).map(|session| self.find_root_session(&session));
        let recipients = match &session {
            Some(session) => self.sessions.get(session).map_or_default(|watched| watched.subscribers.clone()),
            None => self.collect_initialized_clients(),
        };
        if session.is_none() && recipients.is_empty() {
            let error = Error::internal_error().data("no client is connected to answer");
            self.send_to_agent(wire::failure(request.id, error));
            return;
        }
        let id = self.allocate_request_id();
        let agent_request_id = mem::replace(&mut request.id, RawJsonRpcId::Number(id));
        for client in &recipients {
            self.send_to_client(*client, TransportFrame::Single(request.clone().into()));
        }
        self.pending_agent_requests.insert(id, PendingAgentRequest { agent_request_id, request, session, recipients });
    }

    fn on_agent_notification(&mut self, notification: RawJsonRpcNotification) {
        if &*notification.method == "$/cancel_request" {
            let withdrawn = field(notification.params.as_ref(), "requestId").cloned();
            let withdrawn = withdrawn.and_then(|id| serde_json::from_value::<RawJsonRpcId>(id).ok());
            let id = self
                .pending_agent_requests
                .iter()
                .find(|(_, pending)| Some(&pending.agent_request_id) == withdrawn.as_ref())
                .map(|(id, _)| *id);
            if let Some(id) = id
                && let Some(pending) = self.pending_agent_requests.remove(&id)
            {
                for client in pending.recipients {
                    self.send_to_client(client, wire::cancel(id));
                }
            }
            return;
        }
        let Some(session) = session_of(notification.params.as_ref()) else {
            let frame = TransportFrame::Single(notification.into());
            for client in self.collect_initialized_clients() {
                self.send_to_client(client, frame.clone());
            }
            return;
        };
        let root = self.find_root_session(&session);
        let record_in_history = &*notification.method == "session/update";
        if record_in_history {
            let update = field(notification.params.as_ref(), "update");
            if update.and_then(|update| update.get("sessionUpdate")).and_then(Value::as_str) == Some("subagent_update")
                && let Some(child) = update.and_then(|update| update.get("sessionId")).and_then(Value::as_str)
                && child != session
            {
                self.session_parents.insert(child.to_owned(), root.clone());
            }
        }
        let frame = TransportFrame::Single(notification.into());
        self.publish_session_frame(&root, &frame, record_in_history, None);
    }

    fn on_agent_response(&mut self, response: RawJsonRpcResponse) {
        let RawJsonRpcId::Number(id) = *response.id() else {
            return;
        };
        let Some(ForwardedClientRequest { client, id, kind }) = self.forwarded_client_requests.remove(&id) else {
            return;
        };
        let mut response = wire::readdress(response, id);
        let result = match &mut response {
            RawJsonRpcResponse::Result { result, .. } => Ok(result),
            RawJsonRpcResponse::Error { error, .. } => Err(ErrorCode::from(error.code)),
        };
        let joined = match (kind, result) {
            (ClientRequestKind::CreateSession { cwd }, Ok(result)) => {
                let session = result.get("sessionId").and_then(Value::as_str).map(str::to_owned);
                if let Some(id) = session.clone() {
                    let created = self.sessions.entry(id.clone()).or_default();
                    if self.clients.contains_key(&client) {
                        created.subscribers.insert(client);
                    }
                    let early = created.history.clone();
                    self.send_frames_to_client(client, early);
                    self.set_live_session(LiveSession { id, cwd });
                }
                session
            }
            (ClientRequestKind::ResumeSession { session, answers_new }, Ok(result)) => {
                if answers_new && let Some(result) = result.as_object_mut() {
                    result.insert("sessionId".to_owned(), json!(session.id));
                }
                self.set_live_session(session.clone());
                Some(session.id)
            }
            (ClientRequestKind::ResumeSession { session, .. }, Err(code)) => {
                self.on_resume_failed(client, &session, code);
                None
            }
            (ClientRequestKind::CloseSession(session), Ok(_)) => {
                if self.sessions.get(&session).is_some_and(|closed| closed.subscribers.is_empty()) {
                    self.sessions.remove(&session);
                    self.session_parents.retain(|_, parent| *parent != session);
                }
                None
            }
            (ClientRequestKind::SetSessionConfigOption(session), Ok(result)) => {
                if let Some(options) = result.get("configOptions") {
                    let update = json!({
                        "sessionId": session,
                        "update": { "sessionUpdate": "config_option_update", "configOptions": options },
                    });
                    self.publish_session_frame(
                        &session,
                        &wire::notification("session/update", update),
                        true,
                        Some(client),
                    );
                }
                None
            }
            (ClientRequestKind::Prompt(ask), Ok(result)) => {
                if let (Some(chat_channels), Some(message_id)) =
                    (&mut self.chat, result.get("messageId").and_then(Value::as_str))
                {
                    let session = ask.session_id.clone();
                    match chat_channels.record_ask(ask, message_id.into()) {
                        Ok(deliveries) => self.deliver(deliveries),
                        Err(error) => tracing::warn!(%error, %session, "failed to record who prompted the agent"),
                    }
                }
                None
            }
            _ => None,
        };
        self.send_to_client(client, TransportFrame::Single(response.into()));
        if let Some(session) = joined {
            self.deliver_pending_agent_requests(client, &session);
        }
        self.open_waiting_sessions();
    }

    fn set_live_session(&mut self, live: LiveSession) {
        self.sessions.retain(|session, _| *session == live.id);
        self.session_parents.retain(|_, parent| *parent == live.id);
        if let Some(chat_channels) = &mut self.chat {
            chat_channels.keep_only(&SessionId::new(live.id.as_str()));
        }
        self.live = Some(live);
    }

    fn on_resume_failed(&mut self, client: ClientId, session: &LiveSession, code: ErrorCode) {
        let unwatched = !self.unsubscribe(client, &session.id);
        let unknown = matches!(code, ErrorCode::RESOURCE_NOT_FOUND | ErrorCode::INVALID_PARAMS);
        if unwatched && unknown && self.live.as_ref() == Some(session) {
            tracing::warn!(session = session.id, "forgetting the live session, as the agent cannot resume it");
            self.live = None;
        }
    }

    fn unsubscribe(&mut self, client: ClientId, session: &str) -> bool {
        if let Some(chat_channels) = &mut self.chat {
            let left = chat_channels.leave(client, &SessionId::new(session));
            self.deliver(left);
        }
        self.sessions.get_mut(session).is_some_and(|watched| {
            watched.subscribers.remove(&client);
            !watched.subscribers.is_empty()
        })
    }

    fn publish_session_frame(
        &mut self,
        session: &str,
        frame: &TransportFrame,
        record_in_history: bool,
        except: Option<ClientId>,
    ) {
        let watched = self.sessions.entry(session.to_owned()).or_default();
        if record_in_history {
            watched.history.push(frame.clone());
        }
        let audience: Vec<_> = watched.subscribers.iter().copied().filter(|client| Some(*client) != except).collect();
        for client in audience {
            self.send_to_client(client, frame.clone());
        }
    }

    fn deliver_pending_agent_requests(&mut self, client: ClientId, session: &str) {
        let mut pending_frames = Vec::new();
        for pending in
            self.pending_agent_requests.values_mut().filter(|pending| pending.session.as_deref() == Some(session))
        {
            if pending.recipients.insert(client) {
                pending_frames.push(TransportFrame::Single(pending.request.clone().into()));
            }
        }
        self.send_frames_to_client(client, pending_frames);
    }

    fn on_client_disconnected(&mut self, client: ClientId) {
        self.clients.remove(&client);
        self.waiting_opens.retain(|open| open.client != client);
        for session in self.sessions.keys().cloned().collect::<Vec<_>>() {
            self.unsubscribe(client, &session);
        }
        for pending in self.pending_agent_requests.values_mut() {
            pending.recipients.remove(&client);
        }
    }

    fn remote_info(&self) -> RemoteServerInfo {
        match &self.live {
            Some(live) => {
                RemoteServerInfo { cwd: live.cwd.clone(), session_id: Some(SessionId::new(live.id.as_str())) }
            }
            None => RemoteServerInfo { cwd: self.workspace.cwd().to_path_buf(), session_id: None },
        }
    }

    fn find_root_session(&self, session: &str) -> String {
        let mut root = session;
        while let Some(parent) = self.session_parents.get(root) {
            root = parent;
        }
        root.to_owned()
    }

    fn collect_initialized_clients(&self) -> BTreeSet<ClientId> {
        self.clients.iter().filter(|(_, client)| client.initialized).map(|(id, _)| *id).collect()
    }

    fn allocate_request_id(&mut self) -> i64 {
        self.next_request_id += 1;
        self.next_request_id - 1
    }

    fn deliver(&self, deliveries: impl IntoIterator<Item = Delivery<ClientId>>) {
        for Delivery { to, payload } in deliveries {
            let frame = wire::notification("_foxfire/channel_event", payload);
            for client in to {
                self.send_to_client(client, frame.clone());
            }
        }
    }

    fn send_to_agent(&self, frame: TransportFrame) {
        let _ = self.agent.unbounded_send(frame);
    }

    fn send_frames_to_client(&self, client: ClientId, frames: Vec<TransportFrame>) {
        for frame in frames {
            self.send_to_client(client, frame);
        }
    }

    fn send_to_client(&self, client: ClientId, frame: TransportFrame) {
        if let Some(connected) = self.clients.get(&client) {
            let _ = connected.outgoing.unbounded_send(frame);
        }
    }
}
