mod mobile;

use crate::ui::{
    AppearanceField, SettingsField, SettingsModel, ThreadShell, cloud_section, settings, settings_button,
    sidebar_group, sidebar_thread_row, thread_status_icon,
};
use crate::workspace::{Header, ThreadTab, Workspace};
use crate::{UNTITLED, install};
use agent_client_protocol::v2::schema::SessionId;
use foxfire_core::session_core::ChatChannel;
use foxfire_ui::backend::{HostedBackend, HostedState, HostedThread, Project, SessionLink, ThreadId};
use foxfire_ui::memory::MemoryBackend;
use foxfire_ui::platform::ChatPlatform;
use foxfire_ui::thread::{Thread, ThreadStatus};
use foxfire_ui::thread_list::{ThreadList, Workspaces};
use foxfire_ui::ui::tokens::{BASE_SIZE, Space};
use foxfire_ui::ui::{
    ActiveReviewTheme, ActivityMotion, ButtonVariant, FontRole, IconName, InterfaceSize, PanelWidth, ThemeSet, Toasts,
    UiStyles, appearance, breadcrumb, button, empty_state, icon_button, people_avatars, row_action, thread_header,
};
use rouge::core::{AnyView, Div, Role, Styled, View, div, dynamic, each, presentation, show, text, view};
use rouge::persist::{Storage, persisted, provide_storage};
use rouge::platform::open_url;
use rouge::reactive::{ReadSignal, Signal, Stored, effect, memo, signal, spawn, stored, untrack};
use std::collections::HashSet;
use std::rc::Rc;

/// Builds the shared hosted-project shell for a supplied backend. Call inside a Rouge window.
pub fn hosted_view(
    backend: Rc<dyn HostedBackend>,
    platform: Rc<dyn ChatPlatform>,
    storage: Rc<dyn Storage>,
) -> impl View {
    shell_view(storage, move || {
        let platform = platform.clone();
        Threads::Hosted(ThreadList::new(backend.clone(), move |thread| Workspace::new(thread, platform.clone())))
    })
}

/// Shows one session in the usual picker, without provisioning or managing an inventory.
pub fn single_session_view(
    project: Project,
    connect: impl Fn(Option<SessionId>) -> SessionLink + 'static,
    platform: Rc<dyn ChatPlatform>,
    storage: Rc<dyn Storage>,
) -> impl View {
    let single = Threads::Single {
        project: stored(project),
        workspaces: Workspaces::new(move |_, resume| connect(resume)),
        platform: stored(platform),
    };
    shell_view(storage, move || single)
}

fn shell_view(storage: Rc<dyn Storage>, source: impl Fn() -> Threads + 'static) -> impl View {
    install();
    let toasts = Toasts::install();
    let themes = ThemeSet::default();
    let preference = signal(themes.preference());
    let initial =
        preference.with_untracked(|value| themes.theme(value, value.appearance(appearance().get_untracked())));
    let theme = ActiveReviewTheme::install(initial);
    effect({
        let themes = themes.clone();
        move || preference.with(|value| themes.apply(value, theme))
    });
    let _ = ActivityMotion::install(false);
    InterfaceSize::install(BASE_SIZE);
    provide_storage(storage.clone());

    let ready = signal(false);
    let storage = stored(storage);
    spawn({
        let themes = themes.clone();
        async move {
            preference.set(themes.load(storage.get().as_ref(), APPEARANCE_KEY).await);
            ready.set(true);
        }
    });
    let choice = SettingsField::new(memo(move || preference.get()), move |next| {
        ThemeSet::save(storage.get().as_ref(), APPEARANCE_KEY, &next);
        preference.set(next);
    });
    let model = SettingsModel {
        appearance: AppearanceField::new(themes, choice),
        reduced_motion: None,
        interface_size: None,
        display_name: None,
    };
    show(move || ready.get(), move || HostedApp::new(source(), model, toasts).view())
}

/// Builds a self-contained demo with scripted replies and no connection to an agent.
pub fn demo_view(storage: Rc<dyn Storage>) -> impl View {
    let backend = MemoryBackend::new(vec![Project { id: "demo".to_owned(), name: "Demo project".to_owned() }])
        .with_chat()
        .with_changes(&DEMO_CHANGES);
    hosted_view(Rc::new(backend), Rc::new(DemoPlatform), storage)
}

struct DemoPlatform;

impl ChatPlatform for DemoPlatform {
    fn open(&self, target: &str) {
        if target.starts_with("https://") || target.starts_with("http://") {
            let _ = open_url(target);
        }
    }
}

#[derive(Clone, Copy)]
enum Threads {
    Hosted(ThreadList<Workspace>),
    Single { project: Stored<Project>, workspaces: Workspaces<Workspace>, platform: Stored<Rc<dyn ChatPlatform>> },
}

impl Threads {
    fn catalog(self) -> Option<ThreadList<Workspace>> {
        match self {
            Self::Hosted(list) => Some(list),
            Self::Single { .. } => None,
        }
    }

    fn projects(self) -> Vec<Project> {
        match self {
            Self::Hosted(list) => list.projects(),
            Self::Single { project, .. } => vec![project.get()],
        }
    }

    fn project_threads(self, project_id: &str) -> Vec<ThreadId> {
        match self {
            Self::Hosted(list) => list.project_threads(project_id),
            Self::Single { project, .. } => {
                project.with(|p| (p.id == project_id).then(|| ThreadId(p.id.clone()))).into_iter().collect()
            }
        }
    }

    fn thread(self, id: &ThreadId) -> Option<Thread> {
        match self {
            Self::Hosted(list) => list.thread(id),
            Self::Single { workspaces, .. } => workspaces.thread(id),
        }
    }

    fn status(self, id: &ThreadId) -> ThreadStatus {
        match self {
            Self::Hosted(list) => list.status(id),
            Self::Single { .. } => self.thread(id).map_or(ThreadStatus::Idle, |t| t.status()),
        }
    }

    fn selected(self) -> Option<ThreadId> {
        match self {
            Self::Hosted(list) => list.selected(),
            Self::Single { workspaces, project, .. } => {
                let id = ThreadId(project.get().id);
                workspaces.thread(&id).map(|_| id)
            }
        }
    }

    fn select(self, id: &ThreadId) {
        match self {
            Self::Hosted(list) => list.select(id),
            Self::Single { .. } => {
                self.open(id);
            }
        }
    }

    fn open(self, id: &ThreadId) -> Option<(Thread, Workspace)> {
        match self {
            Self::Hosted(list) => list.open(id),
            Self::Single { project, workspaces, platform } if project.with(|p| p.id == id.0) => {
                Some(workspaces.open(id, |thread| Workspace::new(thread, platform.get())))
            }
            Self::Single { .. } => None,
        }
    }
}

#[derive(Clone, Copy)]
struct HostedApp {
    list: Threads,
    settings_open: Signal<bool>,
    settings: SettingsModel,
    toasts: Toasts,
    shell: ThreadShell,
    cloud_expanded: Signal<bool>,
    closed_projects: Signal<HashSet<String>>,
}

#[derive(Clone, PartialEq)]
enum Shown {
    Conversation(ThreadId),
    Waiting(HostedThread),
}

impl HostedApp {
    fn new(list: Threads, settings: SettingsModel, toasts: Toasts) -> Self {
        let collapsed = signal(false);
        let width = signal(None);
        let room_width = persisted(ROOM_WIDTH_KEY, signal(None));
        Self {
            list,
            settings_open: signal(false),
            settings,
            toasts,
            shell: ThreadShell::new(
                memo(move || collapsed.get()),
                move |value| collapsed.set(value),
                PanelWidth::new(memo(move || width.get()), move |value| width.set(value)),
                PanelWidth::new(memo(move || room_width.get()), move |value| room_width.set(value)),
            ),
            cloud_expanded: signal(true),
            closed_projects: signal(HashSet::new()),
        }
    }

    fn title(self, id: &ThreadId) -> String {
        match self.list {
            Threads::Hosted(list) => list.title(id),
            Threads::Single { .. } => self.list.thread(id).and_then(|thread| thread.title()),
        }
        .unwrap_or_else(|| UNTITLED.to_owned())
    }

    fn shown(self, id: &ThreadId) -> Option<Shown> {
        let list = match self.list {
            Threads::Single { project, .. } => {
                return project.with(|p| (p.id == id.0).then(|| Shown::Conversation(id.clone())));
            }
            Threads::Hosted(list) => list,
        };
        let info = list.info(id)?;
        Some(if list.can_open(id) { Shown::Conversation(info.id) } else { Shown::Waiting(info) })
    }

    fn view(self) -> impl View {
        let overlays = self.overlays();
        let root = if presentation().get_untracked().is_touch() {
            div()
                .test_id("hosted")
                .relative()
                .size_full()
                .flex()
                .col()
                .bg(Role::Canvas)
                .text_color(Role::Text)
                .font(FontRole::Ui)
                .body()
                .child(mobile::view(self))
                .children(overlays)
        } else {
            self.pointer_view(overlays)
        };
        InterfaceSize::current().bind(root)
    }

    fn pointer_view(self, overlays: Vec<AnyView>) -> Div {
        let shown = memo(move || self.shown(&self.list.selected()?));
        let main = dynamic(move || match shown.get() {
            Some(Shown::Conversation(id)) => untrack(|| self.conversation(&id)).into_any(),
            Some(Shown::Waiting(info)) => self.shell.placeholder(waiting(self.list, info)).into_any(),
            None => self.shell.no_thread().into_any(),
        });
        self.shell.view("hosted", move || self.controls(), move || self.sections(), main, overlays)
    }

    fn overlays(self) -> Vec<AnyView> {
        let error = dynamic(move || {
            let list = self.list.catalog()?;
            list.error().map(|error| {
                view! {
                    <div absolute bottom=Space::X4 left=Space::X4 right=Space::X4 p=Space::X2 bg=Role::Chrome
                         flex items_center gap=Space::X2>
                        <div flex_1><text>{error.to_string()}</text></div>
                        <icon_button icon=IconName::X label="Dismiss" on_click={move || list.dismiss_error()}/>
                    </div>
                }
            })
        });
        vec![
            error.into_any(),
            self.toasts.into_any(),
            show(
                move || self.settings_open.get(),
                move || settings(self.settings, move || self.settings_open.set(false)),
            )
            .into_any(),
        ]
    }

    fn controls(self) -> impl View {
        let refresh = self.list.catalog().map(|list| {
            icon_button(IconName::RotateCw, "Refresh threads".to_owned())
                .test_id("refresh-threads")
                .on_click(move || list.refresh())
        });
        view! { {refresh}{settings_button(move || self.settings_open.set(true))} }
    }

    fn sections(self) -> impl View {
        cloud_section(
            memo(move || self.cloud_expanded.get()),
            move || self.cloud_expanded.toggle(),
            move || {
                view! {
                    <each items={move || self.list.projects()} key={|project: &Project| project.id.clone()} let:project>
                        {self.project(project)}
                    </each>
                }
            },
        )
    }

    fn project(self, project: ReadSignal<Project>) -> impl View {
        let id = move || project.with(|p| p.id.clone());
        let expanded = memo(move || !self.closed_projects.with(|closed| closed.contains(&id())));
        let header = dynamic(move || {
            let expanded = expanded.get();
            let name = project.with(|project| project.name.clone());
            let icon = if expanded { IconName::FolderOpen } else { IconName::Folder };
            let controls = self.list.catalog().map(|list| view! {
                <icon_button test_id={format!("launch-{}", id())} icon=IconName::Plus label={format!("New thread in {name}")}
                             disabled={move || list.is_launching(&id())} on_click={move || list.launch(id(), move |id| list.select(id))}/>
            });
            sidebar_group(
                format!("project-{}", id()),
                name,
                (format!("project-{}-folder", id()), icon),
                expanded,
                move || {
                    self.closed_projects.update(|closed| {
                        let id = id();
                        if !closed.remove(&id) {
                            closed.insert(id);
                        }
                    });
                },
                controls,
            )
        });
        view! {
            <div w_full flex col>
                {header}
                <show when={move || expanded.get()}>
                    <each items={move || self.list.project_threads(&id())}
                          key=Clone::clone let:thread>
                        {self.row(thread)}
                    </each>
                </show>
            </div>
        }
    }

    fn row(self, row: ReadSignal<ThreadId>) -> impl View {
        let id = move || row.get_untracked();
        let selected = move || self.list.selected() == Some(id());
        let title = move || self.title(&id());
        let row_id = format!("thread-{}", id().0);
        let status = memo(move || self.list.status(&id()));
        let icon = dynamic({
            let row_id = row_id.clone();
            move || thread_status_icon(&row_id, status.get())
        });
        let others = memo(move || {
            self.list.thread(&id()).and_then(|thread| thread.with_chat(ChatChannel::others)).unwrap_or_default()
        });
        let presence = dynamic(move || others.with(|others| (!others.is_empty()).then(|| people_avatars(others))));
        let row = sidebar_thread_row(
            row_id.clone(),
            selected,
            view! {
                <div test_id={format!("{row_id}-title")} min_w=0.0 overflow_hidden><text body no_wrap>{title}</text></div>
            },
            icon,
            None,
            move || self.list.select(&id()),
        )
        .trailing(presence);
        if let Some(list) = self.list.catalog() {
            row.action(
                row_action("Stop thread", move || list.stop(id(), |_| {}))
                    .icon(IconName::Square)
                    .icon_only()
                    .test_id(format!("stop-{}", id().0)),
            )
        } else {
            row
        }
    }

    fn conversation(self, id: &ThreadId) -> Option<Div> {
        let (thread, workspace) = self.list.open(id)?;
        let room = workspace.chat().room();
        let gutter = workspace.chat().gutter();
        let header = thread_header(
            move || if workspace.tab() == ThreadTab::Chat { gutter.get() } else { 0.0 },
            self.shell.reopen(),
            dynamic({
                let id = id.clone();
                move || breadcrumb().test_id("thread-path").min_w(0.0).current(self.title(&id))
            }),
            move || thread.with_core(|core| core.session().agent_name().to_owned()).unwrap_or_default(),
            view! { <div flex_none flex items_center gap=Space::X2>{room.header_controls()}{workspace.switch()}</div> },
        );
        let conversation = view! {
            <div relative size_full>
                {workspace.view(Header::Floating, move |reveal| workspace.dock(reveal))}
                {header}
            </div>
        };
        Some(self.shell.with_room(room, conversation))
    }
}

fn waiting(source: Threads, info: HostedThread) -> impl View {
    let suspended = info.state == HostedState::Suspended;
    let title = match info.state {
        HostedState::Ready | HostedState::Starting => "Starting thread".to_owned(),
        HostedState::Suspended => "Thread suspended".to_owned(),
        HostedState::Ended => "Thread ended".to_owned(),
        HostedState::Failed(error) => error,
    };
    let resume = source.catalog().filter(|_| suspended).map(|list| {
        view! {
            <button test_id="resume-thread" label="Resume" variant=ButtonVariant::Secondary
                    on_click={move || list.resume(&info.id)}/>
        }
    });
    view! {
        <div flex col items_center gap=Space::X3>
            <empty_state icon=IconName::Cloud title=title detail={info.project.name}/>
            {resume}
        </div>
    }
}

const APPEARANCE_KEY: &str = "hosted-appearance";
const ROOM_WIDTH_KEY: &str = "hosted-chat-room-width";
/// The path, old text and new text of each file the demo's agent has changed.
const DEMO_CHANGES: [(&str, &str, &str); 2] = [
    ("src/retry.rs", "pub const MAX_DELAY: u64 = 300;\n", "/// Seconds.\npub const MAX_DELAY: u64 = 30;\n"),
    ("README.md", "# Demo\n", "# Demo\n\nRetries back off for up to 30 seconds.\n"),
];
