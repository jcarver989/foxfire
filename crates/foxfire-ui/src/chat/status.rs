use crate::thread::{Thread, ThreadStatus};
use crate::ui::spawn_in;
use crate::ui::tokens::{HAIRLINE, ICON_SIZE_SMALL, Pace, ROW_HEIGHT, Radius, Space, UNIT};
use crate::ui::{FoxfireMotion, FoxfireState, Hint, Icon, IconName, Theme, UiStyles, foxfire};
use agent_client_protocol::v2::schema as acp;
use foxfire_acp::conversation::{Activity, Conversation, ConversationContent, ToolStatus, is_plan_entry_finished};
use foxfire_core::format::token_count;
use rouge::core::{
    AnyView, Cursor, Div, Ease, Keyframes, PaintRegion, Pose, Role, Styled, TextAlign, View, div, dynamic, show,
    switch, text, view, window_focused,
};
use rouge::reactive::{Memo, Scope, Signal, effect, memo, signal, sleep, stored, untrack};
use rouge::widgets::{Placement, popover, progress, progress_ring};
use std::cell::Cell;
use std::time::Duration;
use web_time::Instant;

#[derive(Clone, Copy)]
pub struct StatusLine {
    thread: Thread,
    running_since: Signal<Option<Instant>>,
    last_turn: Signal<Option<Duration>>,
    elapsed: Signal<Duration>,
    plan_open: Signal<bool>,
    scope: Scope,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Doing {
    Compacting,
    Thinking,
    Writing,
    Waiting,
    Working,
}

impl StatusLine {
    pub fn new(thread: Thread) -> Self {
        let status = Self {
            thread,
            running_since: signal(None),
            last_turn: signal(None),
            elapsed: signal(Duration::ZERO),
            plan_open: signal(false),
            scope: Scope::new(),
        };
        let running = memo(move || matches!(thread.status(), ThreadStatus::Running | ThreadStatus::NeedsInput));
        effect(move || {
            let running = running.get();
            untrack(|| status.sync(running));
        });
        status
    }

    fn sync(self, running: bool) {
        if !running {
            if let Some(since) = self.running_since.get_untracked() {
                self.running_since.set(None);
                self.last_turn.set(Some(since.elapsed()));
            }
            return;
        }
        if self.running_since.with_untracked(Option::is_some) {
            return;
        }
        let since = Instant::now();
        self.running_since.set(Some(since));
        self.elapsed.set(Duration::ZERO);
        spawn_in(self.scope, async move {
            loop {
                sleep(TICK).await;
                if !self.running_since.is_alive() || self.running_since.get_untracked() != Some(since) {
                    return;
                }
                self.elapsed.set(since.elapsed());
            }
        });
    }

    pub fn view(self) -> impl View {
        let thread = self.thread;
        let plan = memo(move || thread.with_core(|core| visible_plan(core.conversation())).flatten());
        let usage = memo(move || thread.with_core(|core| core.conversation().context_usage().cloned()).flatten());
        let doing = memo(move || {
            self.running_since.get()?;
            if thread.status() == ThreadStatus::NeedsInput {
                return Some(Doing::Waiting);
            }
            Some(thread.with_core(|core| Doing::of(core.conversation())).unwrap_or(Doing::Working))
        });
        let activity = memo(move || doing.get().is_some() || self.last_turn.get().is_some());
        let planned = memo(move || plan.with(Option::is_some));
        let timing = memo(move || self.running_since.with(Option::is_some));
        let metered = memo(move || usage.with(Option::is_some));
        let parts: [(Memo<bool>, AnyView); 4] = [
            (activity, self.activity(doing, plan).into_any()),
            (planned, self.plan(plan).into_any()),
            (timing, elapsed(self.elapsed).into_any()),
            (metered, context_meter(usage).into_any()),
        ];
        let visible = parts.each_ref().map(|(shown, _)| *shown);
        let parts = parts.into_iter().enumerate().map(move |(index, (shown, part))| {
            let separated = memo(move || visible[..index].iter().any(|shown| shown.get()));
            let part = std::cell::RefCell::new(Some(part));
            show(
                move || shown.get(),
                move || {
                    view! {
                        <div flex items_center gap=Space::X2 min_w=0.0 flex_shrink={if index == 0 { 1.0 } else { 0.0 }}>
                            <show when={move || separated.get()}>
                                <div flex_none><text muted>"·"</text></div>
                            </show>
                            {part.borrow_mut().take()}
                        </div>
                    }
                },
            )
        });
        view! {
            <div test_id="status" flex_1 min_w=0.0 h=ROW_HEIGHT flex items_center justify_end gap=Space::X2 body secondary>
                {..parts}
            </div>
        }
    }

    fn activity(self, doing: Memo<Option<Doing>>, plan: Memo<Option<Vec<acp::PlanEntry>>>) -> impl View {
        let theme = Theme::current();
        let motion = FoxfireMotion::current();
        let details = signal(false);
        let restoring_focus = stored(false);
        let open_details = move || {
            restoring_focus.set(true);
            details.set(true);
        };
        let label = move || doing.get().map_or_default(|doing| doing.label(plan.get().as_deref()));
        let last_turn = memo(move || self.last_turn.get().filter(|_| doing.get().is_none()));
        let busy = move || doing.get().is_some();
        let mark = switch(
            move || motion.reduced(),
            move |reduced| {
                let mark = show(busy, move || indicator(doing, plan));
                if reduced { mark } else { mark.enter(UNSUMMONED, Pace::Enter).exit(UNSUMMONED, Pace::Exit) }
            },
        );
        let detail = memo(move || activity_detail(self.thread, doing, plan));
        let trigger = view! {
            <div test_id="activity-details-trigger" min_w=0.0 flex items_center gap=Space::X2
                 on_activate=open_details
                 on_focus={move |event| {
                     // Popover dismissal restores focus; that must not reopen it.
                     if restoring_focus.get() {
                         restoring_focus.set(false);
                     } else if event.visible {
                         open_details();
                     }
                 }}>
                {mark}
                <dynamic>{
                    let detail = detail.get();
                    view! {
                        <div test_id="status-label" min_w=0.0 overflow_hidden>
                            <text no_wrap color={move || {
                                if doing.get() == Some(Doing::Waiting) { theme.get().warning } else { theme.get().text_secondary }
                            }}>{label}</text>
                            <dynamic>{last_turn.get().map(|took| view! {
                                <div test_id="last-turn"><text muted no_wrap>{format!("Took {}", clock(took))}</text></div>
                            })}</dynamic>
                        </div>
                    }.tooltip(detail)
                }</dynamic>
            </div>
        };
        let trigger = trigger.focus_visible(move |style| style.bg(Role::SurfaceHover));
        popover(details, trigger, move || {
            view! {
                <div test_id="activity-details" w=PLAN_PANEL_WIDTH p=Space::X3 flex col gap=Space::X2>
                    <div test_id="activity-detail-state"><text label primary>{move || {
                        doing.get().map_or("Turn ended", |doing| doing.describe().0)
                    }}</text></div>
                    <dynamic>{plan.get().and_then(|entries| {
                        entries.into_iter().find(|entry| entry.status == acp::PlanEntryStatus::InProgress)
                    }).map(|entry| view! {
                        <div test_id="activity-detail-plan"><text body secondary>{format!("Working on: {}", entry.content)}</text></div>
                    })}</dynamic>
                    <dynamic>{current_tool(self.thread).map(|tool| view! {
                        <div test_id="activity-detail-tool"><text body secondary>{tool}</text></div>
                    })}</dynamic>
                </div>
            }
        }).placement(Placement::Above).min_w(0.0).flex_shrink(1.0)
    }

    fn plan(self, plan: Memo<Option<Vec<acp::PlanEntry>>>) -> impl View {
        let open = self.plan_open;
        let toggle = dynamic(move || {
            let entries = plan.get().unwrap_or_default();
            let total = entries.len().max(1);
            let done = entries.iter().filter(|entry| is_plan_entry_finished(&entry.status)).count();
            #[allow(clippy::cast_precision_loss, reason = "A plan's entries are far fewer than f32 counts exactly.")]
            let share = done as f32 / total as f32;
            let chevron = if open.get() { IconName::ChevronDown } else { IconName::ChevronRight };
            view! {
                <div test_id="plan-toggle" flex_none flex items_center gap=Space::X2 cursor=Cursor::Pointer
                     on_click={move |event| {
                         event.stop_propagation();
                         open.update(bool::toggle);
                     }}>
                    <progress value=share flex_none w=PLAN_BAR_WIDTH/>
                    <text>{format!("{done}/{}", entries.len())}</text>
                    {Icon::new(chevron, Role::TextMuted).size(ICON_SIZE_SMALL)}
                </div>
            }
        });
        popover(open, toggle, move || plan_entries(&plan.get_untracked().unwrap_or_default()))
            .placement(Placement::Above)
    }
}

impl Doing {
    fn of(conversation: &Conversation) -> Self {
        match conversation.activity() {
            Activity::Compacting => Self::Compacting,
            Activity::Thinking => Self::Thinking,
            Activity::Responding => Self::Writing,
            Activity::RequiresAction => Self::Waiting,
            Activity::Working | Activity::Idle => Self::Working,
        }
    }

    fn label(self, plan: Option<&[acp::PlanEntry]>) -> String {
        let step =
            plan.and_then(|entries| entries.iter().find(|entry| entry.status == acp::PlanEntryStatus::InProgress));
        match (self, step) {
            (Self::Waiting | Self::Compacting, _) | (_, None) => self.describe().0.to_owned(),
            (_, Some(step)) => step.content.clone(),
        }
    }

    fn describe(self) -> (&'static str, &'static str) {
        match self {
            Self::Compacting => ("Compacting the conversation", "activity-compacting"),
            Self::Thinking => ("Thinking", "activity-thinking"),
            Self::Writing => ("Writing", "activity-writing"),
            Self::Waiting => ("Waiting for you", "activity-waiting"),
            Self::Working => ("Working", "activity-working"),
        }
    }

    fn foxfire(self) -> FoxfireState {
        match self {
            Self::Waiting => FoxfireState::Waiting,
            Self::Thinking => FoxfireState::Thinking,
            Self::Compacting => FoxfireState::Compacting,
            Self::Working => FoxfireState::Working,
            Self::Writing => FoxfireState::Writing,
        }
    }
}

fn indicator(doing: Memo<Option<Doing>>, plan: Memo<Option<Vec<acp::PlanEntry>>>) -> impl View {
    let theme = Theme::current();
    let motion = FoxfireMotion::current();
    let focused = window_focused();
    let region = signal(PaintRegion::default());
    let previous = stored(None::<Vec<acp::PlanEntry>>);
    let completed = Cell::new(0_u64);
    let milestones = memo(move || {
        let entries = plan.get();
        let advanced = previous.with(|previous| {
            previous.as_ref().zip(entries.as_ref()).is_some_and(|(before, after)| {
                before.iter().any(|entry| {
                    entry.status == acp::PlanEntryStatus::InProgress
                        && after
                            .iter()
                            .any(|next| next.content == entry.content && next.status == acp::PlanEntryStatus::Completed)
                })
            })
        });
        previous.set(entries);
        if advanced {
            completed.set(completed.get() + 1);
        }
        completed.get()
    });
    let seen = Cell::new(0);
    let active = Cell::new(None);
    let cue_generation = memo(move || {
        let milestone = milestones.get();
        let fresh = milestone > seen.replace(milestone);
        let allowed = !motion.reduced()
            && focused.get()
            && region.with(|region| !region.bounds.is_empty())
            && doing.get().is_some_and(|doing| doing != Doing::Waiting);
        let generation = if !allowed {
            None
        } else if fresh {
            Some(milestone)
        } else {
            active.get()
        };
        active.set(generation);
        generation
    });
    let cue = switch(
        move || cue_generation.get(),
        move |generation| {
            generation.map(|_| {
                div()
                    .test_id("activity-milestone")
                    .absolute()
                    .inset(0.0)
                    .rounded(Radius::Full)
                    .border(HAIRLINE)
                    .border_color(Role::Info)
                    .pass_through()
                    .animate(
                        Keyframes::new()
                            .at(0.0, |pose| pose.scale(1.0).opacity(0.25))
                            .at(1.0, |pose| pose.scale(1.3).opacity(0.0))
                            .duration(0.4)
                            .ease(Ease::Out),
                    )
            })
        },
    );
    let tint = move || if doing.get() == Some(Doing::Waiting) { theme.get().warning } else { theme.get().info };
    view! {
        <div test_id="activity" flex_none relative size=ICON_SIZE_SMALL flex items_center justify_center track_paint_region=region>
            {foxfire(ICON_SIZE_SMALL, tint, move || doing.get().unwrap_or(Doing::Waiting).foxfire())}
            {cue}
            <dynamic>{doing.get().map(|doing| view! {
                <div test_id={doing.describe().1} absolute top=0.0 left=0.0 size_full pass_through/>
            })}</dynamic>
        </div>
    }
}

fn current_tool(thread: Thread) -> Option<String> {
    thread
        .with_core(|core| {
            core.conversation().items().iter().rev().find_map(|item| match item.content() {
                ConversationContent::Tool(tool) if tool.status == ToolStatus::Running => {
                    Some(tool.headline().unwrap_or("Tool running").to_owned())
                }
                _ => None,
            })
        })
        .flatten()
}

fn activity_detail(thread: Thread, doing: Memo<Option<Doing>>, plan: Memo<Option<Vec<acp::PlanEntry>>>) -> String {
    let mut detail = doing.get().map_or("Turn ended", |doing| doing.describe().0).to_owned();
    if let Some(step) = plan
        .get()
        .and_then(|entries| entries.into_iter().find(|entry| entry.status == acp::PlanEntryStatus::InProgress))
    {
        detail.push_str("\nWorking on: ");
        detail.push_str(&step.content);
    }
    if let Some(tool) = current_tool(thread) {
        detail.push('\n');
        detail.push_str(&tool);
    }
    detail
}

fn plan_entries(entries: &[acp::PlanEntry]) -> Div {
    let rows = entries.iter().enumerate().map(|(index, entry)| {
        let (icon, color) = match entry.status {
            acp::PlanEntryStatus::InProgress => (IconName::CircleDot, Role::Accent),
            acp::PlanEntryStatus::Completed => (IconName::CircleCheck, Role::Positive),
            acp::PlanEntryStatus::Cancelled => (IconName::CircleSlash, Role::TextMuted),
            _ => (IconName::Circle, Role::TextMuted),
        };
        let done = is_plan_entry_finished(&entry.status);
        view! {
            <div test_id={format!("plan-entry-{index}")} flex items_center gap=Space::X2>
                {Icon::new(icon, color).size(ICON_SIZE_SMALL)}
                <div flex_1 min_w=0.0>
                    <text color={if done { Role::TextMuted } else { Role::TextSecondary }}>{entry.content.clone()}</text>
                </div>
            </div>
        }
    });
    view! {
        <div test_id="plan-panel" w=PLAN_PANEL_WIDTH px=Space::X3 py=Space::X2 flex col gap=Space::X1>{..rows}</div>
    }
}

fn elapsed(elapsed: Signal<Duration>) -> impl View {
    view! {
        <div test_id="elapsed" flex_none min_w=ELAPSED_MIN_WIDTH>
            <text muted align=TextAlign::End>{move || clock(elapsed.get())}</text>
        </div>
    }
}

fn context_meter(usage: Memo<Option<acp::UsageUpdate>>) -> impl View {
    dynamic(move || {
        let usage = usage.get()?;
        #[allow(
            clippy::cast_precision_loss,
            reason = "The ring only needs subpixel precision; labels use integer counts."
        )]
        let fraction = if usage.size == 0 { 0.0 } else { usage.used.min(usage.size) as f32 / usage.size as f32 };
        let color = if fraction >= FULL {
            Role::Destructive
        } else if fraction >= NEARLY_FULL {
            Role::Warning
        } else {
            Role::Accent
        };
        let label = if usage.size == 0 {
            format!("{} tokens", token_count(usage.used))
        } else {
            format!("{} / {} tokens", token_count(usage.used), token_count(usage.size))
        };
        let counts = (fraction >= NEARLY_FULL).then(|| {
            view! { <div test_id="context-tokens"><text no_wrap color=color>{label.clone()}</text></div> }
        });
        Some(
            view! {
                <div test_id="context-meter" flex_none flex items_center gap=Space::X2>
                    {progress_ring(fraction, color).test_id("context-ring")}
                    {counts}
                </div>
            }
            .tooltip(label),
        )
    })
}

fn clock(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn visible_plan(conversation: &Conversation) -> Option<Vec<acp::PlanEntry>> {
    let entries = conversation.plan()?.entries.as_slice();
    let finished = conversation.turn().is_idle() && entries.iter().all(|entry| is_plan_entry_finished(&entry.status));
    (!entries.is_empty() && !finished).then(|| entries.to_vec())
}

const TICK: Duration = Duration::from_secs(1);
const PLAN_BAR_WIDTH: f32 = 10.0 * UNIT;
const PLAN_PANEL_WIDTH: f32 = 90.0 * UNIT;
const NEARLY_FULL: f32 = 0.75;
const FULL: f32 = 0.9;
const ELAPSED_MIN_WIDTH: f32 = 9.0 * UNIT;

const UNSUMMONED: Pose = Pose::new().scale(0.96).opacity(0.0);
