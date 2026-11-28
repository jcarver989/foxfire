use super::tools::ToolPreviews;
use crate::chat::status::Doing;
use agent_client_protocol::v2::schema as acp;
use foxfire_acp::conversation::{ConversationContent, ConversationItem, ToolCall, ToolStatus};
use std::collections::HashSet;
use std::ops::Range;
use std::path::Path;

/// A turn whose work folds into one line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Group {
    /// From the start of the turn to the last of its work.
    pub(crate) span: Range<usize>,
    pub(crate) work: Vec<usize>,
    /// The calls and thoughts in the work, without what the agent said between them.
    pub(crate) steps: Vec<usize>,
    /// What stays below the line: notices from the host, and the agent's newest message if it came before the last work.
    pub(crate) shown: Vec<usize>,
}

/// What a group's line says about it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Work {
    pub(super) calls: usize,
    /// How many files the work looked at, if it did nothing else.
    pub(super) looked_at: Option<usize>,
    /// What to call each file the work changed.
    pub(super) changed: Vec<String>,
    pub(super) added: usize,
    pub(super) removed: usize,
    /// Calls that failed, except commands that went on to succeed.
    pub(super) failed: usize,
    pub(super) doing: Option<Doing>,
}

pub(crate) fn groups(items: &[ConversationItem]) -> Vec<Group> {
    turns(items).into_iter().filter_map(|turn| group(items, turn)).collect()
}

impl Group {
    /// How many of the oldest steps an unfolded turn leaves out: those before its recent steps and any that are `kept`.
    pub(super) fn earlier(&self, kept: impl Fn(usize) -> bool) -> usize {
        let recent = self.steps.len().saturating_sub(RECENT_STEPS);
        self.steps[..recent].iter().position(|&step| kept(step)).unwrap_or(recent)
    }

    /// The work that follows the `earlier` steps.
    pub(super) fn work_after(&self, earlier: usize) -> &[usize] {
        let Some(last) = earlier.checked_sub(1) else {
            return &self.work;
        };
        &self.work[self.work.partition_point(|&index| index <= self.steps[last])..]
    }

    /// Sums up the group, whose turn is still going if the agent is `doing` something after it.
    pub(super) fn sum_up(&self, items: &[ConversationItem], doing: Option<Doing>, previews: &mut ToolPreviews) -> Work {
        let calls: Vec<(&ConversationItem, &ToolCall)> = self
            .work
            .iter()
            .filter_map(|&index| items.get(index))
            .filter_map(|item| match item.content() {
                ConversationContent::Tool(tool) => Some((item, tool)),
                _ => None,
            })
            .collect();
        let (added, removed) = calls
            .iter()
            .filter(|(_, tool)| tool.diffs().next().is_some())
            .map(|&(item, tool)| previews.get(item, tool).changed_lines())
            .fold((0, 0), |(added, removed), (more, fewer)| (added + more, removed + fewer));
        let latest =
            !items.iter().skip(self.span.end).any(|item| matches!(item.content(), ConversationContent::User(_)));
        let running = calls.iter().any(|(_, tool)| tool.is_running());
        let doing = doing.filter(|_| latest).or_else(|| running.then_some(Doing::Working));
        let mut changed: Vec<&Path> = Vec::new();
        for path in calls.iter().flat_map(|(_, tool)| tool.changed_paths()) {
            if !changed.contains(&path) {
                changed.push(path);
            }
        }
        Work {
            calls: calls.len(),
            looked_at: looked_at(&calls),
            changed: changed.iter().map(|path| file_name(path, &changed)).collect(),
            added,
            removed,
            failed: (0..calls.len()).filter(|&call| is_failure(&calls[call..])).count(),
            doing,
        }
    }
}

impl Work {
    pub(super) fn outcome(&self) -> String {
        match self.changed.as_slice() {
            [] => match (self.looked_at, self.calls) {
                (Some(1), _) => "Looked at 1 file".to_owned(),
                (Some(files), _) => format!("Looked at {files} files"),
                (None, 1) => "1 call".to_owned(),
                (None, calls) => format!("{calls} calls"),
            },
            [file] => format!("Changed {file}"),
            [first, second] => format!("Changed {first} and {second}"),
            [first, second, third] => format!("Changed {first}, {second} and {third}"),
            [first, second, rest @ ..] => format!("Changed {first}, {second} and {} more files", rest.len()),
        }
    }
}

fn turns(items: &[ConversationItem]) -> Vec<Range<usize>> {
    let prompts = items.iter().enumerate().filter(|(_, item)| matches!(item.content(), ConversationContent::User(_)));
    let mut turns = Vec::new();
    let mut start = 0;
    for (index, _) in prompts {
        turns.push(start..index);
        start = index + 1;
    }
    turns.push(start..items.len());
    turns
}

fn group(items: &[ConversationItem], turn: Range<usize>) -> Option<Group> {
    let content = |index: usize| items[index].content();
    if !turn.clone().any(|index| matches!(content(index), ConversationContent::Tool(_))) {
        return None;
    }
    let last = turn
        .clone()
        .rfind(|&index| matches!(content(index), ConversationContent::Tool(_) | ConversationContent::Thought(_)))?;
    let newest = turn.clone().rfind(|&index| matches!(content(index), ConversationContent::Assistant(_)));
    let span = turn.start..last + 1;
    let (shown, work): (Vec<usize>, Vec<usize>) = span.clone().partition(|&index| {
        Some(index) == newest
            || matches!(content(index), ConversationContent::Notice(_) | ConversationContent::Artifact(_))
    });
    let is_step =
        |&index: &usize| matches!(content(index), ConversationContent::Tool(_) | ConversationContent::Thought(_));
    let steps = work.iter().copied().filter(is_step).collect();
    Some(Group { span, work, steps, shown })
}

/// Whether the first of `calls` failed, and none of the rest ran its command successfully.
fn is_failure(calls: &[(&ConversationItem, &ToolCall)]) -> bool {
    let Some(((_, call), later)) = calls.split_first() else {
        return false;
    };
    let made_good = call.bash_command().is_some_and(|command| {
        later.iter().any(|(_, tool)| tool.status == ToolStatus::Success && tool.bash_command() == Some(command))
    });
    call.status == ToolStatus::Failed && !made_good
}

/// How many files `calls` read, if reading and searching is all they did and they say where.
fn looked_at(calls: &[(&ConversationItem, &ToolCall)]) -> Option<usize> {
    let tools = || calls.iter().map(|(_, tool)| tool);
    if !tools().all(|tool| matches!(tool.kind(), acp::ToolKind::Read | acp::ToolKind::Search)) {
        return None;
    }
    let read = tools().filter(|tool| tool.status == ToolStatus::Success && *tool.kind() == acp::ToolKind::Read);
    let files: HashSet<&Path> =
        read.flat_map(|tool| tool.locations()).map(|location| location.path.0.as_path()).collect();
    (!files.is_empty()).then_some(files.len())
}

/// A file's name, with the directory it is in if another of the files has the same name.
fn file_name(path: &Path, files: &[&Path]) -> String {
    let shared = files.iter().any(|file| *file != path && file.file_name() == path.file_name());
    let above = if shared { path.parent().and_then(Path::parent) } else { path.parent() };
    above.and_then(|above| path.strip_prefix(above).ok()).unwrap_or(path).display().to_string()
}

const RECENT_STEPS: usize = 6;
