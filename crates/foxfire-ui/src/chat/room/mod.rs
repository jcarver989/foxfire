mod composer;
mod row;

pub(crate) use row::ask_summary;

use crate::platform::ChatPlatform;
use crate::thread::Thread;
use crate::ui::tokens::{GROUP_HEIGHT, HAIRLINE, HEADER_HEIGHT, ROW_HEIGHT, Radius, Space, UNREAD_DOT_SIZE};
use crate::ui::{
    ButtonVariant, ControlSize, Detent, IconName, Material, MaterialExt, UiStyles, Wording, button, icon_button,
    people_avatars, sheet,
};
use foxfire_acp::chat::{ChatChannelAnchor, ChatChannelPostParams};
use foxfire_core::session_core::{ChatChannel, SessionCore};
use rouge::core::{
    Align, AnyView, FocusHandle, Role, Styled, View, VirtualListHandle, div, dynamic, focus_handle, presentation, show,
    text, view, virtual_list, virtual_list_handle, window_focused,
};
use rouge::reactive::{Memo, Signal, Stored, batch, effect, memo, signal, stored, untrack};
use row::rows;
use std::collections::BTreeSet;
use std::mem::{replace, take};
use std::ops::Range;
use std::rc::Rc;
use web_time::{SystemTime, UNIX_EPOCH};

/// The side room where the people in a thread talk, and hand messages to its agent and back.
#[derive(Clone, Copy)]
pub struct ChatRoom {
    thread: Thread,
    platform: Stored<Rc<dyn ChatPlatform>>,
    shared: Memo<bool>,
    open: Signal<bool>,
    chosen: Stored<bool>,
    focus_on_mount: Stored<bool>,
    selected: Signal<BTreeSet<i64>>,
    highlighted: Signal<Option<i64>>,
    anchor: Signal<Option<ChatChannelAnchor>>,
    /// The messages the next prompt is sent with.
    pub(crate) attached: Signal<Vec<i64>>,
    /// The transcript item someone asked to see.
    pub(crate) sought: Signal<Option<String>>,
    draft: Signal<String>,
    selection: Signal<Range<usize>>,
    focus: FocusHandle,
    list: VirtualListHandle,
    following: Signal<bool>,
}

impl ChatRoom {
    pub(crate) fn new(thread: Thread, platform: Rc<dyn ChatPlatform>) -> Self {
        let room = Self {
            thread,
            platform: stored(platform),
            shared: memo(move || thread.with_chat(|_| ()).is_some()),
            open: signal(false),
            chosen: stored(false),
            focus_on_mount: stored(false),
            selected: signal(BTreeSet::new()),
            highlighted: signal(None),
            anchor: signal(None),
            attached: signal(Vec::new()),
            sought: signal(None),
            draft: signal(String::new()),
            selection: signal(0..0),
            focus: focus_handle(),
            list: virtual_list_handle(),
            following: signal(true),
        };
        let touch = presentation().get_untracked().is_touch();
        let crowded = memo(move || thread.with_chat(|chat| !chat.others().is_empty()) == Some(true));
        effect(move || {
            if crowded.get() && !touch && !room.chosen.update(|chosen| replace(chosen, true)) {
                room.open.set(true);
            }
        });
        let session = memo(move || thread.with_core(|core| core.session_id().clone()));
        effect(move || {
            session.get();
            batch(|| {
                room.selected.update(BTreeSet::clear);
                room.attached.update(Vec::clear);
                room.highlighted.set(None);
                room.anchor.set(None);
            });
        });
        let (unread, focused) = (room.unread(), window_focused());
        effect(move || {
            if unread.get() > 0 && room.is_open() && focused.get() && room.following.get() {
                untrack(|| thread.update_core(SessionCore::mark_chat_read));
            }
        });
        room
    }

    pub fn is_open(self) -> bool {
        self.shared.get() && self.open.get()
    }

    /// Whether the thread is shared with other people, so its UI shows who does what.
    pub fn is_shared(self) -> bool {
        self.shared.get()
    }

    pub fn toggle(self) {
        if self.open.get_untracked() {
            self.close();
        } else if self.shared.get_untracked() {
            self.reveal();
        }
    }

    pub(crate) fn discuss(self, anchor: ChatChannelAnchor) {
        self.anchor.set(Some(anchor));
        self.reveal();
    }

    pub(crate) fn show_message(self, seq: i64) {
        let found = self.thread.with_core(|core| rows(core).iter().position(|row| row.is_message(seq))).flatten();
        self.chosen.set(true);
        batch(|| {
            self.highlighted.set(Some(seq));
            self.following.set(false);
            self.open.set(true);
        });
        if let Some(index) = found {
            self.list.scroll_to_row(index, Align::Center);
        }
    }

    /// Who is in the thread and the room's toggle, for the thread's header.
    pub fn header_controls(self) -> impl View {
        show(
            move || self.shared.get(),
            move || {
                view! {
                    <div flex_none flex items_center gap=Space::X2>
                        <div test_id="thread-presence" h=GROUP_HEIGHT px=Space::X2 flex items_center material=Material::GlassControl>
                            {self.present()}
                        </div>
                        <div flex_none h=GROUP_HEIGHT px=Space::X1 flex items_center material=Material::GlassControl>
                            {self.toggle_button(IconName::PanelRight)}
                        </div>
                    </div>
                }
            },
        )
    }

    pub fn toggle_button(self, icon: IconName) -> impl View {
        let unread = self.unread();
        view! {
            <div relative flex_none flex items_center>
                <icon_button test_id="toggle-chat-room" icon label={Wording::current().room} selected={move || self.open.get()}
                             on_click={move || self.toggle()}/>
                <show when={move || !self.open.get() && unread.get() > 0}>
                    <div test_id="chat-room-unread" absolute top=Space::X1 right=Space::X1 size=UNREAD_DOT_SIZE
                         rounded=Radius::Full bg=Role::Accent pass_through/>
                </show>
            </div>
        }
    }

    /// The room as a sheet, for a host without space beside the conversation.
    pub fn sheet(self) -> impl View {
        show(
            move || self.shared.get(),
            move || {
                sheet(self.open, move || self.panel())
                    .detents([Detent::Medium, Detent::Large])
                    .test_id("chat-room-sheet")
            },
        )
    }

    pub fn panel(self) -> impl View {
        view! {
            <div test_id="chat-room-panel" size_full flex col bg=Role::Chrome>
                <div flex_none h=HEADER_HEIGHT pl=Space::X4 pr=Space::X3 flex items_center gap=Space::X2
                     border_b=HAIRLINE border_color=Role::BorderSubtle>
                    <text body_strong primary no_wrap>{Wording::current().room}</text>
                    <div flex_1/>
                    {self.present()}
                    <icon_button test_id="close-chat-room" icon=IconName::X label="Close panel"
                                 on_click={move || self.close()}/>
                </div>
                {self.messages()}
                <dynamic>{self.selection_bar()}</dynamic>
                {self.composer()}
            </div>
        }
    }

    fn messages(self) -> impl View {
        let thread = self.thread;
        let wording = Wording::current();
        let rows = memo(move || thread.with_core(rows).unwrap_or_default());
        let list = virtual_list(
            move || rows.with(Vec::len),
            ROW_HEIGHT,
            move |index| untrack(|| rows.with(|rows| rows.get(index).map(|row| row.view(&self)))),
        )
        .row_identity(move |index| rows.with(|rows| rows[index].key()))
        .measure_rows(|| 0)
        .handle(self.list)
        .follow_end(self.following)
        .size_full();
        view! {
            <div test_id="chat-room-messages" relative flex_1 min_h=0.0 w_full py=Space::X2>
                {list}
                <show when={move || rows.with(Vec::is_empty)}>
                    <div absolute inset=0.0 py=Space::X6 px=Space::X4 flex justify_center pass_through>
                        <text small muted>{wording.room_detail}</text>
                    </div>
                </show>
            </div>
        }
    }

    fn selection_bar(self) -> Option<AnyView> {
        let count = self.selected.with(BTreeSet::len);
        let agent = self.thread.with_core(|core| core.session().agent_name().to_owned()).unwrap_or_default();
        let ask = move || {
            self.attached.set(self.selected.try_update(take).into_iter().flatten().collect());
        };
        (count > 0).then(|| {
            view! {
                <div test_id="chat-room-selection-bar" flex_none pl=Space::X4 pr=Space::X2 py=Space::X2
                     flex items_center gap=Space::X2 bg=Role::Canvas border_t=HAIRLINE border_color=Role::BorderSubtle>
                    <text small secondary no_wrap>{format!("{count} selected")}</text>
                    <div flex_1/>
                    <button test_id="chat-room-clear-selection" label="Clear" variant=ButtonVariant::Ghost
                            control_size=ControlSize::Small on_click={move || self.selected.update(BTreeSet::clear)}/>
                    <button test_id="chat-room-ask-agent" label={format!("Ask {agent}")} variant=ButtonVariant::Primary
                            control_size=ControlSize::Small on_click=ask/>
                </div>
            }
            .into_any()
        })
    }

    fn present(self) -> impl View {
        let thread = self.thread;
        let present = memo(move || thread.with_chat(ChatChannel::present).unwrap_or_default());
        dynamic(move || present.with(|present| people_avatars(present)))
    }

    fn unread(self) -> Memo<usize> {
        let thread = self.thread;
        memo(move || thread.with_chat(ChatChannel::unread).unwrap_or(0))
    }

    fn reveal(self) {
        self.chosen.set(true);
        if self.open.get_untracked() {
            self.focus.focus();
        } else {
            self.focus_on_mount.set(true);
            batch(|| {
                self.following.set(true);
                self.open.set(true);
            });
        }
    }

    fn close(self) {
        self.chosen.set(true);
        self.open.set(false);
    }

    fn toggle_selected(self, seq: i64) {
        self.highlighted.set(None);
        self.selected.update(|selected| {
            if !selected.remove(&seq) {
                selected.insert(seq);
            }
        });
    }

    /// Dismisses what is picked out in the room, and whether there was anything.
    fn dismiss(self) -> bool {
        let selected = self.selected.try_update(take).unwrap_or_default();
        !selected.is_empty() || self.anchor.try_update(Option::take).flatten().is_some()
    }

    fn send(self) {
        let body = self.draft.get_untracked();
        if body.trim().is_empty() {
            return;
        }
        let sent = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        self.thread.update_core(|core| {
            core.post_chat(ChatChannelPostParams {
                session_id: core.session_id().clone(),
                key: sent.as_nanos().to_string(),
                body,
                anchor: self.anchor.get_untracked(),
                after_item: core
                    .conversation()
                    .items()
                    .iter()
                    .rev()
                    .find_map(|item| item.protocol_id())
                    .map(str::to_owned),
            });
        });
        batch(|| {
            self.draft.set(String::new());
            self.selection.set(0..0);
            self.anchor.set(None);
            self.following.set(true);
        });
    }
}
