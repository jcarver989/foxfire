use super::groups::{Group, groups};
use foxfire_acp::conversation::{Conversation, ConversationId, ConversationItem, ConversationItemId};
use std::collections::HashSet;

#[derive(Clone, Default, PartialEq)]
pub(super) struct Summary {
    pub(super) conversation: Option<ConversationId>,
    pub(super) items: Vec<ConversationItemId>,
    groups: Vec<Group>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Fold {
    Item(ConversationItemId),
    Full(ConversationItemId),
    /// The work of the turn that starts with an item.
    Turn(ConversationItemId),
    /// The earlier steps of the turn that starts with an item.
    Earlier(ConversationItemId),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct ProjectedRow {
    pub(super) index: usize,
    pub(super) id: ConversationItemId,
    pub(super) part: Part,
    pub(super) first: bool,
}

/// What a row shows of its item, or of the turn that starts with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Part {
    Item,
    /// An item of an unfolded turn's work.
    Work,
    /// The line the turn folds into.
    Line,
    /// What shows the turn's earlier steps.
    Earlier,
}

impl ProjectedRow {
    pub(super) fn is_item(self) -> bool {
        matches!(self.part, Part::Item | Part::Work)
    }
}

#[derive(Clone, Copy)]
pub(super) enum FoldAction {
    Toggle(Fold),
    Earlier(ConversationItemId),
    OpenStep { turn: ConversationItemId, item: ConversationItemId },
    Reveal(usize),
}

impl Summary {
    pub(super) fn new(conversation: &Conversation) -> Self {
        let items = conversation.items();
        Self {
            conversation: Some(conversation.id()),
            items: items.iter().map(ConversationItem::id).collect(),
            groups: groups(items),
        }
    }

    pub(super) fn apply(&self, unfolded: &mut HashSet<Fold>, action: FoldAction) {
        match action {
            FoldAction::Toggle(fold) => {
                if !unfolded.remove(&fold) {
                    unfolded.insert(fold);
                }
                if let Fold::Turn(id) = fold {
                    unfolded.remove(&Fold::Earlier(id));
                }
            }
            FoldAction::Earlier(turn) => {
                unfolded.insert(Fold::Earlier(turn));
            }
            FoldAction::OpenStep { turn, item } => unfolded.extend([Fold::Turn(turn), Fold::Item(item)]),
            FoldAction::Reveal(index) => {
                if let Some(group) = self.groups.iter().find(|group| group.work.contains(&index)) {
                    let turn = self.items[group.span.start];
                    unfolded.extend([Fold::Turn(turn), Fold::Earlier(turn)]);
                }
            }
        }
    }

    pub(super) fn visible(&self, unfolded: &HashSet<Fold>) -> Vec<ProjectedRow> {
        let mut rows: Vec<ProjectedRow> = Vec::new();
        let mut push = |index: usize, part: Part| {
            rows.push(ProjectedRow { index, id: self.items[index], part, first: rows.is_empty() });
        };
        let mut next = 0;
        for group in &self.groups {
            let start = group.span.start;
            (next..start).for_each(|index| push(index, Part::Item));
            push(start, Part::Line);
            if unfolded.contains(&Fold::Turn(self.items[start])) {
                let earlier = self.earlier(group, unfolded);
                if earlier > 0 {
                    push(start, Part::Earlier);
                }
                group.work_after(earlier).iter().for_each(|&index| push(index, Part::Work));
            }
            group.shown.iter().for_each(|&index| push(index, Part::Item));
            next = group.span.end;
        }
        (next..self.items.len()).for_each(|index| push(index, Part::Item));
        rows
    }

    /// How many of a turn's oldest steps unfolding it leaves out.
    pub(super) fn earlier(&self, group: &Group, unfolded: &HashSet<Fold>) -> usize {
        if unfolded.contains(&Fold::Earlier(self.items[group.span.start])) {
            return 0;
        }
        group.earlier(|index| unfolded.contains(&Fold::Item(self.items[index])))
    }

    pub(super) fn group(&self, start: usize) -> Option<&Group> {
        self.groups.iter().find(|group| group.span.start == start)
    }
}
