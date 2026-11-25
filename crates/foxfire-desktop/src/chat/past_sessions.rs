use crate::agents::{AgentProfile, Catalogs};
use crate::clock::Clock;
use crate::history::{History, Listing, SavedSession};
use crate::registry::Registry;
use crate::target::SessionTarget;
use crate::threads::{Thread, ThreadManager};
use crate::ui::tokens::{COMPOSER_POPUP_HEIGHT, ICON_SIZE_SMALL, Pace, ROW_HEIGHT, Space};
use crate::ui::{
    ActivityState, Hint, Icon, IconName, Material, MaterialExt, NoticeTone, PickerItem, SearchPickerEvent,
    SearchPickerHandle, UiStyles, activity, badge, list_row, search_picker, segment, segmented_control, selection,
    skeleton,
};
use chrono::{DateTime, Utc};
use clankerdiff_core::format_age;
use rouge::core::{AnyView, Role, Styled, View, auto, div, dynamic, motion, pct, px, show, text, view};
use rouge::reactive::{Signal, Stored, effect, memo, signal, stored, untrack};
use std::sync::Arc;

/// Opens or closes the past sessions tray under a new thread's composer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TogglePastSessions;

pub(crate) const PAST_SESSIONS_KEYS: &str = "secondary-o";

/// The sessions a new thread's agent saved before, any one of which the thread can be swapped for.
///
/// Nothing asks the agent for them until the tray opens.
#[derive(Clone, Copy)]
pub(crate) struct PastSessions {
    thread: Thread,
    threads: ThreadManager,
    history: History,
    agents: Catalogs,
    registry: Registry,
    clock: Stored<Arc<dyn Clock>>,
    open: Signal<bool>,
    chosen: Signal<Option<AgentProfile>>,
}

type PickerRows = Vec<(PickerItem, SavedSession)>;

impl PastSessions {
    pub(crate) fn new(
        thread: Thread,
        threads: ThreadManager,
        history: History,
        agents: Catalogs,
        registry: Registry,
        clock: Stored<Arc<dyn Clock>>,
    ) -> Self {
        Self { thread, threads, history, agents, registry, clock, open: signal(false), chosen: signal(None) }
    }

    pub(crate) fn offered(self) -> bool {
        self.thread.is_fresh() && self.home().is_some()
    }

    pub(crate) fn is_open(self) -> bool {
        self.open.get() && self.offered()
    }

    pub(crate) fn toggle(self) {
        if self.open.get_untracked() {
            self.open.set(false);
        } else if untrack(|| self.offered()) {
            self.open.set(true);
            self.list();
        }
    }

    pub(crate) fn toggle_view(self) -> impl View {
        let open = self.open;
        let turn = motion(0.0);
        effect(move || {
            turn.animate_to(if open.get() { HALF_TURN } else { 0.0 }, Pace::Move);
        });
        view! {
            <list_row test_id="past-sessions-toggle" flex_none selection={move || selection(open.get())}
                      on_click={move || self.toggle()}>
                <div test_id="past-sessions-toggle-label"><text no_wrap>"Past sessions"</text></div>
                <div flex rotate=turn>{Icon::new(IconName::ChevronDown, Role::TextMuted).size(ICON_SIZE_SMALL)}</div>
            </list_row>
        }
        .tooltip("Pick up a past session")
        .ml(auto())
    }

    pub(crate) fn tray_view(self, on_dismiss: impl Fn() + 'static) -> impl View {
        let now = self.clock.with(|clock| clock.now());
        let agent = memo(move || self.agent());
        let model =
            agent.select(move |agent| agent.as_ref().map_or_else(Default::default, |agent| self.model(agent, now)));
        let rows = model.select(|(_, rows)| rows.clone());
        let listing = model.select(|(listing, _)| listing.clone());
        let displayed = stored(Vec::<SavedSession>::new());
        let handle = SearchPickerHandle::new(Vec::new(), move |event| {
            if let SearchPickerEvent::Chose(index) = event
                && let Some(session) = displayed.with(|rows| rows.get(index).cloned())
            {
                self.resume(session);
            } else if matches!(event, SearchPickerEvent::Chose(_) | SearchPickerEvent::Dismissed) {
                self.open.set(false);
                on_dismiss();
            }
        });
        effect(move || {
            let (items, sessions) = rows.get().into_iter().unzip();
            displayed.set(sessions);
            handle.set_items(items);
        });
        let empty = memo(move || match (rows.with(Vec::is_empty), listing.get()) {
            (false, _) => "No matching sessions",
            (true, Some(Listing::Listed)) => "No past sessions",
            _ => "",
        });
        let loading = memo(move || rows.with(Vec::is_empty) && listing.get() == Some(Listing::Listing));
        view! {
            <div test_id="past-sessions" flex col gap=Space::X2 mt=Space::X1 p=Space::X2 material=Material::Glass>
                <dynamic>{self.header(agent.get(), listing.get())}</dynamic>
                <show when={move || loading.get()}>{loading_rows()}</show>
                <dynamic>{{
                    let empty = empty.get();
                    untrack(|| view! {
                        <search_picker test_id="past-sessions-picker" handle material={Material::Bare}
                                       searchable="Search past sessions…" empty=empty
                                       max_list_height={px(COMPOSER_POPUP_HEIGHT)} w={pct(100.0)} p=0.0/>
                    })
                }}</dynamic>
            </div>
        }
    }

    fn model(self, agent: &AgentProfile, now: DateTime<Utc>) -> (Option<Listing>, PickerRows) {
        self.registry.with(|state| {
            self.history.with_listing(agent, |listing, rows| {
                let rows = rows
                    .iter()
                    .filter_map(|session| {
                        let key = session.key();
                        (!state.archived.contains(&key)).then(|| {
                            let item = PickerItem::new(
                                format!("past-session-{key}"),
                                state.title(Some(&key), session.title.clone()),
                            )
                            .group(recency(session.updated_at, now))
                            .detail(session.updated_at.map_or_else(String::new, |at| age(at, now)));
                            (item, session.clone())
                        })
                    })
                    .collect();
                (listing.cloned(), rows)
            })
        })
    }

    fn header(self, agent: Option<AgentProfile>, listing: Option<Listing>) -> Option<AnyView> {
        let agents = untrack(|| self.project_agents());
        let scope = (agents.len() > 1).then(|| self.agent_scope(agents));
        let badge = status(&agent?.name, listing);
        (scope.is_some() || badge.is_some()).then(|| {
            view! {
                <div flex items_center gap=Space::X2 min_h=ROW_HEIGHT px=Space::X1>
                    {scope}
                    <div flex_1/>
                    {badge}
                </div>
            }
            .into_any()
        })
    }

    fn agent_scope(self, agents: Vec<AgentProfile>) -> AnyView {
        let segments = agents.into_iter().map(|agent| {
            let id = format!("past-sessions-agent-{}", agent.name);
            let label = agent.name.clone();
            let shown = agent.clone();
            segment(label).test_id(id).selected(move || self.agent().as_ref() == Some(&shown)).on_click(move || {
                self.chosen.set(Some(agent.clone()));
                self.history.list(std::slice::from_ref(&agent));
            })
        });
        segmented_control().test_id("past-sessions-agents").children(segments).into_any()
    }

    /// The agent the thread was started with, if it runs on this machine.
    fn home(self) -> Option<AgentProfile> {
        match self.thread.target() {
            SessionTarget::Agent { agent, .. } => Some(agent),
            SessionTarget::Remote { .. } => None,
        }
    }

    fn agent(self) -> Option<AgentProfile> {
        self.chosen.get().or_else(|| self.home())
    }

    fn project_agents(self) -> Vec<AgentProfile> {
        self.home().map_or_default(|home| self.agents.catalog(&home.workspace).agents().to_vec())
    }

    fn list(self) {
        if let Some(agent) = untrack(|| self.agent()) {
            self.history.list(&[agent]);
        }
    }

    fn resume(self, session: SavedSession) {
        self.open.set(false);
        let fresh = self.thread;
        self.threads.resume(session.id, session.agent);
        if fresh.is_fresh() {
            self.threads.stop(fresh);
        }
    }
}

fn loading_rows() -> impl View {
    let rows = LOADING_ROW_WIDTHS.map(|width| {
        view! { <div h=ROW_HEIGHT px=Space::X2 flex items_center><skeleton w=pct(width)/></div> }
    });
    view! { <div test_id="past-sessions-loading" flex col>{..rows}</div> }
}

const LOADING_ROW_WIDTHS: [f32; 3] = [60.0, 80.0, 45.0];

fn status(agent: &str, listing: Option<Listing>) -> Option<AnyView> {
    match listing? {
        Listing::Listing => Some(
            view! {
                <div test_id="past-sessions-status" flex_none flex items_center gap=Space::X2>
                    {activity(ICON_SIZE_SMALL, Role::TextMuted, || ActivityState::Connecting)}
                    <div test_id="past-sessions-status-label"><text small muted no_wrap>{format!("Asking {agent}…")}</text></div>
                </div>
            }
            .into_any(),
        ),
        Listing::Failed(error) => {
            let failed = badge(format!("Couldn't list {agent}'s sessions"), NoticeTone::Warning);
            let row = view! {
                <div test_id="past-sessions-status" flex_none><div test_id="past-sessions-status-label">{failed}</div></div>
            };
            Some(row.tooltip(error).into_any())
        }
        Listing::Listed => None,
    }
}

fn recency(updated_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> &'static str {
    let Some(at) = updated_at else {
        return EARLIER;
    };
    match (now.date_naive() - at.date_naive()).num_days() {
        ..=0 => "Today",
        1 => "Yesterday",
        2..=6 => "This week",
        _ => EARLIER,
    }
}

fn age(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    format_age(u64::try_from(now.signed_duration_since(at).num_seconds()).unwrap_or(0))
}

const EARLIER: &str = "Earlier";
const HALF_TURN: f32 = 180.0;
