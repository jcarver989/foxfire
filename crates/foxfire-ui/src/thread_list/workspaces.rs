use crate::backend::{SessionLink, ThreadId};
use crate::thread::Thread;
use agent_client_protocol::v2::schema::SessionId;
use indexmap::IndexMap;
use rouge::core::{WindowHandle, window_focused};
use rouge::reactive::{Scope, Signal, Stored, signal, stored, use_context};

#[derive(Clone, Copy)]
struct Opened<T> {
    thread: Thread,
    pane: T,
    scope: Scope,
}

/// Retains each opened connection and its pane in a child scope independent of the current view.
#[derive(Clone, Copy)]
pub struct Workspaces<T: 'static> {
    opened: Signal<IndexMap<ThreadId, Opened<T>>>,
    connect: Stored<Box<Connector>>,
    scope: Scope,
}

type Connector = dyn Fn(&ThreadId, Option<SessionId>) -> SessionLink;

impl<T: Copy + 'static> Workspaces<T> {
    /// Creates an empty cache. `connect` attaches to a thread, optionally resuming its ACP session.
    pub fn new(connect: impl Fn(&ThreadId, Option<SessionId>) -> SessionLink + 'static) -> Self {
        Self { opened: signal(IndexMap::new()), connect: stored(Box::new(connect)), scope: Scope::new() }
    }

    /// The existing connection, if this workspace has ever been opened.
    pub fn thread(self, id: &ThreadId) -> Option<Thread> {
        self.opened.with(|all| all.get(id).map(|opened| opened.thread))
    }

    /// Connects and builds `pane` only on first use, including when a touch route is revisited.
    pub fn open(self, id: &ThreadId, pane: impl FnOnce(Thread) -> T) -> (Thread, T) {
        if let Some(opened) = self.opened.with_untracked(|all| all.get(id).copied()) {
            return (opened.thread, opened.pane);
        }
        let target = id.clone();
        let scope = self.scope.child();
        let opened = scope.run(|| {
            let thread = Thread::connect(0, None, None, move |resume| self.connect.with(|open| open(&target, resume)));
            let window = use_context::<WindowHandle>().map(|_| window_focused());
            thread.keep_connected(move || window.is_some_and(|focused| focused.get()));
            Opened { thread, pane: pane(thread), scope }
        });
        self.opened.update(|all| {
            all.insert(id.clone(), opened);
        });
        (opened.thread, opened.pane)
    }

    pub(super) fn retain(self, keep: impl Fn(&ThreadId, Thread) -> bool) {
        let removed: Vec<_> = self.opened.with_untracked(|all| {
            all.iter().filter(|(id, pane)| !keep(id, pane.thread)).map(|(id, _)| id.clone()).collect()
        });
        for id in removed {
            if let Some(opened) = self.opened.try_update(|all| all.shift_remove(&id)).flatten() {
                opened.scope.dispose();
            }
        }
    }
}
