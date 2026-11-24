use crate::support::Edges;
use crate::support::{replying, texts};
use agent_client_protocol::v2::schema as acp;
use foxfire_acp::testing::{FakeAgent, FakeAgentRequests, model_select, select};
use foxfire_core::testing::TestWorkspace;
use foxfire_desktop::testing::{DesktopHarness, DesktopHarnessBuilder};
use foxfire_desktop::threads::ThreadStatus;
use rouge::core::access::Role;
use rouge::core::testing::TestError;

#[test]
fn up_and_down_walk_the_prompts_sent_before() {
    let (mut harness, thread) = DesktopHarness::alpha(replying("alpha-session", "ok"));
    harness.send(thread, "first");
    harness.send(thread, "second");

    harness.press("up");
    assert!(harness.shows("composer-text", "second"));
    harness.press("up");
    assert!(harness.shows("composer-text", "first"));
    harness.press("down");
    assert!(harness.shows("composer-text", "second"));
    harness.press("down");
    assert!(harness.shows("composer-text", ""));
}

#[test]
fn up_in_a_draft_moves_the_cursor_instead_of_recalling() {
    let (mut harness, thread) = DesktopHarness::alpha(replying("alpha-session", "ok"));
    harness.send(thread, "first");

    harness.type_text("draft");
    harness.press("up");

    assert!(harness.shows("composer-text", "draft"));
}

#[test]
fn a_slash_offers_the_agents_commands_and_sends_the_chosen_one() {
    let (mut harness, thread) = DesktopHarness::alpha(advertising_commands());
    harness.send(thread, "hi");

    harness.type_text("/rev");
    assert!(harness.bounds("completion-item-0").is_some(), "review matches");
    assert!(harness.bounds("completion-item-1").is_none(), "explain does not");
    harness.press("enter");
    harness.run_until(|_| texts(thread).contains(&"/review".to_owned()));

    assert!(harness.shows("composer-text", ""));
}

#[test]
fn a_long_description_never_hides_the_command_name() {
    let (mut harness, thread) = DesktopHarness::alpha(advertising_commands());
    harness.send(thread, "hi");

    harness.type_text("/sum");
    let menu = harness.bounds("completion-menu").expect("menu painted");
    let name = harness.bounds("completion-item-0-label").expect("name painted");

    assert!(name.size.width > 0.0, "the name keeps its width: {name:?}");
    assert!(name.max().x <= menu.max().x, "the name sits inside the menu: {name:?} {menu:?}");
    assert!(name.origin.x >= menu.origin.x, "the name sits inside the menu: {name:?} {menu:?}");
}

#[test]
fn a_command_that_takes_input_waits_for_it() {
    let (mut harness, thread) = DesktopHarness::alpha(advertising_commands());
    harness.send(thread, "hi");

    harness.type_text("/expl");
    harness.press("tab");

    assert!(harness.shows("composer-text", "/explain "));
    assert!(harness.bounds("completion-menu").is_none());
}

#[test]
fn escape_closes_the_menu_until_the_token_ends() {
    let (mut harness, thread) = DesktopHarness::alpha(advertising_commands());
    harness.send(thread, "hi");
    harness.type_text("/");
    assert!(harness.bounds("completion-menu").is_some());

    harness.press("escape");
    harness.type_text("r");

    assert!(harness.bounds("completion-menu").is_none());
    assert!(harness.shows("composer-text", "/r"));
}

#[test]
fn an_at_mention_sends_the_file_with_the_prompt() {
    let workspace = TestWorkspace::new()
        .git_init()
        .file("src/lib.rs", "pub fn answer() -> u32 { 42 }\n")
        .file("README.md", "# alpha\n");
    let root = workspace.root();
    let (agent, mut requests) = FakeAgent::new("alpha-session").capture();
    let (mut harness, _) = DesktopHarnessBuilder::default().agent(root, agent).start(root);

    harness.type_text("explain @lib");
    harness.run_until_painted("completion-item-0");
    harness.press("enter");
    assert!(harness.shows("composer-text", "explain @src/lib.rs "));
    harness.press("enter");

    let prompt = harness.next(&mut requests.prompt);
    let sent = format!("{:?}", prompt.prompt);
    assert!(sent.contains("explain @src/lib.rs"), "the prompt names the file: {sent}");
    assert!(sent.contains("pub fn answer() -> u32 { 42 }"), "the prompt carries the file: {sent}");
}

#[test]
fn the_option_shows_the_model_the_agent_switched_to() {
    let (agent, mut requests) = agent_with_models(&["fast", "slow"], "fast");
    let (mut harness, thread) = DesktopHarness::alpha(agent);
    assert!(harness.shows("config-model-value", "fast"));

    harness.click("config-model").unwrap();
    harness.click("config-model-picker-model-slow").unwrap();
    harness.next(&mut requests.config);
    harness.run_until(|_| {
        thread
            .with_connected_core(|core| {
                let options = core.session().config_options();
                let model = options.iter().find(|option| option.id == "model");
                model.and_then(|model| model.select()).is_some_and(|model| model.current_value == "model-slow")
            })
            .unwrap_or(false)
    });

    assert!(harness.shows("config-model-value", "slow"));
}

#[test]
fn only_options_of_a_known_category_are_led_by_an_icon() {
    let option = |id: &str| select(id, id, "one", &[("one", "One")]);
    let options = vec![
        option("mode").category(acp::SessionConfigOptionCategory::Mode),
        option("model").category(acp::SessionConfigOptionCategory::Model),
        option("reasoning_effort").category(acp::SessionConfigOptionCategory::ThoughtLevel),
        option("verbosity"),
    ];
    let (mut harness, _) = DesktopHarness::alpha(offering(options));

    for categorized in ["mode", "model", "reasoning_effort"] {
        assert!(harness.bounds(&format!("config-{categorized}-icon")).is_some(), "{categorized} has an icon");
    }
    assert!(harness.bounds("config-verbosity").is_some());
    assert!(harness.bounds("config-verbosity-icon").is_none(), "an option of no known category has no icon");
}

#[test]
fn the_sessions_options_sit_in_the_status_row_under_the_composer_in_the_chat_alone() {
    let (agent, mut requests) = agent_with_models(&["fast", "slow"], "fast");
    let (mut harness, thread) = DesktopHarness::alpha(agent);
    assert!(harness.bounds("new-thread-heading").is_some(), "a new thread asks what to build");
    assert_options_lead_the_status_row(&mut harness);

    let turn = harness.start_turn(thread, &mut requests, "hi");
    turn.update(acp::SessionUpdate::UsageUpdate(acp::UsageUpdate::new(50_000, 200_000)));
    turn.end(acp::StopReason::EndTurn);
    harness.run_until(|_| thread.status() == ThreadStatus::Idle);

    assert!(harness.bounds("new-thread-heading").is_none(), "the composer has docked");
    assert_options_lead_the_status_row(&mut harness);
    let [model, meter] = ["config-model", "context-meter"].map(|id| harness.bounds(id).expect("painted"));
    assert!(model.right() < meter.left(), "the options come before the context meter: {model:?} {meter:?}");

    harness.press("secondary-2");

    assert!(harness.bounds("config-model").is_none(), "the options leave with the chat");
}

#[test]
fn the_model_picker_opens_on_the_current_model() {
    let (agent, mut requests) = agent_with_models(&["one", "two", "three"], "two");
    let (mut harness, _) = DesktopHarness::alpha(agent);

    harness.click("config-model").unwrap();
    assert!(harness.bounds("picker-query").is_none(), "a short list needs no search");
    harness.press("down enter");

    let request = harness.next(&mut requests.config);
    assert_eq!(format!("{:?}", request.value), format!("{:?}", acp::SessionConfigOptionValue::from("model-three")));
}

#[test]
fn a_long_list_of_models_is_searchable_and_scrolls_inside_the_window() {
    let names: Vec<String> = (0..80).map(|n| format!("m{n}")).collect();
    let (agent, _requests) = agent_with_models(&names.iter().map(String::as_str).collect::<Vec<_>>(), "m0");
    let (mut harness, _) =
        DesktopHarnessBuilder::default().agent("/alpha", agent).window_size(1000.0, 600.0).start("/alpha");

    harness.click("config-model").unwrap();

    assert!(harness.bounds("picker-query").is_some());
    let picker = harness.bounds("config-model-picker").expect("the picker opens");
    assert!(picker.top() >= 0.0, "the picker starts inside the window");
    assert!(picker.bottom() <= 600.0, "the list scrolls instead of growing past the window");
}

#[test]
fn the_context_meter_shows_once_the_agent_reports_usage() -> Result<(), TestError> {
    let (agent, mut requests) = FakeAgent::new("alpha-session").capture();
    let (mut harness, thread) = DesktopHarness::alpha(agent);
    assert!(harness.bounds("context-meter").is_none(), "nothing is reported yet");

    let turn = harness.start_turn(thread, &mut requests, "hi");
    turn.update(usage(50_000));
    turn.end(acp::StopReason::EndTurn);
    harness.run_until(|_| thread.status() == ThreadStatus::Idle);

    let (meter, composer) = (harness.bounds("context-meter").unwrap(), harness.bounds("composer").unwrap());
    assert!(meter.top() >= composer.bottom(), "the meter is on the bar below the composer");
    let cells = harness.ui().access_node(Role::ProgressIndicator, "Context used")?;
    assert_eq!(cells.numeric_value, Some(0.25));
    assert!(harness.bounds("session-cost").is_none(), "the agent reports no cost");

    harness.click("context-meter")?;
    assert!(harness.shows("usage-context", "Context50k / 200k tokens"));
    assert!(harness.shows("usage-left", "Left150k tokens"));
    assert!(harness.bounds("usage-cost").is_none(), "the agent reports no cost");
    Ok(())
}

#[test]
fn the_context_meter_shows_the_sessions_cost_beside_its_cells_and_follows_both_while_open() -> Result<(), TestError> {
    let (agent, mut requests) = FakeAgent::new("alpha-session").capture();
    let (mut harness, thread) = DesktopHarness::alpha(agent);
    let turn = harness.start_turn(thread, &mut requests, "hi");

    turn.update(acp::SessionUpdate::UsageUpdate(
        acp::UsageUpdate::new(94_000, 200_000).cost(acp::Cost::new(2.47, "USD")),
    ));
    harness.run_until(|harness| harness.shows("session-cost", "$2.47"));
    let [cells, cost] = ["context-cells", "session-cost"].map(|id| harness.bounds(id).expect("painted"));
    assert!(cells.right() < cost.left(), "the cost follows the cells: {cells:?} {cost:?}");

    harness.click("context-meter")?;
    assert!(harness.shows("usage-cost", "Session cost$2.47"));

    turn.update(usage(160_000));
    harness.run_until(|harness| harness.shows("usage-context", "Context160k / 200k tokens"));
    assert!(harness.shows("usage-cost", "Session cost$2.47"), "an update without a cost leaves it as it was");
    Ok(())
}

#[test]
fn a_window_of_unknown_size_shows_only_the_tokens_used() -> Result<(), TestError> {
    let (agent, mut requests) = FakeAgent::new("alpha-session").capture();
    let (mut harness, thread) = DesktopHarness::alpha(agent);
    let turn = harness.start_turn(thread, &mut requests, "hi");

    turn.update(acp::SessionUpdate::UsageUpdate(acp::UsageUpdate::new(12_000, 0)));
    harness.run_until_painted("context-meter");

    harness.click("context-meter")?;
    assert!(harness.shows("usage-context", "Context12k tokens"));
    assert!(harness.bounds("usage-left").is_none());
    Ok(())
}

#[test]
fn a_prompt_the_agent_has_not_taken_up_shows_as_queued() {
    let (agent, mut requests) = FakeAgent::new("alpha-session").hold::<acp::PromptRequest>().capture();
    let (mut harness, thread) = DesktopHarness::alpha(agent);

    harness.type_text("later");
    harness.press("enter");
    let (_, responder) = harness.next(requests.held::<acp::PromptRequest>());
    assert!(harness.bounds("queued-prompts").is_some());

    responder.respond(acp::PromptResponse::new("user-message")).unwrap();
    harness.run_until(|_| thread.with_core(|core| core.queued_prompts().is_empty()).unwrap_or(false));
    assert!(harness.bounds("queued-prompts").is_none());
}

#[test]
fn prompt_search_puts_a_past_prompt_back_in_the_composer() {
    let agent = replying("alpha-session", "ok").prompt_history(&["fix the build", "write the docs"]);
    let (mut harness, _) = DesktopHarness::alpha(agent);

    harness.press("secondary-r");
    harness.type_text("build");
    harness.run_until_painted("prompt-search-result-0");
    assert!(harness.bounds("prompt-search-result-1").is_none(), "only one prompt matches");
    harness.press("enter");

    assert!(harness.bounds("prompt-search").is_none());
    assert!(harness.shows("composer-text", "fix the build"));
}

#[test]
fn prompt_search_is_offered_only_by_agents_that_support_it() {
    let (mut harness, _) = DesktopHarness::alpha(replying("alpha-session", "ok"));

    harness.press("secondary-r");

    assert!(harness.bounds("prompt-search").is_none());
}

#[test]
fn prompt_search_is_not_offered_once_the_agent_disconnects() {
    let agent = replying("alpha-session", "ok").prompt_history(&["fix the build"]);
    let (mut harness, thread) = DesktopHarness::alpha(agent);

    harness.connector().kill_agents("/alpha");
    harness.run_until(|_| thread.status() == ThreadStatus::Disconnected);
    harness.press("secondary-r");

    assert!(harness.bounds("prompt-search").is_none(), "a disconnected agent cannot answer a search");
}

fn advertising_commands() -> FakeAgent {
    let explain = acp::Command::new("explain", "Explain something")
        .input(acp::AvailableCommandInput::Text(acp::TextCommandInput::new("topic")));
    let review = acp::Command::new("review", "Review the diff");
    let summarize = acp::Command::new("summarize", "Summarize ".repeat(80));
    let commands = acp::AvailableCommandsUpdate::new(vec![
        acp::AvailableCommand::Command(review),
        acp::AvailableCommand::Command(explain),
        acp::AvailableCommand::Command(summarize),
    ]);
    FakeAgent::new("alpha-session").prompt_updates(vec![acp::SessionUpdate::AvailableCommandsUpdate(commands)])
}

fn agent_with_models(names: &[&str], current: &str) -> (FakeAgent, FakeAgentRequests) {
    let values: Vec<_> = names.iter().map(|name| format!("model-{name}")).collect();
    let values: Vec<_> = values.iter().map(String::as_str).zip(names.iter().copied()).collect();
    offering(vec![model_select(&format!("model-{current}"), &values)]).capture()
}

fn usage(used: u64) -> acp::SessionUpdate {
    acp::SessionUpdate::UsageUpdate(acp::UsageUpdate::new(used, 200_000))
}

fn offering(options: Vec<acp::SessionConfigOption>) -> FakeAgent {
    FakeAgent::default().new_session_response(acp::NewSessionResponse::new("alpha-session").config_options(options))
}

fn assert_options_lead_the_status_row(harness: &mut DesktopHarness) {
    let [model, composer] = ["config-model", "composer"].map(|id| harness.bounds(id).expect("painted"));
    assert!(model.top() >= composer.bottom(), "the options sit below the composer: {model:?} {composer:?}");
}

#[test]
fn a_keystroke_repaints_only_the_composer_and_redraws_a_sliver_of_the_window() {
    let (mut harness, thread) = DesktopHarness::alpha(replying("alpha-session", "ok"));
    harness.send(thread, "first");
    harness.type_text("draft");
    let ui = harness.ui();
    ui.draw().unwrap();
    let before = ui.stats();

    ui.type_text("x");
    let work = ui.stats() - before;
    let frame = ui.draw().unwrap();

    assert_eq!(work.relayouts, 0);
    assert_eq!(work.layer_paints, 1, "only the composer's layer is repainted");
    assert!(frame.damaged < 50_000, "redrew {} pixels, not a sliver of the 1280×840 window", frame.damaged);
}
