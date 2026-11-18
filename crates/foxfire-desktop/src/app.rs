use crate::agents::{AUTO_ADDED_AGENT, AgentProfile, Agents, Catalogs};
use crate::chat::PastSessions;
use crate::clock::Clock;
use crate::file_picker::file_picker;
use crate::history::History;
use crate::platform::{Notification, Platform};
use crate::registry::{Registry, State, WindowSize};
use crate::remotes::Fleet;
use crate::sessions::{SessionKey, SessionSource};
use crate::settings::settings;
use crate::sidebar::{Sidebar, SwitcherEntry, toggle_button};
use crate::target::AgentConnector;
use crate::tasks::spawn_in;
use crate::thread_view::{ThreadTab, ThreadView};
use crate::threads::{Attention, Thread, ThreadEvent, ThreadManager};
use crate::ui::DEFAULT_INTERFACE_SIZE;
use crate::ui::tokens::{GUTTER, HEADER_HEIGHT, MODAL_WIDTH, PICKER_LIST_HEIGHT};
use crate::ui::{
    ActiveReviewTheme, FoxfireMotion, IconName, InterfaceSize, Material, PickerItem, SearchPickerHandle, StyledExt,
    Toasts, UI_FONT, UiStyles, appearance, backdrop, empty_state, picker_closed, search_picker,
};
use clankerdiff_core::RepoPath;
use clankerdiff_remote::Remotes;
use clankerdiff_rouge::ReviewHandle;
use rouge::core::{
    AnyView, Chord, Div, Role, Styled, View, WindowHandle, div, dynamic, px, view, window_focused, window_handle,
    window_size, zoom,
};
use rouge::persist::Storage;
use rouge::reactive::{Scope, Signal, Stored, effect, memo, signal, spawn, stored, untrack};
use std::cell::Cell;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectNextThread;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectPreviousThread;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenThreadSwitcher;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShowChat;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShowFiles;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenFile;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenSettings;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewThread;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewCloudThread;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToggleSidebar;

pub struct DesktopConfig {
    pub connector: Arc<AgentConnector>,
    pub clock: Arc<dyn Clock>,
    pub agents: Agents,
    pub remotes: Arc<dyn Remotes>,
    pub remotes_file: Option<PathBuf>,
    pub sessions: Arc<dyn SessionSource>,
    pub platform: Rc<dyn Platform>,
    pub storage: Rc<dyn Storage>,
    /// The state `storage` held when the app launched.
    pub saved: State,
}

#[derive(Clone, Copy)]
pub struct DesktopApp {
    threads: ThreadManager,
    history: History,
    registry: Registry,
    agents: Catalogs,
    fleet: Fleet,
    sidebar: Sidebar,
    views: Stored<HashMap<usize, (Scope, ThreadView)>>,
    modal: Signal<Option<Modal>>,
    platform: Stored<Rc<dyn Platform>>,
    clock: Stored<Arc<dyn Clock>>,
    restore: Stored<Option<SessionKey>>,
    window: WindowHandle,
    toasts: Toasts,
    scope: Scope,
}

#[derive(Clone)]
enum Modal {
    Switcher(Rc<[SwitcherEntry]>),
    Files(Thread),
    Settings,
}

pub(crate) const NEW_THREAD_KEYS: &str = "secondary-n";
pub(crate) const NEW_CLOUD_THREAD_KEYS: &str = "secondary-alt-n";
pub(crate) const TOGGLE_SIDEBAR_KEYS: &str = "secondary-b";
pub(crate) const SWITCHER_KEYS: &str = "secondary-k";
pub(crate) const NEXT_THREAD_KEYS: &str = "secondary-]";
pub(crate) const PREVIOUS_THREAD_KEYS: &str = "secondary-[";
pub(crate) const SHOW_CHAT_KEYS: &str = "secondary-1";
pub(crate) const SHOW_FILES_KEYS: &str = "secondary-2";
pub(crate) const OPEN_FILE_KEYS: &str = "secondary-p";
pub(crate) const SETTINGS_KEYS: &str = "secondary-,";

impl DesktopApp {
    pub fn new(config: DesktopConfig) -> Self {
        let DesktopConfig { connector, clock, agents, remotes, remotes_file, sessions, platform, storage, saved } =
            config;
        let toasts = Toasts::install();
        let registry = Registry::restore(saved, storage);
        let preference = memo(move || registry.with(|state| state.theme.clone()));
        let active = ActiveReviewTheme::install(preference.get_untracked().theme(appearance().get_untracked()));
        effect(move || active.apply(&preference.get()));
        let motion = FoxfireMotion::install(registry.with_untracked(|state| state.reduced_motion));
        effect(move || motion.set_reduced(registry.with(|state| state.reduced_motion)));
        let size = InterfaceSize::install(DEFAULT_INTERFACE_SIZE);
        if let Some(saved) = registry.with(|state| state.interface_size) {
            size.set(saved);
        }
        effect(move || {
            let committed = size.committed();
            untrack(|| {
                registry
                    .update(|state| state.interface_size = (committed != DEFAULT_INTERFACE_SIZE).then_some(committed));
            });
        });
        let agents = Catalogs::new(auto_add_agent(agents, platform.as_ref(), registry));
        let threads = ThreadManager::new(connector, agents, clock.clone());
        let history = History::new(sessions);
        let fleet = Fleet::new(remotes, remotes_file);
        let platform = stored(platform);
        let sidebar = Sidebar::new(threads, registry, agents, fleet, clock.clone(), platform);
        let app = Self {
            threads,
            history,
            registry,
            agents,
            fleet,
            sidebar,
            views: stored(HashMap::new()),
            modal: signal(None),
            platform,
            clock: stored(clock),
            restore: stored(registry.with(|state| state.last_selected.clone())),
            window: window_handle(),
            toasts,
            scope: Scope::new(),
        };
        app.observe();
        app
    }

    pub fn threads(&self) -> ThreadManager {
        self.threads
    }

    pub fn history(&self) -> History {
        self.history
    }

    pub fn registry(&self) -> Registry {
        self.registry
    }

    pub fn fleet(&self) -> Fleet {
        self.fleet
    }

    pub fn diff_review(&self, thread: Thread) -> Option<ReviewHandle> {
        let view = self.views.with(|views| views.get(&thread.number()).map(|(_, view)| *view))?;
        view.files().review()
    }

    pub fn view(self) -> impl View {
        let collapsed = memo(move || self.registry.with(|state| state.sidebar_collapsed));
        let selected = memo(move || self.threads.selected());
        let root = view! {
            <div test_id="desktop" relative size_full flex bg=Role::Canvas text_color=Role::Text font=UI_FONT body>
                <dynamic>{(!collapsed.get()).then(|| self.sidebar.view())}</dynamic>
                <div test_id="main" flex_1 min_w=0.0 h_full>
                    <dynamic>{match selected.get() {
                        Some(thread) => untrack(|| self.view_of(thread).view(collapsed)).into_any(),
                        None => no_thread(collapsed.get()).into_any(),
                    }}</dynamic>
                </div>
                <dynamic>{self.modal.get().map(|modal| untrack(|| self.render_modal(modal)))}</dynamic>
                {self.toasts}
            </div>
        };
        InterfaceSize::current().bind(self.bind(root))
    }

    fn bind(self, root: Div) -> Div {
        let root = root
            .bind_key(chord(NEW_THREAD_KEYS), NewThread)
            .bind_key(chord(NEW_CLOUD_THREAD_KEYS), NewCloudThread)
            .bind_key(chord(TOGGLE_SIDEBAR_KEYS), ToggleSidebar)
            .bind_key(chord(SWITCHER_KEYS), OpenThreadSwitcher)
            .bind_key(chord(NEXT_THREAD_KEYS), SelectNextThread)
            .bind_key(chord(PREVIOUS_THREAD_KEYS), SelectPreviousThread)
            .bind_key(chord(SHOW_CHAT_KEYS), ShowChat)
            .bind_key(chord(SHOW_FILES_KEYS), ShowFiles)
            .bind_key(chord(OPEN_FILE_KEYS), OpenFile)
            .bind_key(chord(SETTINGS_KEYS), OpenSettings);
        root.on_action(move |_: &NewThread| self.sidebar.new_thread())
            .on_action(move |_: &NewCloudThread| self.sidebar.new_cloud_thread())
            .on_action(move |_: &ToggleSidebar| self.toggle_sidebar())
            .on_action(move |_: &SelectNextThread| self.step_selection(1))
            .on_action(move |_: &SelectPreviousThread| self.step_selection(-1))
            .on_action(move |_: &OpenThreadSwitcher| self.open_switcher())
            .on_action(move |_: &OpenSettings| self.present(Modal::Settings))
            .on_action(move |_: &ShowChat| self.show_tab(ThreadTab::Chat))
            .on_action(move |_: &ShowFiles| self.show_tab(ThreadTab::Files))
            .on_action(move |_: &OpenFile| self.open_file_picker())
    }

    fn observe(self) {
        let threads = self.threads;
        threads.on_attention(move |attention| self.on_attention(&attention));
        let focused = window_focused();
        let first = Cell::new(true);
        effect(move || {
            let active = focused.get();
            untrack(|| {
                threads.set_window_active(active);
                if active && !first.replace(false) {
                    self.agents.reload();
                    self.fleet.reload();
                }
            });
        });
        let (size, zoom) = (window_size(), zoom());
        effect(move || {
            let (size, zoom) = (size.get(), zoom.get());
            untrack(|| {
                self.registry.update(|state| {
                    state.window_size = Some(WindowSize { width: size.width * zoom, height: size.height * zoom });
                });
            });
        });
        let live = memo(move || threads.threads());
        effect(move || {
            let live = live.get();
            untrack(|| self.drop_views(&live));
        });
        let followed = memo(move || {
            threads
                .threads()
                .iter()
                .map(|thread| (thread.project(), thread.target().instance().map(str::to_owned)))
                .collect::<Vec<_>>()
        });
        let launched = stored(BTreeSet::new());
        effect(move || {
            let followed = followed.get();
            untrack(|| self.follow_threads(&followed, launched));
        });
        let selected_session = memo(move || threads.selected().and_then(|thread| thread.session_key()));
        let remembered = Cell::new(false);
        effect(move || {
            let session_id = selected_session.get();
            if remembered.replace(true) || session_id.is_some() {
                untrack(|| self.registry.update(|state| state.last_selected = session_id));
            }
        });
        let roots = memo(move || self.registry.with(|state| state.projects.clone()));
        effect(move || {
            self.agents.revision();
            roots.with(|roots| untrack(|| self.list_restorable(roots)));
        });
        if let Some(key) = self.restore.get() {
            let candidate = memo(move || self.history.restorable(&key));
            effect(move || {
                let (ready, session) = candidate.get();
                untrack(|| {
                    if ready
                        && self.threads.selected().is_none()
                        && self.restore.update(Option::take).is_some()
                        && let Some(session) = session
                    {
                        self.threads.resume(session.id, session.agent);
                    }
                });
            });
        }
    }

    fn view_of(self, thread: Thread) -> ThreadView {
        if let Some((_, view)) = self.views.with(|views| views.get(&thread.number()).copied()) {
            return view;
        }
        let scope = self.scope.child();
        let view = scope.run(|| {
            let past = PastSessions::new(thread, self.threads, self.history, self.agents, self.registry, self.clock);
            ThreadView::new(thread, self.registry, self.platform.get(), &past)
        });
        self.views.update(|views| views.insert(thread.number(), (scope, view)));
        view
    }

    fn drop_views(self, live: &[Thread]) {
        let dropped: Vec<Scope> = self.views.update(|views| {
            let gone: Vec<usize> =
                views.keys().copied().filter(|number| live.iter().all(|thread| thread.number() != *number)).collect();
            gone.into_iter().filter_map(|number| views.remove(&number)).map(|(scope, _)| scope).collect()
        });
        if dropped.is_empty() {
            return;
        }
        self.history.refresh();
        spawn(async move {
            for scope in dropped {
                scope.dispose();
            }
        });
    }

    fn follow_threads(self, followed: &[(Option<PathBuf>, Option<String>)], launched: Stored<BTreeSet<String>>) {
        let listed = self.registry.with(|state| state.projects.clone());
        let unlisted: Vec<PathBuf> =
            followed.iter().filter_map(|(project, _)| project.clone()).filter(|root| !listed.contains(root)).collect();
        if !unlisted.is_empty() {
            self.registry.update(|state| unlisted.into_iter().for_each(|root| state.add_project(root)));
        }
        let now: BTreeSet<String> = followed.iter().filter_map(|(_, instance)| instance.clone()).collect();
        let grew = launched.with(|launched| !now.is_subset(launched));
        launched.set(now);
        if grew {
            self.fleet.refresh();
        }
    }

    /// Asks only the agent that saved the last selected session, in each project that has it, for its sessions.
    fn list_restorable(self, roots: &[PathBuf]) {
        let Some(name) = self.restore.with(|restore| restore.as_ref().map(|key| key.agent().to_owned())) else {
            return;
        };
        let agents: Vec<AgentProfile> = roots
            .iter()
            .filter_map(|root| self.agents.catalog(root).get(&name).cloned())
            .filter(|agent| self.history.listing(agent).is_none())
            .collect();
        self.history.list(&agents);
    }

    fn select_session(self, session_id: &SessionKey) {
        if let Some(thread) = self.threads.thread_with_session(session_id) {
            self.threads.select(thread);
        }
    }

    fn on_attention(self, attention: &Attention) {
        if attention.event == ThreadEvent::TurnFinished {
            self.history.refresh();
        }
        if !attention.watched {
            self.notify(attention);
        }
    }

    fn notify(self, attention: &Attention) {
        let thread = attention.thread;
        let Some(session_id) = thread.session_key() else {
            return;
        };
        let body = match attention.event {
            ThreadEvent::NeedsInput => {
                thread.elicitation().map_or_else(|| NEEDS_INPUT.to_owned(), |(_, elicitation)| elicitation.question())
            }
            _ => TURN_FINISHED.to_owned(),
        };
        let title = self.registry.with(|state| state.title(Some(&session_id), thread.title()));
        let clicked =
            self.platform.with(|platform| platform.notify(Notification { tag: session_id.to_string(), title, body }));
        let window = self.window;
        spawn_in(self.scope, async move {
            if clicked.await {
                window.focus();
                self.select_session(&session_id);
            }
        });
    }

    fn step_selection(self, step: isize) {
        let order = self.sidebar.live_order();
        let len = order.len();
        if len == 0 {
            return;
        }
        let selected = self.threads.selected();
        let position = selected.and_then(|selected| order.iter().position(|listed| *listed == selected));
        let from = position.unwrap_or(if step > 0 { len - 1 } else { 0 });
        let next = (from + len).wrapping_add_signed(step) % len;
        self.threads.select(order[next]);
    }

    fn open_switcher(self) {
        if self.modal.with(Option::is_none) {
            self.present(Modal::Switcher(self.sidebar.switcher_entries().into()));
        }
    }

    fn open_file_picker(self) {
        if let Some(thread) = self.threads.selected().filter(|_| self.modal.with(Option::is_none)) {
            self.present(Modal::Files(thread));
        }
    }

    fn present(self, modal: Modal) {
        if self.modal.with(Option::is_none) {
            self.modal.set(Some(modal));
        }
    }

    fn close_modal(self) {
        self.modal.set(None);
    }

    fn show_tab(self, tab: ThreadTab) {
        if let Some(thread) = self.threads.selected() {
            self.view_of(thread).show(tab);
        }
    }

    fn open_file(self, thread: Thread, path: RepoPath) {
        self.threads.select(thread);
        self.view_of(thread).open_file(path, None);
    }

    fn toggle_sidebar(self) {
        self.registry.update(|state| state.sidebar_collapsed.toggle());
    }

    fn render_modal(self, modal: Modal) -> AnyView {
        match modal {
            Modal::Switcher(entries) => {
                let items = entries
                    .iter()
                    .map(|entry| {
                        PickerItem::new(format!("switcher-{}", entry.key.selector()), entry.title.clone())
                            .detail(entry.project.clone())
                    })
                    .collect();
                let handle = SearchPickerHandle::new(items, move |event| {
                    let closed = picker_closed(&event, |index| {
                        if let Some(entry) = entries.get(index) {
                            self.threads.select(entry.thread);
                        }
                    });
                    if closed {
                        self.close_modal();
                    }
                });
                view! {
                    <backdrop test_id="switcher-backdrop" align_top on_dismiss={move || self.close_modal()}>
                        <search_picker test_id="switcher" handle material={Material::Glass} searchable="Search threads…"
                                       empty="No matching threads" footer=FOOTER w={px(MODAL_WIDTH)}
                                       h={px(PICKER_LIST_HEIGHT)}/>
                    </backdrop>
                }
                .into_any()
            }
            Modal::Files(thread) => file_picker(thread, move |picked: Option<RepoPath>| {
                self.close_modal();
                if let Some(path) = picked {
                    self.open_file(thread, path);
                }
            })
            .into_any(),
            Modal::Settings => settings(self.registry, move || self.close_modal()).into_any(),
        }
    }
}

pub(crate) fn chord(keys: &str) -> Chord {
    Chord::parse(keys).unwrap_or_else(|error| unreachable!("built-in chord {keys}: {error}"))
}

fn auto_add_agent(mut agents: Agents, platform: &dyn Platform, registry: Registry) -> Agents {
    let name = AUTO_ADDED_AGENT.name;
    if registry.with_untracked(|state| state.auto_added_agents.contains(name))
        || platform.find_program(AUTO_ADDED_AGENT.command).is_none()
    {
        return agents;
    }
    match agents.add(AUTO_ADDED_AGENT) {
        Ok(()) => registry.update(|state| {
            state.auto_added_agents.insert(name.to_owned());
        }),
        Err(error) => tracing::warn!(%error, agent = name, "could not add an agent found on PATH"),
    }
    agents
}

fn no_thread(collapsed: bool) -> impl View {
    let header = collapsed.then(|| {
        view! { <div top_bar h=HEADER_HEIGHT pr=GUTTER>{toggle_button(false)}</div> }
    });
    view! {
        <div size_full flex col>
            {header}
            <div flex_1 min_h=0.0 flex items_center justify_center>
                <empty_state icon=IconName::MessageSquareText title="No thread selected"
                             detail="Select a thread, or start a new one"/>
            </div>
        </div>
    }
}

const FOOTER: &str = "↑↓ Move    ↵ Open    Esc Close";
const TURN_FINISHED: &str = "The agent finished its turn";
const NEEDS_INPUT: &str = "The agent needs your input";
