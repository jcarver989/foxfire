use crate::wire::{self, field, session_of};
use agent_client_protocol::{
    Error, RawJsonRpcId, RawJsonRpcMessage, RawJsonRpcNotification, RawJsonRpcParams, RawJsonRpcRequest,
    RawJsonRpcResponse, TransportFrame,
};
use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use std::mem;
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
    initialize: Value,
    clients: HashMap<ClientId, Client>,
    sessions: HashMap<String, Session>,
    session_parents: HashMap<String, String>,
    forwarded_client_requests: HashMap<i64, ForwardedClientRequest>,
    pending_agent_requests: HashMap<i64, PendingAgentRequest>,
    next_request_id: i64,
}

struct Client {
    outgoing: UnboundedSender<TransportFrame>,
    initialized: bool,
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

enum ClientRequestKind {
    Other,
    CreateSession,
    ResumeSession(String),
    CloseSession(String),
    SetSessionConfigOption(String),
}

struct PendingAgentRequest {
    agent_request_id: RawJsonRpcId,
    request: RawJsonRpcRequest,
    session: Option<String>,
    recipients: BTreeSet<ClientId>,
}

impl Hub {
    pub(crate) fn create(agent: UnboundedSender<TransportFrame>, initialize: Value) -> Self {
        Self {
            agent,
            initialize,
            clients: HashMap::new(),
            sessions: HashMap::new(),
            session_parents: HashMap::new(),
            forwarded_client_requests: HashMap::new(),
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
        self.clients.insert(client, Client { outgoing, initialized: false });
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

    fn on_client_request(&mut self, client: ClientId, request: RawJsonRpcRequest) {
        let session = session_of(request.params.as_ref());
        match (&*request.method, session) {
            ("initialize", _) => {
                if let Some(connected) = self.clients.get_mut(&client) {
                    connected.initialized = true;
                }
                self.send_to_client(client, wire::result(request.id, &self.initialize));
            }
            ("session/new", _) => self.forward_client_request(client, request, ClientRequestKind::CreateSession),
            ("session/resume", Some(session)) => self.on_client_resume_session(client, request, session),
            ("session/close", Some(session)) => self.on_client_close_session(client, request, session),
            ("session/set_config_option", Some(session)) => {
                self.forward_client_request(client, request, ClientRequestKind::SetSessionConfigOption(session))
            }
            _ => self.forward_client_request(client, request, ClientRequestKind::Other),
        }
    }

    fn on_client_resume_session(&mut self, client: ClientId, mut request: RawJsonRpcRequest, session: String) {
        let joined = self.sessions.entry(session.clone()).or_default();
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
        self.forward_client_request(client, request, ClientRequestKind::ResumeSession(session));
    }

    fn on_client_close_session(&mut self, client: ClientId, request: RawJsonRpcRequest, session: String) {
        let watched = self.sessions.get_mut(&session).is_some_and(|watched| {
            watched.subscribers.remove(&client);
            !watched.subscribers.is_empty()
        });
        if watched {
            self.send_to_client(client, wire::result(request.id, json!({})));
        } else {
            self.forward_client_request(client, request, ClientRequestKind::CloseSession(session));
        }
    }

    fn on_client_notification(&mut self, client: ClientId, notification: RawJsonRpcNotification) {
        match &*notification.method {
            "$/cancel_request" => {
                let requested = field(notification.params.as_ref(), "requestId").cloned();
                let requested = requested.and_then(|id| serde_json::from_value::<RawJsonRpcId>(id).ok());
                let matching_request = self
                    .forwarded_client_requests
                    .iter()
                    .find(|(_, entry)| entry.client == client && Some(&entry.id) == requested.as_ref());
                if let Some((id, _)) = matching_request {
                    self.send_to_agent(wire::cancel(*id));
                }
            }
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
            self.send_to_agent(wire::response(request.id, Err(Box::new(error.into()))));
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
        let response = wire::readdress(response, id);
        let result = match &response {
            RawJsonRpcResponse::Result { result, .. } => Some(result),
            RawJsonRpcResponse::Error { .. } => None,
        };
        let joined = match (kind, result) {
            (ClientRequestKind::CreateSession, Some(result)) => {
                let session = result.get("sessionId").and_then(Value::as_str).map(str::to_owned);
                if let Some(created) = session.as_ref().map(|session| self.sessions.entry(session.clone()).or_default())
                {
                    created.subscribers.insert(client);
                    let early = created.history.clone();
                    self.send_frames_to_client(client, early);
                }
                session
            }
            (ClientRequestKind::ResumeSession(session), Some(_)) => Some(session),
            (ClientRequestKind::ResumeSession(session), None) => {
                if let Some(watched) = self.sessions.get_mut(&session) {
                    watched.subscribers.remove(&client);
                }
                None
            }
            (ClientRequestKind::CloseSession(session), Some(_)) => {
                if self.sessions.get(&session).is_some_and(|closed| closed.subscribers.is_empty()) {
                    self.sessions.remove(&session);
                    self.session_parents.retain(|_, parent| *parent != session);
                }
                None
            }
            (ClientRequestKind::SetSessionConfigOption(session), Some(result)) => {
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
            _ => None,
        };
        self.send_to_client(client, TransportFrame::Single(response.into()));
        if let Some(session) = joined {
            self.deliver_pending_agent_requests(client, &session);
        }
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
        for session in self.sessions.values_mut() {
            session.subscribers.remove(&client);
        }
        for pending in self.pending_agent_requests.values_mut() {
            pending.recipients.remove(&client);
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
