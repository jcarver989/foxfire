use crate::agents::{AgentProfile, Agents, CommandLine, Endpoint};
use crate::clock::Clock;
use crate::history::History;
use crate::platform::{Notification, PathPrompt, Platform, PlatformFuture};
use crate::registry::{self, Registry, STATE_KEY, State};
use crate::remotes::{Acp, Fleet, InstanceState, RemoteSpec};
use crate::sessions::{ListFuture, SessionSource};
use crate::target::{AgentConnector, ConnectError, Transport, TransportFuture};
use crate::threads::{Thread, ThreadManager, ThreadStatus};
use crate::ui::KINDLED_AFTER;
use crate::ui::tokens::BASE_SIZE;
use crate::{ActiveReviewTheme, DesktopApp, DesktopConfig, window_size};
use agent_client_protocol::v2::schema::{AbsolutePath, SessionId, SessionInfo};
use agent_client_protocol::{Client, DynConnectTo};
use chrono::{DateTime, Utc};
use clankerdiff_remote::RemotesFile;
use clankerdiff_remote::testing::FakeRemotes;
use clankerdiff_ui_core::ReviewTheme;
use foxfire_acp::testing::{Channel, FakeAgent, FakeAgentRequests, QueuedTurns};
use foxfire_core::testing::{FakeGit, git_diff_agent};
use rouge::core::testing::{TestError, TestUi};
use rouge::core::{Clipboard, ClipboardMedia, PasteRequest, Point, Rect, Size, window_focused, zoom};
use rouge::persist::{FileStorage, MemoryStorage, Storage};
use rouge::reactive::{Entered, ReadSignal, Runtime as Reactive, Signal, set_tokio};
use rouge_widgets::testing::component_test;
use rouge_widgets::tokens::MotionScale;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::future;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime};
use tempfile::TempDir;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::oneshot;
use tokio::sync::watch;
use tokio::task::AbortHandle;

pub const FAKE_AGENT_LOCK_FILE: &str = "agent.lock";

/// The agent a harness passes with `--agent`, unless it's built `without_cli_agent`.
pub const TEST_AGENT: &str = "agent";

pub struct DesktopHarnessBuilder {
    window_size: Option<(f32, f32)>,
    connector: FakeConnector,
    projects: Vec<PathBuf>,
    sessions: FakeSessions,
    state_dir: Option<PathBuf>,
    remotes: Vec<RemoteSpec>,
    now: DateTime<Utc>,
    cli_agent: Option<CommandLine>,
    user_agents_file: Option<PathBuf>,
    platform: FakePlatform,
    motion: MotionScale,
}

#[derive(Clone, Default)]
pub struct FakeConnector {
    state: Arc<Mutex<FakeConnectorState>>,
    remotes: FakeRemotes,
}

#[derive(Clone, Default)]
pub struct FakeSessions {
    state: Arc<Mutex<FakeSessionsState>>,
}

#[derive(Default)]
struct FakeSessionsState {
    saved: Vec<(String, SessionInfo)>,
    failing: HashSet<String>,
    asked: Vec<(String, PathBuf)>,
    held: Option<watch::Sender<bool>>,
}

#[derive(Clone, Default)]
pub struct FakePlatform {
    state: Rc<RefCell<FakePlatformState>>,
}

pub struct DesktopHarness {
    ui: TestUi,
    app: DesktopApp,
    focused: ReadSignal<bool>,
    zoom: Signal<f32>,
    theme: ActiveReviewTheme,
    platform: FakePlatform,
    connector: FakeConnector,
    sessions: FakeSessions,
    remotes_dir: TempDir,
    runtime: Option<Runtime>,
    _entered: Entered,
}

pub fn launching_remote(name: &str, root: impl Into<PathBuf>) -> RemoteSpec {
    let command = |words: &[&str]| Some(words.iter().map(|word| (*word).to_owned()).collect());
    RemoteSpec {
        list: command(&["list"]),
        launch: command(&["launch"]),
        suspend: command(&["suspend", "${ID}"]),
        resume: command(&["resume", "${ID}"]),
        stop: command(&["stop", "${ID}"]),
        ..static_remote(name, root)
    }
}

pub fn static_remote(name: &str, root: impl Into<PathBuf>) -> RemoteSpec {
    let root = root.into();
    let cwd = root.to_string_lossy().into_owned();
    RemoteSpec {
        dir: Some(root),
        acp: Some(Acp { cwd: Some(cwd), ..Acp::stdio(vec!["agent".to_owned(), "${ID}".to_owned()]) }),
        ..RemoteSpec::named(name)
    }
}

impl Default for DesktopHarnessBuilder {
    fn default() -> Self {
        Self {
            window_size: None,
            connector: FakeConnector::default(),
            projects: Vec::new(),
            sessions: FakeSessions::default(),
            state_dir: None,
            remotes: Vec::new(),
            now: parse_time(DEFAULT_NOW),
            cli_agent: Some(CommandLine::new(TEST_AGENT)),
            user_agents_file: None,
            platform: FakePlatform::default(),
            motion: MotionScale::None,
        }
    }
}

impl DesktopHarnessBuilder {
    /// Widget motion is off unless a test asks for it, so no test waits for a panel to settle.
    pub fn motion(mut self, motion: MotionScale) -> Self {
        self.motion = motion;
        self
    }

    pub fn window_size(mut self, width: f32, height: f32) -> Self {
        self.window_size = Some((width, height));
        self
    }

    pub fn now(mut self, now: &str) -> Self {
        self.now = parse_time(now);
        self
    }

    pub fn agent(self, cwd: impl Into<PathBuf>, agent: FakeAgent) -> Self {
        let cwd = cwd.into();
        self.script(&cwd, agent).project(cwd)
    }

    pub fn script(self, cwd: impl Into<PathBuf>, agent: FakeAgent) -> Self {
        self.connector.script(cwd, agent);
        self
    }

    pub fn git(self, cwd: impl Into<PathBuf>, git: FakeGit) -> Self {
        self.connector.serve_git(cwd, git);
        self
    }

    pub fn remote(mut self, remote: RemoteSpec) -> Self {
        self.remotes.push(remote);
        self
    }

    pub fn failing(self, name: &str, problem: &str) -> Self {
        self.connector.remotes().fail(name, problem);
        self
    }

    pub fn instance(self, remote: &str, id: &str, state: InstanceState, expires_at: Option<&str>) -> Self {
        let expires_at = expires_at.map(|time| SystemTime::from(parse_time(time)));
        self.connector.remotes().add(remote, id, state, expires_at);
        self
    }

    pub fn project(mut self, root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        if !self.projects.contains(&root) {
            self.projects.push(root);
        }
        self
    }

    pub fn session<'a>(
        self,
        session_id: &str,
        cwd: impl AsRef<Path>,
        updated_at: &str,
        title: impl Into<Option<&'a str>>,
    ) -> Self {
        self.agent_session(TEST_AGENT, session_id, cwd, updated_at, title)
    }

    pub fn agent_session<'a>(
        self,
        agent: &str,
        session_id: &str,
        cwd: impl AsRef<Path>,
        updated_at: &str,
        title: impl Into<Option<&'a str>>,
    ) -> Self {
        self.sessions.save(agent, saved_session(session_id, cwd.as_ref(), updated_at, title.into()));
        self
    }

    /// Starts with only the agents of the user's and projects' files.
    pub fn without_cli_agent(mut self) -> Self {
        self.cli_agent = None;
        self
    }

    pub fn user_agents_file(mut self, file: impl Into<PathBuf>) -> Self {
        self.user_agents_file = Some(file.into());
        self
    }

    /// Puts `program` on the fake `PATH`.
    pub fn on_path(self, program: &str) -> Self {
        self.platform.state.borrow_mut().programs.insert(program.to_owned());
        self
    }

    pub fn state_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.state_dir = Some(dir.into());
        self
    }

    pub fn start(self, cwd: impl Into<PathBuf>) -> (DesktopHarness, Thread) {
        let mut harness = self.build();
        let thread = harness.start_thread(cwd);
        (harness, thread)
    }

    pub fn build(self) -> DesktopHarness {
        let entered = Reactive::new().enter();
        let runtime =
            Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("start the test Tokio runtime");
        let projects = self.projects;
        let (storage, state): (Rc<dyn Storage>, State) = if let Some(dir) = self.state_dir {
            // Dropping the seeding storage waits for its write, so the app's own storage starts from it.
            let state = seed_projects(&FileStorage::new(&dir), registry::saved_in(&dir), &projects);
            (Rc::new(FileStorage::new(dir)), state)
        } else {
            let storage = MemoryStorage::default();
            let state = seed_projects(&storage, State::default(), &projects);
            (Rc::new(storage), state)
        };
        let (width, height) = self.window_size.unwrap_or_else(|| {
            let size = window_size(&state);
            (size.width, size.height)
        });
        let remotes_dir = tempfile::tempdir().expect("a directory for the remotes file");
        let remotes_file = remotes_dir.path().join("remotes.json");
        let file = serde_json::to_string(&RemotesFile { remotes: self.remotes }).expect("the remotes serialize");
        fs::write(&remotes_file, file).expect("the remotes file is written");
        let platform = self.platform;
        let remotes = Arc::new(self.connector.remotes().clone());
        let config = DesktopConfig {
            connector: Arc::new(AgentConnector::new(remotes.clone(), Arc::new(self.connector.clone()))),
            clock: Arc::new(FakeClock(self.now)),
            agents: Agents::new(self.cli_agent, self.user_agents_file),
            remotes,
            remotes_file: Some(remotes_file),
            sessions: Arc::new(self.sessions.clone()),
            platform: Rc::new(platform.clone()),
            storage,
            saved: state,
        };
        let (handle, motion) = (runtime.handle().clone(), self.motion);
        let (mut ui, (app, focused, zoom, theme)) = component_test().size(width, height).build_with(move || {
            set_tokio(handle);
            motion.provide();
            let app = DesktopApp::new(config);
            (app.view(), (app, window_focused(), zoom(), ActiveReviewTheme::current()))
        });
        ui.set_clipboard(platform.clone());
        let mut harness = DesktopHarness {
            ui,
            app,
            focused,
            zoom,
            theme,
            platform,
            connector: self.connector,
            sessions: self.sessions,
            remotes_dir,
            runtime: Some(runtime),
            _entered: entered,
        };
        harness.advance();
        let fleet = harness.fleet();
        harness.run_until(|_| fleet.with_inventory(clankerdiff_remote::Inventory::has_listed));
        harness
    }
}

impl FakeConnector {
    pub fn script(&self, cwd: impl Into<PathBuf>, agent: FakeAgent) {
        self.lock().scripts.entry(cwd.into()).or_default().push_back(agent);
    }

    pub fn serve_git(&self, cwd: impl Into<PathBuf>, git: FakeGit) {
        self.lock().gits.insert(cwd.into(), git);
    }

    pub fn live_agents(&self, cwd: impl AsRef<Path>) -> usize {
        self.lock().live.get(cwd.as_ref()).map_or(0, Vec::len)
    }

    pub fn remotes(&self) -> &FakeRemotes {
        &self.remotes
    }

    pub fn kill_agents(&self, cwd: impl AsRef<Path>) {
        let agents = self.lock().live.remove(cwd.as_ref()).unwrap_or_default();
        for agent in agents {
            agent.task.abort();
        }
    }

    fn serve(&self, cwd: &Path, agent: FakeAgent, transport: Channel) {
        let served = self.clone();
        let served_cwd = cwd.to_path_buf();
        let mut state = self.lock();
        let id = state.next_agent;
        state.next_agent += 1;
        let git = state.gits.get(cwd).cloned();
        let task = tokio::spawn(async move {
            let _ = match git {
                Some(git) => git_diff_agent(agent, git).connect_to(transport).await,
                None => agent.agent().connect_to(transport).await,
            };
            served.lock().disconnected(&served_cwd, id);
        });
        state.live.entry(cwd.to_path_buf()).or_default().push(ServedAgent { id, task: task.abort_handle() });
    }

    fn lock(&self) -> MutexGuard<'_, FakeConnectorState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Transport for FakeConnector {
    fn stdio<'a>(&'a self, line: &'a CommandLine, dir: &'a Path) -> TransportFuture<'a, DynConnectTo<Client>> {
        Box::pin(async move {
            let agent = self.lock().take(dir).ok_or_else(|| ConnectError::Spawn {
                program: line.command().display().to_string(),
                source: io::Error::new(io::ErrorKind::NotFound, "no fake agent is scripted for this directory"),
            })?;
            let (agent_transport, client_transport) = Channel::duplex();
            self.serve(dir, agent, agent_transport);
            Ok(DynConnectTo::new(client_transport))
        })
    }

    fn websocket<'a>(&'a self, endpoint: &'a Endpoint, _: Duration) -> TransportFuture<'a, DynConnectTo<Client>> {
        Box::pin(async move {
            Err(ConnectError::Spawn {
                program: endpoint.url.clone(),
                source: io::Error::new(io::ErrorKind::Unsupported, "no fake agent is served over a WebSocket"),
            })
        })
    }

    fn run<'a>(&'a self, _: &'a CommandLine, _: &'a Path) -> TransportFuture<'a, Vec<u8>> {
        Box::pin(std::future::pending())
    }
}

impl FakeSessions {
    fn save(&self, agent: &str, session: SessionInfo) {
        self.lock().saved.push((agent.to_owned(), session));
    }

    /// Makes subsequent list requests for this agent fail.
    pub fn fail(&self, agent: &str) {
        self.lock().failing.insert(agent.to_owned());
    }

    /// Leaves list requests unanswered until `release`.
    pub fn hold(&self) {
        self.lock().held = Some(watch::channel(false).0);
    }

    /// Answers the list requests `hold` kept waiting.
    pub fn release(&self) {
        if let Some(held) = self.lock().held.take() {
            held.send_replace(true);
        }
    }

    /// Each agent asked for its sessions, by name and workspace, in the order they were asked.
    pub fn asked(&self) -> Vec<(String, PathBuf)> {
        self.lock().asked.clone()
    }

    fn lock(&self) -> MutexGuard<'_, FakeSessionsState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl SessionSource for FakeSessions {
    fn list<'a>(&'a self, agent: &'a AgentProfile) -> ListFuture<'a> {
        let mut state = self.lock();
        state.asked.push((agent.name.clone(), agent.workspace.clone()));
        if state.failing.contains(&agent.name) {
            let error = io::Error::new(io::ErrorKind::BrokenPipe, "the agent hung up");
            return Box::pin(future::ready(Err(ConnectError::Spawn { program: agent.name.clone(), source: error })));
        }
        let listed = |(name, session): &(String, SessionInfo)| *name == agent.name && session.cwd.0 == agent.workspace;
        let saved = state.saved.iter().filter(|saved| listed(saved)).map(|(_, session)| session.clone()).collect();
        let held = state.held.as_ref().map(watch::Sender::subscribe);
        Box::pin(async move {
            if let Some(mut held) = held {
                let _ = held.wait_for(|released| *released).await;
            }
            Ok(saved)
        })
    }
}

impl FakePlatform {
    pub fn notifications(&self) -> Vec<Notification> {
        self.state.borrow().notifications.clone()
    }

    pub fn click_notification(&self, tag: &str) {
        if let Some(clicked) = self.state.borrow_mut().clicks.remove(tag) {
            let _ = clicked.send(true);
        }
    }

    pub fn path_prompts(&self) -> Vec<PathPrompt> {
        self.state.borrow().prompts.iter().map(|(prompt, _)| prompt.clone()).collect()
    }

    pub fn answer_path_prompt(&self, paths: Option<Vec<PathBuf>>) -> Option<PathPrompt> {
        let (prompt, answer) = self.state.borrow_mut().prompts.pop_front()?;
        let _ = answer.send(paths);
        Some(prompt)
    }

    pub fn opened(&self) -> Vec<String> {
        self.state.borrow().opened.clone()
    }

    pub fn copy_text(&self, text: &str) {
        self.copy(Some(text.to_owned()), None);
    }

    pub fn copy_image(&self, png: &[u8]) {
        self.copy(None, Some(ClipboardMedia::Png(png.to_vec())));
    }

    pub fn copy_files(&self, paths: Vec<PathBuf>) {
        let names: Vec<_> =
            paths.iter().filter_map(|path| path.file_name()).map(|name| name.to_string_lossy().into_owned()).collect();
        self.copy(Some(names.join("\n")), Some(ClipboardMedia::Files(paths)));
    }

    pub fn copied_text(&self) -> Option<String> {
        self.state.borrow().clipboard_text.clone()
    }

    fn copy(&self, text: Option<String>, media: Option<ClipboardMedia>) {
        let mut state = self.state.borrow_mut();
        state.clipboard_text = text;
        state.clipboard_media = media;
    }
}

impl Clipboard for FakePlatform {
    fn read(&mut self, request: PasteRequest) {
        let (text, media) = {
            let state = self.state.borrow();
            (state.clipboard_text.clone(), state.clipboard_media.clone())
        };
        match (media.filter(|_| request.wants_media()), text) {
            (Some(media), _) => request.media(media),
            (None, Some(text)) => request.text(text),
            (None, None) => {}
        }
    }

    fn write(&mut self, text: &str) {
        self.copy_text(text);
    }
}

impl Platform for FakePlatform {
    fn notify(&self, notification: Notification) -> PlatformFuture<bool> {
        let (clicked, response) = oneshot::channel();
        let mut state = self.state.borrow_mut();
        state.clicks.insert(notification.tag.clone(), clicked);
        state.notifications.push(notification);
        Box::pin(async move { response.await.unwrap_or(false) })
    }

    fn pick_paths(&self, prompt: PathPrompt) -> PlatformFuture<Option<Vec<PathBuf>>> {
        let (answer, response) = oneshot::channel();
        self.state.borrow_mut().prompts.push_back((prompt, answer));
        Box::pin(async move { response.await.ok().flatten() })
    }

    fn open(&self, target: &str) {
        self.state.borrow_mut().opened.push(target.to_owned());
    }

    fn find_program(&self, program: &str) -> Option<PathBuf> {
        self.state.borrow().programs.contains(program).then(|| Path::new(FAKE_BIN).join(program))
    }
}

impl DesktopHarness {
    pub fn alpha(agent: FakeAgent) -> (Self, Thread) {
        DesktopHarnessBuilder::default().agent("/alpha", agent).start("/alpha")
    }

    pub fn app(&self) -> DesktopApp {
        self.app
    }

    pub fn threads(&self) -> ThreadManager {
        self.app.threads()
    }

    pub fn selected_thread(&self) -> Option<Thread> {
        self.threads().selected()
    }

    pub fn new_thread(&mut self, cwd: impl Into<PathBuf>) -> Thread {
        let cwd = cwd.into();
        let threads = self.threads();
        let agent = threads.default_agent(&cwd).expect("the project has an agent");
        let thread = threads.new_thread(cwd, agent);
        self.advance();
        // A fresh thread's composer is hidden, and so can't be clicked, until its kindling has shown it.
        self.ui.run(KINDLED_AFTER);
        thread
    }

    pub fn resume(&self, session_id: &str, cwd: impl AsRef<Path>) -> Thread {
        let threads = self.threads();
        threads
            .resume(SessionId::new(session_id), threads.default_agent(cwd.as_ref()).expect("the project has an agent"))
    }

    pub fn start_thread(&mut self, cwd: impl Into<PathBuf>) -> Thread {
        let thread = self.new_thread(cwd);
        self.run_until(|_| thread.status() == ThreadStatus::Idle);
        thread
    }

    pub fn connected_thread(&mut self) -> Thread {
        let thread = self.selected_thread().expect("a thread is selected");
        self.run_until(|_| thread.status() == ThreadStatus::Idle);
        thread
    }

    pub fn start_turn(&mut self, thread: Thread, requests: &mut FakeAgentRequests, prompt: &str) -> QueuedTurns {
        thread.send(prompt.into());
        self.next(&mut requests.prompt);
        self.run_until(|_| {
            thread.status() == ThreadStatus::Running
                && thread.with_core(|core| core.queued_prompts().is_empty()).unwrap_or(false)
        });
        requests.turns.clone()
    }

    pub fn send(&mut self, thread: Thread, prompt: &str) {
        self.until_answered(thread, prompt, |harness| {
            harness.type_text(prompt);
            harness.press("enter");
        });
    }

    pub fn submit(&mut self, thread: Thread, prompt: &str) {
        self.until_answered(thread, prompt, |_| thread.send(prompt.into()));
    }

    pub fn fleet(&self) -> Fleet {
        self.app.fleet()
    }

    pub fn remotes(&self) -> &FakeRemotes {
        self.connector.remotes()
    }

    pub fn remotes_file(&self) -> PathBuf {
        self.remotes_dir.path().join("remotes.json")
    }

    pub fn history(&self) -> History {
        self.app.history()
    }

    pub fn registry(&self) -> Registry {
        self.app.registry()
    }

    pub fn projects(&self) -> Vec<PathBuf> {
        self.registry().with(|state| state.projects.clone())
    }

    pub fn connector(&self) -> &FakeConnector {
        &self.connector
    }

    pub fn platform(&self) -> &FakePlatform {
        &self.platform
    }

    pub fn save_session(&self, session_id: &str, cwd: impl AsRef<Path>, updated_at: &str, title: Option<&str>) {
        self.sessions.save(TEST_AGENT, saved_session(session_id, cwd.as_ref(), updated_at, title));
    }

    pub fn sessions(&self) -> &FakeSessions {
        &self.sessions
    }

    pub fn ui(&mut self) -> &mut TestUi {
        &mut self.ui
    }

    pub fn bounds(&mut self, id: &str) -> Option<Rect> {
        self.advance();
        self.ui.bounds_of(id)
    }

    pub fn text(&mut self, id: &str) -> Option<String> {
        self.advance();
        self.ui.text_of(id)
    }

    pub fn shows(&mut self, id: &str, text: &str) -> bool {
        self.text(id).as_deref() == Some(text)
    }

    pub fn click(&mut self, id: &str) -> Result<(), TestError> {
        self.ui.click_on(id)
    }

    pub fn click_at(&mut self, point: Point) {
        self.ui.click_at(point.x, point.y);
    }

    pub fn click_text(&mut self, text: &str) -> Result<(), TestError> {
        self.advance();
        self.ui.click_text(text)
    }

    pub fn theme(&mut self) -> ReviewTheme {
        self.advance();
        self.theme.review_signal().get_untracked()
    }

    pub fn right_click(&mut self, id: &str) -> Result<(), TestError> {
        self.ui.right_click_on(id)
    }

    pub fn hover(&mut self, id: &str) -> Result<(), TestError> {
        self.ui.hover_on(id)
    }

    pub fn drag(&mut self, id: &str, to: Point) -> Result<(), TestError> {
        self.ui.drag_on(id, to)
    }

    pub fn scroll(&mut self, id: &str, delta: Point) -> Result<(), TestError> {
        self.ui.scroll_on(id, delta)
    }

    pub fn click_nth(&mut self, id: &str, index: usize) -> Result<(), TestError> {
        self.ui.click_nth(id, index)
    }

    pub fn scroll_into_view(&mut self, scroller: &str, id: &str) -> Result<(), TestError> {
        self.ui.scroll_into_view(scroller, id)
    }

    pub fn type_text(&mut self, text: &str) {
        self.ui.type_text(text);
    }

    pub fn press(&mut self, keys: &str) {
        self.ui.press(keys);
        self.advance();
    }

    pub fn drop_files(&mut self, on: &str, paths: Vec<PathBuf>) -> Result<(), TestError> {
        self.ui.drop_files_on(on, paths)
    }

    pub fn resize(&mut self, width: f32, height: f32) {
        self.ui.set_size(Size::new(width, height));
        self.advance();
    }

    /// The interface size the window is zoomed to, in the px the UI is designed at.
    pub fn interface_size(&mut self) -> f32 {
        self.advance();
        self.zoom.get_untracked() * BASE_SIZE
    }

    pub fn advance(&mut self) {
        self.ui.advance(0.0);
        while !self.ui.take_window_commands().is_empty() {
            self.ui.advance(0.0);
        }
    }

    pub fn run_until(&mut self, condition: impl Fn(&mut Self) -> bool) {
        loop {
            self.advance();
            if condition(self) {
                return;
            }
            std::thread::sleep(POLL);
        }
    }

    pub fn next<T>(&mut self, receiver: &mut UnboundedReceiver<T>) -> T {
        loop {
            self.advance();
            if let Ok(message) = receiver.try_recv() {
                return message;
            }
            std::thread::sleep(POLL);
        }
    }

    pub fn run_until_painted(&mut self, id: &str) {
        self.run_until(|harness| harness.ui.bounds_of(id).is_some());
    }

    pub fn activate(&mut self) {
        self.ui.set_focused(true);
        self.advance();
    }

    pub fn deactivate(&mut self) {
        self.ui.set_focused(false);
        self.advance();
    }

    pub fn is_active(&mut self) -> bool {
        self.advance();
        self.focused.get_untracked()
    }

    pub fn click_notification(&mut self, tag: &str) {
        self.platform.click_notification(tag);
        self.advance();
    }

    /// Closes the app, saving what it remembers.
    pub fn quit(self) {
        drop(self);
    }

    fn until_answered(&mut self, thread: Thread, prompt: &str, send: impl FnOnce(&mut Self)) {
        let times_sent = |thread: Thread| {
            thread
                .with_core(|core| {
                    core.conversation()
                        .items()
                        .iter()
                        .filter(|item| item.text().is_some_and(|text| text == prompt))
                        .count()
                })
                .unwrap_or(0)
        };
        let expected = times_sent(thread) + 1;
        send(self);
        self.run_until(|_| thread.status() == ThreadStatus::Idle && times_sent(thread) == expected);
    }
}

impl Drop for DesktopHarness {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

/// Adds `projects` to `state`, saving it in `storage`, and returns it.
fn seed_projects(storage: &dyn Storage, mut state: State, projects: &[PathBuf]) -> State {
    let before = state.projects.len();
    for root in projects {
        state.add_project(root.clone());
    }
    if state.projects.len() != before {
        // File and memory storage both take the write as it's asked for; the future only reports it done.
        drop(storage.write(STATE_KEY, serde_json::to_string_pretty(&state).expect("the state serializes")));
    }
    state
}

struct FakeClock(DateTime<Utc>);

impl Clock for FakeClock {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }
}

#[derive(Default)]
struct FakeConnectorState {
    scripts: HashMap<PathBuf, VecDeque<FakeAgent>>,
    gits: HashMap<PathBuf, FakeGit>,
    live: HashMap<PathBuf, Vec<ServedAgent>>,
    next_agent: u64,
}

struct ServedAgent {
    id: u64,
    task: AbortHandle,
}

#[derive(Default)]
struct FakePlatformState {
    notifications: Vec<Notification>,
    clicks: HashMap<String, oneshot::Sender<bool>>,
    prompts: VecDeque<(PathPrompt, oneshot::Sender<Option<Vec<PathBuf>>>)>,
    opened: Vec<String>,
    clipboard_text: Option<String>,
    clipboard_media: Option<ClipboardMedia>,
    programs: HashSet<String>,
}

impl FakeConnectorState {
    fn take(&mut self, cwd: &Path) -> Option<FakeAgent> {
        self.scripts.get_mut(cwd)?.pop_front()
    }

    fn disconnected(&mut self, cwd: &Path, id: u64) {
        if let Some(live) = self.live.get_mut(cwd) {
            live.retain(|agent| agent.id != id);
        }
    }
}

fn saved_session(session_id: &str, cwd: &Path, updated_at: &str, title: Option<&str>) -> SessionInfo {
    SessionInfo::new(session_id, AbsolutePath::new(cwd))
        .updated_at(updated_at.to_owned())
        .title(title.map(str::to_owned))
}

const FAKE_BIN: &str = "/fake/bin";
const DEFAULT_NOW: &str = "2026-01-07T12:00:00Z";
const POLL: Duration = Duration::from_millis(1);

fn parse_time(time: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(time).expect("an RFC 3339 time").to_utc()
}
