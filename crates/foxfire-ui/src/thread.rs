use crate::backend::{ConnectFailure, SessionEvent, SessionLink};
use agent_client_protocol::v2::schema::SessionId;
use clankerdiff_client::{ClientError, ClientState, LookupReply};
use foxfire_acp::artifact::{ArtifactContent, ArtifactId};
use foxfire_acp::content::{first_text, prompt_title};
use foxfire_acp::conversation::ConversationContent;
use foxfire_core::command::{Command, Reply};
use foxfire_core::composer::file_listing::{FileListing, RepoFiles};
use foxfire_core::composer::submission::Submission;
use foxfire_core::elicitation::{ArtifactReviewElicitationMeta, Elicitation, ElicitationId};
use foxfire_core::session_core::{ChatChannel, Effect, SessionCore};
use rouge::reactive::{Callback, Scope, Signal, Stored, Task, batch, effect, memo, signal, sleep, spawn, stored};
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
use web_time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadEvent {
    Launched(String),
    TurnFinished,
    NeedsInput,
    SubmissionRestored(Submission),
    ReviewActionFinished(Result<(), ClientError>),
    ReviewLookupFinished(LookupReply),
    ReviewSubmitted,
    PromptSearchUpdated,
    ArtifactLoaded(ArtifactContent),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadStatus {
    Connecting,
    /// Lost its agent, and is getting it back by itself.
    Reconnecting,
    Idle,
    Running,
    Background,
    NeedsInput,
    Errored,
    Disconnected,
    Dormant,
}

#[derive(Clone, Copy)]
pub struct Thread {
    number: usize,
    state: Signal<ThreadState>,
    driver: Stored<Option<Driver>>,
    connect: Stored<Connect>,
    cwd: Stored<Option<PathBuf>>,
    listeners: Stored<Listeners>,
    scope: Scope,
}

type Connect = Box<dyn Fn(Option<SessionId>) -> SessionLink>;

struct Driver {
    commands: UnboundedSender<Command>,
    _receive: Task<()>,
}

type Listeners = Vec<(Stored<()>, Callback<(ThreadEvent,)>)>;

struct ThreadState {
    session_id: Option<SessionId>,
    core: Option<SessionCore>,
    link: Link,
    keeps_connected: bool,
    /// How often it has tried to reconnect by itself since it was last connected.
    attempts: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Link {
    Connecting,
    Connected,
    Failed(ConnectFailure),
    Closed,
    Stopped,
}

impl Thread {
    /// Opens a target, and keeps enough information to reattach without provisioning it again.
    pub fn connect(
        number: usize,
        cwd: Option<PathBuf>,
        session_id: Option<SessionId>,
        open: impl Fn(Option<SessionId>) -> SessionLink + 'static,
    ) -> Self {
        let thread = Self {
            number,
            state: signal(ThreadState {
                session_id: session_id.clone(),
                core: None,
                link: Link::Connecting,
                keeps_connected: false,
                attempts: 0,
            }),
            driver: stored(None),
            connect: stored(Box::new(open) as Connect),
            cwd: stored(cwd),
            listeners: stored(Vec::new()),
            scope: Scope::new(),
        };
        thread.open_session(session_id);
        thread
    }

    pub fn number(&self) -> usize {
        self.number
    }

    pub fn is_alive(&self) -> bool {
        self.state.is_alive()
    }

    pub fn session_id(&self) -> Option<SessionId> {
        self.state.with(|state| state.session_id.clone())
    }

    pub(crate) fn fallback_cwd(&self) -> Option<PathBuf> {
        self.cwd.get()
    }

    /// What the first prompt calls the thread.
    pub fn title(&self) -> Option<String> {
        self.state.with(|state| {
            let prompt = state.core.as_ref().and_then(|core| {
                core.conversation().items().iter().find_map(|item| match item.content() {
                    ConversationContent::User(blocks) => Some(blocks),
                    _ => None,
                })
            });
            prompt.map(|blocks| prompt_title(first_text(blocks)))
        })
    }

    pub fn with_core<T>(&self, read: impl FnOnce(&SessionCore) -> T) -> Option<T> {
        self.state.with(|state| state.core.as_ref().map(read))
    }

    pub fn with_connected_core<T>(&self, read: impl FnOnce(&SessionCore) -> T) -> Option<T> {
        self.state.with(|state| state.connected_core().map(read))
    }

    pub fn status(&self) -> ThreadStatus {
        self.state.with(ThreadState::status)
    }

    /// Not yet sent a prompt: connected, or still starting a brand-new session. A resumed session is never
    /// fresh while it connects, because its history is about to replay.
    pub fn is_fresh(&self) -> bool {
        self.state.with(|state| {
            let empty = state.core.as_ref().is_none_or(|core| core.conversation().items().is_empty());
            let starting = state.link == Link::Connecting && state.session_id.is_none();
            empty && (starting || state.status() == ThreadStatus::Idle)
        })
    }

    pub fn error(&self) -> Option<ConnectFailure> {
        self.state.with(|state| match &state.link {
            Link::Failed(failure) => Some(failure.clone()),
            _ => None,
        })
    }

    pub fn elicitation(&self) -> Option<(ElicitationId, Elicitation)> {
        self.with_connected_core(|core| core.elicitation().map(|(id, elicitation)| (id, elicitation.clone()))).flatten()
    }

    pub fn pending_review(&self) -> Option<(ElicitationId, ArtifactReviewElicitationMeta)> {
        match self.elicitation()? {
            (id, Elicitation::ArtifactReview(meta)) => Some((id, meta)),
            _ => None,
        }
    }

    pub fn has_pending_review(&self) -> bool {
        self.with_connected_core(|core| matches!(core.elicitation(), Some((_, Elicitation::ArtifactReview(_)))))
            .unwrap_or(false)
    }

    pub fn review(&self) -> Option<Arc<ClientState>> {
        self.with_connected_core(|core| core.git_review().cloned()).flatten()
    }

    pub fn changed_files(&self) -> Option<usize> {
        self.with_connected_core(|core| {
            core.git_review().and_then(|review| review.snapshot.as_ref().map(|snapshot| snapshot.files.len()))
        })
        .flatten()
    }

    pub fn listed_files(&self) -> Result<RepoFiles, String> {
        self.state.with(|state| state.core.as_ref().map_or(FileListing::Pending, SessionCore::files).into_result())
    }

    pub fn send(&self, submission: Submission) {
        if self.state.with_untracked(|state| state.connected_core().is_some()) {
            self.update_core(|core| core.submit(submission));
        } else {
            self.emit(&ThreadEvent::SubmissionRestored(submission));
        }
    }

    pub fn load_artifact(&self, id: ArtifactId) {
        self.update_core(|core| core.load_artifact(id));
    }

    pub fn report_error(&self, message: &str) {
        self.mutate(|state| {
            if let Some(core) = state.core.as_mut() {
                core.report_error(message);
            }
        });
    }

    pub fn can_search_prompts(&self) -> bool {
        self.with_connected_core(|core| core.session().capabilities().prompt_search).unwrap_or(false)
    }

    /// Reads the session's chat channel while other people share it.
    pub fn with_chat<T>(&self, read: impl FnOnce(&ChatChannel) -> T) -> Option<T> {
        self.with_core(|core| core.chat().map(read)).flatten()
    }

    pub fn update_core(&self, change: impl FnOnce(&mut SessionCore)) {
        if !self.state.is_alive() {
            return;
        }
        self.mutate(|state| {
            if let Some(core) = state.core.as_mut().filter(|_| state.link == Link::Connected) {
                change(core);
            }
        });
    }

    /// Gets a dropped agent back by itself, waiting longer after each attempt that fails, or without waiting when the
    /// window is `focused` again. Gives up on an agent that the user has to sign in for, or that has ended.
    pub fn keep_connected(&self, focused: impl Fn() -> bool + 'static) {
        let thread = *self;
        self.state.update(|state| state.keeps_connected = true);
        self.scope.run(|| {
            let due = memo(move || thread.state.with(ThreadState::next_attempt));
            effect(move || {
                if let Some(attempt) = due.get() {
                    spawn(async move {
                        sleep(backoff(attempt)).await;
                        thread.state.update(|state| state.attempts += 1);
                        thread.reconnect();
                    });
                }
            });
            let focused = memo(focused);
            effect(move || {
                if focused.get() && thread.is_dropped() {
                    thread.reconnect();
                }
            });
        });
    }

    /// Whether the agent was lost in a way that reconnecting may fix.
    pub(crate) fn is_dropped(&self) -> bool {
        self.state.with_untracked(ThreadState::is_dropped)
    }

    pub fn reconnect(&self) {
        let session_id = self.state.with_untracked(|state| {
            matches!(state.link, Link::Failed(_) | Link::Closed | Link::Stopped).then(|| state.session_id.clone())
        });
        if let Some(session_id) = session_id {
            self.open_session(session_id);
        }
    }

    pub fn on_event(&self, listener: impl Fn(ThreadEvent) + 'static) {
        let subscription = (stored(()), Callback::from(listener));
        self.listeners.update(|listeners| {
            listeners.retain(|(alive, _)| alive.is_alive());
            listeners.push(subscription);
        });
    }

    pub fn stop(&self) {
        if self.driver.update(Option::take).is_none() {
            return;
        }
        self.state.update(|state| state.link = Link::Stopped);
    }

    fn receive(&self, messages: Vec<SessionEvent>, started: Instant) -> bool {
        if !self.state.is_alive() {
            return false;
        }
        let mut connected = false;
        batch(|| {
            for message in messages {
                connected |= matches!(&message, SessionEvent::Connected(..));
                self.on_driver_message(message);
            }
        });
        if connected && self.state.is_alive() && self.state.with_untracked(|state| state.connected_core().is_some()) {
            tracing::debug!(target: "foxfire_startup", thread = self.number, elapsed_ms = started.elapsed().as_millis(), "Composer ready");
        }
        true
    }

    fn on_driver_message(&self, message: SessionEvent) {
        if let SessionEvent::Launched(id) = message {
            self.emit(&ThreadEvent::Launched(id));
            return;
        }
        self.mutate(|state| match message {
            SessionEvent::Launched(_) => unreachable!(),
            SessionEvent::Connected(config, replay) => {
                state.session_id = Some(config.session_response.session_id.clone());
                let core = state.core.insert(SessionCore::new(*config));
                for input in replay {
                    core.on_input(input);
                }
                state.link = Link::Connected;
                state.attempts = 0;
            }
            SessionEvent::OpenReview => {
                if let Some(core) = state.core.as_mut() {
                    core.open_git_review();
                }
            }
            SessionEvent::ConnectFailed(failure) => state.link = Link::Failed(failure),
            SessionEvent::Input(input) => {
                if let Some(core) = state.core.as_mut() {
                    core.on_input(input);
                }
            }
        });
    }

    fn open_session(&self, session_id: Option<SessionId>) {
        let started = Instant::now();
        self.state.update(|state| state.link = Link::Connecting);
        let thread = *self;
        let driver = self.scope.run(|| {
            let SessionLink { commands, mut events } = self.connect.with(|open| open(session_id));
            let receive = spawn(async move {
                while let Some(event) = events.recv().await {
                    let batch = std::iter::once(event).chain(std::iter::from_fn(|| events.try_recv().ok()));
                    if !thread.receive(batch.collect(), started) {
                        return;
                    }
                }
                thread.state.try_update(|state| {
                    if state.link == Link::Connected || state.link == Link::Connecting {
                        state.link = Link::Closed;
                    }
                });
            });
            Driver { commands, _receive: receive.cancel_on_drop() }
        });
        self.driver.set(Some(driver));
    }

    fn mutate(&self, change: impl FnOnce(&mut ThreadState)) {
        let effects = self.state.try_update(|state| {
            change(state);
            let effects = state.core.as_mut().map_or_default(SessionCore::take_effects);
            if effects.iter().any(|effect| matches!(effect, Effect::ConnectionClosed)) {
                state.link = Link::Closed;
            }
            effects
        });
        batch(|| {
            for effect in effects.unwrap_or_default() {
                self.apply(effect);
            }
        });
    }

    fn apply(&self, effect: Effect) {
        let event = match effect {
            Effect::Dispatch(command) => {
                return self.driver.with(|driver| {
                    if let Some(driver) = driver {
                        let _ = driver.commands.send(command);
                    }
                });
            }
            Effect::ElicitationRequested => ThreadEvent::NeedsInput,
            Effect::RestoreSubmission(submission) => ThreadEvent::SubmissionRestored(submission),
            Effect::TurnFinished => ThreadEvent::TurnFinished,
            Effect::ReviewSubmitted => ThreadEvent::ReviewSubmitted,
            Effect::Reply(Reply::GitReviewAction(result)) => ThreadEvent::ReviewActionFinished(result),
            Effect::Reply(Reply::GitReviewLookup(reply)) => ThreadEvent::ReviewLookupFinished(reply),
            Effect::PromptSearchUpdated => ThreadEvent::PromptSearchUpdated,
            Effect::ArtifactLoaded(content) => ThreadEvent::ArtifactLoaded(content),
            Effect::ConnectionClosed
            | Effect::ElicitationDismissed
            | Effect::SessionChanged
            | Effect::SessionSettingsChanged(_)
            | Effect::GitReviewChanged
            | Effect::WorkspacesListed(_)
            | Effect::Reply(_) => return,
        };
        self.emit(&event);
    }

    fn emit(&self, event: &ThreadEvent) {
        let listeners = self.listeners.with(|listeners| {
            listeners.iter().filter(|(alive, _)| alive.is_alive()).map(|(_, listener)| *listener).collect::<Vec<_>>()
        });
        for listener in listeners {
            listener.call(event.clone());
        }
    }
}

impl ThreadState {
    fn connected_core(&self) -> Option<&SessionCore> {
        self.core.as_ref().filter(|_| self.link == Link::Connected)
    }

    fn is_dropped(&self) -> bool {
        self.core.is_some() && matches!(self.link, Link::Closed | Link::Failed(ConnectFailure::Retryable(_)))
    }

    /// The attempt to reconnect that is due, unless it has made too many.
    fn next_attempt(&self) -> Option<u32> {
        (self.keeps_connected && self.is_dropped() && self.attempts < MAX_ATTEMPTS).then_some(self.attempts)
    }

    fn status(&self) -> ThreadStatus {
        match &self.link {
            Link::Connecting if self.keeps_connected && self.core.is_some() => ThreadStatus::Reconnecting,
            Link::Connecting => ThreadStatus::Connecting,
            Link::Closed | Link::Failed(_) if self.next_attempt().is_some() => ThreadStatus::Reconnecting,
            Link::Failed(_) => ThreadStatus::Errored,
            Link::Closed => ThreadStatus::Disconnected,
            Link::Stopped => ThreadStatus::Dormant,
            Link::Connected => match self.core.as_ref() {
                Some(core) if core.elicitation().is_some() => ThreadStatus::NeedsInput,
                Some(core) if core.waiting_for_response() => ThreadStatus::Running,
                Some(core) if core.is_agent_busy() => ThreadStatus::Background,
                _ => ThreadStatus::Idle,
            },
        }
    }
}

impl PartialEq for Thread {
    fn eq(&self, other: &Self) -> bool {
        self.number == other.number && self.state == other.state
    }
}

impl Eq for Thread {}

impl Hash for Thread {
    fn hash<T: Hasher>(&self, state: &mut T) {
        self.number.hash(state);
    }
}

impl Debug for Thread {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_tuple("Thread").field(&self.number).finish()
    }
}

impl ThreadStatus {
    pub fn is_stopped(self) -> bool {
        matches!(self, Self::Errored | Self::Disconnected | Self::Dormant)
    }

    pub fn has_problem(self) -> bool {
        matches!(self, Self::Errored | Self::Disconnected | Self::Reconnecting)
    }
}

fn backoff(attempt: u32) -> Duration {
    FIRST_RETRY_DELAY.saturating_mul(2_u32.saturating_pow(attempt)).min(MAX_RETRY_DELAY)
}

const FIRST_RETRY_DELAY: Duration = Duration::from_millis(250);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);
/// About two minutes of trying, after which the user can still reconnect, or return to the window.
const MAX_ATTEMPTS: u32 = 10;
