use crate::app::chord;
use crate::artifact_review::ArtifactReview;
use crate::chat::{NativeChatPlatform, PAST_SESSIONS_KEYS, PastSessions, TogglePastSessions};
use crate::files::Files;
use crate::platform::Platform;
use crate::registry::Registry;
use crate::sidebar::project_name;
use crate::threads::{Thread, ThreadStatus};
use crate::ui::tokens::{GROUP_HEIGHT, HEADER_HEIGHT, Radius, Space, TINT_ALPHA, UNREAD_DOT_SIZE};
use crate::ui::{
    ButtonVariant, ControlSize, IconName, Material, MaterialExt, Theme, ThreadShell, UiStyles, breadcrumb, button,
    grid_overlay, popping, presence, segment, segmented_control, thread_header,
};
use agent_client_protocol::v2::schema as acp;
use clankerdiff_core::RepoPath;
use foxfire_ui::ChatPane;
use rouge::core::{
    AnyView, Color, FileDropEvent, FileDropPhase, Motion, Role, Styled, View, div, dynamic, motion, px, show, text,
    view,
};
use rouge::reactive::{Callback, Memo, Signal, Stored, effect, memo, signal, stored, untrack};
use std::rc::Rc;

#[derive(Clone, Copy)]
pub(crate) struct ThreadView {
    thread: Thread,
    registry: Registry,
    platform: Stored<Rc<dyn Platform>>,
    tab: Signal<ThreadTab>,
    changes_seen: Signal<Option<usize>>,
    dropping: Signal<bool>,
    grid_shown: Signal<bool>,
    chat: ChatPane,
    past: PastSessions,
    files: Files,
    review: ArtifactReview,
}

/// Shows or hides the column grid over a thread, for checking that its layout sits on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ToggleGridOverlay;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThreadTab {
    Chat,
    Files,
    Review,
}

impl ThreadView {
    pub(crate) fn new(thread: Thread, registry: Registry, platform: Rc<dyn Platform>, past: &PastSessions) -> Self {
        let tab = signal(ThreadTab::Chat);
        let changes_seen = signal(None);
        let show = move |next: ThreadTab| show_tab(thread, tab, changes_seen, next);
        let files = Files::new(thread, move || show(ThreadTab::Chat));
        let review = ArtifactReview::new(thread, platform.clone(), move || {
            if tab.get_untracked() == ThreadTab::Review {
                show(ThreadTab::Chat);
            }
        });
        let chat = ChatPane::new(
            thread.chat(),
            Rc::new(NativeChatPlatform::new(platform.clone())),
            move |location| open_location(thread, &files, tab, changes_seen, &location),
            move || show(ThreadTab::Review),
        );
        let view = Self {
            thread,
            registry,
            platform: stored(platform),
            tab,
            changes_seen,
            dropping: signal(false),
            grid_shown: signal(false),
            chat,
            past: *past,
            files,
            review,
        };
        thread.on_event(move |event| {
            if event == crate::threads::ThreadEvent::ReviewSubmitted {
                show(ThreadTab::Chat);
            }
        });
        let changed = memo(move || changed_files(thread));
        effect(move || {
            changed.get();
            untrack(|| see_changes(thread, tab, changes_seen));
        });
        view
    }

    pub(crate) fn files(self) -> Files {
        self.files
    }

    pub(crate) fn show(self, tab: ThreadTab) {
        show_tab(self.thread, self.tab, self.changes_seen, tab);
    }

    pub(crate) fn open_file(self, path: RepoPath, line: Option<u32>) {
        self.show(ThreadTab::Files);
        self.files.open(path, line);
    }

    pub(crate) fn view(self, shell: ThreadShell) -> impl View {
        let tab = self.tab;
        let chat = memo(move || tab.get() == ThreadTab::Chat);
        let body = self.body(chat, motion(1.0));
        view! {
            <div relative size_full>
                {body}
                {self.header(shell)}
            </div>
        }
    }

    fn header(self, shell: ThreadShell) -> impl View {
        let (thread, registry, platform, tab) = (self.thread, self.registry, self.platform, self.tab);
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
        let gutter = self.chat.gutter();
        thread_header(
            move || if tab.get() == ThreadTab::Chat { gutter.get() } else { 0.0 },
            shell.reopen(),
            path,
            move || agent.clone(),
            self.switch(),
        )
    }

    fn switch(self) -> impl View {
        let (thread, tab, seen) = (self.thread, self.tab, self.changes_seen);
        let files = memo(move || changed_files(thread));
        let working = memo(move || match thread.status() {
            _ if tab.get() == ThreadTab::Chat => None,
            ThreadStatus::NeedsInput => Some("chat-waiting"),
            ThreadStatus::Running | ThreadStatus::Background => Some("chat-working"),
            _ => None,
        });
        let unseen =
            memo(move || files.get().is_some_and(|files| tab.get() != ThreadTab::Files && seen.get() != Some(files)));
        let reviewing = memo(move || thread.has_pending_review());
        let choose = move |value: ThreadTab| move || self.show(value);
        let working_dot = dynamic(move || {
            working.get().map(|id| corner_dot(id, if id == "chat-waiting" { Role::Warning } else { Role::Info }))
        });
        let unseen_dot = move || dynamic(move || unseen.get().then(|| corner_dot("changes-unseen", Role::Accent)));
        view! {
            <div test_id="view-switch" flex_none material=Material::GlassControl>
                <segmented_control bg=Color::TRANSPARENT>
                    <segment test_id="tab-chat" label="Chat" icon=IconName::MessageSquareText labelled
                             selected={move || tab.get() == ThreadTab::Chat} badge=working_dot
                             on_click={choose(ThreadTab::Chat)}/>
                    <dynamic>{{
                        let changed = files.get().map(|files| files.to_string());
                        view! {
                            <segment test_id="tab-files" label="Files" icon=IconName::Folder labelled detail?=changed
                                     selected={move || tab.get() == ThreadTab::Files} badge={unseen_dot()}
                                     on_click={choose(ThreadTab::Files)}/>
                        }
                    }}</dynamic>
                    <dynamic>{reviewing.get().then(|| view! {
                        <segment test_id="tab-review" label="Review" icon=IconName::Eye labelled
                                 selected={move || tab.get() == ThreadTab::Review} on_click={choose(ThreadTab::Review)}/>
                    })}</dynamic>
                </segmented_control>
            </div>
        }
    }

    /// `reveal` is the composer's opacity, which a fresh thread's kindling holds back until it has formed.
    fn body(self, chat: Memo<bool>, reveal: Motion) -> impl View {
        let (tab, dropping, composer) = (self.tab, self.dropping, self.chat.composer());
        let theme = Theme::current();
        let shown = memo(move || tab.get());
        let content = dynamic(move || match shown.get() {
            ThreadTab::Chat => untrack(|| self.chat.transcript()).into_any(),
            ThreadTab::Files => below_header(untrack(|| self.files.view())),
            ThreadTab::Review => below_header(untrack(|| self.review.view())),
        });
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
        let thread = self.thread;
        let kindle = memo(move || chat.get() && thread.is_fresh());
        let (grid_shown, gutter) = (self.grid_shown, self.chat.gutter());
        let scrollbar = move || px(if chat.get() { gutter.get() } else { 0.0 });
        view! {
            <div relative size_full on_file_drop=drop track_bounds={self.chat.stage()}
                 bg={move || if dropping.get() { theme.get().accent.with_alpha(TINT_ALPHA) } else { Color::TRANSPARENT }}>
                {content}
                {self.review_pill()}
                <show when={move || kindle.get()}>{self.chat.kindling(reveal)}</show>
                <show when={move || chat.get()}>{self.dock(reveal)}</show>
                <div absolute top=0.0 bottom=0.0 left=0.0 right=scrollbar pass_through>
                    <grid_overlay visible={move || grid_shown.get()}/>
                </div>
            </div>
        }
        .bind_key(chord(GRID_OVERLAY_KEYS), ToggleGridOverlay)
        .on_action(move |_: &ToggleGridOverlay| grid_shown.update(bool::toggle))
    }

    fn dock(self, reveal: Motion) -> impl View {
        let (past, composer) = (self.past, self.chat.composer());
        let offered = memo(move || past.offered());
        let open = memo(move || past.is_open());
        let actions = show(move || offered.get(), move || past.toggle_view());
        let tray = presence(move || open.get(), move || past.tray_view(move || composer.focus()));
        self.chat
            .dock(reveal, actions, tray)
            .bind_key(chord(PAST_SESSIONS_KEYS), TogglePastSessions)
            .on_action(move |_: &TogglePastSessions| self.past.toggle())
    }

    fn review_pill(self) -> impl View {
        let (tab, files) = (self.tab, self.files);
        let comments =
            memo(move || Some(files.comments()).filter(|&comments| tab.get() == ThreadTab::Files && comments > 0));
        popping(comments, move |comments| {
            let label = if comments == 1 { "1 comment".to_owned() } else { format!("{comments} comments") };
            view! {
                <div absolute left=0.0 right=0.0 bottom=Space::X4 flex justify_center>
                    <div test_id="review-pill" h=GROUP_HEIGHT p=Space::X1 pl=Space::X3 flex items_center gap=Space::X1 material=Material::Glass
                         rounded=Radius::Control small>
                        <div test_id="review-comments" mr=Space::X2><text no_wrap secondary>{label}</text></div>
                        {pill_button("review-copy", "Copy", false, Callback::from(move || files.copy()))}
                        {pill_button("review-submit", "Submit review", true, Callback::from(move || files.submit()))}
                    </div>
                </div>
            }
        })
    }
}

fn show_tab(thread: Thread, tab: Signal<ThreadTab>, seen: Signal<Option<usize>>, next: ThreadTab) {
    if next == ThreadTab::Review && !thread.has_pending_review() {
        return;
    }
    tab.set(next);
    see_changes(thread, tab, seen);
}

fn see_changes(thread: Thread, tab: Signal<ThreadTab>, seen: Signal<Option<usize>>) {
    if tab.get_untracked() == ThreadTab::Files {
        seen.set_if_changed(untrack(|| changed_files(thread)));
    }
}

fn open_location(
    thread: Thread,
    files: &Files,
    tab: Signal<ThreadTab>,
    seen: Signal<Option<usize>>,
    location: &acp::ToolCallLocation,
) {
    let path = location.path.0.as_path();
    let Some(file) = thread.listed_files().ok().and_then(|files| files.repo_path(path)) else {
        let message = format!("Can't open {}: it isn't in this thread's repository", path.display());
        return thread.report_error(&message);
    };
    show_tab(thread, tab, seen, ThreadTab::Files);
    files.open(file, location.line.map(std::num::NonZeroU32::get));
}

fn below_header(content: impl View) -> AnyView {
    view! { <div size_full pt=HEADER_HEIGHT>{content}</div> }.into_any()
}

fn corner_dot(id: &'static str, color: Role) -> AnyView {
    view! { <div test_id=id size=UNREAD_DOT_SIZE rounded=Radius::Full bg=color/> }.into_any()
}

fn pill_button(id: &'static str, label: &'static str, primary: bool, on_click: Callback) -> AnyView {
    let variant = if primary { ButtonVariant::Primary } else { ButtonVariant::Ghost };
    let button = view! {
        <button test_id=id label variant=variant control_size=ControlSize::Small rounded=Radius::Inner on_click=on_click/>
    };
    if primary { button.ripple().into_any() } else { button.into_any() }
}

fn changed_files(thread: Thread) -> Option<usize> {
    thread.changed_files().filter(|&files| files > 0)
}

const GRID_OVERLAY_KEYS: &str = "secondary-alt-g";
