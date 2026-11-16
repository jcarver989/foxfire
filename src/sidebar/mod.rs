mod listing;
mod row;

pub(crate) use listing::SwitcherEntry;

use crate::agents::{AGENTS_FILE, AgentProfile, Catalogs, KNOWN_AGENTS, KnownAgent};
use crate::app::{OpenSettings, OpenThreadSwitcher};
use crate::clock::Clock;
use crate::history::{History, git_root};
use crate::platform::{PathPrompt, Platform};
use crate::registry::{Collapsed, Registry};
use crate::remotes::{Action, Fleet, RemoteSpec};
use crate::sessions::SessionKey;
use crate::tasks::spawn_in;
use crate::threads::{Thread, ThreadManager};
use crate::ui::tokens::{GUTTER, HEADER_HEIGHT};
use crate::ui::{
    Icon, IconName, ListRow, MenuItem, Side, StyledExt, UiStyles, context_menu, dispatch, icon_button, icon_slot,
    list_row, resizable_panel, section_header, tree_row,
};
use client_core::fs::write_json_atomic;
use listing::{Listing, RemoteSection, Row, Section, project_name};
use rouge::core::{AnyView, Div, Role, Styled, View, div, dynamic, each, scroll, show, text, view, window_size};
use rouge::reactive::{Callback, Memo, ReadSignal, Scope, Signal, Stored, effect, memo, signal, stored, untrack};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

pub const MIN_MAIN_WIDTH: f32 = 320.0;

#[derive(Clone, Copy)]
pub struct Sidebar {
    threads: ThreadManager,
    registry: Registry,
    agents: Catalogs,
    fleet: Fleet,
    platform: Stored<Rc<dyn Platform>>,
    sources: Sources,
    limits: Signal<HashMap<PathBuf, usize>>,
    sections: Memo<Vec<Section>>,
    remotes: Memo<Vec<RemoteSection>>,
    scope: Scope,
}

#[derive(Clone, Copy)]
struct Sources {
    threads: ThreadManager,
    history: History,
    registry: Registry,
    fleet: Fleet,
    clock: Stored<Arc<dyn Clock>>,
    rename: Signal<Option<Rename>>,
}

#[derive(Clone, PartialEq)]
struct Rename {
    session_id: SessionKey,
    value: Signal<String>,
}

pub(crate) fn toggle_button(open: bool) -> AnyView {
    view! {
        <div flex_none flex items_center>
            <icon_button test_id="toggle-sidebar" icon=IconName::PanelLeft label="Toggle sidebar" selected=open
                         on_click={|| dispatch(crate::ToggleSidebar)}/>
        </div>
    }
    .into_any()
}

impl Sidebar {
    pub(crate) fn new(
        threads: ThreadManager,
        history: History,
        registry: Registry,
        agents: Catalogs,
        fleet: Fleet,
        clock: Arc<dyn Clock>,
        platform: Stored<Rc<dyn Platform>>,
    ) -> Self {
        let sources = Sources { threads, history, registry, fleet, clock: stored(clock), rename: signal(None) };
        let limits = signal(HashMap::new());
        let sections = memo(move || limits.with(|limits| sources.with_listing(|listing| listing.sections(limits))));
        #[allow(
            clippy::redundant_closure_for_method_calls,
            reason = "The method path cannot name the listing's borrowed lifetime."
        )]
        let remotes = memo(move || sources.with_listing(|listing| listing.remotes()));
        Self { threads, registry, agents, fleet, platform, sources, limits, sections, remotes, scope: Scope::new() }
    }

    pub(crate) fn new_thread(self) {
        if let Some(root) = self.default_project() {
            self.new_thread_in(root);
        }
    }

    pub(crate) fn new_cloud_thread(self) {
        if let Some(remote) = self.default_remote() {
            self.threads.new_cloud_thread(&remote);
        }
    }

    pub(crate) fn live_order(self) -> Vec<Thread> {
        let listed = self.sections.with(|sections| {
            sections.iter().flat_map(|section| &section.rows).filter_map(Row::live).collect::<Vec<_>>()
        });
        let cloud = self
            .remotes
            .with(|remotes| remotes.iter().flat_map(|section| &section.rows).filter_map(Row::live).collect::<Vec<_>>());
        let mut order: Vec<Thread> = listed.into_iter().chain(cloud).collect();
        for thread in self.threads.threads() {
            if !order.contains(&thread) {
                order.push(thread);
            }
        }
        order
    }

    pub(crate) fn switcher_entries(self) -> Vec<SwitcherEntry> {
        let order = self.live_order();
        self.with_listing(|listing| listing.switcher_entries(&order))
    }

    pub(crate) fn view(self) -> impl View {
        let stored_width = memo(move || self.registry.with(|state| state.sidebar_width));
        let width = signal(stored_width.get_untracked().unwrap_or(DEFAULT_SIDEBAR_WIDTH));
        effect(move || {
            width.set_if_changed(stored_width.get().unwrap_or(DEFAULT_SIDEBAR_WIDTH));
        });
        let window = window_size();
        let on_resize = move |requested: f32| {
            let available = window.get_untracked().width;
            let widest = (available - MIN_MAIN_WIDTH).clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
            let chosen = requested.min(widest);
            width.set_if_changed(chosen);
            self.registry.update(|state| state.sidebar_width = Some(chosen));
        };
        let on_reset = move || self.registry.update(|state| state.sidebar_width = None);
        view! {
            <resizable_panel test_id="sidebar" edge=Side::Left size=width min_size=MIN_SIDEBAR_WIDTH
                             max_size=MAX_SIDEBAR_WIDTH on_resize=on_resize on_reset=on_reset h_full flex col bg=Role::Chrome>
                <div top_bar h=HEADER_HEIGHT pr=GUTTER gap=4.0>
                    <div flex_1/>
                    <icon_button test_id="search-threads" icon=IconName::Search label="Search threads"
                                 on_click={|| dispatch(OpenThreadSwitcher)}/>
                    <icon_button test_id="open-settings" icon=IconName::Settings label="Settings"
                                 on_click={|| dispatch(OpenSettings)}/>
                    {toggle_button(true)}
                </div>
                <scroll test_id="projects" flex_1 min_h=0.0 w_full>
                    <div px=8.0 pb=8.0 flex col gap=4.0>
                        {self.local()}
                        {self.cloud()}
                    </div>
                </scroll>
            </resizable_panel>
        }
    }

    fn with_listing<T>(self, read: impl FnOnce(&Listing<'_>) -> T) -> T {
        self.sources.with_listing(read)
    }

    fn is_collapsed(self, group: &Collapsed) -> bool {
        self.registry.with(|state| state.collapsed.contains(group))
    }

    fn toggle(self, group: Collapsed) {
        self.registry.update(|state| state.toggle_collapsed(group));
    }

    fn new_thread_in(self, root: PathBuf) {
        let catalog = self.agents.catalog(&root);
        let last = self.registry.with(|state| state.last_agents.get(&root).cloned());
        let agent = last.and_then(|name| catalog.get(&name)).or_else(|| catalog.default_agent()).cloned();
        if let Some(agent) = agent {
            self.threads.new_thread(root, agent);
        }
    }

    fn add_agent(self, root: PathBuf, agent: KnownAgent) {
        match self.agents.add(agent) {
            Ok(()) => self.new_thread_in(root),
            Err(error) => tracing::warn!(%error, agent = agent.name, "could not add an agent"),
        }
    }

    fn open_agents_file(self) {
        let Some(file) = self.agents.user_file() else {
            return;
        };
        if !file.exists()
            && let Err(error) = write_json_atomic(&file, &json!({ "agents": {} }))
        {
            tracing::warn!(%error, "could not create the agents file");
            return;
        }
        self.open_path(&file);
    }

    fn new_thread_with(self, root: PathBuf, agent: AgentProfile) {
        let name = agent.name.clone();
        self.registry.update(|state| {
            state.last_agents.insert(root.clone(), name);
        });
        self.threads.new_thread(root, agent);
    }

    fn default_project(self) -> Option<PathBuf> {
        let selected = self.threads.selected().and_then(|thread| thread.project());
        selected.or_else(|| self.sections.with(|sections| sections.first().map(|section| section.root.clone())))
    }

    fn default_remote(self) -> Option<RemoteSpec> {
        let selected = self.threads.selected().and_then(|thread| thread.target().remote_name().map(str::to_owned));
        self.fleet.with_inventory(|inventory| {
            selected
                .and_then(|name| inventory.remote(&name))
                .filter(|remote| starts_threads(remote))
                .or_else(|| inventory.remotes().iter().find(|remote| starts_threads(remote)))
                .cloned()
        })
    }

    fn browse_for_project(self) {
        let prompt = PathPrompt { title: "Add project".to_owned(), directories: true, multiple: false };
        let picked = self.platform.with(|platform| platform.pick_paths(prompt));
        let registry = self.registry;
        spawn_in(self.scope, async move {
            let Some(paths) = picked.await else {
                return;
            };
            registry.update(|state| {
                for path in paths {
                    state.add_project(git_root(&path));
                }
            });
        });
    }

    fn archive_inactive(self, root: &Path) {
        let inactive = self.with_listing(|listing| listing.inactive_sessions(root));
        self.registry.update(|state| state.archived.extend(inactive));
    }

    fn remove_project(self, root: &Path) {
        if self.threads.projects().iter().all(|project| project != root) {
            self.registry.update(|state| state.projects.retain(|project| project != root));
        }
    }

    fn show_more(self, root: PathBuf) {
        self.limits.update(|limits| *limits.entry(root).or_insert(listing::SAVED_ROWS) += MORE_SAVED_ROWS);
    }

    fn start_rename(self, session_id: SessionKey, title: &str) {
        let value = self.scope.run(|| signal(title.to_owned()));
        self.sources.rename.set(Some(Rename { session_id, value }));
    }

    fn commit_rename(self) {
        if let Some(Rename { session_id, value }) = self.sources.rename.get_untracked() {
            let title = value.get_untracked();
            self.sources.rename.set(None);
            self.registry.update(|state| state.rename(session_id, &title));
        }
    }

    fn cancel_rename(self) {
        if self.sources.rename.with_untracked(Option::is_some) {
            self.sources.rename.set(None);
        }
    }

    fn local(self) -> impl View {
        let collapsed = memo(move || self.is_collapsed(&Collapsed::Section(LOCAL.to_owned())));
        let header = move || {
            let add = view! {
                <icon_button test_id="add-project" icon=IconName::FolderPlus label="Add project"
                             on_click={move || self.browse_for_project()}/>
            };
            view! {
                <section_header test_id="section-local" title="Local" expanded={!collapsed.get()}
                                icon=IconName::Laptop
                                action={add.into_any()} actions_always_visible
                                on_toggle={move || self.toggle(Collapsed::Section(LOCAL.to_owned()))}/>
            }
        };
        let sections = self.sections;
        view! {
            <div flex col gap=4.0>
                <dynamic>{header()}</dynamic>
                <show when={move || !collapsed.get()}>
                    <div flex col gap=4.0>
                        <show when={move || sections.with(Vec::is_empty)}>
                            {placeholder("local-empty", "Add a project to start a thread in it")}
                        </show>
                        <each items={move || sections.get()} key={|section: &Section| section.root.clone()}
                              let:section>
                            {self.section(section)}
                        </each>
                    </div>
                </show>
            </div>
        }
    }

    fn cloud(self) -> impl View {
        let collapsed = memo(move || self.is_collapsed(&Collapsed::Section(CLOUD.to_owned())));
        let problem = memo(move || self.fleet.file_problem());
        let remotes = self.remotes;
        let shown = memo(move || !remotes.with(Vec::is_empty) || problem.with(Option::is_some));
        view! {
            <show when={move || shown.get()}>
                <div flex col gap=4.0 pt=8.0>
                    <dynamic>{view! {
                        <section_header test_id="section-cloud" title="Cloud" expanded={!collapsed.get()}
                                        icon=IconName::Cloud
                                        on_toggle={move || self.toggle(Collapsed::Section(CLOUD.to_owned()))}/>
                    }}</dynamic>
                    <show when={move || !collapsed.get()}>
                        <div flex col gap=4.0>
                            <dynamic>{problem.get().map(|problem| placeholder("cloud-problem", problem))}</dynamic>
                            <each items={move || remotes.get()}
                                  key={|section: &RemoteSection| section.remote.name.clone()} let:section>
                                {self.remote(section)}
                            </each>
                        </div>
                    </show>
                </div>
            </show>
        }
    }

    fn section(self, section: ReadSignal<Section>) -> impl View {
        let root = section.with_untracked(|section| section.root.clone());
        let name = project_name(&root);
        let id = format!("project-{name}");
        let collapsed = memo(move || section.with(|section| section.collapsed));
        let hidden = memo(move || section.with(|section| section.hidden));
        let header = {
            let (root, name, id) = (root.clone(), name.clone(), id.clone());
            move || {
                let collapsed = collapsed.get();
                self.agents.revision();
                untrack(|| {
                    let folder = if collapsed { IconName::Folder } else { IconName::FolderOpen };
                    let actions_root = root.clone();
                    self.group_header(Header {
                        id: id.clone(),
                        label: name.clone(),
                        icon: (format!("{id}-folder"), folder),
                        group: Collapsed::Project(root.clone()),
                        collapsed,
                        actions: ("Project actions", self.project_actions(&actions_root)),
                        new_thread: self.new_thread_button(&id, &name, &root),
                    })
                })
            }
        };
        let show_more = move || {
            let hidden = hidden.get();
            (hidden > 0).then(|| {
                let root = root.clone();
                view! {
                    <tree_row test_id={format!("{id}-show-more")} label={format!("Show {hidden} more")} depth=1
                              leading={icon_slot().into_any()} label_color=Role::TextMuted
                              on_click={move || self.show_more(root.clone())}/>
                }
            })
        };
        view! {
            <div w_full flex col>
                <dynamic>{header()}</dynamic>
                <each items={move || section.with(|section| section.rows.clone())} key={|row: &Row| row.key.clone()}
                      let:row>
                    <dynamic>{row::row(&self, &row.get())}</dynamic>
                </each>
                <dynamic>{show_more()}</dynamic>
            </div>
        }
    }

    fn new_thread_button(self, id: &str, name: &str, root: &Path) -> AnyView {
        let catalog = self.agents.catalog(root);
        let button_id = format!("{id}-new-thread");
        let label = format!("New thread in {name}");
        if catalog.agents().len() == 1 && catalog.problems().is_empty() {
            let root = root.to_path_buf();
            return view! {
                <icon_button test_id=button_id icon=IconName::Plus label on_click={move || self.new_thread_in(root.clone())}/>
            }
            .into_any();
        }
        let agents = catalog.agents().iter().map(|agent| {
            let (root, agent) = (root.to_path_buf(), agent.clone());
            let id = format!("new-thread-agent-{}", agent.name);
            MenuItem::callback(agent.name.clone(), move || self.new_thread_with(root.clone(), agent.clone())).id(id)
        });
        let discovered = catalog.agents().is_empty().then(|| self.discovered_agents(root)).into_iter().flatten();
        let problems = catalog.problems().iter().enumerate().map(|(index, problem)| {
            let file = problem.path().to_path_buf();
            MenuItem::callback(problem.to_string(), move || self.open_path(&file))
                .id(format!("new-thread-problem-{index}"))
                .icon(IconName::CircleX)
        });
        let items: Vec<MenuItem> = agents.chain(discovered).chain(problems).collect();
        view! {
            <context_menu items dropdown><icon_button test_id=button_id icon=IconName::Plus label/></context_menu>
        }
        .into_any()
    }

    /// With no agents to start a thread with, the known ones found on `PATH`, and the file to define others in.
    fn discovered_agents(self, root: &Path) -> Vec<MenuItem> {
        let found = KNOWN_AGENTS
            .iter()
            .filter(|agent| self.platform.with(|platform| platform.find_program(agent.command).is_some()))
            .map(|&agent| {
                let root = root.to_path_buf();
                MenuItem::callback(format!("Add {}", agent.name), move || self.add_agent(root.clone(), agent))
                    .id(format!("new-thread-add-{}", agent.name))
                    .icon(IconName::Plus)
            });
        let edit = self.agents.user_file().map(|_| {
            MenuItem::callback(format!("Define an agent in {AGENTS_FILE}…"), move || self.open_agents_file())
                .id("new-thread-open-agents-file")
                .icon(IconName::FileText)
        });
        found.chain(edit).collect()
    }

    fn remote(self, section: ReadSignal<RemoteSection>) -> impl View {
        let remote = section.with_untracked(|section| section.remote.clone());
        let id = format!("remote-{}", remote.name);
        let starts = starts_threads(&remote);
        let header = {
            let (remote, id) = (remote.clone(), id.clone());
            move || {
                let collapsed = section.with(|section| section.collapsed);
                let new_thread = {
                    let remote = remote.clone();
                    let label = format!("New thread on {}", remote.name);
                    view! {
                        <icon_button test_id={format!("{id}-new-thread")} icon=IconName::Plus label disabled={!starts}
                                     on_click={move || {
                                         self.threads.new_cloud_thread(&remote);
                                     }}/>
                    }
                    .into_any()
                };
                self.group_header(Header {
                    id: id.clone(),
                    label: remote.name.clone(),
                    icon: (format!("{id}-icon"), IconName::Cpu),
                    group: Collapsed::Remote(remote.name.clone()),
                    collapsed,
                    actions: ("Remote actions", self.remote_actions()),
                    new_thread,
                })
            }
        };
        let body = move || {
            let (collapsed, problem, empty) =
                section.with(|section| (section.collapsed, section.problem.clone(), section.rows.is_empty()));
            if collapsed {
                return None;
            }
            if let Some(problem) = problem {
                return Some(placeholder(format!("{id}-problem"), problem));
            }
            empty.then(|| {
                let text = match (remote.is_static(), starts) {
                    (true, _) => "No threads. Start one…",
                    (false, true) => "Nothing running. Launch one…",
                    (false, false) => "Nothing running",
                };
                let remote = remote.clone();
                let empty = placeholder(format!("{id}-empty"), text);
                if starts {
                    empty.on_click(move || {
                        self.threads.new_cloud_thread(&remote);
                    })
                } else {
                    empty
                }
            })
        };
        view! {
            <div w_full flex col>
                <dynamic>{header()}</dynamic>
                <dynamic>{body()}</dynamic>
                <each items={move || section.with(|section| section.rows.clone())} key={|row: &Row| row.key.clone()}
                      let:row>
                    <dynamic>{row::row(&self, &row.get())}</dynamic>
                </each>
            </div>
        }
    }

    fn group_header(self, header: Header) -> Div {
        let Header { id, label, icon: (icon_id, icon), group, collapsed, actions, new_thread } = header;
        let muted = Role::TextMuted;
        let hovered = signal(false);
        let (actions_label, items) = actions;
        let actions_id = format!("{id}-actions");
        let toggle = {
            let group = group.clone();
            Callback::from(move || self.toggle(group.clone()))
        };
        let leading = view! { <div test_id=icon_id flex>{Icon::new(icon, muted)}</div> }.into_any();
        let row = view! {
            <tree_row test_id=id label expanded={!collapsed} leading=leading label_color=muted on_click=toggle on_toggle=toggle>
                <div flex items_center opacity={move || if hovered.get() { 1.0 } else { 0.0 }}>
                    <context_menu items dropdown><icon_button test_id=actions_id icon=IconName::Ellipsis label=actions_label/></context_menu>
                    {new_thread}
                </div>
            </tree_row>
        };
        div()
            .on_hover(move |inside| {
                hovered.set_if_changed(inside);
            })
            .child(row)
    }

    fn project_actions(self, root: &Path) -> Vec<MenuItem> {
        let (archive_root, remove_root) = (root.to_path_buf(), root.to_path_buf());
        vec![
            MenuItem::callback("Archive inactive threads", move || self.archive_inactive(&archive_root))
                .id("menu-archive-inactive")
                .icon(IconName::Archive),
            MenuItem::callback("Remove from sidebar", move || self.remove_project(&remove_root))
                .id("menu-remove-project")
                .icon(IconName::X),
        ]
    }

    fn remote_actions(self) -> Vec<MenuItem> {
        let fleet = self.fleet;
        let refresh = MenuItem::callback("Refresh", move || fleet.refresh())
            .id("menu-refresh-instances")
            .icon(IconName::RotateCw);
        let open = fleet.file().map(|file| {
            MenuItem::callback("Open remotes.json", move || self.open_path(&file))
                .id("menu-open-remotes-file")
                .icon(IconName::FileText)
        });
        std::iter::once(refresh).chain(open).collect()
    }

    fn open_path(self, path: &Path) {
        let target = path.display().to_string();
        self.platform.with(|platform| platform.open(&target));
    }
}

impl Sources {
    fn with_listing<T>(self, read: impl FnOnce(&Listing<'_>) -> T) -> T {
        let now = self.clock.with(|clock| clock.now());
        self.registry.with(|state| {
            self.history.with_sessions(|sessions| {
                self.rename.with(|rename| {
                    read(&Listing {
                        state,
                        threads: self.threads,
                        history: sessions,
                        fleet: self.fleet,
                        renaming: rename.as_ref().map(|rename| &rename.session_id),
                        now,
                    })
                })
            })
        })
    }
}

struct Header {
    id: String,
    label: String,
    icon: (String, IconName),
    group: Collapsed,
    collapsed: bool,
    actions: (&'static str, Vec<MenuItem>),
    new_thread: AnyView,
}

const DEFAULT_SIDEBAR_WIDTH: f32 = 272.0;
const MIN_SIDEBAR_WIDTH: f32 = 200.0;
const MAX_SIDEBAR_WIDTH: f32 = 480.0;

const MORE_SAVED_ROWS: usize = 20;

const LOCAL: &str = "local";
const CLOUD: &str = "cloud";

fn starts_threads(remote: &RemoteSpec) -> bool {
    remote.is_static() || remote.can(Action::Launch)
}

fn placeholder(id: impl Into<String>, label: impl Into<String>) -> ListRow {
    let label = label.into();
    view! {
        <list_row test_id=id depth=1>
            {icon_slot()}
            <div flex_1 min_w=0.0 py=4.0><text caption muted>{label}</text></div>
        </list_row>
    }
}
