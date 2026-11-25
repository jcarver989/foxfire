mod mobile;
mod preferences;

use crate::ChatPane;
use crate::backend::{BackendError, HostedBackend, HostedState, HostedThread, Project, ThreadId};
use crate::memory::MemoryBackend;
use crate::platform::ChatPlatform;
use crate::thread::{Thread, ThreadStatus, UNTITLED};
use crate::ui::tokens::{BASE_SIZE, Space};
use crate::ui::{
    ActiveReviewTheme, FoxfireMotion, IconName, InterfaceSize, PanelWidth, SettingsField, SettingsModel,
    ThemePreference, ThreadShell, UI_FONT, UiStyles, appearance, breadcrumb, cloud_section, empty_state, icon_button,
    row_action, settings, settings_button, sidebar_group, sidebar_thread_row, thread_header, thread_status_icon,
};
use indexmap::IndexMap;
use rouge::core::{AnyView, Div, Role, Styled, View, div, dynamic, each, motion, presentation, show, text, view};
use rouge::persist::Storage;
use rouge::platform::open_url;
use rouge::reactive::{ReadSignal, Scope, Signal, Stored, effect, memo, signal, sleep, spawn, stored, untrack};
use std::collections::HashSet;
use std::hash::Hash;
use std::rc::Rc;
use std::time::Duration;

/// Builds the shared hosted-project shell for a supplied backend. Call inside a Rouge window.
pub fn hosted_view(
    backend: Rc<dyn HostedBackend>,
    platform: Rc<dyn ChatPlatform>,
    storage: Rc<dyn Storage>,
) -> impl View {
    let preference = signal(ThemePreference::default());
    let system = appearance();
    let theme = ActiveReviewTheme::install(preference.with_untracked(|value| value.theme(system.get_untracked())));
    effect(move || preference.with(|value| theme.set(value.theme(system.get()))));
    let _ = FoxfireMotion::install(false);
    InterfaceSize::install(BASE_SIZE);

    let ready = signal(false);
    let storage = stored(storage);
    spawn(async move {
        let saved = preferences::load(storage.get().as_ref()).await;
        theme.set(saved.theme(system.get_untracked()));
        preference.set(saved);
        ready.set(true);
    });
    let model = SettingsModel {
        appearance: SettingsField::new(memo(move || preference.get()), move |next| {
            preferences::save(storage.get().as_ref(), &next);
            preference.set(next);
        }),
        reduced_motion: None,
        interface_size: None,
        display_name: None,
    };
    show(move || ready.get(), move || HostedApp::new(backend.clone(), platform.clone(), model).view())
}

/// Builds a self-contained demo with scripted replies and no connection to an agent.
pub fn demo_view(storage: Rc<dyn Storage>) -> impl View {
    let backend = MemoryBackend::new(vec![Project { id: "demo".to_owned(), name: "Demo project".to_owned() }]);
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
struct HostedApp {
    backend: Stored<Rc<dyn HostedBackend>>,
    platform: Stored<Rc<dyn ChatPlatform>>,
    projects: Signal<Vec<Project>>,
    threads: Signal<IndexMap<ThreadId, Entry>>,
    selected: Signal<Option<ThreadId>>,
    error: Signal<Option<String>>,
    settings_open: Signal<bool>,
    settings: SettingsModel,
    shell: ThreadShell,
    inventory_loaded: Signal<bool>,
    cloud_expanded: Signal<bool>,
    closed_projects: Signal<HashSet<String>>,
    launching: Signal<HashSet<String>>,
    stopping: Signal<HashSet<ThreadId>>,
    epoch: Stored<u64>,
    scope: Scope,
}

struct Entry {
    info: HostedThread,
    pane: Option<(Scope, Thread, ChatPane)>,
}

impl Entry {
    fn dispose(self) {
        if let Some((scope, _, _)) = self.pane {
            scope.dispose();
        }
    }
}

impl HostedApp {
    fn new(backend: Rc<dyn HostedBackend>, platform: Rc<dyn ChatPlatform>, settings: SettingsModel) -> Self {
        let collapsed = signal(false);
        let width = signal(None);
        let app = Self {
            backend: stored(backend),
            platform: stored(platform),
            projects: signal(Vec::new()),
            threads: signal(IndexMap::new()),
            selected: signal(None),
            error: signal(None),
            settings_open: signal(false),
            settings,
            shell: ThreadShell::new(
                memo(move || collapsed.get()),
                move |value| collapsed.set(value),
                PanelWidth::new(memo(move || width.get()), move |value| width.set(value)),
            ),
            inventory_loaded: signal(false),
            cloud_expanded: signal(true),
            closed_projects: signal(HashSet::new()),
            launching: signal(HashSet::new()),
            stopping: signal(HashSet::new()),
            epoch: stored(0),
            scope: Scope::new(),
        };
        let starting =
            memo(move || app.threads.with(|all| all.values().any(|entry| entry.info.state == HostedState::Starting)));
        effect(move || {
            if starting.get() {
                spawn(async move {
                    loop {
                        sleep(PROVISIONING_POLL).await;
                        app.refresh().await;
                    }
                });
            }
        });
        app.scope.run(|| {
            spawn(async move {
                let backend = app.backend.get();
                match backend.projects().await {
                    Ok(projects) => app.projects.set(projects),
                    Err(error) => app.error.set(Some(error.to_string())),
                }
                app.refresh().await;
            })
        });
        app
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
                .font(UI_FONT)
                .body()
                .child(mobile::view(self))
                .children(overlays)
        } else {
            self.pointer_view(overlays)
        };
        InterfaceSize::current().bind(root)
    }

    fn pointer_view(self, overlays: Vec<AnyView>) -> Div {
        let selected = memo(move || {
            let id = self.selected.get()?;
            self.threads.with(|threads| threads.get(&id).map(|entry| entry.info.clone()))
        });
        let main = dynamic(move || match selected.get() {
            Some(info) if info.state == HostedState::Ready => untrack(|| self.conversation(&info)).into_any(),
            Some(info) => self.shell.placeholder(waiting(info)).into_any(),
            None => self.shell.no_thread().into_any(),
        });
        self.shell.view("hosted", move || self.controls(), move || self.sections(), main, overlays)
    }

    fn overlays(self) -> Vec<AnyView> {
        let error = dynamic(move || {
            self.error.get().map(|error| {
                view! {
                    <div absolute bottom=Space::X4 left=Space::X4 right=Space::X4 p=Space::X2 bg=Role::Chrome
                         flex items_center gap=Space::X2>
                        <div flex_1><text>{error}</text></div>
                        <icon_button icon=IconName::X label="Dismiss" on_click={move || self.error.set(None)}/>
                    </div>
                }
            })
        });
        vec![
            error.into_any(),
            show(
                move || self.settings_open.get(),
                move || settings(self.settings, move || self.settings_open.set(false)),
            )
            .into_any(),
        ]
    }

    fn controls(self) -> impl View {
        view! {
            <icon_button test_id="refresh-threads" icon=IconName::RotateCw label="Refresh threads"
                         on_click={move || { self.scope.run(|| spawn(self.refresh())); }}/>
            {settings_button(move || self.settings_open.set(true))}
        }
    }

    fn sections(self) -> impl View {
        cloud_section(
            memo(move || self.cloud_expanded.get()),
            move || self.cloud_expanded.toggle(),
            move || {
                view! {
                    <each items={move || self.projects.get()} key={|project: &Project| project.id.clone()} let:project>
                        {self.project(project)}
                    </each>
                }
            },
        )
    }

    fn project(self, project: ReadSignal<Project>) -> impl View {
        let id = move || project.with(|p| p.id.clone());
        let launching = move || self.project_launching(&id());
        let expanded = memo(move || !self.closed_projects.with(|closed| closed.contains(&id())));
        let header = dynamic(move || {
            let expanded = expanded.get();
            let name = project.with(|project| project.name.clone());
            let icon = if expanded { IconName::FolderOpen } else { IconName::Folder };
            let controls = view! {
                <icon_button test_id={format!("launch-{}", id())} icon=IconName::Plus label={format!("New thread in {name}")}
                             disabled=launching on_click={move || self.launch(id(), move |id| self.select(id))}/>
            };
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
                    <each items={move || self.project_threads(&id())}
                          key=Clone::clone let:thread>
                        {self.row(thread)}
                    </each>
                </show>
            </div>
        }
    }

    fn row(self, row: ReadSignal<ThreadId>) -> impl View {
        let id = move || row.get_untracked();
        let selected = move || self.selected.with(|selected| selected.as_ref() == Some(&id()));
        let title = move || self.thread_title(&id());
        let row_id = format!("thread-{}", id().0);
        let status = memo(move || self.thread_status(&id()));
        let icon = dynamic({
            let row_id = row_id.clone();
            move || thread_status_icon(&row_id, status.get())
        });
        sidebar_thread_row(
            row_id.clone(),
            selected,
            view! {
                <div test_id={format!("{row_id}-title")} min_w=0.0 overflow_hidden><text body no_wrap>{title}</text></div>
            },
            icon,
            None,
            move || self.select(id()),
        )
        .action(
            row_action("Stop thread", move || self.stop(id(), |_| {}))
                .icon(IconName::Square)
                .icon_only()
                .test_id(format!("stop-{}", id().0)),
        )
    }

    fn project_launching(self, id: &str) -> bool {
        self.launching.with(|all| all.contains(id))
            || self.threads.with(|all| {
                all.values().any(|entry| entry.info.project.id == id && entry.info.state == HostedState::Starting)
            })
    }

    fn project_threads(self, id: &str) -> Vec<ThreadId> {
        self.threads.with(|threads| {
            threads.values().filter(|entry| entry.info.project.id == id).map(|entry| entry.info.id.clone()).collect()
        })
    }

    fn thread_title(self, id: &ThreadId) -> String {
        self.threads
            .with(|all| all.get(id).and_then(|entry| entry.pane))
            .map_or_else(|| UNTITLED.to_owned(), |(_, thread, _)| thread.title())
    }

    fn thread_status(self, id: &ThreadId) -> ThreadStatus {
        self.threads.with(|all| {
            all.get(id).map_or(ThreadStatus::Dormant, |entry| match &entry.info.state {
                HostedState::Starting => ThreadStatus::Connecting,
                HostedState::Failed(_) => ThreadStatus::Errored,
                HostedState::Ready => entry.pane.map_or(ThreadStatus::Idle, |(_, thread, _)| thread.status()),
            })
        })
    }

    fn conversation_pane(self, id: ThreadId) -> (Thread, ChatPane) {
        if let Some((_, thread, pane)) = self.threads.with_untracked(|all| all.get(&id).and_then(|entry| entry.pane)) {
            return (thread, pane);
        }
        let (_, thread, pane) = self
            .threads
            .try_update(|all| {
                *all.get_mut(&id).expect("listed thread").pane.get_or_insert_with(|| {
                    let scope = self.scope.child();
                    scope.run(|| {
                        let backend = self.backend.get();
                        let thread = Thread::connect(0, None, None, move |resume| backend.open(id.clone(), resume));
                        (scope, thread, ChatPane::new(thread, self.platform.get(), |_| {}, || {}))
                    })
                })
            })
            .expect("live inventory");
        (thread, pane)
    }

    fn conversation(self, info: &HostedThread) -> impl View + use<> {
        let (thread, pane) = self.conversation_pane(info.id.clone());
        let gutter = pane.gutter();
        let header = thread_header(
            move || gutter.get(),
            self.shell.reopen(),
            dynamic(move || breadcrumb().test_id("thread-path").min_w(0.0).current(thread.title())),
            move || thread.with_core(|core| core.session().agent_name().to_owned()).unwrap_or_default(),
            (),
        );
        view! {
            <div relative size_full>
                {conversation_surface(thread, &pane, false)}
                {header}
            </div>
        }
    }

    async fn refresh(self) {
        let epoch = self.epoch.update(|epoch| {
            *epoch += 1;
            *epoch
        });
        let result = self.backend.get().list_threads().await;
        if epoch != self.epoch.get() {
            return;
        }
        match result {
            Ok(threads) => self.threads.update(|old| {
                let next = threads
                    .into_iter()
                    .map(|info| {
                        let id = info.id.clone();
                        let pane = old.swap_remove(&id).and_then(|entry| entry.pane);
                        (id, Entry { info, pane })
                    })
                    .collect::<IndexMap<_, _>>();
                if self.selected.with_untracked(|id| id.as_ref().is_some_and(|id| !next.contains_key(id))) {
                    self.selected.set(None);
                }
                for (_, stale) in std::mem::replace(old, next) {
                    stale.dispose();
                }
            }),
            Err(error) => self.error.set(Some(error.to_string())),
        }
        self.inventory_loaded.set_if_changed(true);
    }

    fn launch(self, project: String, on_launch: impl FnOnce(ThreadId) + 'static) {
        self.run(self.launching, project, async move |project| {
            let info = self.backend.get().launch(project).await?;
            self.epoch.update(|epoch| *epoch += 1);
            let id = info.id.clone();
            self.threads.update(|all| match all.get_mut(&id) {
                Some(entry) => entry.info = info,
                None => {
                    all.insert(id.clone(), Entry { info, pane: None });
                }
            });
            self.inventory_loaded.set_if_changed(true);
            on_launch(id);
            Ok(())
        });
    }

    fn select(self, id: ThreadId) {
        self.selected.set_if_changed(Some(id));
    }

    fn stop(self, id: ThreadId, on_stop: impl FnOnce(&ThreadId) + 'static) {
        self.run(self.stopping, id, async move |id| {
            self.backend.get().stop(id.clone()).await?;
            on_stop(&id);
            self.epoch.update(|epoch| *epoch += 1);
            if self.selected.with_untracked(|current| current.as_ref() == Some(&id)) {
                self.selected.set(None);
            }
            if let Some(entry) = self.threads.try_update(|all| all.shift_remove(&id)).flatten() {
                entry.dispose();
            }
            Ok(())
        });
    }

    fn run<T: Clone + Eq + Hash + 'static>(
        self,
        pending: Signal<HashSet<T>>,
        key: T,
        work: impl AsyncFnOnce(T) -> Result<(), BackendError> + 'static,
    ) {
        if pending.try_update(|all| all.insert(key.clone())) != Some(true) {
            return;
        }
        self.scope.run(|| {
            spawn(async move {
                if let Err(error) = work(key.clone()).await {
                    self.error.set(Some(error.to_string()));
                }
                pending.update(|all| {
                    all.remove(&key);
                });
            })
        });
    }
}

fn conversation_surface(thread: Thread, pane: &ChatPane, in_flow_header: bool) -> Div {
    let pane = *pane;
    let reveal = motion(1.0);
    view! {
        <div test_id="conversation-surface" relative size_full min_h=0.0 track_bounds={pane.stage()}>
            {if in_flow_header { pane.transcript_below_header().into_any() } else { pane.transcript().into_any() }}
            <show when={move || thread.is_fresh()}>{pane.kindling(reveal)}</show>
            {pane.dock(reveal, None::<AnyView>, None::<AnyView>)}
        </div>
    }
}

fn waiting(info: HostedThread) -> impl View {
    let title = match info.state {
        HostedState::Ready | HostedState::Starting => "Starting thread".to_owned(),
        HostedState::Failed(error) => error,
    };
    view! {
        <empty_state icon=IconName::Cloud title=title detail={info.project.name}/>
    }
}

const PROVISIONING_POLL: Duration = Duration::from_secs(1);
