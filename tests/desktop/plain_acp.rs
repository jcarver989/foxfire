//! Most agents speak only standard ACP. A whole thread against one must work without touching Aether's extensions.

use crate::support::texts;
use acp_utils::conversation::{ConversationContent, Notice, NoticeLevel};
use acp_utils::testing::{FakeAgent, Question, select};
use aether_desktop::testing::DesktopHarnessBuilder;
use aether_desktop::threads::{Thread, ThreadStatus};
use agent_client_protocol::v2::schema as acp;
use tempfile::TempDir;

#[test]
fn a_plain_acp_agent_runs_a_whole_thread_without_any_aether_extension() {
    let workspace = TempDir::new().unwrap();
    let (agent, mut requests) = plain_agent().capture();
    let (mut harness, thread) = DesktopHarnessBuilder::default().agent(workspace.path(), agent).start(workspace.path());

    harness.platform().copy_image(PNG);
    harness.press("secondary-v");
    harness.press("enter");
    let prompt = harness.next(&mut requests.prompt).prompt;
    assert!(
        prompt.iter().any(|block| matches!(block, acp::ContentBlock::Image(_))),
        "an image goes to an agent that takes images, though its model says nothing about them: {prompt:?}"
    );

    harness.run_until(|_| thread.status() == ThreadStatus::NeedsInput);
    harness.click("permission-option-0").unwrap();
    harness.run_until(|_| thread.status() == ThreadStatus::Idle && texts(thread).len() == 2);

    assert!(texts(thread).last().is_some_and(|answer| answer.contains(r#""optionId":"allow""#)));

    assert!(harness.shows("config-llm-value", "Fast"), "the model is found by its category, not its id");
    harness.click("config-llm").unwrap();
    harness.click("config-llm-picker-slow").unwrap();
    assert_eq!(harness.next(&mut requests.config).config_id.0.as_ref(), "llm");

    assert_eq!(errors(thread), Vec::<String>::new());
    assert_eq!(requests.extensions.try_recv().ok(), None, "no Aether extension is asked for");
}

const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

fn plain_agent() -> FakeAgent {
    let images = acp::PromptCapabilities::new().image(acp::PromptImageCapabilities::new());
    let model = select("llm", "LLM", "fast", &[("fast", "Fast"), ("slow", "Slow")])
        .category(acp::SessionConfigOptionCategory::Model);
    let options = vec![
        acp::PermissionOption::new("allow", "Allow once", acp::PermissionOptionKind::AllowOnce),
        acp::PermissionOption::new("reject", "Reject", acp::PermissionOptionKind::RejectOnce),
    ];
    FakeAgent::default()
        .agent_info(acp::Implementation::new("plain-agent", "1.0"))
        .capabilities(acp::AgentCapabilities::new().session(acp::SessionCapabilities::new().prompt(images)))
        .new_session_response(acp::NewSessionResponse::new("plain-session").config_options(vec![model]))
        .prompt_asks("Run the tests?", Question::Permission(options))
}

fn errors(thread: Thread) -> Vec<String> {
    thread
        .with_core(|core| {
            let items = core.conversation().items().iter();
            items
                .filter_map(|item| match item.content() {
                    ConversationContent::Notice(Notice { level: NoticeLevel::Error, text }) => Some(text.clone()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}
