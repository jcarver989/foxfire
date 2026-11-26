use super::ChatRoom;
use crate::chat::transcript::hash;
use crate::ui::tokens::{AVATAR_SIZE_SMALL, ICON_SIZE_SMALL, Pace, Radius, Space, TINT_ALPHA};
use crate::ui::{
    ActivityState, ButtonVariant, ControlSize, IconName, UiStyles, activity, button, markdown, person_avatar,
    person_color,
};
use chrono::{DateTime, Local};
use foxfire_acp::chat::{ChatChannelAnchor, ChatMessage, ChatMessageKind, Participant};
use foxfire_acp::content::prompt_title;
use foxfire_core::session_core::{ChatChannel, Earlier, PendingPost, SessionCore};
use rouge::core::{AnyView, Color, Cursor, Div, Role, RowKey, Styled, Theme, View, div, dynamic, show, text, view};
use rouge::markdown::MarkdownDocument;
use rouge::reactive::{memo, signal};

#[derive(Clone, PartialEq)]
pub(super) struct Row {
    id: String,
    author: Participant,
    grouped: bool,
    content: Content,
}

#[derive(Clone, PartialEq)]
enum Content {
    Earlier,
    Ask { summary: String, prompt: String },
    Post(ChatMessage, Option<ChatChannelAnchor>),
    Pending(PendingPost),
}

pub(super) fn rows(core: &SessionCore) -> Vec<Row> {
    let Some(chat) = core.chat() else {
        return Vec::new();
    };
    let nobody = Participant { id: String::new(), name: String::new() };
    let earlier = (chat.earlier != Earlier::None).then(|| Row {
        id: "earlier".to_owned(),
        author: nobody,
        grouped: false,
        content: Content::Earlier,
    });
    let mut previous: Option<&ChatMessage> = None;
    let messages = chat.messages.values().map(|message| {
        let content = match &message.kind {
            ChatMessageKind::Post { anchor, .. } => Content::Post(message.clone(), anchor.clone()),
            ChatMessageKind::Ask { message_id, .. } => {
                let asked = ask_summary(core, chat, message);
                let body = message.body.trim();
                let summary = if body.is_empty() { asked } else { format!("{asked}: {}", prompt_title(Some(body))) };
                Content::Ask { summary, prompt: message_id.to_string() }
            }
        };
        let grouped = previous.replace(message).is_some_and(|previous| {
            matches!(previous.kind, ChatMessageKind::Post { .. })
                && previous.author == message.author
                && message.sent_at_ms - previous.sent_at_ms < GROUP_WINDOW_MS
        });
        Row { id: message.seq.to_string(), author: known(chat, &message.author), grouped, content }
    });
    let pending = chat.pending.iter().map(|pending| Row {
        id: pending.post.key.clone(),
        author: known(chat, &chat.me),
        grouped: false,
        content: Content::Pending(pending.clone()),
    });
    earlier.into_iter().chain(messages).chain(pending).collect()
}

/// Who asked the agent, and with how many messages.
pub(crate) fn ask_summary(core: &SessionCore, chat: &ChatChannel, ask: &ChatMessage) -> String {
    let (who, agent) = (known(chat, &ask.author).name, core.session().agent_name());
    match ask.sources().len() {
        0 => format!("{who} asked {agent}"),
        1 => format!("{who} asked {agent} with 1 message"),
        count => format!("{who} asked {agent} with {count} messages"),
    }
}

impl Row {
    pub(super) fn key(&self) -> RowKey<String> {
        let error = match &self.content {
            Content::Pending(pending) => pending.error.as_deref(),
            _ => None,
        };
        RowKey::new(self.id.clone(), hash(&(self.grouped, &self.author.name, error)))
    }

    pub(super) fn is_message(&self, seq: i64) -> bool {
        matches!(&self.content, Content::Post(message, _) if message.seq == seq)
    }

    pub(super) fn view(&self, room: &ChatRoom) -> AnyView {
        let row = match &self.content {
            Content::Earlier => earlier(room).into_any(),
            Content::Ask { summary, prompt } => ask(room, &self.id, summary, prompt).into_any(),
            Content::Post(message, anchor) => self.post(room, message, anchor.as_ref()).into_any(),
            Content::Pending(pending) => self.pending(room, pending).into_any(),
        };
        view! { <div w_full px=Space::X2 pb=Space::X1>{row}</div> }.into_any()
    }

    fn post(&self, room: &ChatRoom, message: &ChatMessage, anchor: Option<&ChatChannelAnchor>) -> Div {
        let room = *room;
        let (seq, theme, hovered) = (message.seq, Theme::current(), signal(false));
        let selected = move || room.selected.with(|selected| selected.contains(&seq));
        let quote = anchor.map(|anchor| {
            let item = anchor.item.clone();
            quote(&anchor.excerpt)
                .test_id(format!("chat-anchor-{seq}"))
                .rounded(Radius::Inner)
                .bg(Role::SurfaceHover)
                .cursor(Cursor::Pointer)
                .on_click(move |event| {
                    event.stop_propagation();
                    room.sought.set(Some(item.clone()));
                })
        });
        let select = dynamic(move || {
            let (label, icon) = if selected() { ("Remove", IconName::X) } else { ("Select", IconName::Plus) };
            view! {
                <button test_id={format!("chat-select-{seq}")} label icon=icon variant=ButtonVariant::Secondary
                        control_size=ControlSize::Small on_click={move || room.toggle_selected(seq)}/>
            }
        });
        let background = move || {
            if selected() || room.highlighted.get() == Some(seq) {
                theme.get().accent.with_alpha(TINT_ALPHA)
            } else if hovered.get() {
                theme.get().surface_hover
            } else {
                Color::TRANSPARENT
            }
        };
        let time = DateTime::from_timestamp_millis(message.sent_at_ms)
            .map_or_default(|at| at.with_timezone(&Local).format("%H:%M").to_string());
        let picking = move || room.selected.with(|selected| !selected.is_empty());
        let body = view! { <div flex col gap=Space::X1>{quote}{body(&room, &message.body)}</div> };
        self.bubble((!self.grouped).then_some(time), body)
            .test_id(format!("chat-post-{seq}"))
            .bg(background)
            .cursor(move || if picking() { Cursor::Pointer } else { Cursor::Default })
            .on_hover(move |inside| {
                hovered.set_if_changed(inside);
            })
            .on_click(move |_| {
                if picking() {
                    room.toggle_selected(seq);
                }
            })
            .child(view! {
                <show when=selected>
                    <div absolute left=0.0 top=Space::X1 bottom=Space::X1 w=SELECTED_BAR_WIDTH rounded=Radius::Inner
                         bg=Role::Accent/>
                </show>
            })
            .child(view! {
                <div absolute top=Space::X1 right=Space::X1 transition=Pace::Move
                     opacity={move || if hovered.get() || selected() { 1.0 } else { 0.0 }}>{select}</div>
            })
    }

    fn pending(&self, room: &ChatRoom, pending: &PendingPost) -> Div {
        let room = *room;
        let (key, post) = (pending.post.key.clone(), pending.post.clone());
        let failure = pending.error.as_ref().map(|error| {
            let retry = move || {
                room.thread.update_core(|core| {
                    core.post_chat(post.clone());
                });
            };
            let dismiss = move || room.thread.update_core(|core| core.dismiss_chat(&key));
            view! {
                <div flex items_center gap=Space::X2 flex_wrap>
                    <text caption color=Role::Destructive>{format!("Failed: {error}")}</text>
                    <button test_id="chat-retry" label="Retry" variant=ButtonVariant::Ghost
                            control_size=ControlSize::Small on_click=retry/>
                    <button test_id="chat-dismiss" label="Dismiss" variant=ButtonVariant::Ghost
                            control_size=ControlSize::Small on_click=dismiss/>
                </div>
            }
        });
        let body = view! { <div flex col gap=Space::X1>{body(&room, &pending.post.body)}{failure}</div> };
        let status = if pending.error.is_some() { "" } else { "Sending…" };
        self.bubble(Some(status.to_owned()), body).opacity(PENDING_OPACITY)
    }

    /// A message beside its author, who is named unless `time` is left out.
    fn bubble(&self, time: Option<String>, body: impl View) -> Div {
        let (theme, id) = (Theme::current(), self.author.id.clone());
        let avatar = time.is_some().then(|| person_avatar(&self.author));
        let header = time.map(|time| {
            view! {
                <div flex items_center gap=Space::X2 min_w=0.0>
                    <text body_strong color={move || person_color(&id, theme)} no_wrap>{self.author.name.clone()}</text>
                    <text caption muted no_wrap>{time}</text>
                </div>
            }
        });
        view! {
            <div relative w_full px=Space::X2 py=Space::X1 rounded=Radius::Control flex items_start gap=Space::X2>
                <div flex_none w=AVATAR_SIZE_SMALL pt=Space::X1>{avatar}</div>
                <div flex_1 min_w=0.0 flex col selectable>{header}{body}</div>
            </div>
        }
    }
}

/// An excerpt of the transcript, marked as the agent's.
pub(super) fn quote(excerpt: &str) -> Div {
    view! {
        <div px=Space::X2 py=Space::X1 flex items_center gap=Space::X1 min_w=0.0>
            {activity(ICON_SIZE_SMALL, Role::Accent, || ActivityState::Waiting)}
            <div flex_1 min_w=0.0 overflow_hidden><text small secondary no_wrap>{prompt_title(Some(excerpt))}</text></div>
        </div>
    }
}

fn earlier(room: &ChatRoom) -> Div {
    let thread = room.thread;
    let loading = memo(move || thread.with_chat(|chat| chat.earlier == Earlier::Loading) == Some(true));
    view! {
        <div flex justify_center>
            <button test_id="chat-room-load-more" label="Load earlier messages" variant=ButtonVariant::Ghost
                    control_size=ControlSize::Small disabled={move || loading.get()}
                    on_click={move || thread.update_core(SessionCore::load_earlier_chat)}/>
        </div>
    }
}

fn ask(room: &ChatRoom, id: &str, summary: &str, prompt: &str) -> Div {
    let room = *room;
    let prompt = prompt.to_owned();
    quote(summary)
        .test_id(format!("chat-ask-{id}"))
        .rounded(Radius::Inner)
        .cursor(Cursor::Pointer)
        .hover(|style| style.bg(Role::SurfaceHover))
        .on_click(move |_| room.sought.set(Some(prompt.clone())))
}

fn body(room: &ChatRoom, text: &str) -> impl View + use<> {
    let platform = room.platform;
    // Markdown would otherwise join the lines of a message.
    markdown(MarkdownDocument::parse(text.replace('\n', "  \n")))
        .compact()
        .on_link(move |url: String| platform.with(|platform| platform.open(&url)))
}

fn known(chat: &ChatChannel, id: &str) -> Participant {
    let person = chat.person(id);
    if id == chat.me { Participant { name: "You".to_owned(), ..person } } else { person }
}

const GROUP_WINDOW_MS: i64 = 5 * 60 * 1000;
const SELECTED_BAR_WIDTH: f32 = 2.0;
const PENDING_OPACITY: f32 = 0.7;
