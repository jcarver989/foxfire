use crate::app::chord;
use crate::artifact_review::ArtifactReview;
use crate::chat::{
    Composer, PAST_SESSIONS_KEYS, PastSessions, SessionOptions, StatusLine, TogglePastSessions, Transcript,
    reconnect_banner,
};
use crate::elicitation::ElicitationCard;
use crate::files::Files;
use crate::platform::Platform;
use crate::registry::Registry;
use crate::sidebar::{project_name, toggle_button};
use crate::threads::{Thread, ThreadStatus};
use crate::ui::tokens::{
    BAR_HEIGHT, GRID_MARGIN, GROUP_HEIGHT, HEADER_HEIGHT, ICON_BUTTON_SIZE, ICON_SIZE_SMALL, PANEL_INSET, ROW_HEIGHT,
    Radius, Space, TINT_ALPHA, UNIT, UNREAD_DOT_SIZE,
};
use crate::ui::{
    ButtonVariant, ControlSize, FoxfireState, IconName, Material, MaterialExt, Theme, UiStyles, avatar, breadcrumb,
    button, column_grid, foxfire, grid_overlay, kindling, popping, presence, reading_cell, scroll_edge, segment,
    segmented_control,
};
use agent_client_protocol::v2::schema as acp;
use clankerdiff_core::RepoPath;
use rouge::core::{
    AnyView, Color, FileDropEvent, FileDropPhase, Motion, Rect, Role, Styled, TextAlign, View, auto, div, dynamic,
    motion, px, show, text, view,
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
    dock: Signal<Rect>,
    stage: Signal<Rect>,
    composer_bounds: Signal<Rect>,
    transcript: Transcript,
    options: SessionOptions,
    status: StatusLine,
    elicitation: ElicitationCard,
    composer: Composer,
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
        let (dock, stage) = (signal(Rect::ZERO), signal(Rect::ZERO));
        let composer = Composer::new(thread, platform.clone());
        let transcript = Transcript::new(
            thread,
            platform.clone(),
            dock,
            stage,
            move |location: acp::ToolCallLocation| open_location(thread, files, tab, changes_seen, &location),
            move |typed: String| composer.type_text(&typed),
        );
        let view = Self {
            thread,
            registry,
            platform: stored(platform.clone()),
            tab,
            changes_seen,
            dropping: signal(false),
            grid_shown: signal(false),
            dock,
            stage,
            composer_bounds: signal(Rect::ZERO),
            transcript,
            options: SessionOptions::new(thread),
            status: StatusLine::new(thread),
            elicitation: ElicitationCard::new(thread, platform, move || show(ThreadTab::Review)),
            composer,
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

    pub(crate) fn view(self, sidebar_collapsed: Memo<bool>) -> impl View {
        let tab = self.tab;
        let chat = memo(move || tab.get() == ThreadTab::Chat);
        let body = self.body(chat, motion(1.0));
        view! {
            <div relative size_full>
                {body}
                {self.header(sidebar_collapsed)}
            </div>
        }
    }

    fn header(self, sidebar_collapsed: Memo<bool>) -> impl View {
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
        let gutter = self.transcript.gutter();
        let scrollbar = move || px(if tab.get() == ThreadTab::Chat { gutter.get() } else { 0.0 });
        view! {
            <div test_id="thread-header" absolute top=0.0 left=0.0 right=scrollbar h=HEADER_HEIGHT px=GRID_MARGIN
                 flex items_center gap=Space::X3 pass_through>
                {scroll_edge(true, PANEL_INSET, HEADER_HEIGHT)}
                <div flex_1 min_w=0.0 flex pass_through>
                    <div test_id="thread-heading" min_w=0.0 h=GROUP_HEIGHT px=Space::X3 flex items_center gap=Space::X2
                         material=Material::GlassControl>
                        <dynamic>{sidebar_collapsed.get().then(|| toggle_button(false))}</dynamic>
                        {path}
                        <div test_id="thread-agent" flex_none flex items_center gap=Space::X1>
                            {avatar(agent.clone()).avatar_size(ICON_BUTTON_SIZE)}
                            <text small muted no_wrap>{agent}</text>
                        </div>
                    </div>
                </div>
                {self.switch()}
            </div>
        }
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
        let (tab, dropping, composer) = (self.tab, self.dropping, self.composer);
        let theme = Theme::current();
        let shown = memo(move || tab.get());
        let content = dynamic(move || match shown.get() {
            ThreadTab::Chat => untrack(|| self.transcript.view()).into_any(),
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
        let (grid_shown, gutter) = (self.grid_shown, self.transcript.gutter());
        let scrollbar = move || px(if chat.get() { gutter.get() } else { 0.0 });
        view! {
            <div relative size_full on_file_drop=drop track_bounds={self.stage}
                 bg={move || if dropping.get() { theme.get().accent.with_alpha(TINT_ALPHA) } else { Color::TRANSPARENT }}>
                {content}
                {self.review_pill()}
                <show when={move || kindle.get()}>{self.kindling(reveal)}</show>
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
        let thread = self.thread;
        let unreachable = memo(move || unreachable(thread));
        let fresh = memo(move || thread.is_fresh());
        let connecting = memo(move || fresh.get() && thread.status() == ThreadStatus::Connecting);
        let (past, composer) = (self.past, self.composer);
        let offered = memo(move || past.offered());
        let open = memo(move || past.is_open());
        let spacer = move || view! { <show when={move || fresh.get()}><div flex_1/></show> };
        let (reading, gutter) = (self.transcript.reading(), self.transcript.gutter());
        view! {
            <div absolute left=0.0 right={move || px(gutter.get())} bottom=0.0
                 top={move || if fresh.get() { px(0.0) } else { auto() }}
                 pb=Space::X3 flex col pass_through track_bounds={self.dock}>
                <show when={move || !fresh.get()}>{scroll_edge(false, DOCK_EDGE_SOLID, DOCK_EDGE_REACH)}</show>
                {spacer()}
                <column_grid flex_none pass_through>
                <reading_cell span=reading flex col>
                    <show when={move || fresh.get()}>
                        <div test_id="new-thread-heading" flex justify_center mb=HEADING_GAP>
                            <text display align=TextAlign::Center>{NEW_THREAD_HEADING}</text>
                        </div>
                    </show>
                    {popping(unreachable, move |status| reconnect_banner(thread, status).map(|banner| banner.mt(Space::X2)))}
                    {self.elicitation.view()}
                    <div track_bounds={self.composer_bounds} opacity=reveal>{self.composer.view()}</div>
                    <div flex col opacity=reveal>
                        <div test_id="composer-bar" h=ROW_HEIGHT mt=Space::X1 px=Space::X2 flex items_center gap=Space::X4>
                            {self.options.view()}
                            {self.status.view()}
                            <show when={move || connecting.get()}>{connecting_message()}</show>
                            <show when={move || offered.get()}>{past.toggle_view()}</show>
                        </div>
                        {presence(move || open.get(), move || past.tray_view(move || composer.focus()))}
                    </div>
                </reading_cell>
                </column_grid>
                {spacer()}
            </div>
        }
        .bind_key(chord(PAST_SESSIONS_KEYS), TogglePastSessions)
        .on_action(move |_: &TogglePastSessions| self.past.toggle())
    }

    fn kindling(self, reveal: Motion) -> impl View {
        let (stage, composer) = (self.stage, self.composer_bounds);
        kindling(
            move || {
                let (stage, composer) = (stage.get(), composer.get());
                Rect::new(
                    composer.origin.x - stage.origin.x,
                    composer.origin.y - stage.origin.y + COMPOSER_MARGIN,
                    composer.size.width,
                    (composer.size.height - COMPOSER_MARGIN).max(0.0),
                )
            },
            reveal,
        )
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
    files: Files,
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

/// Shown in the composer bar while a new thread's agent starts, so the layout is already the one it will keep.
fn connecting_message() -> impl View {
    let muted = Role::TextMuted;
    view! {
        <div test_id="new-thread-connecting" flex_none flex items_center gap=Space::X2>
            {foxfire(ICON_SIZE_SMALL, muted, || FoxfireState::Connecting)}
            <text body muted>"Connecting…"</text>
        </div>
    }
}

fn unreachable(thread: Thread) -> Option<ThreadStatus> {
    let status = thread.status();
    let has_items = thread.with_core(|core| !core.conversation().items().is_empty()).unwrap_or(false);
    (status.problem().is_some() && has_items).then_some(status)
}

const DOCK_EDGE_SOLID: f32 = BAR_HEIGHT;
const DOCK_EDGE_REACH: f32 = BAR_HEIGHT;
const NEW_THREAD_HEADING: &str = "What do you want to build?";
const GRID_OVERLAY_KEYS: &str = "secondary-alt-g";
/// Room above a fresh thread's composer for the tall flames off its top edge; below it they only lick.
const HEADING_GAP: f32 = 14.0 * UNIT;
/// The composer sits this far below the top of its own bounds; the fire rings its glass, not the margin.
const COMPOSER_MARGIN: f32 = Space::X2.px();
