use super::code::highlights;
use super::groups::Work;
use crate::chat::status::Doing;
use crate::ui::tokens::{HAIRLINE, ICON_SIZE_SMALL, ROW_HEIGHT, Space};
use crate::ui::{
    ActivityState, ButtonVariant, ControlSize, FontRole, Hint, Icon, IconName, Theme, UiStyles, activity, button,
    indent,
};
use agent_client_protocol::v2::schema as acp;
use clankerdiff_core::{DiffDocument, LineStat};
use clankerdiff_rouge::PatchPreview;
use clankerdiff_rouge::style::stat_badges;
use clankerdiff_ui_core::ReviewTheme;
use foxfire_acp::content::first_line;
use foxfire_acp::conversation::{
    ConversationItem, ConversationItemId, Revision, SubAgent, ToolCall, ToolStatus, diff_change_label,
};
use rouge::core::{
    AnyView, Cursor, Div, FocusHandle, Role, SpanStyle, Styled, TextBreak, View, div, dynamic, rich_text, switch, text,
    view,
};
use rouge::reactive::{Callback, Memo, ReadSignal, memo};
use rouge::syntax::{SourceSyntax, analyze};
use std::cell::OnceCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone, Copy)]
pub(crate) struct ToolActions {
    pub(crate) open_diff: Callback<(Arc<DiffDocument>,)>,
    pub(crate) open_location: Callback<(acp::ToolCallLocation,)>,
}

#[derive(Default)]
pub(crate) struct ToolPreviews(HashMap<ConversationItemId, Rc<ToolPreview>>);

pub(crate) struct ToolPreview {
    revision: Revision,
    command: OnceCell<SourceSyntax>,
    patches: Vec<Option<PatchPreview>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolFold {
    /// Only its row, without what its sub-agents do.
    Row,
    Folded,
    Expanded,
    Full,
}

impl ToolPreviews {
    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }

    pub(crate) fn get(&mut self, item: &ConversationItem, tool: &ToolCall) -> Rc<ToolPreview> {
        let preview = self.0.entry(item.id()).or_insert_with(|| Rc::new(ToolPreview::new(item.revision(), tool)));
        if preview.revision != item.revision() {
            *preview = Rc::new(ToolPreview::new(item.revision(), tool));
        }
        preview.clone()
    }
}

impl ToolPreview {
    fn new(revision: Revision, tool: &ToolCall) -> Self {
        let patches = tool
            .diffs()
            .map(|diff| {
                diff.patch
                    .as_ref()
                    .filter(|patch| patch.format == acp::DiffPatchFormat::GitPatch && !patch.text.trim().is_empty())
                    .map(|patch| PatchPreview::parse(&patch.text))
            })
            .collect();
        Self { revision, command: OnceCell::new(), patches }
    }

    pub(super) fn changed_lines(&self) -> (usize, usize) {
        self.patches
            .iter()
            .flatten()
            .filter_map(PatchPreview::stats)
            .fold((0, 0), |(added, removed), (more, fewer)| (added + more, removed + fewer))
    }

    fn patch(&self, position: usize) -> Option<&PatchPreview> {
        self.patches.get(position)?.as_ref()
    }
}

/// The line a turn's work folds into. Its chevron hangs beside the column, so its text lines up with the replies.
pub(crate) fn turn_line(
    row: usize,
    work: Memo<Work>,
    expanded: Memo<bool>,
    review: ReadSignal<ReviewTheme>,
    on_toggle: Callback,
) -> Div {
    let theme = Theme::current();
    let doing = memo(move || work.with(|work| work.doing));
    let live = memo(move || doing.get().is_some());
    let chevron = move || {
        dynamic(move || {
            let icon = if expanded.get() { IconName::ChevronDown } else { IconName::ChevronRight };
            Icon::new(icon, Role::TextMuted).size(ICON_SIZE_SMALL)
        })
    };
    let tint = move || match doing.get() {
        Some(Doing::Waiting) => theme.get().warning,
        _ => theme.get().info,
    };
    let hanging = switch(
        move || live.get(),
        move |live| {
            if !live {
                return chevron().into_any();
            }
            activity(ICON_SIZE_SMALL, tint, move || doing.get().unwrap_or(Doing::Working).state())
                .test_id(format!("turn-activity-{row}"))
                .into_any()
        },
    );
    let content = dynamic(move || work.with(|work| line_content(row, work, review)));
    let trailing = switch(move || live.get(), move |live| live.then(chevron));
    view! {
        <div test_id={format!("turn-summary-{row}")} h=ROW_HEIGHT ml={-ICON_SIZE_SMALL} min_w=0.0 overflow_hidden
             flex items_center gap=Space::X2 body secondary on_activate={move || on_toggle.call()}>
            <div flex_none size=ICON_SIZE_SMALL>{hanging}</div>
            {content}
            {trailing}
        </div>
    }
}

fn line_content(row: usize, work: &Work, review: ReadSignal<ReviewTheme>) -> Div {
    if let Some(doing) = work.doing {
        let doing = if doing == Doing::Waiting { doing } else { Doing::Working };
        let says = doing.describe().0;
        return view! { <div test_id={format!("turn-doing-{row}")} flex_none><text primary no_wrap>{says}</text></div> };
    }
    let outcome = view! {
        <div test_id={format!("turn-outcome-{row}")} min_w=0.0 overflow_hidden><text truncate>{work.outcome()}</text></div>
    };
    let changes = (work.added + work.removed > 0).then(|| {
        let stat = LineStat { added: work.added, deleted: work.removed, files: work.changed.len() };
        view! { <div test_id={format!("turn-changes-{row}")} flex_none caption>{stat_badges(stat, review)}</div> }
    });
    let failed = (work.failed > 0).then(|| {
        view! {
            <div test_id={format!("turn-failed-{row}")} flex_none>
                <text no_wrap color=Role::Destructive>{format!("{} failed", work.failed)}</text>
            </div>
        }
    });
    view! { <div min_w=0.0 flex items_center gap=Space::X2>{outcome}{changes}{failed}</div> }
}

pub(crate) struct ToolCard<'a> {
    pub(crate) review: &'a ReviewTheme,
    pub(crate) theme: ReadSignal<ReviewTheme>,
    pub(crate) preview: &'a ToolPreview,
    pub(crate) actions: ToolActions,
}

impl ToolCard<'_> {
    pub(crate) fn render(
        &self,
        tool: &ToolCall,
        index: usize,
        fold: ToolFold,
        focus: FocusHandle,
        on_toggle: Callback,
        on_show_all: Callback,
    ) -> AnyView {
        let expanded = matches!(fold, ToolFold::Expanded | ToolFold::Full);
        let sub_agents = fold != ToolFold::Row && !tool.sub_agents.is_empty();
        let diffs: Vec<&acp::Diff> = tool.diffs().collect();
        let name = view! {
            <div test_id={format!("tool-title-{index}")} min_w=0.0 overflow_hidden><text no_wrap>{title(tool)}</text></div>
        };
        let summary = self.summary(tool).map(|summary| match tool.locations().first() {
            Some(location) => self.location_link(location.clone(), index, summary),
            None => summary,
        });
        let line = tool_line(tool, tool.id(), name, summary);
        let changes = (!diffs.is_empty()).then(|| self.change_summary(tool, index));
        let content = view! { <div flex items_center gap=Space::X2 min_w=0.0>{..line}{changes}</div> };
        let header = disclosure_row(format!("tool-card-{index}"), expanded, on_toggle, content).track_focus(focus);
        let body = (expanded || sub_agents).then(|| {
            let details = expanded.then(|| self.details(tool, index, fold == ToolFold::Full, on_show_all));
            let changes = (expanded && !diffs.is_empty()).then(|| self.changes(&diffs, index));
            let agents = sub_agents.then(|| self.sub_agent_tree(&tool.sub_agents));
            view! {
                <div flex col gap=Space::X3 ml=Space::X2 mt=Space::X1 mb=Space::X2 pl=Space::X4 pr=Space::X2 py=Space::X1 border_l=HAIRLINE
                     border_color=Role::BorderSubtle>
                    {details}
                    {changes}
                    {agents}
                </div>
            }
        });
        view! { <div flex col min_w=0.0>{header}{body}</div> }.into_any()
    }

    fn summary(&self, tool: &ToolCall) -> Option<AnyView> {
        let error = output_headline(tool).filter(|_| tool.status == ToolStatus::Failed);
        let error = error.map(|error| view! { <text truncate>{error.to_owned()}</text> }.into_any());
        match (self.command_line(tool), error) {
            (Some(command), Some(error)) => Some(
                view! {
                    <div flex items_center gap=Space::X2 min_w=0.0>
                        <div min_w=0.0 overflow_hidden text_color=Role::TextMuted>{command}</div>
                        <div min_w=0.0 overflow_hidden>{error}</div>
                    </div>
                }
                .into_any(),
            ),
            (command, error) => error.or(command).or_else(|| Self::subject(tool)),
        }
    }

    fn command_line(&self, tool: &ToolCall) -> Option<AnyView> {
        let command = tool.bash_command()?;
        let first = command.lines().next()?;
        let spans = self
            .command_highlights(command)
            .into_iter()
            .filter_map(|(range, style)| {
                (range.start < first.len()).then(|| (range.start + 2..range.end.min(first.len()) + 2, style))
            })
            .collect::<Vec<_>>();
        Some(rich_text(format!("$ {first}"), spans).font(FontRole::Code).truncate().into_any())
    }

    fn subject(tool: &ToolCall) -> Option<AnyView> {
        if let Some(value) = tool.display_value().and_then(first_line) {
            return Some(view! { <text no_wrap>{value.to_owned()}</text> }.into_any());
        }
        let path = tool.changed_paths().first()?.display().to_string();
        Some(view! { <text code no_wrap>{path}</text> }.into_any())
    }

    fn command_highlights(&self, command: &str) -> Vec<(Range<usize>, SpanStyle)> {
        let syntax = self.preview.command.get_or_init(|| analyze("bash", command));
        highlights(command, &syntax.highlights, &self.review.syntax)
    }

    fn location_link(&self, location: acp::ToolCallLocation, index: usize, summary: AnyView) -> AnyView {
        let open = self.actions.open_location;
        let label = format!("Open {}", location.path.0.display());
        view! {
            <div test_id={format!("tool-location-{index}")} min_w=0.0 overflow_hidden cursor=Cursor::Pointer
                 on_click={move |event| {
                     event.stop_propagation();
                     open.call(location.clone());
                 }}>
                {summary}
            </div>
        }
        .tooltip(label)
        .into_any()
    }

    fn details(&self, tool: &ToolCall, index: usize, full: bool, on_show_all: Callback) -> Div {
        let sections = detail_sections(tool, full).map(|(label, shown, hidden)| {
            let body = if label == "Command" {
                let spans = self.command_highlights(&shown);
                rich_text(shown, spans).font(FontRole::Code)
            } else {
                text(shown).code()
            };
            let more = (hidden > 0)
                .then(|| show_all(format!("tool-{}-more-{index}", label.to_lowercase()), hidden, on_show_all));
            section(label, body.selection_break(TextBreak::Paragraph).into_any()).child(more)
        });
        view! { <div test_id={format!("tool-details-{index}")} flex col gap=Space::X3>{..sections}</div> }
    }

    fn changes(&self, diffs: &[&acp::Diff], index: usize) -> Div {
        let changes = diffs.iter().enumerate().map(|(position, diff)| {
            let patch = diff.patch.as_ref().filter(|patch| !patch.text.trim().is_empty());
            match (patch, self.preview.patch(position)) {
                (Some(_), Some(preview)) => {
                    let open = self.actions.open_diff;
                    preview
                        .render(
                            format!("tool-diff-{index}-{position}"),
                            self.review,
                            move |document: &Arc<DiffDocument>| {
                                open.call(document.clone());
                            },
                        )
                        .into_any()
                }
                (Some(patch), None) => {
                    section("Patch", view! { <text code>{patch.text.clone()}</text> }.into_any()).into_any()
                }
                (None, _) => {
                    let labels = diff.changes.iter().map(|change| {
                        view! { <text small color=Role::TextSecondary>{diff_change_label(change)}</text> }
                    });
                    view! { <div flex col>{..labels}</div> }.into_any()
                }
            }
        });
        view! {
            <div test_id={format!("tool-changes-{index}")} flex col gap=Space::X2 small font=FontRole::Code unselectable>
                {..changes}
            </div>
        }
    }

    fn change_summary(&self, tool: &ToolCall, index: usize) -> AnyView {
        let (added, removed) = self.preview.changed_lines();
        let files = tool.changed_paths().len();
        let changed_lines = added > 0 || removed > 0;
        let count = (files > 1 || !changed_lines).then(|| {
            let label = match files {
                0 => "File changes".to_owned(),
                1 => "1 file".to_owned(),
                _ => format!("{files} files"),
            };
            view! { <text caption muted no_wrap>{label}</text> }
        });
        let badges = changed_lines.then(|| stat_badges(LineStat { added, deleted: removed, files }, self.theme));
        view! {
            <div test_id={format!("tool-change-summary-{index}")} flex flex_none items_center gap=Space::X2 caption>
                {count}
                {badges}
            </div>
        }
        .into_any()
    }

    fn sub_agent_tree(&self, sub_agents: &[SubAgent]) -> Div {
        let mut rows: Vec<AnyView> = Vec::new();
        for agent in sub_agents {
            let agent_id = agent.session_id().0.to_string();
            let icon = agent.status().map_or_else(
                || Icon::new(IconName::Circle, Role::TextMuted).size(ICON_SIZE_SMALL).into_any(),
                |status| status_icon(status, &agent_id),
            );
            let name = agent.title().unwrap_or("sub-agent").to_owned();
            rows.push(view! { <div flex items_center gap=Space::X2>{icon}<text label>{name}</text></div> }.into_any());
            let tool_calls: Vec<&ToolCall> = agent.tool_calls().collect();
            let hidden = tool_calls.len().saturating_sub(SUB_AGENT_VISIBLE_TOOLS);
            if hidden > 0 {
                rows.push(
                    view! { <div pl={indent(SUB_AGENT_TOOL_DEPTH)}><text muted>{earlier_calls(hidden)}</text></div> }
                        .into_any(),
                );
            }
            for tool in &tool_calls[hidden..] {
                let name = view! { <div flex_none><text>{title(tool)}</text></div> };
                let line = tool_line(tool, &format!("{agent_id}-{}", tool.id()), name, self.summary(tool));
                rows.push(
                    view! { <div pl={indent(SUB_AGENT_TOOL_DEPTH)} flex items_center gap=Space::X2 min_w=0.0>{..line}</div> }
                        .into_any(),
                );
            }
        }
        view! { <div flex col gap=Space::X1 small unselectable>{..rows}</div> }
    }
}

const SUB_AGENT_VISIBLE_TOOLS: usize = 3;

const SUB_AGENT_TOOL_DEPTH: usize = 2;

const SECTION_MAX_LINES: usize = 200;

const SECTION_MAX_BYTES: usize = 16 * 1024;

/// A tool's icon, name and summary, leaving out the name if the icon and summary say as much.
fn tool_line(tool: &ToolCall, id: &str, name: Div, summary: Option<AnyView>) -> Vec<AnyView> {
    let kind = (tool.status == ToolStatus::Success).then(|| kind_icon(tool)).flatten();
    let stands_in = summary.is_some() && kind.is_some_and(|(_, says_enough)| says_enough);
    let icon = match kind {
        Some((icon, _)) => {
            let icon = Icon::new(icon, Role::TextMuted).size(ICON_SIZE_SMALL);
            let icon = view! { <div test_id={format!("tool-kind-{id}")} flex_none>{icon}</div> };
            if stands_in { icon.tooltip(title(tool)).into_any() } else { icon.into_any() }
        }
        None => status_icon(tool.status, id),
    };
    let name = (!stands_in).then(|| name.body_strong().text_color(Role::TextSecondary).into_any());
    let color = match tool.status {
        ToolStatus::Failed => Role::Destructive,
        _ if stands_in => Role::TextSecondary,
        _ => Role::TextMuted,
    };
    let summary = view! { <div flex_1 min_w=0.0 overflow_hidden body text_color=color>{summary}</div> };
    [Some(icon), name, Some(summary.into_any())].into_iter().flatten().collect()
}

/// The icon for what a tool did, and whether it is as clear as the tool's name.
fn kind_icon(tool: &ToolCall) -> Option<(IconName, bool)> {
    if !tool.sub_agents.is_empty() {
        return Some((IconName::Bot, false));
    }
    match tool.kind() {
        acp::ToolKind::Read => Some((IconName::FileText, true)),
        acp::ToolKind::Edit => Some((IconName::Pencil, true)),
        acp::ToolKind::Search => Some((IconName::Search, true)),
        acp::ToolKind::Execute => Some((IconName::SquareTerminal, true)),
        acp::ToolKind::Think => Some((IconName::Brain, false)),
        acp::ToolKind::Fetch => Some((IconName::Cloud, false)),
        _ => None,
    }
}

fn earlier_calls(hidden: usize) -> String {
    if hidden == 1 { "1 earlier tool call".to_owned() } else { format!("{hidden} earlier tool calls") }
}

fn output_text(tool: &ToolCall) -> String {
    tool.content().iter().filter_map(content_text).collect::<Vec<_>>().join("\n")
}

fn output_headline(tool: &ToolCall) -> Option<&str> {
    tool.content().iter().filter_map(content_text).find_map(first_line)
}

fn content_text(content: &acp::ToolCallContent) -> Option<&str> {
    match content {
        acp::ToolCallContent::Content(content) => match &content.content {
            acp::ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        },
        _ => None,
    }
}

fn title(tool: &ToolCall) -> String {
    tool.headline().unwrap_or("Tool call").to_owned()
}

pub(super) fn excerpt(tool: &ToolCall) -> String {
    let paths = tool.changed_paths();
    let path = paths.first().map(|path| path.to_string_lossy());
    let detail = tool.bash_command().or_else(|| tool.display_value()).and_then(first_line).or(path.as_deref());
    detail.map_or_else(|| title(tool), |detail| format!("{} {detail}", title(tool)))
}

pub(super) fn details_text(tool: &ToolCall, full: bool) -> String {
    detail_sections(tool, full)
        .map(|(_, text, _)| text)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn detail_sections(tool: &ToolCall, full: bool) -> impl Iterator<Item = (&'static str, String, usize)> + use<> {
    let input = if tool.diffs().next().is_some() { String::new() } else { tool.raw_input() };
    let command = tool.bash_command().map(|command| ("Command", command.to_owned(), 0));
    let sections = [("Input", input), ("Output", output_text(tool))];
    command.into_iter().chain(sections.into_iter().filter_map(move |(label, mut source)| {
        if source.is_empty() {
            return None;
        }
        let hidden = if full {
            0
        } else {
            let (shown, hidden) = clip(&source);
            source.truncate(shown.len());
            hidden
        };
        Some((label, source, hidden))
    }))
}

fn section(label: &'static str, content: AnyView) -> Div {
    view! {
        <div flex col gap=Space::X1>
            <text label muted unselectable>{label}</text>
            <div text_color=Role::TextSecondary small font=FontRole::Code>{content}</div>
        </div>
    }
}

fn clip(text: &str) -> (&str, usize) {
    let lines = text.match_indices('\n').nth(SECTION_MAX_LINES - 1).map_or(text.len(), |(end, _)| end);
    let mut end = text.floor_char_boundary(lines.min(SECTION_MAX_BYTES));
    if end < lines
        && let Some(newline) = text[..end].rfind('\n')
    {
        end = newline;
    }
    let rest = &text[end..];
    (&text[..end], rest.trim_prefix('\n').lines().count())
}

fn show_all(id: String, hidden: usize, on_click: Callback) -> AnyView {
    let label = if hidden == 1 { "Show 1 more line".to_owned() } else { format!("Show {hidden} more lines") };
    view! { <button test_id=id label variant=ButtonVariant::Ghost control_size=ControlSize::Small on_click=on_click/> }
        .into_any()
}

fn status_icon(status: ToolStatus, id: &str) -> AnyView {
    let (icon, color) = match status {
        ToolStatus::Running => {
            return view! {
                <div test_id={format!("tool-status-{id}")} flex_none flex items_center gap=Space::X1>
                    {activity(ICON_SIZE_SMALL, Role::Info, || ActivityState::Working).test_id(format!("tool-activity-{id}"))}
                    <text small muted>"Running"</text>
                </div>
            }
            .into_any();
        }
        ToolStatus::Success => (IconName::Check, Role::TextMuted),
        ToolStatus::Failed => (IconName::CircleX, Role::Destructive),
        ToolStatus::Cancelled => (IconName::CircleSlash, Role::TextMuted),
    };
    Icon::new(icon, color).size(ICON_SIZE_SMALL).into_any()
}

fn disclosure_row(id: String, expanded: bool, on_toggle: Callback, content: impl View) -> Div {
    let chevron = if expanded { IconName::ChevronDown } else { IconName::ChevronRight };
    view! {
        <div test_id=id h=ROW_HEIGHT overflow_hidden flex items_center gap=Space::X2 on_activate={move || on_toggle.call()}>
            {Icon::new(chevron, Role::TextMuted).size(ICON_SIZE_SMALL)}
            <div>{content}</div>
        </div>
    }
}
