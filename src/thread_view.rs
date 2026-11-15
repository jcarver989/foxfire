use crate::artifact_review::ArtifactReview;
use crate::chat::{Composer, StatusLine, Transcript, reconnect_banner};
use crate::elicitation::ElicitationCard;
use crate::files::Files;
use crate::platform::Platform;
use crate::registry::Registry;
use crate::sidebar::toggle_button;
use crate::threads::{Thread, ThreadStatus};
use crate::ui::tokens::{GROUP_HEIGHT, GUTTER, HEADER_HEIGHT, ICON_SIZE_SMALL, PANEL_INSET, ROW_HEIGHT, Radius};
use crate::ui::{
    Icon, IconName, Material, MaterialExt, StyledExt, Theme, UiStyles, WispState, kindling, scroll_edge, wisp,
};
use agent_client_protocol::v2::schema as acp;
use clankerdiff_core::RepoPath;
use rouge::core::{
    AnyView, Color, Cursor, Div, FileDropEvent, FileDropPhase, Motion, Rect, Role, Styled, TextAlign, View, auto, div,
    dynamic, motion, px, show, text, view,
};
use rouge::reactive::{Callback, Memo, Signal, effect, memo, signal, untrack};
use std::rc::Rc;

#[derive(Clone, Copy)]
pub(crate) struct ThreadView {
    thread: Thread,
    registry: Registry,
    tab: Signal<ThreadTab>,
    changes_seen: Signal<Option<usize>>,
    dropping: Signal<bool>,
    dock: Signal<Rect>,
    stage: Signal<Rect>,
    composer_bounds: Signal<Rect>,
    transcript: Transcript,
    status: StatusLine,
    elicitation: ElicitationCard,
    composer: Composer,
    files: Files,
    review: ArtifactReview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThreadTab {
    Chat,
    Files,
    Review,
}

impl ThreadView {
    pub(crate) fn new(thread: Thread, registry: Registry, platform: Rc<dyn Platform>) -> Self {
        let tab = signal(ThreadTab::Chat);
        let changes_seen = signal(None);
        let show = move |next: ThreadTab| show_tab(thread, tab, changes_seen, next);
        let files = Files::new(thread, move || show(ThreadTab::Chat));
        let review = ArtifactReview::new(thread, platform.clone(), move || {
            if tab.get_untracked() == ThreadTab::Review {
                show(ThreadTab::Chat);
            }
        });
        let dock = signal(Rect::ZERO);
        let transcript = Transcript::new(thread, platform.clone(), dock, move |location: acp::ToolCallLocation| {
            open_location(thread, files, tab, changes_seen, &location);
        });
        let view = Self {
            thread,
            registry,
            tab,
            changes_seen,
            dropping: signal(false),
            dock,
            stage: signal(Rect::ZERO),
            composer_bounds: signal(Rect::ZERO),
            transcript,
            status: StatusLine::new(thread),
            elicitation: ElicitationCard::new(thread, platform.clone(), move || show(ThreadTab::Review)),
            composer: Composer::new(thread, platform),
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
        let (thread, registry) = (self.thread, self.registry);
        let title = memo(move || registry.with(|state| state.title(thread.session_id().as_ref(), thread.title())));
        let project = thread.project().map(|project| project.display().to_string());
        let agent = thread.target().label().to_owned();
        let project = project.map(|project| {
            view! { <div min_w=0.0 overflow_hidden><text small muted no_wrap>{project}</text></div> }
        });
        view! {
            <div test_id="thread-header" absolute top=0.0 left=0.0 right=0.0 h=HEADER_HEIGHT px=GUTTER
                 flex items_center gap=12.0 pass_through>
                {scroll_edge(true, PANEL_INSET, HEADER_HEIGHT)}
                <div flex_1 min_w=0.0 flex pass_through>
                    <div test_id="thread-heading" min_w=0.0 h=GROUP_HEIGHT px=12.0 flex items_center gap=8.0
                         material=Material::GlassControl>
                        <dynamic>{sidebar_collapsed.get().then(|| toggle_button(false))}</dynamic>
                        <div test_id="thread-title" min_w=0.0 overflow_hidden>
                            <text title no_wrap>{move || title.get()}</text>
                        </div>
                        {project}
                        <div test_id="thread-agent" flex_none><text small muted>{agent}</text></div>
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
        let chat = segment(&self, "tab-chat", IconName::MessageSquareText, "Chat", ThreadTab::Chat).child(dynamic(
            move || {
                working.get().map(|id| {
                    let color = if id == "chat-waiting" { Role::Warning } else { Role::Info };
                    corner_dot(id, color)
                })
            },
        ));
        let files_tab = segment(&self, "tab-files", IconName::Folder, "Files", ThreadTab::Files)
            .child(dynamic(move || {
                files.get().map(|files| {
                    view! { <div test_id="changes-count"><text muted>{files.to_string()}</text></div> }
                })
            }))
            .child(dynamic(move || unseen.get().then(|| corner_dot("changes-unseen", Role::Accent))));
        view! {
            <div test_id="view-switch" flex_none h=GROUP_HEIGHT p=4.0 flex items_center gap=4.0
                 material=Material::GlassControl small>
                {chat}
                {files_tab}
                <dynamic>{reviewing.get().then(|| segment(&self, "tab-review", IconName::Eye, "Review", ThreadTab::Review))}</dynamic>
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
        view! {
            <div relative size_full on_file_drop=drop track_bounds={self.stage}
                 bg={move || if dropping.get() { theme.get().accent.with_alpha(DROP_TINT) } else { Color::TRANSPARENT }}>
                {content}
                {self.review_pill()}
                <show when={move || kindle.get()}>{self.kindling(reveal)}</show>
                <show when={move || chat.get()}>{self.dock(reveal)}</show>
            </div>
        }
    }

    /// Docked under the transcript, or on a fresh thread centred beneath a heading asking what to build.
    fn dock(self, reveal: Motion) -> impl View {
        let thread = self.thread;
        let unreachable = memo(move || unreachable(thread));
        let fresh = memo(move || thread.is_fresh());
        let connecting = memo(move || fresh.get() && thread.status() == ThreadStatus::Connecting);
        let spacer = move || view! { <show when={move || fresh.get()}><div flex_1/></show> };
        view! {
            <div absolute left=0.0 right=0.0 bottom=0.0 top={move || if fresh.get() { px(0.0) } else { auto() }}
                 px=16.0 pb=12.0 flex col pass_through track_bounds={self.dock}>
                <show when={move || !fresh.get()}>{scroll_edge(false, DOCK_EDGE_SOLID, DOCK_EDGE_REACH)}</show>
                {spacer()}
                <div content_width flex col>
                    <show when={move || fresh.get()}>
                        <div test_id="new-thread-heading" flex justify_center mb=HEADING_GAP>
                            <text display align=TextAlign::Center>{NEW_THREAD_HEADING}</text>
                        </div>
                    </show>
                    <dynamic>{unreachable.get().and_then(|status| reconnect_banner(thread, status)).map(|banner| banner.mt(8.0))}</dynamic>
                    {self.elicitation.view()}
                    <div track_bounds={self.composer_bounds} opacity=reveal>{self.composer.view()}</div>
                    <div test_id="composer-bar" h=ROW_HEIGHT mt=4.0 px=8.0 flex items_center gap=16.0 opacity=reveal>
                        {self.status.view()}
                        <show when={move || connecting.get()}>{connecting_message()}</show>
                    </div>
                </div>
                {spacer()}
            </div>
        }
    }

    /// A snowflake that blooms into the composer and sets it alight while the thread waits for its first prompt.
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
        let comments = memo(move || if tab.get() == ThreadTab::Files { files.comments() } else { 0 });
        dynamic(move || {
            let comments = comments.get();
            (comments > 0).then(|| {
                let label = if comments == 1 { "1 comment".to_owned() } else { format!("{comments} comments") };
                view! {
                    <div absolute left=0.0 right=0.0 bottom=16.0 flex justify_center>
                        <div test_id="review-pill" h=GROUP_HEIGHT p=4.0 pl=12.0 flex items_center gap=4.0 material=Material::Glass
                             rounded=Radius::Control small>
                            <div test_id="review-comments" mr=8.0><text no_wrap secondary>{label}</text></div>
                            {pill_button("review-copy", "Copy", false, Callback::from(move || files.copy()))}
                            {pill_button("review-submit", "Submit review", true, Callback::from(move || files.submit()))}
                        </div>
                    </div>
                }
            })
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

fn segment(view: &ThreadView, id: &'static str, icon: IconName, label: &'static str, value: ThreadTab) -> Div {
    let (theme, view) = (Theme::current(), *view);
    let tab = view.tab;
    let selected = memo(move || tab.get() == value);
    let hovered = signal(false);
    let foreground = move || {
        if selected.get() || hovered.get() { theme.get().text } else { theme.get().text_secondary }
    };
    view! {
        <div test_id=id relative h_full px=12.0 flex items_center gap=4.0 rounded=Radius::Inner
             cursor=Cursor::Pointer text_color=foreground
             bg={move || if selected.get() { theme.get().text.with_alpha(SELECTED_TINT) } else { Color::TRANSPARENT }}
             on_hover={move |inside| {
                 hovered.set_if_changed(inside);
             }}
             on_click={move |event| {
                 event.stop_propagation();
                 view.show(value);
             }}>
            <div test_id={format!("{id}-icon")} flex>{Icon::new(icon, Role::TextMuted).size(ICON_SIZE_SMALL)}</div>
            <text>{label}</text>
        </div>
    }
}

fn below_header(content: impl View) -> AnyView {
    view! { <div size_full pt=HEADER_HEIGHT>{content}</div> }.into_any()
}

fn corner_dot(id: &'static str, color: Role) -> AnyView {
    view! {
        <div test_id=id absolute top=0.0 right=0.0 size=CORNER_DOT_SIZE rounded=Radius::Full
             bg=color/>
    }
    .into_any()
}

fn pill_button(id: &'static str, label: &'static str, primary: bool, on_click: Callback) -> AnyView {
    let theme = Theme::current();
    let hovered = signal(false);
    view! {
        <div test_id=id h_full px=12.0 flex items_center rounded=Radius::Inner cursor=Cursor::Pointer
             bg={move || match (primary, hovered.get()) {
                 (true, false) => theme.get().accent,
                 (true, true) => theme.get().accent.with_alpha(PILL_HOVER_OPACITY),
                 (false, true) => theme.get().text.with_alpha(SELECTED_TINT),
                 (false, false) => Color::TRANSPARENT,
             }}
             on_hover={move |inside| {
                 hovered.set_if_changed(inside);
             }}
             on_click={move |event| {
                 event.stop_propagation();
                 on_click.call();
             }}>
            <text no_wrap color={move || if primary { theme.get().accent_foreground } else { theme.get().text }}>{label}</text>
        </div>
    }
    .into_any()
}

fn changed_files(thread: Thread) -> Option<usize> {
    thread.changed_files().filter(|&files| files > 0)
}

/// Shown in the composer bar while a new thread's agent starts, so the layout is already the one it will keep.
fn connecting_message() -> impl View {
    let muted = Role::TextMuted;
    view! {
        <div test_id="new-thread-connecting" flex_none flex items_center gap=8.0>
            {wisp(ICON_SIZE_SMALL, muted, || WispState::Connecting)}
            <text body muted>"Connecting…"</text>
        </div>
    }
}

fn unreachable(thread: Thread) -> Option<ThreadStatus> {
    let status = thread.status();
    let has_items = thread.with_core(|core| !core.conversation().items().is_empty()).unwrap_or(false);
    (status.problem().is_some() && has_items).then_some(status)
}

const DROP_TINT: f32 = 0.08;
const SELECTED_TINT: f32 = 0.12;
const DOCK_EDGE_SOLID: f32 = 40.0;
const DOCK_EDGE_REACH: f32 = 40.0;
const CORNER_DOT_SIZE: f32 = 8.0;
const PILL_HOVER_OPACITY: f32 = 0.85;
const NEW_THREAD_HEADING: &str = "What do you want to build?";
/// Room above a fresh thread's composer for the tall flames off its top edge; below it they only lick.
const HEADING_GAP: f32 = 56.0;
/// The composer sits this far below the top of its own bounds; the fire rings its glass, not the margin.
const COMPOSER_MARGIN: f32 = 8.0;
