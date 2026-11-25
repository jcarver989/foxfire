use super::ChatRoom;
use super::row::quote;
use crate::chat::composer::{Face, round_button};
use crate::ui::tokens::{HAIRLINE, Radius, Space};
use crate::ui::typography::BODY;
use crate::ui::{FontRole, IconName, Material, MaterialExt, format_names, icon_button};
use foxfire_core::session_core::ChatChannel;
use rouge::core::{Apply, Cursor, Editable, Key, Role, Styled, View, div, dynamic, editable, presentation, view};
use rouge::reactive::memo;
use std::mem::take;

impl ChatRoom {
    pub(super) fn composer(self) -> impl View {
        let (thread, touch) = (self.thread, presentation().get_untracked().is_touch());
        let placeholder = memo(move || {
            format!("Message {}…", format_names(&thread.with_chat(ChatChannel::others).unwrap_or_default()))
        });
        let empty = memo(move || self.draft.with(|draft| draft.trim().is_empty()));
        let anchor = dynamic(move || {
            let anchor = self.anchor.get()?;
            Some(view! {
                <div test_id="chat-room-anchor-chip" mx=Space::X2 mt=Space::X2 rounded=Radius::Inner
                     bg=Role::SurfaceHover flex items_center min_w=0.0>
                    {quote(&anchor.excerpt).flex_1()}
                    <icon_button test_id="chat-room-clear-anchor" icon=IconName::X label="Remove quote"
                                 on_click={move || self.anchor.set(None)}/>
                </div>
            })
        });
        let send = dynamic(move || {
            let face = if empty.get() { Face::Idle } else { Face::Filled };
            round_button("chat-room-send", IconName::ArrowUp, ("Send", "enter"), face, touch, move || self.send())
        });
        let field = editable(self.draft)
            .test_id("chat-room-input")
            .multiline()
            .min_rows(1)
            .max_rows(6)
            .submit_on_enter(!touch)
            .selection(self.selection)
            .track_focus(self.focus)
            .when(self.focus_on_mount.update(take) && !touch, Editable::autofocus)
            .font(FontRole::Ui)
            .text_size(BODY.size)
            .line_height(BODY.line_height)
            .color(Role::Text)
            .placeholder(move || placeholder.get(), Role::TextMuted)
            .w_full()
            .on_key_down(move |event| {
                if event.key == Key::Escape && self.dismiss() {
                    event.stop_propagation();
                }
            })
            .on_submit(move |_| self.send());
        view! {
            <div flex_none p=Space::X2>
                <div test_id="chat-room-composer" w_full flex col material=Material::Surface bg=Role::Canvas
                     border=HAIRLINE border_color=Role::BorderSubtle rounded=Radius::Control>
                    {anchor}
                    <div w_full min_w=0.0 px=Space::X3 pt=Space::X2 cursor=Cursor::Text>{field}</div>
                    <div px=Space::X2 pb=Space::X2 flex justify_end preserve_focus>{send}</div>
                </div>
            </div>
        }
    }
}
