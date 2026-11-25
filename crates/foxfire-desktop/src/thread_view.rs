use crate::app::chord;
use crate::chat::{NativeChatPlatform, PAST_SESSIONS_KEYS, PastSessions, TogglePastSessions};
use crate::platform::Platform;
use crate::registry::Registry;
use crate::sidebar::project_name;
use crate::threads::Thread;
use crate::ui::tokens::{Space, TINT_ALPHA};
use crate::ui::{Theme, ThreadShell, breadcrumb, grid_overlay, presence, thread_header};
use clankerdiff_core::RepoPath;
use foxfire_shell::conversation::dock;
use foxfire_shell::workspace::{Header, ThreadTab, Workspace};
use foxfire_ui::Files;
use rouge::core::{Color, FileDropEvent, FileDropPhase, Motion, Styled, View, div, dynamic, px, show, view};
use rouge::reactive::{Signal, Stored, memo, signal, stored};
use std::rc::Rc;

#[derive(Clone, Copy)]
pub(crate) struct ThreadView {
    thread: Thread,
    registry: Registry,
    platform: Stored<Rc<dyn Platform>>,
    workspace: Workspace,
    dropping: Signal<bool>,
    grid_shown: Signal<bool>,
    past: PastSessions,
}

/// Shows or hides the column grid over a thread, for checking that its layout sits on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ToggleGridOverlay;

impl ThreadView {
    pub(crate) fn new(thread: Thread, registry: Registry, platform: Rc<dyn Platform>, past: &PastSessions) -> Self {
        let workspace = Workspace::new(thread.chat(), Rc::new(NativeChatPlatform::new(platform.clone())));
        Self {
            thread,
            registry,
            platform: stored(platform),
            workspace,
            dropping: signal(false),
            grid_shown: signal(false),
            past: *past,
        }
    }

    pub(crate) fn files(self) -> Files {
        self.workspace.files()
    }

    pub(crate) fn show(self, tab: ThreadTab) {
        self.workspace.show(tab);
    }

    pub(crate) fn toggle_chat_room(self) {
        self.workspace.chat().room().toggle();
    }

    pub(crate) fn open_file(self, path: RepoPath, line: Option<u32>) {
        self.workspace.open_file(path, line);
    }

    pub(crate) fn view(self, shell: ThreadShell) -> impl View {
        let conversation = view! {
            <div relative size_full>
                {self.body()}
                {self.header(shell)}
            </div>
        };
        shell.with_room(self.workspace.chat().room(), conversation)
    }

    fn header(self, shell: ThreadShell) -> impl View {
        let (thread, registry, platform, workspace) = (self.thread, self.registry, self.platform, self.workspace);
        let title = memo(move || registry.with(|state| state.title(thread.session_key().as_ref(), thread.title())));
        let project = thread.project();
        let agent = thread.target().label().to_owned();
        let path = dynamic(move || {
            let crumbs = breadcrumb().test_id("thread-path").min_w(0.0);
            let crumbs = match &project {
                Some(project) => {
                    let folder = project.display().to_string();
                    crumbs.link(project_name(project), move || platform.with(|platform| platform.open(&folder)))
                }
                None => crumbs,
            };
            crumbs.current(title.get())
        });
        let gutter = workspace.chat().gutter();
        thread_header(
            move || if workspace.tab() == ThreadTab::Chat { gutter.get() } else { 0.0 },
            shell.reopen(),
            path,
            move || agent.clone(),
            view! {
                <div flex_none flex items_center gap=Space::X2>{workspace.chat().room().header_controls()}{workspace.switch()}</div>
            },
        )
    }

    fn body(self) -> impl View {
        let (workspace, dropping, composer) = (self.workspace, self.dropping, self.workspace.chat().composer());
        let theme = Theme::current();
        let chat = memo(move || workspace.tab() == ThreadTab::Chat);
        let drop = move |event: &FileDropEvent| {
            if !chat.get_untracked() {
                return;
            }
            event.stop_propagation();
            match event.phase {
                FileDropPhase::Hover => dropping.set(true),
                FileDropPhase::Leave => dropping.set(false),
                FileDropPhase::Drop => {
                    dropping.set(false);
                    composer.attach(&event.paths);
                    composer.focus();
                }
            }
        };
        let (grid_shown, gutter) = (self.grid_shown, workspace.chat().gutter());
        let scrollbar = move || px(if chat.get() { gutter.get() } else { 0.0 });
        let grid = view! {
            <div absolute top=0.0 bottom=0.0 left=0.0 right=scrollbar pass_through>
                <grid_overlay visible={move || grid_shown.get()}/>
            </div>
        };
        workspace
            .view(Header::Floating, move |reveal| self.dock(reveal))
            .on_file_drop(drop)
            .bg(move || if dropping.get() { theme.get().accent.with_alpha(TINT_ALPHA) } else { Color::TRANSPARENT })
            .child(grid)
            .bind_key(chord(GRID_OVERLAY_KEYS), ToggleGridOverlay)
            .on_action(move |_: &ToggleGridOverlay| grid_shown.update(bool::toggle))
    }

    fn dock(self, reveal: Motion) -> impl View {
        let (past, chat) = (self.past, self.workspace.chat());
        let composer = chat.composer();
        let offered = memo(move || past.offered());
        let open = memo(move || past.is_open());
        let actions = show(move || offered.get(), move || past.toggle_view());
        let tray = presence(move || open.get(), move || past.tray_view(move || composer.focus()));
        dock(&chat, reveal, actions, tray)
            .bind_key(chord(PAST_SESSIONS_KEYS), TogglePastSessions)
            .on_action(move |_: &TogglePastSessions| self.past.toggle())
    }
}

const GRID_OVERLAY_KEYS: &str = "secondary-alt-g";
