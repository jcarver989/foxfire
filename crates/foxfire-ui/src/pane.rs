use crate::platform::ChatPlatform;
use crate::thread::Thread;
use crate::ui::popping;
use crate::ui::tokens::Space;
use crate::{ChatRoom, Composer, ElicitationCard, SessionOptions, StatusLine, Transcript, reconnect_banner};
use agent_client_protocol::v2::schema::ToolCallLocation;
use rouge::core::{Rect, Styled, View};
use rouge::reactive::{Memo, Signal, memo, signal};
use std::rc::Rc;

/// The parts of a conversation and what connects them, for a host to lay out. Each part is drawn at most once.
#[derive(Clone, Copy)]
pub struct ChatPane {
    thread: Thread,
    stage: Signal<Rect>,
    dock: Signal<Rect>,
    composer_bounds: Signal<Rect>,
    transcript: Transcript,
    composer: Composer,
    room: ChatRoom,
    options: SessionOptions,
    status: StatusLine,
    elicitation: ElicitationCard,
}

impl ChatPane {
    /// Creates a pane with host callbacks for workspace locations and artifact review.
    pub fn new(
        thread: Thread,
        platform: Rc<dyn ChatPlatform>,
        open_location: impl Fn(ToolCallLocation) + 'static,
        open_review: impl Fn() + 'static,
    ) -> Self {
        let (dock, stage) = (signal(Rect::ZERO), signal(Rect::ZERO));
        let room = ChatRoom::new(thread, platform.clone());
        let composer = Composer::new(thread, platform.clone(), room);
        let transcript = Transcript::new(thread, platform.clone(), room, dock, stage, open_location, move |typed| {
            composer.type_text(&typed);
        });
        Self {
            thread,
            stage,
            dock,
            composer_bounds: signal(Rect::ZERO),
            transcript,
            options: SessionOptions::new(thread),
            status: StatusLine::new(thread),
            elicitation: ElicitationCard::new(thread, platform, open_review),
            composer,
            room,
        }
    }

    pub fn thread(self) -> Thread {
        self.thread
    }

    /// Where the host draws the conversation, which it tracks the bounds of.
    pub fn stage(self) -> Signal<Rect> {
        self.stage
    }

    /// Where the host draws whatever covers the end of the transcript, which it tracks the bounds of.
    pub fn dock(self) -> Signal<Rect> {
        self.dock
    }

    /// Where the host draws the composer, which it tracks the bounds of.
    pub fn composer_bounds(self) -> Signal<Rect> {
        self.composer_bounds
    }

    /// The columns the transcript reads in, for whatever lines up with it.
    pub fn reading(self) -> Memo<u16> {
        self.transcript.reading()
    }

    /// The width of the transcript's scrollbar.
    pub fn gutter(self) -> Memo<f32> {
        self.transcript.gutter()
    }

    pub fn composer(self) -> Composer {
        self.composer
    }

    pub fn room(self) -> ChatRoom {
        self.room
    }

    pub fn transcript(self) -> impl View {
        self.transcript.view()
    }

    /// The transcript under a header that takes space of its own, instead of floating over it.
    pub fn transcript_below_header(self) -> impl View {
        self.transcript.below_header().view()
    }

    /// The session's settings, such as its model.
    pub fn options(self) -> impl View {
        self.options.view()
    }

    /// What the agent is doing, its plan and its usage.
    pub fn status(self) -> impl View {
        self.status.view()
    }

    /// What the agent is asking, such as for a permission.
    pub fn elicitation(self) -> impl View {
        self.elicitation.view()
    }

    /// Offers to reconnect while a transcript is shown that its agent has gone from.
    pub fn reconnect_banner(self) -> impl View {
        let thread = self.thread;
        let unreachable = memo(move || {
            let status = thread.status();
            let has_items = thread.with_core(|core| !core.conversation().items().is_empty()).unwrap_or(false);
            (status.has_problem() && has_items).then_some(status)
        });
        popping(unreachable, move |status| reconnect_banner(thread, status).map(|banner| banner.mt(Space::X2)))
    }
}
