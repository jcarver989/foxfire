use crate::backend::{
    BackendError, ConnectFailure, HostedBackend, HostedState, HostedThread, Project, ThreadId, ThreadTitle,
};
use crate::thread::{Thread, ThreadStatus};
use indexmap::IndexMap;
use rouge::core::{WindowHandle, window_focused};
use rouge::reactive::{Scope, Signal, Stored, batch, effect, memo, signal, sleep, spawn, stored, untrack, use_context};
use std::collections::HashSet;
use std::hash::Hash;
use std::rc::Rc;
use std::time::Duration;

/// The threads of a hosted backend, and what a host shows for each one it has opened. Draws nothing itself.
pub struct ThreadList<T: 'static> {
    backend: Stored<Rc<dyn HostedBackend>>,
    pane: Stored<Rc<dyn Fn(Thread) -> T>>,
    projects: Signal<Vec<Project>>,
    threads: Signal<IndexMap<ThreadId, HostedThread>>,
    opened: Signal<IndexMap<ThreadId, Opened<T>>>,
    selected: Signal<Option<ThreadId>>,
    error: Signal<Option<BackendError>>,
    loaded: Signal<bool>,
    launching: Signal<HashSet<String>>,
    changing: Signal<HashSet<ThreadId>>,
    epoch: Stored<u64>,
    scope: Scope,
}

#[derive(Clone, Copy)]
struct Opened<T> {
    thread: Thread,
    pane: T,
    scope: Scope,
}

impl<T: Copy + 'static> ThreadList<T> {
    /// Lists the backend's threads. `pane` builds what is kept for a thread for as long as it is open.
    pub fn new(backend: Rc<dyn HostedBackend>, pane: impl Fn(Thread) -> T + 'static) -> Self {
        let list = Self {
            backend: stored(backend),
            pane: stored(Rc::new(pane)),
            projects: signal(Vec::new()),
            threads: signal(IndexMap::new()),
            opened: signal(IndexMap::new()),
            selected: signal(None),
            error: signal(None),
            loaded: signal(false),
            launching: signal(HashSet::new()),
            changing: signal(HashSet::new()),
            epoch: stored(0),
            scope: Scope::new(),
        };
        let starting =
            memo(move || list.threads.with(|all| all.values().any(|entry| entry.state == HostedState::Starting)));
        effect(move || {
            if starting.get() {
                spawn(async move {
                    loop {
                        sleep(PROVISIONING_POLL).await;
                        list.load_threads().await;
                    }
                });
            }
        });
        list.refresh();
        list
    }

    /// Whether the threads have been listed yet.
    pub fn is_loaded(self) -> bool {
        self.loaded.get()
    }

    pub fn projects(self) -> Vec<Project> {
        self.projects.get()
    }

    pub fn threads(self) -> Vec<HostedThread> {
        self.threads.with(|all| all.values().cloned().collect())
    }

    pub fn project_threads(self, project: &str) -> Vec<ThreadId> {
        self.threads.with(|all| {
            all.values().filter(|entry| entry.project.id == project).map(|entry| entry.id.clone()).collect()
        })
    }

    pub fn info(self, id: &ThreadId) -> Option<HostedThread> {
        self.threads.with(|all| all.get(id).cloned())
    }

    /// A thread that has been opened.
    pub fn thread(self, id: &ThreadId) -> Option<Thread> {
        self.opened.with(|all| all.get(id).map(|opened| opened.thread))
    }

    /// What a thread is called, once it has been prompted or renamed.
    pub fn title(self, id: &ThreadId) -> Option<String> {
        let chosen = self.threads.with(|all| all.get(id)?.title.clone());
        chosen.or_else(|| self.thread(id)?.title())
    }

    pub fn status(self, id: &ThreadId) -> ThreadStatus {
        self.threads.with(|all| {
            let Some(info) = all.get(id) else {
                return ThreadStatus::Dormant;
            };
            let opened = self.thread(id).map(|thread| thread.status());
            match &info.state {
                HostedState::Starting => ThreadStatus::Connecting,
                HostedState::Failed(_) => ThreadStatus::Errored,
                HostedState::Ready => opened.unwrap_or(ThreadStatus::Idle),
                HostedState::Suspended | HostedState::Ended => opened.unwrap_or(ThreadStatus::Dormant),
            }
        })
    }

    pub fn selected(self) -> Option<ThreadId> {
        self.selected.get()
    }

    pub fn error(self) -> Option<BackendError> {
        self.error.get()
    }

    pub fn dismiss_error(self) {
        self.error.set(None);
    }

    /// Whether a project is launching a thread, or has one that is starting.
    pub fn is_launching(self, project: &str) -> bool {
        self.launching.with(|all| all.contains(project))
            || self.threads.with(|all| {
                all.values().any(|entry| entry.project.id == project && entry.state == HostedState::Starting)
            })
    }

    /// Whether a thread is being renamed, suspended or stopped.
    pub fn is_changing(self, id: &ThreadId) -> bool {
        self.changing.with(|all| all.contains(id))
    }

    pub fn refresh(self) {
        self.scope.run(|| {
            spawn(async move {
                match self.backend.get().projects().await {
                    Ok(projects) => {
                        self.projects.set_if_changed(projects);
                    }
                    Err(error) => self.report(error),
                }
                self.load_threads().await;
            })
        });
    }

    /// Selects a thread, resuming it if it is suspended.
    pub fn select(self, id: &ThreadId) {
        self.resume(id);
        self.selected.set_if_changed(Some(id.clone()));
    }

    /// Wakes a suspended thread by opening it.
    pub fn resume(self, id: &ThreadId) {
        if self.is_suspended(id) {
            let (thread, _) = self.opened(id);
            if untrack(|| thread.status()) == ThreadStatus::Dormant {
                thread.reconnect();
            }
        }
    }

    /// Whether a thread has a conversation to show: it is ready, resuming, or ended while it was open.
    pub fn can_open(self, id: &ThreadId) -> bool {
        self.threads.with(|all| {
            all.get(id).is_some_and(|entry| match (&entry.state, self.thread(id)) {
                (HostedState::Ready, _) | (HostedState::Ended, Some(_)) => true,
                (HostedState::Suspended, Some(opened)) => opened.status() != ThreadStatus::Dormant,
                _ => false,
            })
        })
    }

    /// A thread and its pane, opened the first time.
    pub fn open(self, id: &ThreadId) -> Option<(Thread, T)> {
        untrack(|| self.can_open(id).then(|| self.opened(id)))
    }

    pub fn launch(self, project: String, on_launch: impl FnOnce(&ThreadId) + 'static) {
        self.run(self.launching, project, async move |project| {
            let info = self.backend.get().launch(project).await?;
            let id = info.id.clone();
            self.reconcile(|all| {
                all.insert(id.clone(), info);
            });
            self.loaded.set_if_changed(true);
            on_launch(&id);
            Ok(())
        });
    }

    pub fn rename(self, id: ThreadId, title: String) {
        self.run(self.changing, id, async move |id| {
            self.backend.get().rename(id.clone(), ThreadTitle::Chosen(title.clone())).await?;
            self.reconcile(|all| {
                if let Some(info) = all.get_mut(&id) {
                    info.title = Some(title);
                }
            });
            Ok(())
        });
    }

    /// Suspends a thread, and disconnects from it until it is opened again.
    pub fn suspend(self, id: ThreadId) {
        self.run(self.changing, id, async move |id| {
            self.backend.get().suspend(id.clone()).await?;
            self.reconcile(|all| {
                if let Some(info) = all.get_mut(&id) {
                    info.state = HostedState::Suspended;
                }
            });
            if let Some(thread) = untrack(|| self.thread(&id)) {
                thread.stop();
            }
            Ok(())
        });
    }

    pub fn stop(self, id: ThreadId, on_stop: impl FnOnce(&ThreadId) + 'static) {
        self.run(self.changing, id, async move |id| {
            self.backend.get().stop(id.clone()).await?;
            on_stop(&id);
            self.remove(&id);
            Ok(())
        });
    }

    fn opened(self, id: &ThreadId) -> (Thread, T) {
        if let Some(opened) = self.opened.with_untracked(|all| all.get(id).copied()) {
            return (opened.thread, opened.pane);
        }
        let scope = self.scope.child();
        let opened = scope.run(|| {
            let (backend, target) = (self.backend.get(), id.clone());
            let thread = Thread::connect(0, None, None, move |resume| backend.open(target.clone(), resume));
            self.observe(id, thread);
            Opened { thread, pane: self.pane.get()(thread), scope }
        });
        self.opened.update(|all| {
            all.insert(id.clone(), opened);
        });
        (opened.thread, opened.pane)
    }

    fn observe(self, id: &ThreadId, thread: Thread) {
        let window = use_context::<WindowHandle>().map(|_| window_focused());
        thread.keep_connected(move || window.is_some_and(|focused| focused.get()));
        let seen = memo(move || {
            let (status, failure) = (thread.status(), thread.error());
            (status, failure, thread.is_dropped())
        });
        effect({
            let id = id.clone();
            move || {
                let (status, failure, dropped) = seen.get();
                untrack(|| match (status, failure) {
                    (_, Some(ConnectFailure::Unauthenticated(_))) => self.report(BackendError::Unauthenticated),
                    (_, Some(ConnectFailure::Ended(_))) => self.recheck(),
                    (ThreadStatus::Idle, _) if self.is_suspended(&id) => self.recheck(),
                    _ if dropped => self.recheck(),
                    _ => {}
                });
            }
        });
        let title = memo(move || thread.title());
        let id = id.clone();
        effect(move || {
            if let Some(title) = title.get() {
                untrack(|| self.suggest_title(&id, title));
            }
        });
    }

    fn suggest_title(self, id: &ThreadId, title: String) {
        if self.threads.with_untracked(|all| all.get(id).is_none_or(|entry| entry.title.is_some())) {
            return;
        }
        let id = id.clone();
        self.scope.run(|| {
            spawn(async move {
                match self.backend.get().rename(id, ThreadTitle::Suggested(title)).await {
                    // The suggestion may have lost to a title chosen by another client.
                    Ok(()) => self.load_threads().await,
                    Err(error) => tracing::warn!(%error, "could not title a thread after its first prompt"),
                }
            })
        });
    }

    fn is_suspended(self, id: &ThreadId) -> bool {
        self.threads.with_untracked(|all| all.get(id).is_some_and(|entry| entry.state == HostedState::Suspended))
    }

    fn recheck(self) {
        self.scope.run(|| spawn(self.load_threads()));
    }

    async fn load_threads(self) {
        let epoch = self.supersede();
        let result = self.backend.get().list_threads().await;
        if epoch != self.epoch.get() {
            return;
        }
        match result {
            Ok(threads) => {
                self.reconcile(|all| *all = threads.into_iter().map(|info| (info.id.clone(), info)).collect());
            }
            Err(error) => self.report(error),
        }
        self.loaded.set_if_changed(true);
    }

    fn reconcile(self, update: impl FnOnce(&mut IndexMap<ThreadId, HostedThread>)) {
        self.supersede();
        batch(|| {
            self.threads.update(update);
            self.threads.with_untracked(|all| {
                if self.selected.with_untracked(|id| id.as_ref().is_some_and(|id| !all.contains_key(id))) {
                    self.selected.set(None);
                }
                let opened = self.opened.get_untracked();
                for (id, pane) in opened {
                    match all.get(&id) {
                        None => {
                            self.opened.update(|panes| {
                                panes.shift_remove(&id);
                            });
                            pane.scope.dispose();
                        }
                        // Reconnecting would resume a thread that someone else suspended.
                        Some(info) if info.state == HostedState::Suspended && pane.thread.is_dropped() => {
                            pane.thread.stop();
                        }
                        _ => {}
                    }
                }
            });
        });
    }

    fn remove(self, id: &ThreadId) {
        self.reconcile(|all| {
            all.shift_remove(id);
        });
    }

    /// Makes any listing that is still loading out of date.
    fn supersede(self) -> u64 {
        self.epoch.update(|epoch| {
            *epoch += 1;
            *epoch
        })
    }

    fn report(self, error: BackendError) {
        if let BackendError::UnknownThread(id) = &error {
            self.remove(id);
        }
        self.error.set(Some(error));
    }

    fn run<U: Clone + Eq + Hash + 'static>(
        self,
        pending: Signal<HashSet<U>>,
        key: U,
        work: impl AsyncFnOnce(U) -> Result<(), BackendError> + 'static,
    ) {
        if pending.try_update(|all| all.insert(key.clone())) != Some(true) {
            return;
        }
        self.scope.run(|| {
            spawn(async move {
                if let Err(error) = work(key.clone()).await {
                    self.report(error);
                }
                pending.update(|all| {
                    all.remove(&key);
                });
            })
        });
    }
}

impl<T> Clone for ThreadList<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for ThreadList<T> {}

const PROVISIONING_POLL: Duration = Duration::from_secs(1);
