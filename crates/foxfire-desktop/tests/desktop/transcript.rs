use crate::support::{Edges, name_question, replying, texts};
use agent_client_protocol::v2::schema as acp;
use foxfire_acp::conversation::ConversationContent;
use foxfire_acp::testing::{
    FakeAgent, FakeAgentRequests, announced, message_chunk, plan, running_state, thought_chunk, tool_call,
};
use foxfire_core::session_core::SessionCore;
use foxfire_desktop::testing::{DesktopHarness, DesktopHarnessBuilder};
use foxfire_desktop::threads::{Thread, ThreadStatus};
use rouge::core::testing::TestError;
use rouge::core::{Color, Point};
use rouge::syntax::analyze;
use rouge::widgets::syntax_spans;
use std::fmt::Write;

#[test]
fn each_code_block_in_a_reply_copies_its_own_code() {
    let reply = "```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n\nRun it.\n\n```sh\nsecond\n```\n";
    let mut harness = answered(vec![message(reply)]);

    harness.click_nth("markdown-copy-code", 0).unwrap();
    assert_eq!(harness.platform().copied_text().as_deref(), Some("fn main() {\n    println!(\"hi\");\n}\n"));

    harness.click_nth("markdown-copy-code", 1).unwrap();
    assert_eq!(harness.platform().copied_text().as_deref(), Some("second\n"));
}

#[test]
fn links_in_a_reply_open_when_clicked() {
    let reply = "Read [the guide](https://example.com/guide) or https://docs.rs/rouge.";
    let mut harness = answered(vec![message(reply)]);

    harness.click_text("the guide").unwrap();
    harness.click_text("docs.rs").unwrap();
    harness.click_text("Read").unwrap();

    assert_eq!(harness.platform().opened(), ["https://example.com/guide", "https://docs.rs/rouge"]);
}

#[test]
fn a_reply_streamed_in_chunks_is_as_tall_as_the_same_reply_sent_whole() {
    let chunks = ["# Plan\n\nFirst ", "step.\n\n- one\n", "- two\n\n```sh\nls\n```\n"];
    let height = |updates| answered(updates).bounds("transcript-row-1").unwrap().size.height;

    let streamed = height(chunks.iter().map(|chunk| message(chunk)).collect());
    assert_eq!(streamed, height(vec![message(&chunks.concat())]));
    assert!(streamed > height(vec![message("Done.")]), "the streamed blocks are all laid out");
}

#[test]
fn a_prompt_or_reply_copies_its_text() {
    let mut harness = answered(vec![message("Some **bold** advice.")]);

    harness.click("copy-message-1").unwrap();
    assert_eq!(harness.platform().copied_text().as_deref(), Some("Some **bold** advice."));

    harness.click("copy-message-0").unwrap();
    assert_eq!(harness.platform().copied_text().as_deref(), Some("go"));
}

#[test]
fn a_prompt_is_an_opaque_card_in_the_flow_of_the_page_not_another_pane_of_glass() {
    let (mut harness, thread) = DesktopHarness::alpha(replying("alpha-session", "ok"));
    let before = harness.ui().draw().unwrap().backdrop_passes;

    harness.send(thread, "go");

    assert!(harness.bounds("prompt-0").is_some());
    assert_eq!(harness.ui().draw().unwrap().backdrop_passes, before, "the prompt blurs nothing behind it");
}

#[test]
fn a_turns_tool_calls_fold_into_one_line() -> Result<(), TestError> {
    let mut harness = answered(finished_tools(&["Read a", "Read b", "Edit c"]));
    assert!(harness.shows("turn-outcome-1", "3 calls"));
    assert!(harness.bounds("tool-card-1").is_none(), "the work starts folded");

    harness.click("turn-summary-1")?;
    for card in ["tool-card-1", "tool-card-2", "tool-card-3"] {
        assert!(harness.bounds(card).is_some(), "unfolding the line shows {card}");
    }
    harness.click("tool-card-2")?;
    assert!(harness.bounds("tool-details-2").is_some(), "a call under the line still unfolds");

    harness.click("turn-summary-1")?;
    assert!(harness.bounds("tool-card-2").is_none(), "folding the line hides its calls again");
    Ok(())
}

#[test]
fn a_focused_turn_line_can_be_toggled_with_the_keyboard() -> Result<(), TestError> {
    let mut harness = answered(finished_tools(&["Read a"]));
    harness.click("turn-summary-1")?;
    harness.hover("sidebar")?;

    harness.press("space");
    assert!(harness.bounds("tool-card-1").is_none(), "Space folds the focused line");

    harness.press("enter");
    assert!(harness.bounds("tool-card-1").is_some(), "Enter unfolds the focused line");
    Ok(())
}

#[test]
fn a_turn_stays_folded_while_it_runs_when_it_ends_and_after_the_next_prompt() {
    let (agent, mut requests) = FakeAgent::new("alpha-session").capture();
    let (mut harness, thread) = DesktopHarness::alpha(agent);
    let turn = harness.start_turn(thread, &mut requests, "go");
    turn.update(tool_call("call-0", "Read a", acp::ToolCallStatus::InProgress));
    harness.run_until_painted("turn-tail-1");
    assert!(harness.bounds("tool-card-1").is_some(), "a running turn shows its newest call");

    turn.update(tool_call("call-0", "Read a", acp::ToolCallStatus::Completed));
    turn.reply("Done.");
    turn.end(acp::StopReason::EndTurn);
    harness.run_until(|_| thread.status() == ThreadStatus::Idle);
    assert!(harness.bounds("turn-summary-1").is_some());
    assert!(harness.bounds("turn-tail-1").is_none());
    assert!(harness.bounds("tool-card-1").is_none());
    assert!(harness.bounds("reply-2").is_some(), "the reply stays in view");

    let _next = harness.start_turn(thread, &mut requests, "again");
    assert!(harness.bounds("turn-summary-1").is_some());
    assert!(harness.bounds("tool-card-1").is_none());
    assert!(harness.bounds("reply-2").is_some(), "the reply stays in view");
}

#[test]
fn an_unfolded_turn_stays_unfolded_as_it_continues() -> Result<(), TestError> {
    let (agent, mut requests) = FakeAgent::new("alpha-session").capture();
    let (mut harness, thread) = DesktopHarness::alpha(agent);
    let turn = harness.start_turn(thread, &mut requests, "go");
    turn.update(message_chunk("message-0", "Looking."));
    turn.update(tool_call("call-0", "Read a", acp::ToolCallStatus::InProgress));
    harness.run_until_painted("turn-summary-1");
    harness.click("turn-summary-1")?;
    assert!(harness.bounds("tool-card-2").is_some());

    turn.update(tool_call("call-0", "Read a", acp::ToolCallStatus::Completed));
    turn.update(message_chunk("message-1", "Changing it."));
    turn.update(tool_call("call-1", "Edit b", acp::ToolCallStatus::Completed));
    harness.run_until_painted("tool-card-4");
    assert!(harness.bounds("tool-card-2").is_some());

    turn.reply("Done.");
    turn.end(acp::StopReason::EndTurn);
    harness.run_until(|_| thread.status() == ThreadStatus::Idle);
    assert!(harness.bounds("tool-card-4").is_some(), "ending the turn does not fold it");
    Ok(())
}

#[test]
fn the_newest_sentence_shows_under_the_line_until_the_next_takes_its_place() -> Result<(), TestError> {
    let (agent, mut requests) = FakeAgent::new("alpha-session").capture();
    let (mut harness, thread) = DesktopHarness::alpha(agent);
    let turn = harness.start_turn(thread, &mut requests, "go");
    turn.update(message_chunk("message-0", "Looking."));
    turn.update(tool_call("call-0", "Read a", acp::ToolCallStatus::Completed));
    harness.run_until_painted("turn-summary-1");
    let line = top(&mut harness, "turn-summary-1");
    let sentence = top(&mut harness, "reply-1");
    assert!(sentence > line, "the sentence {sentence:?} follows the line {line:?}");

    turn.reply("Done.");
    turn.end(acp::StopReason::EndTurn);
    harness.run_until(|_| thread.status() == ThreadStatus::Idle);
    harness.run_until_painted("reply-3");
    assert!(harness.bounds("reply-1").is_none(), "the earlier sentence folds with the work");

    harness.click("turn-summary-1")?;
    let sentence = top(&mut harness, "reply-1");
    let call = top(&mut harness, "tool-card-2");
    let reply = top(&mut harness, "reply-3");
    assert!(sentence.is_some() && sentence < call, "the sentence {sentence:?} introduces its call {call:?}");
    assert!(call < reply, "the reply {reply:?} follows the work {call:?}");
    Ok(())
}

#[test]
fn a_running_turn_shows_its_last_three_steps() -> Result<(), TestError> {
    let (agent, mut requests) = FakeAgent::new("alpha-session").capture();
    let (mut harness, thread) = DesktopHarness::alpha(agent);
    let turn = harness.start_turn(thread, &mut requests, "go");
    turn.update(message_chunk("message-0", "Looking."));
    for update in finished_tools(&["Read a", "Read b"]) {
        turn.update(update);
    }
    turn.update(thought_chunk("thought-0", "pondering"));
    turn.update(bash("call-2", "cargo check", acp::ToolCallStatus::InProgress));
    harness.run_until_painted("tool-card-5");

    assert!(harness.bounds("tool-card-2").is_none(), "only the newest steps show");
    let tail = harness.ui().rect_of("turn-tail-1")?;
    let steps = ["tool-card-3", "thinking-4", "tool-card-5"].map(|step| top(&mut harness, step));
    assert!(steps.iter().all(|step| step.is_some_and(|step| step >= tail.top() && step < tail.bottom())));
    assert!(steps.is_sorted(), "the newest step comes last: {steps:?}");

    turn.update(tool_call("call-3", "Read c", acp::ToolCallStatus::InProgress));
    harness.run_until_painted("tool-card-6");
    assert!(harness.bounds("tool-card-3").is_none(), "a new step takes the place of the oldest");
    assert_eq!(harness.ui().rect_of("turn-tail-1")?.size, tail.size);
    Ok(())
}

#[test]
fn a_running_turns_steps_fade_upward_once_there_are_more_than_show() -> Result<(), TestError> {
    let (agent, mut requests) = FakeAgent::new("alpha-session").capture();
    let (mut harness, thread) = DesktopHarness::alpha(agent);
    let turn = harness.start_turn(thread, &mut requests, "go");
    let command = "echo '████████'";
    let string = syntax_color(&mut harness, command, "████████").expect("the theme colors strings");
    for call in 1..=3 {
        turn.update(bash(&format!("call-{call}"), command, acp::ToolCallStatus::Completed));
    }
    harness.run_until_painted("tool-card-3");
    harness.ui().run(2.0);
    assert!(paints(&mut harness, "tool-card-1", string)?, "nothing comes before the first step");

    turn.update(bash("call-4", command, acp::ToolCallStatus::Completed));
    harness.run_until_painted("tool-card-4");
    harness.ui().run(2.0);
    assert!(!paints(&mut harness, "tool-card-2", string)?, "the oldest step shown fades");
    assert!(paints(&mut harness, "tool-card-4", string)?, "the newest step does not");
    Ok(())
}

#[test]
fn pressing_a_running_turns_step_unfolds_the_turn_at_that_step() -> Result<(), TestError> {
    let (agent, mut requests) = FakeAgent::new("alpha-session").capture();
    let (mut harness, thread) = DesktopHarness::alpha(agent);
    let turn = harness.start_turn(thread, &mut requests, "go");
    for update in many_tools(5) {
        turn.update(update);
    }
    harness.run_until_painted("tool-card-5");
    harness.click("tool-card-4")?;

    assert!(harness.bounds("turn-tail-1").is_none());
    assert!(harness.bounds("tool-details-4").is_some());
    assert!(harness.bounds("tool-card-1").is_some());
    assert!(harness.shows("turn-doing-1", "Working"));

    harness.click("turn-summary-1")?;
    assert!(harness.bounds("turn-tail-1").is_some());
    assert!(harness.bounds("tool-details-4").is_none(), "a step in the tail is always one line");
    Ok(())
}

#[test]
fn a_running_turns_steps_leave_out_what_their_sub_agents_do() -> Result<(), TestError> {
    let (agent, mut requests) = FakeAgent::new("alpha-session").capture();
    let (mut harness, thread) = DesktopHarness::alpha(agent);
    let turn = harness.start_turn(thread, &mut requests, "go");
    turn.update(tool_call("call-0", "spawn_subagent", acp::ToolCallStatus::InProgress));
    harness.run_until_painted("tool-card-1");
    let tail = harness.ui().rect_of("turn-tail-1")?;

    let child = acp::SubagentUpdate::new("child-a").title("Look around").state(running_state());
    harness
        .wait_for(|| requests.try_connection())
        .send_notification(announced("alpha-session", "call-0", child))
        .unwrap();
    harness.run_until(|_| thread.with_core(has_sub_agent).unwrap_or_default());
    assert_eq!(harness.ui().rect_of("turn-tail-1")?.size, tail.size);
    Ok(())
}

#[test]
fn an_unfolded_turn_starts_from_its_last_six_steps() -> Result<(), TestError> {
    let mut harness = answered(many_tools(100).into_iter().chain([message("Done.")]).collect());
    let reply = top(&mut harness, "reply-101").expect("the reply is in view");
    harness.click("turn-summary-1")?;

    assert!(harness.shows("turn-earlier-1", "Show 94 earlier"));
    assert!(harness.bounds("tool-card-94").is_none());
    for call in 95..=100 {
        assert!(harness.bounds(&format!("tool-card-{call}")).is_some());
    }
    let moved = top(&mut harness, "reply-101").expect("the reply stays in view") - reply;
    assert!(moved > 0.0 && moved < 240.0, "a hundred calls move the reply by {moved}");
    Ok(())
}

#[test]
fn earlier_steps_show_on_demand_until_the_turn_folds() -> Result<(), TestError> {
    let mut harness = worked(many_tools(8));
    let earlier = top(&mut harness, "turn-earlier-1");
    assert!(harness.shows("turn-earlier-1", "Show 2 earlier"));

    harness.click("turn-earlier-1")?;
    assert!(harness.bounds("turn-earlier-1").is_none());
    assert_eq!(top(&mut harness, "tool-card-1"), earlier, "the earlier steps start where the button was");

    harness.click("turn-summary-1")?;
    harness.click("turn-summary-1")?;
    assert!(harness.shows("turn-earlier-1", "Show 2 earlier"), "unfolding starts from the end again");
    Ok(())
}

#[test]
fn an_unfolded_step_stays_in_view_as_its_turn_continues() -> Result<(), TestError> {
    let (agent, mut requests) = FakeAgent::new("alpha-session").capture();
    let (mut harness, thread) = DesktopHarness::alpha(agent);
    let turn = harness.start_turn(thread, &mut requests, "go");
    for update in many_tools(7) {
        turn.update(update);
    }
    harness.run_until_painted("tool-card-7");
    harness.click("turn-summary-1")?;
    harness.click("tool-card-2")?;

    for call in 8..=10 {
        turn.update(tool_call(&format!("later-{call}"), "Read", acp::ToolCallStatus::Completed));
    }
    harness.run_until(|harness| harness.bounds("tool-card-10").is_some());
    assert!(harness.bounds("tool-details-2").is_some());
    assert!(harness.shows("turn-earlier-1", "Show 1 earlier"));

    harness.click("tool-card-2")?;
    assert!(harness.bounds("tool-card-2").is_none());
    assert!(harness.shows("turn-earlier-1", "Show 4 earlier"), "folded, it goes with the rest");
    Ok(())
}

#[test]
fn selecting_leaves_out_how_to_show_earlier_steps() -> Result<(), TestError> {
    let long = acp::ToolCallUpdate::new("last")
        .title("Read file")
        .status(acp::ToolCallStatus::Completed)
        .content(vec![acp::ToolCallContent::from("file contents")]);
    let updates = many_tools(7).into_iter().chain([acp::SessionUpdate::ToolCallUpdate(long), message("Done.")]);
    let mut harness = worked(updates.collect());
    harness.click("tool-card-8")?;

    harness.click("reply-9")?;
    harness.press("cmd-a");
    harness.press("cmd-c");

    assert_eq!(harness.platform().copied_text().as_deref(), Some("go\n\nfile contents\n\nDone."));
    Ok(())
}

#[test]
fn a_turn_line_says_working_whatever_the_agent_does_until_the_turn_ends() {
    let (agent, mut requests) = FakeAgent::new("alpha-session").capture();
    let (mut harness, thread) = DesktopHarness::alpha(agent);
    let turn = harness.start_turn(thread, &mut requests, "go");
    turn.update(bash("call-0", "cargo check", acp::ToolCallStatus::InProgress));
    harness.run_until_painted("turn-activity-1");
    assert!(harness.shows("turn-doing-1", "Working"));

    turn.update(bash("call-0", "cargo check", acp::ToolCallStatus::Completed));
    turn.update(thought_chunk("thought-0", "pondering"));
    harness.run_until_painted("activity-thinking");
    assert!(harness.shows("turn-doing-1", "Working"));

    turn.end(acp::StopReason::EndTurn);
    harness.run_until(|_| thread.status() == ThreadStatus::Idle);
    assert!(harness.bounds("turn-doing-1").is_none());
    assert!(harness.bounds("turn-activity-1").is_none());
    assert!(harness.shows("turn-outcome-1", "1 call"));
}

#[test]
fn a_turn_line_says_when_it_waits_on_the_user() {
    let (agent, mut requests) = FakeAgent::new("alpha-session").capture();
    let (mut harness, thread) = DesktopHarness::alpha(agent);
    let turn = harness.start_turn(thread, &mut requests, "go");
    turn.update(bash("call-0", "cargo check", acp::ToolCallStatus::InProgress));
    let _answer = turn.ask(name_question());
    harness.run_until(|_| thread.status() == ThreadStatus::NeedsInput);

    assert!(harness.shows("turn-doing-1", "Waiting for you"));
}

#[test]
fn a_turn_line_says_only_that_it_works_until_the_turn_ends() {
    let (agent, mut requests) = FakeAgent::new("alpha-session").capture();
    let (mut harness, thread) = DesktopHarness::alpha(agent);
    let turn = harness.start_turn(thread, &mut requests, "go");
    turn.update(write_tool("call-0", Some(added_file(5)), ""));
    turn.update(bash("call-1", "cargo check", acp::ToolCallStatus::Failed));
    harness.run_until_painted("tool-card-2");
    assert!(harness.shows("turn-summary-1", "Working"));

    turn.end(acp::StopReason::EndTurn);
    harness.run_until(|_| thread.status() == ThreadStatus::Idle);
    assert!(harness.shows("turn-outcome-1", "Changed page.html"));
    assert!(harness.bounds("turn-changes-1").is_some());
    assert!(harness.bounds("turn-failed-1").is_some());
}

#[test]
fn a_turn_line_names_what_changed() {
    let mut harness = answered(vec![edit_of("call-0", "/alpha/page.html"), edit_of("call-1", "/alpha/page.html")]);
    assert!(harness.shows("turn-outcome-1", "Changed page.html"));
    assert!(harness.text("turn-changes-1").is_some_and(|changes| changes.contains("+10")));
    drop(harness);

    let line = |paths: &[&str]| {
        let edits = paths.iter().enumerate().map(|(call, path)| edit_of(&format!("call-{call}"), path));
        answered(edits.collect()).text("turn-outcome-1")
    };
    assert_eq!(line(&["/alpha/a.rs", "/alpha/b.rs"]).as_deref(), Some("Changed a.rs and b.rs"));
    assert_eq!(line(&["/a.rs", "/b.rs", "/c.rs"]).as_deref(), Some("Changed a.rs, b.rs and c.rs"));
    assert_eq!(line(&["/a.rs", "/b.rs", "/c.rs", "/d.rs"]).as_deref(), Some("Changed a.rs, b.rs and 2 more files"));
    assert_eq!(line(&["/ui/mod.rs", "/chat/mod.rs"]).as_deref(), Some("Changed ui/mod.rs and chat/mod.rs"));
}

#[test]
fn a_turn_line_keeps_its_counts_in_a_narrow_window() -> Result<(), TestError> {
    let paths = ["a_file_with_a_long_name.rs", "another_file_with_a_long_name.rs", "a_third_file_with_a_long_name.rs"];
    let edits = paths.iter().map(|name| edit_of(name, &format!("/alpha/{name}")));
    let mut harness = answered(edits.collect());
    harness.resize(420.0, 840.0);

    let line = harness.ui().rect_of("turn-summary-1")?;
    let changes = harness.ui().rect_of("turn-changes-1")?;
    assert!(changes.right() <= line.right(), "{changes:?} spills out of {line:?}");
    Ok(())
}

#[test]
fn an_error_amid_the_work_stays_in_view() {
    let (agent, mut requests) = FakeAgent::new("alpha-session").capture();
    let (mut harness, thread) = DesktopHarness::alpha(agent);
    let turn = harness.start_turn(thread, &mut requests, "go");
    turn.update(tool_call("call-0", "Read a", acp::ToolCallStatus::Completed));
    harness.run_until_painted("turn-summary-1");
    thread.report_error("The mode could not be changed");
    turn.update(tool_call("call-1", "Read b", acp::ToolCallStatus::Completed));
    harness.run_until_painted("tool-card-3");

    assert!(harness.bounds("transcript-error-2").is_some());
}

#[test]
fn a_failed_prompt_shows_as_an_error() {
    let (agent, mut requests) = FakeAgent::new("alpha-session").hold::<acp::PromptRequest>().capture();
    let (mut harness, thread) = DesktopHarness::alpha(agent);

    thread.send("hi".into());
    let (_, responder) = harness.next(requests.held::<acp::PromptRequest>());
    responder.respond_with_error(agent_client_protocol::Error::internal_error()).unwrap();

    harness.run_until_painted("transcript-error-0");
}

#[test]
fn a_tool_card_unfolds_its_input_and_output() {
    let tool = acp::ToolCallUpdate::new("call-1")
        .title("Read file")
        .status(acp::ToolCallStatus::Completed)
        .content(vec![acp::ToolCallContent::from("file contents")]);
    let mut harness = worked(vec![acp::SessionUpdate::ToolCallUpdate(tool)]);
    assert!(harness.bounds("tool-card-1").is_some(), "the tool call follows the prompt");
    assert!(harness.bounds("tool-details-1").is_none(), "a tool card starts folded");

    harness.click("tool-card-1").unwrap();
    assert!(harness.bounds("tool-details-1").is_some(), "clicking unfolds it");

    harness.click("tool-card-1").unwrap();
    assert!(harness.bounds("tool-details-1").is_none(), "clicking again folds it");
}

#[test]
fn a_focused_tool_row_can_be_toggled_with_the_keyboard() {
    let mut harness = worked(vec![tool_call("call-1", "Read file", acp::ToolCallStatus::Completed)]);
    harness.click("tool-card-1").unwrap();
    harness.hover("sidebar").unwrap();

    harness.press("space");
    assert!(harness.bounds("tool-details-1").is_none(), "Space folds the focused row");

    harness.press("enter");
    assert!(harness.bounds("tool-details-1").is_some(), "Enter unfolds the focused row");
}

#[test]
fn a_multi_line_tool_title_stays_within_its_row() {
    let command = "python3 - <<'EOF'\nprint('one')\nprint('two')\nprint('three')\nEOF\necho ok";
    let mut harness = worked(vec![tool_call("call-1", command, acp::ToolCallStatus::Completed)]);

    let row = harness.bounds("tool-card-1").unwrap();
    let title = harness.bounds("tool-title-1").unwrap();
    assert!(title.top() >= row.top() && title.bottom() <= row.bottom(), "{title:?} spills out of {row:?}");
}

#[test]
fn a_tool_row_highlights_its_command() -> Result<(), TestError> {
    let command = "echo '████████'";
    let mut harness = worked(vec![bash("call-1", command, acp::ToolCallStatus::Completed)]);

    let string = syntax_color(&mut harness, command, "████████").expect("the theme colors strings");
    assert!(paints(&mut harness, "tool-card-1", string)?, "the string is {string:?}");
    Ok(())
}

#[test]
fn a_tool_row_shows_a_failed_command_beside_its_error() {
    let failed = acp::ToolCallUpdate::new("call-1")
        .title("Bash")
        .name("bash")
        .status(acp::ToolCallStatus::Failed)
        .raw_input(serde_json::json!({ "command": "cargo check" }))
        .content(vec![acp::ToolCallContent::from("error[E0382]: borrow of moved value")]);
    let mut harness = worked(vec![acp::SessionUpdate::ToolCallUpdate(failed)]);

    assert!(harness.shows("tool-card-1", "Bash$ cargo checkerror[E0382]: borrow of moved value"));
}

#[test]
fn a_tool_rows_icon_stands_in_for_the_name_of_what_it_did() {
    let done = acp::ToolCallStatus::Completed;
    let mut harness = worked(vec![
        of_kind(acp::ToolKind::Read, "Read file", "main.rs", done.clone()),
        of_kind(acp::ToolKind::Search, "Web search", "'rouge'", done.clone()),
        of_kind(acp::ToolKind::Edit, "Edit file", "lib.rs", done.clone()),
        of_kind(acp::ToolKind::Execute, "Ran", "cargo check", done.clone()),
        of_kind(acp::ToolKind::Fetch, "Fetch URL", "docs.rs", done.clone()),
        of_kind(acp::ToolKind::Other, "LSP errors", "workspace", done),
    ]);

    let rows = ["main.rs", "'rouge'", "lib.rs", "cargo check", "Fetch URLdocs.rs", "LSP errorsworkspace"];
    for (row, text) in (1..).zip(rows) {
        assert!(harness.shows(&format!("tool-card-{row}"), text), "row {row} shows {text}");
    }
}

#[test]
fn a_tool_row_keeps_its_name_unless_its_icon_and_subject_say_enough() {
    let read = |value, status| of_kind(acp::ToolKind::Read, "Read file", value, status);
    let mut harness = worked(vec![
        read("main.rs", acp::ToolCallStatus::Failed),
        read("lib.rs", acp::ToolCallStatus::Cancelled),
        read("", acp::ToolCallStatus::Completed),
    ]);

    assert!(harness.shows("tool-card-1", "Read filemain.rs"), "a failure's icon does not say what failed");
    assert!(harness.shows("tool-card-2", "Read filelib.rs"));
    assert!(harness.shows("tool-card-3", "Read file"), "without a subject the name is all there is");
}

#[test]
fn a_running_tool_row_keeps_its_name_until_it_is_done() {
    let (agent, mut requests) = FakeAgent::new("alpha-session").capture();
    let (mut harness, thread) = DesktopHarness::alpha(agent);
    let turn = harness.start_turn(thread, &mut requests, "go");
    turn.update(of_kind(acp::ToolKind::Read, "Read file", "main.rs", acp::ToolCallStatus::InProgress));
    harness.run_until(|harness| harness.shows("tool-card-1", "RunningRead filemain.rs"));

    turn.update(of_kind(acp::ToolKind::Read, "Read file", "main.rs", acp::ToolCallStatus::Completed));
    harness.run_until(|harness| harness.shows("tool-card-1", "main.rs"));
}

#[test]
fn a_tool_with_sub_agents_keeps_its_name() {
    let (agent, mut requests) = FakeAgent::new("alpha-session").capture();
    let (mut harness, thread) = DesktopHarness::alpha(agent);
    let turn = harness.start_turn(thread, &mut requests, "go");
    let spawn = acp::ToolCallUpdate::new("call-0")
        .title("Spawn agent")
        .name("subagents__spawn_subagent")
        .kind(acp::ToolKind::Execute)
        .status(acp::ToolCallStatus::Completed)
        .meta(display_value("explorer"));
    turn.update(acp::SessionUpdate::ToolCallUpdate(spawn));
    let child = acp::SubagentUpdate::new("child-a").state(running_state());
    harness
        .wait_for(|| requests.try_connection())
        .send_notification(announced("alpha-session", "call-0", child))
        .unwrap();
    harness.run_until(|_| thread.with_core(has_sub_agent).unwrap_or_default());

    assert!(harness.shows("tool-card-1", "Spawn agentexplorer"));
}

#[test]
fn a_tool_row_summarizes_changes_and_only_unfolds_the_patch_on_demand() {
    let patch = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n";
    let diff = acp::Diff::patch(patch, vec![acp::DiffChange::modify("/alpha/src/lib.rs")]);
    let edit = acp::ToolCallUpdate::new("call-1")
        .title("Edit file")
        .status(acp::ToolCallStatus::Completed)
        .content(vec![acp::ToolCallContent::from(diff)]);
    let read = tool_call("call-2", "Read file", acp::ToolCallStatus::Completed);
    let mut harness = worked(vec![acp::SessionUpdate::ToolCallUpdate(edit), read]);

    assert!(harness.bounds("tool-change-summary-1").is_some(), "the edit summarizes its changes while folded");
    assert!(harness.bounds("tool-changes-1").is_none(), "patches start folded");
    assert!(harness.bounds("tool-change-summary-2").is_none(), "a read has no change summary");

    harness.click("tool-card-1").unwrap();
    assert!(harness.bounds("tool-changes-1").is_some(), "expanding reveals the patch");

    harness.click("tool-card-1").unwrap();
    assert!(harness.bounds("tool-changes-1").is_none(), "folding hides the patch again");
}

#[test]
fn a_long_patch_previews_its_start_and_opens_in_full_in_a_diff_viewer() {
    let mut harness = worked(vec![write_tool("call-1", Some(added_file(1000)), "")]);
    harness.click("tool-card-1").unwrap();
    assert!(harness.bounds("tool-diff-1-0").is_some(), "the patch previews in the card");

    harness.scroll_into_view("transcript-selection", "tool-diff-1-0-more").unwrap();
    harness.click("tool-diff-1-0-more").unwrap();
    assert!(harness.bounds("full-diff-panel").is_some(), "the whole patch opens in a diff viewer");

    harness.click("close-full-diff").unwrap();
    assert!(harness.bounds("full-diff-panel").is_none(), "closing returns to the transcript");

    harness.click("tool-diff-1-0-more").unwrap();
    harness.press("escape");
    assert!(harness.bounds("full-diff-panel").is_none(), "Escape closes the diff viewer");
}

#[test]
fn a_short_patch_previews_in_full() {
    let mut harness = worked(vec![write_tool("call-1", Some(added_file(5)), "")]);
    harness.click("tool-card-1").unwrap();

    assert!(harness.bounds("tool-diff-1-0").is_some());
    assert!(harness.bounds("tool-diff-1-0-more").is_none());
}

#[test]
fn a_long_tool_output_is_cut_short_until_shown_in_full() {
    let tool = acp::ToolCallUpdate::new("call-1")
        .title("Read file")
        .status(acp::ToolCallStatus::Completed)
        .content(vec![acp::ToolCallContent::from(numbered_lines(1000))]);
    let mut harness = worked(vec![acp::SessionUpdate::ToolCallUpdate(tool)]);
    harness.click("tool-card-1").unwrap();
    let cut = harness.bounds("tool-details-1").unwrap().size.height;

    harness.scroll_into_view("transcript-selection", "tool-output-more-1").unwrap();
    harness.click("tool-output-more-1").unwrap();

    assert!(harness.bounds("tool-output-more-1").is_none(), "the whole output shows");
    assert!(harness.bounds("tool-details-1").unwrap().size.height > cut);
}

#[test]
fn a_tool_with_a_patch_does_not_repeat_the_patched_text_as_its_input() {
    let input = numbered_lines(1000);
    let mut patched = worked(vec![write_tool("call-1", Some(added_file(5)), &input)]);
    patched.click("tool-card-1").unwrap();
    assert!(patched.bounds("tool-details-1").is_some());
    assert!(patched.bounds("tool-input-more-1").is_none(), "the patch already shows the change");
    drop(patched);

    let mut unpatched = worked(vec![write_tool("call-1", None, &input)]);
    unpatched.click("tool-card-1").unwrap();
    assert!(unpatched.bounds("tool-input-more-1").is_some(), "without a patch the input shows, cut short");
}

#[test]
fn reasoning_unfolds_to_show_its_text() {
    let mut harness = answered(vec![thought_chunk("thought-0", "pondering"), message_chunk("message-0", "done")]);
    assert!(harness.bounds("thinking-text-1").is_none(), "reasoning starts folded");

    harness.click("thinking-1").unwrap();

    assert!(harness.bounds("thinking-text-1").is_some());
}

#[test]
fn the_plan_shows_its_progress_while_it_has_work_left_and_hides_once_finished() {
    let working = plan(
        "plan",
        vec![entry("write tests", acp::PlanEntryStatus::Completed), entry("fix bug", acp::PlanEntryStatus::InProgress)],
    );
    let finished = plan("plan", vec![entry("fix bug", acp::PlanEntryStatus::Completed)]);
    let (mut harness, alpha) = DesktopHarnessBuilder::default()
        .agent("/alpha", FakeAgent::new("alpha-session").prompt_updates(vec![working]))
        .agent("/beta", FakeAgent::new("beta-session").prompt_updates(vec![finished]))
        .start("/alpha");
    assert!(harness.bounds("plan-toggle").is_none(), "there is no plan yet");

    harness.send(alpha, "plan it");
    assert!(harness.bounds("plan-toggle").is_some());
    assert!(harness.bounds("plan-entry-0").is_none(), "the plan starts folded to its progress");

    harness.click("plan-toggle").unwrap();
    assert!(harness.bounds("plan-entry-0").is_some());
    assert!(harness.bounds("plan-entry-1").is_some());

    harness.click("plan-toggle").unwrap();
    assert!(harness.bounds("plan-entry-0").is_none(), "the plan folds back to its progress");

    let beta = harness.start_thread("/beta");
    harness.send(beta, "plan it");
    assert!(harness.bounds("plan-toggle").is_none(), "a finished plan hides once the turn ends");
}

#[test]
fn a_long_transcript_follows_its_latest_reply_until_scrolled_away() {
    let reply = (0..200).map(|line| format!("line {line}")).collect::<Vec<_>>().join("\n\n");
    let (mut harness, thread) = DesktopHarnessBuilder::default()
        .agent("/alpha", FakeAgent::new("alpha-session").prompt_reply(&reply))
        .window_size(900.0, 600.0)
        .start("/alpha");
    harness.send(thread, "talk a lot");
    let track = harness.bounds("transcript-scrollbar").expect("a scrollbar beside the transcript");
    let thumb = harness.bounds("scroll-thumb-vertical").expect("a thumb on the scrollbar");
    assert_eq!(thumb.bottom(), track.bottom(), "the transcript follows its latest reply");
    assert!(harness.bounds("jump-to-latest").is_none());

    harness.scroll("transcript", Point::new(0.0, 200.0)).unwrap();
    assert!(harness.bounds("jump-to-latest").is_some(), "scrolling up stops following");
    harness.click("jump-to-latest").unwrap();
    assert!(harness.bounds("jump-to-latest").is_none(), "jumping back follows again");

    harness.drag("scroll-thumb-vertical", Point::new(track.center().x, track.top())).unwrap();
    assert!(harness.bounds("jump-to-latest").is_some(), "dragging up stops following");
    harness.drag("scroll-thumb-vertical", Point::new(track.center().x, track.bottom())).unwrap();
    assert!(harness.bounds("jump-to-latest").is_none(), "dragging to the bottom follows again");
}

#[test]
fn streaming_respects_the_position_selected_with_the_scrollbar() {
    let (agent, mut requests) = FakeAgent::new("alpha-session").capture();
    let (mut harness, thread) = DesktopHarness::alpha(agent);
    let turn = harness.start_turn(thread, &mut requests, "go");
    let reply = "A long reply.\n\n".repeat(80);
    turn.update(message(&reply));
    harness.run_until(|_| texts(thread).last() == Some(&reply));
    let track = harness.bounds("scroll-track-vertical").unwrap();
    let thumb = harness.bounds("scroll-thumb-vertical").unwrap();
    assert_eq!(thumb.bottom(), track.bottom(), "streaming initially follows the latest text");

    harness.click_at(Point::new(track.center().x, track.top() + 1.0));
    assert!(harness.bounds("jump-to-latest").is_some());
    let prompt = harness.bounds("prompt-0").unwrap();
    turn.update(message("New tail.\n\n"));
    harness.run_until(|_| texts(thread).last().is_some_and(|text| text.ends_with("New tail.\n\n")));
    assert_eq!(harness.bounds("prompt-0").unwrap(), prompt, "new text does not move the reader");
    assert!(harness.bounds("jump-to-latest").is_some());

    harness.click("jump-to-latest").unwrap();
    turn.update(message("Latest tail.\n\n"));
    harness.run_until(|_| texts(thread).last().is_some_and(|text| text.ends_with("Latest tail.\n\n")));
    assert!(harness.bounds("jump-to-latest").is_none());
    let thumb = harness.bounds("scroll-thumb-vertical").unwrap();
    assert_eq!(thumb.bottom(), track.bottom(), "jumping back restores following while streaming");
    turn.end(acp::StopReason::EndTurn);
    harness.run_until(|_| thread.status() == ThreadStatus::Idle);
}

#[test]
fn an_empty_transcript_has_no_scrollbar() {
    let (mut harness, _) = DesktopHarness::alpha(FakeAgent::new("alpha-session"));

    assert!(harness.bounds("transcript-scrollbar").is_none());
    assert!(harness.bounds("scroll-track-vertical").is_none());
}

#[test]
fn a_short_transcript_has_no_scrollbar() {
    let mut harness = answered(vec![message("Hello")]);

    assert!(harness.bounds("transcript-scrollbar").is_none());
}

#[test]
fn a_tool_whose_sub_agent_still_runs_stays_under_a_line_that_says_so() -> Result<(), TestError> {
    let (agent, mut requests) = FakeAgent::new("alpha-session").capture();
    let (mut harness, thread) = DesktopHarness::alpha(agent);
    let turn = harness.start_turn(thread, &mut requests, "go");

    for update in finished_tools(&["Read a", "Read b", "spawn_subagent"]) {
        turn.update(update);
    }
    let child = acp::SubagentUpdate::new("child-a").state(running_state());
    harness
        .wait_for(|| requests.try_connection())
        .send_notification(announced("alpha-session", "call-2", child))
        .unwrap();
    turn.end(acp::StopReason::EndTurn);
    harness.run_until(|_| thread.status() == ThreadStatus::Background);

    assert!(harness.bounds("turn-tail-1").is_some(), "the running sub-agent does not unfold the line");
    assert!(harness.shows("turn-doing-1", "Working"));
    assert!(harness.bounds("turn-activity-1").is_some());

    harness.click("turn-summary-1")?;
    assert!(harness.bounds("tool-card-3").is_some());
    Ok(())
}

#[test]
fn dragging_across_a_prompt_and_its_reply_copies_their_text() {
    let mut harness = answered(vec![message("Some **bold** advice.")]);
    let prompt = harness.bounds("prompt-0").unwrap();
    let reply = harness.bounds("reply-1").unwrap();

    harness.ui().drag(Point::new(prompt.left() + 1.0, prompt.center().y), Point::new(reply.right(), reply.bottom()));
    harness.press("cmd-c");

    assert_eq!(harness.platform().copied_text().as_deref(), Some("go\n\nSome bold advice."));
}

#[test]
fn select_all_copies_the_whole_transcript_including_rows_scrolled_out_of_view() {
    let (agent, mut requests) = FakeAgent::new("alpha-session").capture();
    let (mut harness, thread) = DesktopHarness::alpha(agent);
    let reply = |turn: usize| (0..8).map(|line| format!("reply {turn} line {line}")).collect::<Vec<_>>().join("\n\n");
    for turn in 0..30 {
        play_turn(&mut harness, thread, &mut requests, Vec::new(), &reply(turn));
    }
    assert!(harness.bounds("prompt-0").is_none(), "the first prompt has scrolled out of view");

    harness.click("reply-59").unwrap();
    harness.press("cmd-a");
    harness.press("cmd-c");

    let expected = (0..30).map(|turn| format!("go\n\n{}", reply(turn))).collect::<Vec<_>>().join("\n\n");
    assert_eq!(harness.platform().copied_text(), Some(expected));
}

#[test]
fn a_selection_leaves_out_tool_headers_and_section_labels() {
    let tool = acp::ToolCallUpdate::new("call-1")
        .title("Read file")
        .status(acp::ToolCallStatus::Completed)
        .content(vec![acp::ToolCallContent::from("file contents")]);
    let mut harness = worked(vec![acp::SessionUpdate::ToolCallUpdate(tool), message("Done.")]);
    harness.click("tool-card-1").unwrap();

    harness.click("reply-2").unwrap();
    harness.press("cmd-a");
    harness.press("cmd-c");

    assert_eq!(harness.platform().copied_text().as_deref(), Some("go\n\nfile contents\n\nDone."));
}

#[test]
fn typing_while_the_transcript_has_focus_types_into_the_composer() {
    let mut harness = answered(vec![message("Some advice.")]);
    harness.click("reply-1").unwrap();

    harness.type_text("hi");

    assert!(harness.shows("composer-text", "hi"));
}

fn answered(updates: Vec<acp::SessionUpdate>) -> DesktopHarness {
    let (mut harness, thread) = DesktopHarness::alpha(FakeAgent::new("alpha-session").prompt_updates(updates));
    harness.send(thread, "go");
    harness
}

/// An answered prompt with the work that followed it unfolded.
fn worked(updates: Vec<acp::SessionUpdate>) -> DesktopHarness {
    let mut harness = answered(updates);
    harness.click("turn-summary-1").unwrap();
    harness
}

fn many_tools(count: usize) -> Vec<acp::SessionUpdate> {
    let titles: Vec<String> = (1..=count).map(|call| format!("Read {call}")).collect();
    finished_tools(&titles.iter().map(String::as_str).collect::<Vec<_>>())
}

fn has_sub_agent(core: &SessionCore) -> bool {
    core.conversation().items().iter().any(|item| match item.content() {
        ConversationContent::Tool(tool) => !tool.sub_agents.is_empty(),
        _ => false,
    })
}

fn top(harness: &mut DesktopHarness, id: &str) -> Option<f32> {
    harness.bounds(id).map(|bounds| bounds.top())
}

/// The color the theme gives `part` of a command.
fn syntax_color(harness: &mut DesktopHarness, command: &str, part: &str) -> Option<Color> {
    let at = command.find(part)?;
    let syntax = analyze("bash", command);
    let spans = syntax_spans(syntax.highlights.line(0)?, &harness.theme().syntax);
    spans.into_iter().find(|(range, _)| range.contains(&at))?.1.color
}

fn paints(harness: &mut DesktopHarness, id: &str, color: Color) -> Result<bool, TestError> {
    let bounds = harness.ui().rect_of(id)?;
    let image = harness.ui().render()?;
    let color = color.to_rgba8();
    let within = |x: u16, y: u16| bounds.contains(Point::new(f32::from(x), f32::from(y)));
    let mut pixels = (0..u16::MAX)
        .take_while(|&y| u32::from(y) < image.height)
        .flat_map(|y| (0..u16::MAX).take_while(|&x| u32::from(x) < image.width).map(move |x| (x, y)));
    Ok(pixels.any(|(x, y)| within(x, y) && image.pixel(x.into(), y.into()) == color))
}

fn bash(id: &str, command: &str, status: acp::ToolCallStatus) -> acp::SessionUpdate {
    let tool = acp::ToolCallUpdate::new(id.to_owned())
        .title("Bash")
        .name("bash")
        .status(status)
        .raw_input(serde_json::json!({ "command": command }));
    acp::SessionUpdate::ToolCallUpdate(tool)
}

fn of_kind(kind: acp::ToolKind, title: &str, value: &str, status: acp::ToolCallStatus) -> acp::SessionUpdate {
    let tool = acp::ToolCallUpdate::new(format!("{title} {value}"))
        .title(title)
        .kind(kind)
        .status(status)
        .meta(display_value(value));
    acp::SessionUpdate::ToolCallUpdate(tool)
}

fn display_value(value: &str) -> serde_json::Map<String, serde_json::Value> {
    serde_json::Map::from_iter([("display_value".to_owned(), value.into())])
}

fn play_turn(
    harness: &mut DesktopHarness,
    thread: Thread,
    requests: &mut FakeAgentRequests,
    work: Vec<acp::SessionUpdate>,
    reply: &str,
) {
    let turn = harness.start_turn(thread, requests, "go");
    for update in work {
        turn.update(update);
    }
    turn.reply(reply);
    turn.end(acp::StopReason::EndTurn);
    harness.run_until(|_| thread.status() == ThreadStatus::Idle);
}

fn finished_tools(titles: &[&str]) -> Vec<acp::SessionUpdate> {
    titles
        .iter()
        .enumerate()
        .map(|(index, title)| tool_call(&format!("call-{index}"), title, acp::ToolCallStatus::Completed))
        .collect()
}

fn entry(content: &str, status: acp::PlanEntryStatus) -> acp::PlanEntry {
    acp::PlanEntry::new(content, acp::PlanEntryPriority::Medium, status)
}

fn message(text: &str) -> acp::SessionUpdate {
    message_chunk("message-0", text)
}

fn edit_of(id: &str, path: &str) -> acp::SessionUpdate {
    write_tool_at(id, path, Some(added_file(5)), "")
}

fn write_tool(id: &str, patch: Option<String>, input: &str) -> acp::SessionUpdate {
    write_tool_at(id, "/alpha/page.html", patch, input)
}

fn write_tool_at(id: &str, file: &str, patch: Option<String>, input: &str) -> acp::SessionUpdate {
    let content: Vec<_> = patch
        .map(|patch| acp::Diff::patch(patch, vec![acp::DiffChange::add(file)]))
        .map(acp::ToolCallContent::from)
        .into_iter()
        .collect();
    let tool = acp::ToolCallUpdate::new(id.to_owned())
        .title("Write page.html")
        .status(acp::ToolCallStatus::Completed)
        .raw_input(serde_json::Value::String(input.to_owned()))
        .content(content);
    acp::SessionUpdate::ToolCallUpdate(tool)
}

fn added_file(lines: usize) -> String {
    let body = (0..lines).fold(String::new(), |mut body, line| {
        let _ = writeln!(body, "+<p>line {line}</p>");
        body
    });
    format!(
        "diff --git a/page.html b/page.html\nnew file mode 100644\n--- /dev/null\n+++ b/page.html\n@@ -0,0 +1,{lines} @@\n{body}"
    )
}

fn numbered_lines(lines: usize) -> String {
    (0..lines).map(|line| format!("line {line}")).collect::<Vec<_>>().join("\n")
}
