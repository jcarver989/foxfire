use super::groups::{Work, groups};
use super::{Fold, FoldAction, Part, Summary, ToolPreviews};
use agent_client_protocol::v2::schema as acp;
use foxfire_acp::conversation::Conversation;
use foxfire_acp::testing::{artifact_submitted, message_chunk, on_session, tool_call};
use std::collections::HashSet;

#[test]
fn a_single_call_folds_without_a_reply_and_short_turns_expand_in_full() {
    for count in 1..=6 {
        let conversation = conversation((0..count).map(call));
        let summary = Summary::new(&conversation);
        let mut folds = HashSet::new();
        assert_eq!(rows(&summary, &folds), [(0, Part::Line)]);
        summary.apply(&mut folds, FoldAction::Toggle(Fold::Turn(summary.items[0])));
        let expected = [(0, Part::Line)].into_iter().chain((0..count).map(|index| (index, Part::Work)));
        assert_eq!(rows(&summary, &folds), expected.collect::<Vec<_>>());
    }
}

#[test]
fn showing_recent_work_keeps_its_intro_and_revealing_a_quote_opens_earlier_work() {
    let updates = [message_chunk("m0", "Looking."), call(0), message_chunk("m1", "Changing it.")]
        .into_iter()
        .chain((1..7).map(call))
        .chain([message_chunk("m2", "Done.")]);
    let summary = Summary::new(&conversation(updates));
    let turn = Fold::Turn(summary.items[0]);
    let mut folds = HashSet::new();
    summary.apply(&mut folds, FoldAction::Toggle(turn));
    let recent = rows(&summary, &folds);
    assert_eq!(&recent[..3], &[(0, Part::Line), (0, Part::Earlier), (2, Part::Work)]);
    assert_eq!(recent.last(), Some(&(9, Part::Item)));
    summary.apply(&mut folds, FoldAction::Reveal(1));
    assert!(rows(&summary, &folds).contains(&(1, Part::Work)));
    summary.apply(&mut folds, FoldAction::Toggle(turn));
    summary.apply(&mut folds, FoldAction::Toggle(turn));
    assert_eq!(rows(&summary, &folds), recent);
}

#[test]
fn what_the_agent_submitted_stays_below_the_line_its_turn_folds_into() {
    let mut conversation = conversation([call(0), message_chunk("m0", "Here it is.")]);
    conversation.apply_event(&artifact_submitted("session", 1));
    conversation.apply_event(&on_session("session", call(1)));
    let summary = Summary::new(&conversation);

    assert_eq!(rows(&summary, &HashSet::new()), [(0, Part::Line), (1, Part::Item), (2, Part::Item)]);
}

#[test]
fn only_commands_that_never_recovered_count_as_failures() {
    use acp::ToolCallStatus::{Completed, Failed};
    for (last, failures) in [("cargo check", 0), ("cargo test", 1)] {
        let work = outcome(vec![bash("a", "cargo check", Failed), bash("b", last, Completed)]);
        assert_eq!(work.failed, failures);
    }
    assert_eq!(
        outcome(vec![
            bash("a", "cargo check", Failed),
            bash("b", "cargo test", Failed),
            bash("c", "cargo check", Completed)
        ])
        .failed,
        1
    );
}

#[test]
fn a_turn_line_counts_the_files_of_a_turn_that_only_looked() {
    let read = |id: &str, path: &str, status| {
        let read = acp::ToolCallUpdate::new(id.to_owned())
            .title("Read file")
            .kind(acp::ToolKind::Read)
            .status(status)
            .locations(vec![acp::ToolCallLocation::new(path)]);
        acp::SessionUpdate::ToolCallUpdate(read)
    };
    let done = acp::ToolCallStatus::Completed;
    let search = || {
        acp::SessionUpdate::ToolCallUpdate(
            acp::ToolCallUpdate::new("search").kind(acp::ToolKind::Search).status(done.clone()),
        )
    };
    let label = |work| outcome(work).outcome();

    let one = vec![search(), read("a", "/alpha/a.rs", done.clone())];
    assert_eq!(label(one), "Looked at 1 file");
    let again = [("a", "/alpha/a.rs"), ("b", "/alpha/b.rs"), ("c", "/alpha/a.rs")];
    let again = again.iter().map(|(id, path)| read(id, path, done.clone())).collect();
    assert_eq!(label(again), "Looked at 2 files");

    let ran = vec![read("a", "/alpha/a.rs", done.clone()), bash("b", "cargo check", done.clone())];
    assert_eq!(label(ran), "2 calls", "it did more than look");
    assert_eq!(label(vec![search()]), "1 call", "a search names no file");
    let failed = vec![read("a", "/alpha/a.rs", acp::ToolCallStatus::Failed)];
    assert_eq!(label(failed), "1 call");
}

fn conversation(updates: impl IntoIterator<Item = acp::SessionUpdate>) -> Conversation {
    let mut conversation = Conversation::new("session");
    for update in updates {
        conversation.apply_event(&on_session("session", update));
    }
    conversation
}

fn rows(summary: &Summary, folds: &HashSet<Fold>) -> Vec<(usize, Part)> {
    summary.visible(folds).iter().map(|row| (row.index, row.part)).collect()
}

fn call(index: usize) -> acp::SessionUpdate {
    tool_call(&format!("call-{index}"), "Read file", acp::ToolCallStatus::Completed)
}

fn outcome(updates: Vec<acp::SessionUpdate>) -> Work {
    let conversation = conversation(updates);
    let items = conversation.items();
    groups(items)[0].sum_up(items, None, &mut ToolPreviews::default())
}

fn bash(id: &str, command: &str, status: acp::ToolCallStatus) -> acp::SessionUpdate {
    let tool = acp::ToolCallUpdate::new(id.to_owned())
        .title("Bash")
        .name("bash")
        .status(status)
        .raw_input(serde_json::json!({ "command": command }));
    acp::SessionUpdate::ToolCallUpdate(tool)
}
