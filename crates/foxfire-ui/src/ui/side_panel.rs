use super::tokens::UNIT;
use super::{ResizablePanel, Side, resizable_panel};
use crate::ChatRoom;
use rouge::core::{Div, Role, Styled, View, div, dynamic, view, window_size};
use rouge::reactive::{Callback, Memo, effect, signal, untrack};

/// What a panel leaves for the rest of the window.
pub const MIN_MAIN_WIDTH: f32 = 80.0 * UNIT;
/// The narrowest, usual and widest a side room can be.
const ROOM_WIDTHS: [f32; 3] = [64.0 * UNIT, 84.0 * UNIT, 120.0 * UNIT];

/// How wide a host remembers the user leaving a panel.
#[derive(Clone, Copy)]
pub struct PanelWidth {
    saved: Memo<Option<f32>>,
    set: Callback<(Option<f32>,)>,
}

/// A conversation with its side room beside it while the room is open.
pub fn beside_room(room: ChatRoom, width: PanelWidth, conversation: impl View) -> Div {
    let side = dynamic(move || {
        let panel = || width.panel(Side::Right, ROOM_WIDTHS, room.panel()).test_id("chat-room-resizable");
        room.is_open().then(|| untrack(panel))
    });
    view! {
        <div size_full flex>
            <div relative flex_1 min_w=0.0 h_full>{conversation}</div>
            {side}
        </div>
    }
}

impl PanelWidth {
    pub fn new(saved: Memo<Option<f32>>, set: impl Fn(Option<f32>) + 'static) -> Self {
        Self { saved, set: Callback::from(set) }
    }

    /// A panel at the remembered width, or the closest that leaves room for the rest of the window, given the
    /// narrowest, usual and widest it can be.
    pub fn panel(self, edge: Side, [min, default, max]: [f32; 3], content: impl View) -> ResizablePanel {
        let window = window_size();
        let fit = move |width: f32, available: f32| {
            let max = (available - MIN_MAIN_WIDTH).clamp(min, max);
            (width.clamp(min, max) / UNIT).floor() * UNIT
        };
        let width = signal(fit(self.saved.get_untracked().unwrap_or(default), window.get_untracked().width));
        effect(move || {
            width.set_if_changed(fit(self.saved.get().unwrap_or(default), window.get().width));
        });
        let on_resize = move |requested| {
            let chosen = fit(requested, window.get_untracked().width);
            if self.saved.get_untracked() != Some(chosen) {
                self.set.call(Some(chosen));
            }
            width.set_if_changed(chosen);
        };
        let on_reset = move || self.set.call(None);
        view! {
            <resizable_panel edge size=width min_size=min max_size=max reset_size=default on_resize=on_resize
                             on_reset=on_reset h_full flex col bg=Role::Chrome>
                {content}
            </resizable_panel>
        }
    }
}
