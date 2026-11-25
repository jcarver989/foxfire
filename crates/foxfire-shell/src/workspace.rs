use crate::conversation::{dock, fire};
use agent_client_protocol::v2::schema as acp;
use clankerdiff_core::RepoPath;
use foxfire_ui::platform::ChatPlatform;
use foxfire_ui::thread::{Thread, ThreadEvent, ThreadStatus};
use foxfire_ui::ui::tokens::{GROUP_HEIGHT, HEADER_HEIGHT, Radius, Space, UNREAD_DOT_SIZE};
use foxfire_ui::ui::{
    ButtonVariant, ControlSize, IconName, Material, MaterialExt, NoticeTone, Toasts, UiStyles, button, popping,
    segment, segmented_control, toast,
};
use foxfire_ui::{ArtifactReview, ChatPane, Files};
use rouge::core::{AnyView, Color, Div, Motion, Role, Styled, View, div, dynamic, motion, show, text, view};
use rouge::reactive::{Callback, Signal, effect, memo, signal, untrack};
use std::rc::Rc;

/// A thread's conversation, files and artifact review, and which of them is shown.
#[derive(Clone, Copy)]
pub struct Workspace {
    thread: Thread,
    tab: Signal<ThreadTab>,
    changes_seen: Signal<Option<usize>>,
    chat: ChatPane,
    files: Files,
    review: ArtifactReview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadTab {
    Chat,
    Files,
    Review,
}

/// Whether the host's header floats over a workspace or takes space of its own above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Header {
    Floating,
    Above,
}

impl Workspace {
    pub fn new(thread: Thread, platform: Rc<dyn ChatPlatform>) -> Self {
        let tab = signal(ThreadTab::Chat);
        let changes_seen = signal(None);
        let show = move |next: ThreadTab| show_tab(thread, tab, changes_seen, next);
        let toasts = Toasts::current();
        let files = Files::new(
            thread,
            move || show(ThreadTab::Chat),
            move || toasts.show(toast("Review copied").tone(NoticeTone::Positive)),
        );
        let review = ArtifactReview::new(thread, platform.clone(), move || {
            if tab.get_untracked() == ThreadTab::Review {
                show(ThreadTab::Chat);
            }
        });
        let chat = ChatPane::new(
            thread,
            platform,
            move |location| open_location(thread, &files, tab, changes_seen, &location),
            move || show(ThreadTab::Review),
        );
        thread.on_event(move |event| {
            if event == ThreadEvent::ReviewSubmitted {
                show(ThreadTab::Chat);
            }
        });
        let changed = memo(move || changed_files(thread));
        effect(move || {
            changed.get();
            untrack(|| see_changes(thread, tab, changes_seen));
        });
        Self { thread, tab, changes_seen, chat, files, review }
    }

    pub fn chat(self) -> ChatPane {
        self.chat
    }

    pub fn files(self) -> Files {
        self.files
    }

    pub fn tab(self) -> ThreadTab {
        self.tab.get()
    }

    pub fn show(self, tab: ThreadTab) {
        show_tab(self.thread, self.tab, self.changes_seen, tab);
    }

    pub fn open_file(self, path: RepoPath, line: Option<u32>) {
        self.show(ThreadTab::Files);
        self.files.open(path, line);
    }

    /// What the chosen tab shows, with a fresh thread's fire and the dock around its conversation.
    pub fn view<T: View>(self, header: Header, dock: impl Fn(Motion) -> T + 'static) -> Div {
        let thread = self.thread;
        let reveal = motion(1.0);
        let chat = memo(move || self.tab.get() == ThreadTab::Chat);
        let fresh = memo(move || chat.get() && thread.is_fresh());
        let shown = memo(move || self.tab.get());
        let content = dynamic(move || match (shown.get(), header) {
            (ThreadTab::Chat, Header::Floating) => untrack(|| self.chat.transcript()).into_any(),
            (ThreadTab::Chat, Header::Above) => untrack(|| self.chat.transcript_below_header()).into_any(),
            (ThreadTab::Files, _) => below(header, untrack(|| self.files.view())),
            (ThreadTab::Review, _) => below(header, untrack(|| self.review.view())),
        });
        view! {
            <div test_id="conversation-surface" relative size_full min_h=0.0 track_bounds={self.chat.stage()}>
                {content}
                {self.review_pill()}
                <show when={move || fresh.get()}>{fire(&self.chat, reveal)}</show>
                <show when={move || chat.get()}>{dock(reveal)}</show>
            </div>
        }
    }

    /// The dock without any host-specific controls.
    pub fn dock(self, reveal: Motion) -> Div {
        dock(&self.chat, reveal, None::<AnyView>, None::<AnyView>)
    }

    /// Chooses what is shown, and marks what happened out of sight.
    pub fn switch(self) -> impl View {
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

fn below(header: Header, content: impl View) -> AnyView {
    let top = if header == Header::Floating { HEADER_HEIGHT } else { 0.0 };
    view! { <div size_full pt=top>{content}</div> }.into_any()
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
