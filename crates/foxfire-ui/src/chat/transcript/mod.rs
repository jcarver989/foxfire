mod code;
mod groups;
mod markdown;
mod message;
mod tools;

pub use message::reconnect_banner;

use super::attachment::Attachment;
use super::room::{ChatRoom, ask_summary};
use crate::platform::ChatPlatform;
use crate::thread::{Thread, ThreadStatus};
use crate::ui::tokens::{
    CONTENT_MAX_WIDTH, GRID_COLUMNS, HEADER_HEIGHT, Pace, ROW_HEIGHT, Radius, SCROLLBAR_WIDTH, Space,
};
use crate::ui::{
    ActiveReviewTheme, IconName, Material, MaterialExt, UiStyles, Wording, column_grid, empty_state, grid_cell,
    icon_button, reading_cell, reading_span, vertical_scrollbar,
};
use agent_client_protocol::v2::schema as acp;
use clankerdiff_core::DiffDocument;
use clankerdiff_rouge::{DiffViewerEvent, DiffViewerOptions, ReviewController, ReviewHandle, ReviewOutput};
use foxfire_acp::chat::ChatChannelAnchor;
use foxfire_acp::content::{first_text, prompt_title};
use foxfire_acp::conversation::{
    ConversationContent, ConversationId, ConversationItem, ConversationItemId, Notice, NoticeLevel,
};
use foxfire_core::session_core::SessionCore;
use groups::{Group, GroupKind, groups};
use message::{
    error, full_diff, image_preview, message_actions, notice, prompt, prompt_author, reconnect_button, thinking_block,
};
use rouge::core::{
    Align, AnyView, FocusHandle, KeyEvent, Rect, Role, RowKey, Styled, TextAlign, View, VirtualListHandle,
    WindowHandle, div, dynamic, focus_handle, px, show, text, view, virtual_list, virtual_list_handle, window_handle,
};
use rouge::markdown::MarkdownDocument;
use rouge::reactive::{Callback, Memo, Scope, Signal, Stored, effect, memo, signal, stored, untrack};
use rouge::widgets::MarkdownRows;
use std::borrow::Cow;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::Rc;
use std::sync::Arc;
use tools::{ToolActions, ToolCard, ToolFold, ToolPreviews, fold_header};

#[derive(Clone, Copy)]
pub struct Transcript {
    thread: Thread,
    list: VirtualListHandle,
    following: Signal<bool>,
    on_type: Callback<(String,)>,
    dock: Signal<Rect>,
    header_inset: f32,
    gutter: Memo<f32>,
    reading: Memo<u16>,
    unfolded: Signal<HashSet<Fold>>,
    preview: Signal<Option<Arc<[u8]>>>,
    full_diff: Signal<Option<ReviewHandle>>,
    attachments: Stored<HashMap<ConversationItemId, Rc<ItemAttachments>>>,
    tools: Stored<ToolPreviews>,
    focus: Stored<HashMap<ConversationItemId, FocusHandle>>,
    summary: Memo<Summary>,
    projection: Memo<Vec<ProjectedRow>>,
    on_location: Callback<(acp::ToolCallLocation,)>,
    room: ChatRoom,
    platform: Stored<Rc<dyn ChatPlatform>>,
    window: WindowHandle,
    scope: Scope,
}

type ItemAttachments = Vec<(usize, Attachment)>;

#[derive(Clone, Default, PartialEq)]
struct Summary {
    conversation: Option<ConversationId>,
    items: Vec<ConversationItemId>,
    groups: Vec<Group>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Fold {
    Item(ConversationItemId),
    Full(ConversationItemId),
    Rows(GroupKind, ConversationItemId),
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct ProjectedRow {
    index: usize,
    id: ConversationItemId,
    group: Option<Group>,
    collapsed: bool,
}

impl Summary {
    fn visible(&self, unfolded: &HashSet<Fold>) -> Vec<ProjectedRow> {
        let mut groups = self.groups.iter().peekable();
        let mut rows = Vec::new();
        let mut index = 0;
        while let Some(&id) = self.items.get(index) {
            let group = groups.peek().copied().filter(|g| g.rows.contains(&index));
            let collapsed = group.is_some_and(|g| !unfolded.contains(&Fold::Rows(g.kind, self.items[g.rows.start])));
            rows.push(ProjectedRow { index, id, group: group.cloned(), collapsed });
            index = group.filter(|_| collapsed).map_or(index + 1, |g| g.rows.end);
            if group.is_some_and(|g| index == g.rows.end) {
                groups.next();
            }
        }
        rows
    }
}

impl Transcript {
    pub fn new(
        thread: Thread,
        platform: Rc<dyn ChatPlatform>,
        room: ChatRoom,
        dock: Signal<Rect>,
        stage: Signal<Rect>,
        on_location: impl Fn(acp::ToolCallLocation) + 'static,
        on_type: impl Fn(String) + 'static,
    ) -> Self {
        let summary = memo(move || {
            thread
                .with_core(|core| {
                    let conversation = core.conversation();
                    let items = conversation.items();
                    Summary {
                        conversation: Some(conversation.id()),
                        items: items.iter().map(ConversationItem::id).collect(),
                        groups: groups(items),
                    }
                })
                .unwrap_or_default()
        });
        let unfolded: Signal<HashSet<Fold>> = signal(HashSet::new());
        let projection = memo(move || summary.with(|summary| unfolded.with(|folds| summary.visible(folds))));
        let list = virtual_list_handle();
        let scroll = list.scroll_state();
        let gutter = memo(move || {
            let overflows = scroll.with(|scroll| scroll.content.height > scroll.viewport.height + 0.5);
            if overflows { SCROLLBAR_WIDTH } else { 0.0 }
        });
        let reading = memo(move || reading_span(stage.get().size.width - gutter.get(), CONTENT_MAX_WIDTH));
        let transcript = Self {
            thread,
            list,
            following: signal(true),
            on_type: Callback::from(on_type),
            dock,
            header_inset: HEADER_HEIGHT,
            gutter,
            reading,
            unfolded,
            preview: signal(None),
            full_diff: signal(None),
            attachments: stored(HashMap::new()),
            tools: stored(ToolPreviews::default()),
            focus: stored(HashMap::new()),
            summary,
            projection,
            on_location: Callback::from(on_location),
            room,
            platform: stored(platform),
            window: window_handle(),
            scope: Scope::new(),
        };
        transcript.observe();
        transcript
    }

    fn observe(self) {
        let summary = self.summary;
        effect(move || {
            if let Some(item) = self.room.sought.get() {
                self.room.sought.set(None);
                untrack(|| self.reveal(&item));
            }
        });
        let previous: Cell<(Option<ConversationId>, usize)> = Cell::new((None, 0));
        effect(move || {
            let current = summary.with(|summary| (summary.conversation, summary.items.len()));
            let (was, had) = previous.replace(current);
            if was != current.0 || current.1 < had {
                untrack(|| {
                    self.attachments.update(HashMap::clear);
                    self.tools.update(ToolPreviews::clear);
                    self.focus.update(HashMap::clear);
                    self.unfolded.update(HashSet::clear);
                    self.full_diff.set(None);
                });
            }
        });
    }

    /// Brings the item with a protocol id into view.
    fn reveal(self, protocol_id: &str) {
        let found = self.thread.with_core(|core| {
            core.conversation().items().iter().position(|item| item.protocol_id() == Some(protocol_id))
        });
        let Some(index) = found.flatten() else {
            return;
        };
        self.following.set(false);
        let folds: Vec<Fold> = self.summary.with_untracked(|summary| {
            let groups = summary.groups.iter().filter(|group| group.rows.contains(&index));
            groups.map(|group| Fold::Rows(group.kind, summary.items[group.rows.start])).collect()
        });
        self.unfolded.update(|unfolded| unfolded.extend(folds));
        let row = self.projection.with_untracked(|rows| rows.iter().position(|row| row.index == index));
        if let Some(row) = row {
            self.list.scroll_to_row(row, Align::Center);
        }
    }

    /// The columns the reading column spans, for whatever shares the transcript's grid.
    pub fn reading(self) -> Memo<u16> {
        self.reading
    }

    /// The room the scrollbar takes beside the rows, which the grid leaves out.
    pub fn gutter(self) -> Memo<f32> {
        self.gutter
    }

    pub(crate) fn below_header(mut self) -> Self {
        self.header_inset = 0.0;
        self
    }

    pub fn view(self) -> impl View {
        let empty = memo(move || self.summary.with(|summary| summary.items.is_empty()));
        let close_preview = Callback::from(move || self.preview.set(None));
        let close_diff = Callback::from(move || self.full_diff.set(None));
        view! {
            <div test_id="transcript" relative size_full flex col>
                <div flex_1 min_h=0.0 w_full>
                    <dynamic>{if empty.get() { self.unobscured(self.empty_state()) } else { untrack(|| self.list()).into_any() }}</dynamic>
                </div>
                <dynamic>{self.preview.get().map(|bytes| image_preview(bytes, close_preview))}</dynamic>
                <dynamic>{self.full_diff.get().map(|handle| full_diff(&handle, close_diff))}</dynamic>
            </div>
        }
    }

    fn list(self) -> impl View {
        let (projection, list, dock) = (self.projection, self.list, self.dock);
        let at = move |index: usize| projection.with(|rows| rows.get(index).cloned());
        let rows = virtual_list(
            move || projection.with(|rows| rows.len() + 1),
            ROW_ESTIMATE,
            move |index| untrack(|| self.row(at(index))),
        )
        .row_identity(move |index| {
            let row = at(index);
            let folds = row.as_ref().map(|r| (self.expanded(Fold::Item(r.id)), self.expanded(Fold::Full(r.id))));
            let revision = hash(&(self.summary.with(|s| s.conversation), &row, folds));
            RowKey::new(row.map(|r| r.id), revision)
        })
        .measure_rows(|| 0)
        .row_text(move |index| at(index).filter(|r| !r.collapsed).map_or_else(String::new, |r| self.row_text(r.index)))
        .handle(list)
        .follow_end(self.following)
        .size_full();
        let following = self.following;
        let bottom_inset = move || px(dock.get().size.height);
        let (reading, gutter) = (self.reading, self.gutter);
        let jump = move || {
            let latest = move || {
                following.set(true);
                list.scroll_to_row(projection.with_untracked(Vec::len), Align::Bottom);
            };
            view! {
                <div flex_none material=Material::GlassControl>
                    <icon_button test_id="jump-to-latest" icon=IconName::ArrowDown label="Jump to latest" size=ROW_HEIGHT
                                 on_click=latest/>
                </div>
            }
        };
        view! {
            <div size_full flex>
                <div test_id="transcript-selection" relative flex_1 min_w=0.0 h_full selectable
                     on_key_down={move |event| self.type_into_composer(event)}>
                    {rows}
                    <div test_id="transcript-view" absolute left=0.0 right=0.0 top={self.header_inset}
                         bottom=bottom_inset pb=FADE_HEIGHT flex items_end pass_through>
                        <column_grid w_full pass_through>
                            <reading_cell span=reading flex justify_end pass_through>
                                <show when={move || !following.get()}>{jump()}</show>
                            </reading_cell>
                        </column_grid>
                    </div>
                </div>
                <show when={move || gutter.get() > 0.0}>
                    <div test_id="transcript-scrollbar" relative flex_none w=SCROLLBAR_WIDTH h_full>
                        {vertical_scrollbar(list.scroll_state())}
                    </div>
                </show>
            </div>
        }
    }

    fn type_into_composer(self, event: &KeyEvent) {
        let modifiers = event.modifiers;
        if modifiers.ctrl || modifiers.meta || modifiers.alt {
            return;
        }
        let Some(typed) = event.text.as_ref().filter(|typed| !typed.is_empty() && !typed.chars().any(char::is_control))
        else {
            return;
        };
        event.stop_propagation();
        self.on_type.call(typed.clone());
    }

    fn expanded(self, fold: Fold) -> bool {
        self.unfolded.with(|unfolded| unfolded.contains(&fold))
    }

    /// The selectable text of a row that is not mounted in the virtual list.
    fn row_text(self, index: usize) -> String {
        self.thread
            .with_core(|core| {
                let item = core.conversation().items().get(index)?;
                let id = item.id();
                Some(match item.content() {
                    ConversationContent::User(blocks) => prompt_text(blocks),
                    ConversationContent::Assistant(_) => {
                        let text = item.text().unwrap_or_default().into_owned();
                        MarkdownRows::new(MarkdownDocument::parse(text).blocks()).plain_text()
                    }
                    ConversationContent::Thought(_) if self.expanded(Fold::Item(id)) => {
                        item.text().unwrap_or_default().into_owned()
                    }
                    ConversationContent::Tool(tool) if self.expanded(Fold::Item(id)) => {
                        tools::details_text(tool, self.expanded(Fold::Full(id)))
                    }
                    ConversationContent::Notice(Notice { text, .. }) => text.clone(),
                    ConversationContent::Thought(_) | ConversationContent::Tool(_) => String::new(),
                })
            })
            .flatten()
            .unwrap_or_default()
    }

    fn unobscured(self, content: impl View) -> AnyView {
        let dock = self.dock;
        view! { <div size_full pt={self.header_inset} pb={move || px(dock.get().size.height)}>{content}</div> }
            .into_any()
    }

    fn row(self, projected: Option<ProjectedRow>) -> AnyView {
        let Some(ProjectedRow { index, id, group, collapsed: folded }) = projected else {
            let dock = self.dock;
            return view! { <div pt=FADE_HEIGHT><div h={move || px(dock.get().size.height)}/></div> }.into_any();
        };
        let Some((kind, header)) = self
            .thread
            .with_core(|core| {
                let items = core.conversation().items();
                let kind = kind_of(items.get(index)?);
                let header = group.as_ref().filter(|g| g.rows.start == index).and_then(|g| {
                    let fold = Fold::Rows(g.kind, id);
                    Some(fold_header(
                        g.kind,
                        index,
                        items.get(g.rows.clone())?,
                        !folded,
                        Callback::from(move || self.toggle(fold)),
                    ))
                });
                Some((kind, header))
            })
            .flatten()
        else {
            return ().into_any();
        };
        let hovered = signal(false);
        let content = (!folded).then(|| {
            let indented = group.as_ref().is_some_and(|group| group.kind == GroupKind::Run);
            let item = self.item(index, kind, hovered);
            let row = if indented { div().pl(Space::X4).child(item) } else { div().child(item) };
            row.relative().child(self.discuss(index, kind, hovered))
        });
        let turn_header = header.is_some() && group.as_ref().is_some_and(|group| group.kind == GroupKind::Turn);
        let top = if index == 0 { HEADER_HEIGHT } else { 0.0 };
        let padding = match kind {
            Kind::User => (Space::X6.px(), Space::X2.px()),
            Kind::Tool if !turn_header => (0.0, 0.0),
            _ if group.is_some() && !turn_header => (0.0, 0.0),
            _ => (Space::X2.px(), Space::X2.px()),
        };
        let reading = self.reading;
        let cell = if kind == Kind::User {
            view! {
                <grid_cell span={move || reading.get() - 1} start={move || (GRID_COLUMNS - reading.get()) / 2 + 2}>
                    {header}{content}
                </grid_cell>
            }
        } else {
            view! { <reading_cell span=reading>{header}{content}</reading_cell> }
        };
        view! {
            <div test_id={format!("transcript-row-{index}")} w_full pt={padding.0 + top} pb=padding.1
                 on_hover={move |inside| {
                     hovered.set_if_changed(inside);
                 }}>
                <column_grid>{cell}</column_grid>
            </div>
        }
        .into_any()
    }

    /// Quotes an item in the side room, which needs an id everyone there knows the item by.
    fn discuss(self, index: usize, kind: Kind, hovered: Signal<bool>) -> impl View {
        let (thread, room) = (self.thread, self.room);
        let quoted = memo(move || {
            room.is_shared().then_some(())?;
            thread.with_core(|core| quote_of(core.conversation().items().get(index)?)).flatten()
        });
        dynamic(move || {
            let anchor = quoted.get()?;
            let button = view! {
                <div absolute bg=Role::Canvas rounded=Radius::Control transition=Pace::Move unselectable
                     opacity={move || if hovered.get() { 1.0 } else { 0.0 }}>
                    <icon_button test_id={format!("discuss-{index}")} icon=IconName::MessageCircle label="Discuss"
                                 on_click={move || room.discuss(anchor.clone())}/>
                </div>
            };
            Some(match kind {
                Kind::User => button.left(0.0).bottom(0.0),
                Kind::Tool => button.right(0.0).top(0.0),
                _ => button.right(0.0).bottom(0.0),
            })
        })
    }

    fn item(self, index: usize, kind: Kind, hovered: Signal<bool>) -> AnyView {
        let thread = self.thread;
        match kind {
            Kind::User => self.user(index, hovered),
            Kind::Assistant => {
                let text = memo(move || {
                    thread
                        .with_core(|core| {
                            core.conversation().items().get(index).and_then(|item| item.text().map(Cow::into_owned))
                        })
                        .flatten()
                        .unwrap_or_default()
                });
                let open = memo(move || {
                    thread
                        .with_core(|core| core.conversation().items().get(index).is_some_and(ConversationItem::is_open))
                        .unwrap_or(false)
                });
                let (window, platform) = (self.window, self.platform);
                view! {
                    <div reading>
                        <div test_id={format!("reply-{index}")}>{markdown::reply(text, platform)}</div>
                        <dynamic>{(!open.get()).then(|| message_actions(index, text.get_untracked(), hovered, window))}</dynamic>
                    </div>
                }
                .into_any()
            }
            Kind::Thought | Kind::Tool | Kind::Notice => {
                let revision = memo(move || {
                    thread
                        .with_core(|core| core.conversation().items().get(index).map(ConversationItem::revision))
                        .flatten()
                });
                dynamic(move || {
                    revision.get();
                    untrack(|| self.settled_item(index))
                })
                .into_any()
            }
        }
    }

    fn user(self, index: usize, hovered: Signal<bool>) -> AnyView {
        let Some((id, message_id, text, blocks)) = self
            .thread
            .with_core(|core| {
                let item = core.conversation().items().get(index)?;
                let ConversationContent::User(blocks) = item.content() else {
                    return None;
                };
                Some((item.id(), item.message_id().cloned(), prompt_text(blocks), blocks.clone()))
            })
            .flatten()
        else {
            return ().into_any();
        };
        let attachments = self.attachments.update(|attachments| {
            attachments
                .entry(id)
                .or_insert_with(|| {
                    Rc::new(
                        blocks
                            .iter()
                            .enumerate()
                            .filter_map(|(block, content)| Some((block, Attachment::of(content)?)))
                            .collect(),
                    )
                })
                .clone()
        });
        let open = Callback::from(move |bytes: Arc<[u8]>| self.preview.set(Some(bytes)));
        let (thread, room) = (self.thread, self.room);
        let asked = memo(move || {
            let read = |core: &SessionCore| {
                let chat = core.chat()?;
                let ask = chat.ask(message_id.as_ref()?)?;
                Some((chat.person(&ask.author), ask_summary(core, chat, ask), ask.sources().first().copied()))
            };
            thread.with_core(read).flatten()
        });
        let author = dynamic(move || {
            let (author, summary, source) = asked.get()?;
            Some(prompt_author(
                index,
                &author,
                summary,
                source.map(|seq| Callback::from(move || room.show_message(seq))),
            ))
        });
        view! {
            <div flex col items_end>
                {author}
                {prompt(index, text.clone(), &attachments, open)}
                {message_actions(index, text, hovered, self.window)}
            </div>
        }
        .into_any()
    }

    fn settled_item(self, index: usize) -> AnyView {
        let theme = ActiveReviewTheme::current();
        let review = theme.review();
        let actions = ToolActions {
            open_diff: Callback::from(move |document: Arc<DiffDocument>| self.open_full_diff(document)),
            open_location: self.on_location,
        };
        self.thread
            .with_core(|core| {
                let item = core.conversation().items().get(index)?;
                let id = item.id();
                let expanded = self.expanded(Fold::Item(id));
                let toggle = Callback::from(move || self.toggle(Fold::Item(id)));
                Some(match item.content() {
                    ConversationContent::Thought(_) => {
                        let text = item.text().unwrap_or_default().into_owned();
                        thinking_block(text, item.is_open(), index, expanded, toggle)
                    }
                    ConversationContent::Tool(tool) => {
                        let full = Fold::Full(id);
                        let fold = match (expanded, self.expanded(full)) {
                            (false, _) => ToolFold::Folded,
                            (true, false) => ToolFold::Expanded,
                            (true, true) => ToolFold::Full,
                        };
                        let show_all = Callback::from(move || self.toggle(full));
                        let preview = self.tools.update(|tools| tools.get(item, tool));
                        let focus = self
                            .focus
                            .update(|handles| *handles.entry(id).or_insert_with(|| self.scope.run(focus_handle)));
                        let card =
                            ToolCard { review: &review, theme: theme.review_signal(), preview: &preview, actions };
                        card.render(tool, index, fold, focus, toggle, show_all)
                    }
                    ConversationContent::Notice(Notice { level: NoticeLevel::Info, text }) => notice(text),
                    ConversationContent::Notice(Notice { level: NoticeLevel::Error, text }) => error(index, text),
                    ConversationContent::User(_) | ConversationContent::Assistant(_) => ().into_any(),
                })
            })
            .flatten()
            .unwrap_or_else(|| ().into_any())
    }

    fn toggle(self, fold: Fold) {
        self.following.set(false);
        self.unfolded.update(|unfolded| {
            if !unfolded.remove(&fold) {
                unfolded.insert(fold);
            }
        });
    }

    fn open_full_diff(self, document: Arc<DiffDocument>) {
        let theme = ActiveReviewTheme::current().review();
        let options = DiffViewerOptions { themes: false, review_bar: false, ..DiffViewerOptions::default() };
        let handle = self.scope.run(|| {
            ReviewController::with_options(document, theme, options).mount(move |output| {
                if output == ReviewOutput::Event(DiffViewerEvent::Cancel) {
                    self.full_diff.set(None);
                }
            })
        });
        self.full_diff.set(Some(handle));
    }

    fn empty_state(self) -> impl View {
        let thread = self.thread;
        let wording = Wording::current();
        let state = memo(move || (thread.status(), thread.error(), thread.is_fresh()));
        dynamic(move || {
            let (status, error, fresh) = state.get();
            if fresh {
                return div().into_any();
            }
            if let Some(problem) = wording.problem(status) {
                let detail =
                    error.map_or_else(|| wording.disconnected_detail.to_owned(), |failure| failure.to_string());
                let color = if status == ThreadStatus::Errored { Role::Destructive } else { Role::Warning };
                return view! {
                    <div size_full p=Space::X6 flex col items_center justify_center gap=Space::X3>
                        <text heading color=color align=TextAlign::Center>{problem}</text>
                        <text body muted align=TextAlign::Center>{detail}</text>
                        {reconnect_button(thread)}
                    </div>
                }
                .into_any();
            }
            let (title, detail) = match status {
                ThreadStatus::Idle => return div().into_any(),
                ThreadStatus::Connecting => (wording.connecting, wording.connecting_detail),
                ThreadStatus::Dormant => (wording.stopped, wording.stopped_detail),
                _ => (wording.untitled, wording.unprompted),
            };
            view! {
                <div size_full flex items_center justify_center>
                    <empty_state icon=IconName::MessageSquareText title detail/>
                </div>
            }
            .into_any()
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    User,
    Assistant,
    Thought,
    Tool,
    Notice,
}

fn kind_of(item: &ConversationItem) -> Kind {
    match item.content() {
        ConversationContent::User(_) => Kind::User,
        ConversationContent::Assistant(_) => Kind::Assistant,
        ConversationContent::Thought(_) => Kind::Thought,
        ConversationContent::Tool(_) => Kind::Tool,
        ConversationContent::Notice(_) => Kind::Notice,
    }
}

fn quote_of(item: &ConversationItem) -> Option<ChatChannelAnchor> {
    let excerpt = match item.content() {
        ConversationContent::User(blocks) => prompt_title(first_text(blocks)),
        ConversationContent::Assistant(_) if !item.is_open() => prompt_title(item.text().as_deref()),
        ConversationContent::Tool(tool) => tools::excerpt(tool),
        _ => return None,
    };
    Some(ChatChannelAnchor { item: item.protocol_id()?.to_owned(), excerpt })
}

fn prompt_text(blocks: &[acp::ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            acp::ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect()
}

pub(crate) fn hash(value: &impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

const FADE_HEIGHT: f32 = Space::X6.px();
const ROW_ESTIMATE: f32 = Space::X12.px();
