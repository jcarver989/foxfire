use foxfire_ui::ChatPane;
use foxfire_ui::backend::{BackendError, HostedBackend, HostedState, HostedThread, Project, ThreadId};
use foxfire_ui::platform::ChatPlatform;
use foxfire_ui::thread::{Thread, ThreadStatus, UNTITLED};
use foxfire_ui::ui::tokens::{BASE_SIZE, Space};
use foxfire_ui::ui::{
    ActiveReviewTheme, FoxfireMotion, IconName, InterfaceSize, ThemePreference, ThreadShell, UiStyles, appearance,
    breadcrumb, cloud_section, empty_state, icon_button, row_action, sidebar_group, sidebar_thread_row, thread_header,
    thread_status_icon,
};
use indexmap::IndexMap;
use rouge::core::{AnyView, Role, Styled, View, div, dynamic, each, motion, show, text, view};
use rouge::reactive::{ReadSignal, Scope, Signal, Stored, effect, memo, signal, sleep, spawn, stored, untrack};
use std::collections::HashSet;
use std::hash::Hash;
use std::rc::Rc;
use std::time::Duration;

#[derive(Clone, Copy)]
pub(crate) struct WebApp {
    backend: Stored<Rc<dyn HostedBackend>>,
    platform: Stored<Rc<dyn ChatPlatform>>,
    projects: Signal<Vec<Project>>,
    threads: Signal<IndexMap<ThreadId, Entry>>,
    selected: Signal<Option<ThreadId>>,
    error: Signal<Option<String>>,
    shell: ThreadShell,
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

impl WebApp {
    pub(crate) fn new(backend: Rc<dyn HostedBackend>, platform: Rc<dyn ChatPlatform>) -> Self {
        let preference = ThemePreference::default();
        let theme = ActiveReviewTheme::install(preference.theme(appearance().get_untracked()));
        effect(move || theme.apply(&preference));
        let _ = FoxfireMotion::install(false);
        InterfaceSize::install(BASE_SIZE);
        let collapsed = signal(false);
        let width = signal(None);
        let app = Self {
            backend: stored(backend),
            platform: stored(platform),
            projects: signal(Vec::new()),
            threads: signal(IndexMap::new()),
            selected: signal(None),
            error: signal(None),
            shell: ThreadShell::new(
                memo(move || collapsed.get()),
                memo(move || width.get()),
                move |value| collapsed.set(value),
                move |value| width.set(value),
            ),
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

    pub(crate) fn view(self) -> impl View {
        let selected = memo(move || {
            let id = self.selected.get()?;
            self.threads.with(|threads| threads.get(&id).map(|entry| entry.info.clone()))
        });
        let main = dynamic(move || match selected.get() {
            Some(info) if info.state == HostedState::Ready => untrack(|| self.conversation(&info)).into_any(),
            Some(info) => self.shell.placeholder(waiting(info)).into_any(),
            None => self.shell.no_thread().into_any(),
        });
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
        let root = self.shell.view("web", move || self.controls(), move || self.sections(), main, error);
        InterfaceSize::current().bind(root)
    }

    fn controls(self) -> impl View {
        view! {
            <icon_button test_id="refresh-threads" icon=IconName::RotateCw label="Refresh threads"
                         on_click={move || { self.scope.run(|| spawn(self.refresh())); }}/>
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
        let launching = move || {
            self.launching.with(|all| all.contains(&id()))
                || self.threads.with(|all| {
                    all.values().any(|entry| entry.info.project.id == id() && entry.info.state == HostedState::Starting)
                })
        };
        let expanded = memo(move || !self.closed_projects.with(|closed| closed.contains(&id())));
        let header = dynamic(move || {
            let expanded = expanded.get();
            let name = project.with(|project| project.name.clone());
            let icon = if expanded { IconName::FolderOpen } else { IconName::Folder };
            let controls = view! {
                <icon_button test_id={format!("launch-{}", id())} icon=IconName::Plus label={format!("New thread in {name}")}
                             disabled=launching on_click={move || self.launch(id())}/>
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
                    <each items={move || self.threads.with(|threads| threads.values().filter(|entry| entry.info.project.id == id()).map(|entry| entry.info.id.clone()).collect::<Vec<_>>())}
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
        let title = move || {
            self.threads
                .with(|all| all.get(&id()).and_then(|entry| entry.pane))
                .map_or_else(|| UNTITLED.to_owned(), |(_, thread, _)| thread.title())
        };
        let row_id = format!("thread-{}", id().0);
        let status = memo(move || {
            self.threads.with(|all| {
                all.get(&id()).map_or(ThreadStatus::Dormant, |entry| match &entry.info.state {
                    HostedState::Starting => ThreadStatus::Connecting,
                    HostedState::Failed(_) => ThreadStatus::Errored,
                    HostedState::Ready => entry.pane.map_or(ThreadStatus::Idle, |(_, thread, _)| thread.status()),
                })
            })
        });
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
            move || self.selected.set(Some(id())),
        )
        .action(
            row_action("Stop thread", move || self.stop(id()))
                .icon(IconName::Square)
                .icon_only()
                .test_id(format!("stop-{}", id().0)),
        )
    }

    fn conversation(self, info: &HostedThread) -> impl View + use<> {
        let id = info.id.clone();
        let (_, thread, pane) = self
            .threads
            .try_update(|all| {
                *all.get_mut(&id).expect("selected thread").pane.get_or_insert_with(|| {
                    let scope = self.scope.child();
                    scope.run(|| {
                        let backend = self.backend.get();
                        let thread = Thread::connect(0, None, None, move |resume| backend.open(id.clone(), resume));
                        (scope, thread, ChatPane::new(thread, self.platform.get(), |_| {}, || {}))
                    })
                })
            })
            .expect("live inventory");
        let reveal = motion(1.0);
        let gutter = pane.gutter();
        let header = thread_header(
            move || gutter.get(),
            self.shell.reopen(),
            dynamic(move || breadcrumb().test_id("thread-path").min_w(0.0).current(thread.title())),
            move || thread.with_core(|core| core.session().agent_name().to_owned()).unwrap_or_default(),
            (),
        );
        view! {
            <div relative size_full track_bounds={pane.stage()}>
                {pane.transcript()}
                <show when={move || thread.is_fresh()}>{pane.kindling(reveal)}</show>
                {pane.dock(reveal, None::<AnyView>, None::<AnyView>)}
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
    }

    fn launch(self, project: String) {
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
            self.selected.set(Some(id));
            Ok(())
        });
    }

    fn stop(self, id: ThreadId) {
        self.run(self.stopping, id, async move |id| {
            self.backend.get().stop(id.clone()).await?;
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
