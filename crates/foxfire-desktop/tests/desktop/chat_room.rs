use crate::support::Edges;
use agent_client_protocol::v2::schema::{self as acp, SessionId};
use foxfire_acp::chat::{
    ChatChannelAnchor, ChatChannelJoinResponse, ChatChannelMember, ChatChannelPage, ChatMessage, ChatMessageKind,
    ClientMeta, FoxfireMeta, Participant,
};
use foxfire_acp::testing::{FakeAgent, tool_call};
use foxfire_desktop::testing::DesktopHarness;
use foxfire_desktop::threads::{Thread, ThreadStatus};
use rouge::core::Point;
use rouge::core::testing::TestError;
use std::path::Path;

#[test]
fn a_shared_thread_introduces_the_user_and_opens_the_room_beside_it() {
    let (agent, mut requests) = shared_agent().capture();
    let (mut harness, _) = DesktopHarness::alpha(agent);
    harness.run_until_painted("chat-room-panel");
    let introduced = ClientMeta::from_meta(harness.next(&mut requests.initialize).meta.as_ref()).participant;
    let introduced = introduced.filter(|participant| !participant.name.is_empty());
    assert_eq!(introduced.map(|participant| participant.id), Some(harness.registry().participant().id));
    assert!(harness.bounds("thread-row-0-people").is_some());

    let room = harness.bounds("chat-room-resizable").unwrap_or_default();
    let edge = Point::new(room.origin.x, room.center().y);
    harness.ui().drag(edge, Point::new(edge.x - 40.0, edge.y));
    assert!(harness.registry().with(|state| state.chat_room_width) > Some(room.size.width));

    harness.press("secondary-j");
    assert!(harness.bounds("chat-room-panel").is_none());
    harness.press("secondary-j");
    assert!(harness.bounds("chat-room-panel").is_some());
}

#[test]
fn messages_picked_for_a_prompt_stay_with_the_session_they_came_from() -> Result<(), TestError> {
    let (mut harness, thread): (_, Thread) = DesktopHarness::alpha(shared_agent());
    harness.run_until_painted("chat-post-1");
    harness.hover("chat-post-1")?;
    harness.click("chat-select-1")?;
    harness.click("chat-room-ask-agent")?;
    harness.advance();
    assert!(harness.bounds("composer-chat-sources").is_some());

    thread.update_core(|core| core.resume_session(&SessionId::new("elsewhere"), Path::new("/alpha")));
    harness.run_until_painted("chat-post-1");
    assert!(harness.bounds("composer-chat-sources").is_none());
    Ok(())
}

#[test]
fn following_a_quote_unfolds_the_turn_and_shows_the_call() -> Result<(), TestError> {
    for running in [false, true] {
        let (agent, mut requests) = quoting_agent("call-50").capture();
        let (mut harness, thread) = DesktopHarness::alpha(agent);
        let turn = harness.start_turn(thread, &mut requests, "go");
        for call in 1..=100 {
            let id = format!("call-{call}");
            turn.update(tool_call(&id, &format!("Read {call}"), acp::ToolCallStatus::Completed));
        }
        harness.run_until_painted("tool-card-100");
        if !running {
            turn.end(acp::StopReason::EndTurn);
            harness.run_until(|_| thread.status() == ThreadStatus::Idle);
        }

        harness.click("chat-anchor-1")?;
        harness.run_until_painted("tool-card-50");
        let view = harness.ui().rect_of("transcript-view")?;
        let call = harness.ui().rect_of("tool-card-50")?;
        assert!(call.top() >= view.top() && call.bottom() <= view.bottom(), "{call:?} is out of {view:?}");

        harness.scroll("transcript-selection", Point::new(0.0, -4000.0))?;
        harness.run_until_painted("tool-card-100");
        harness.click("chat-anchor-1")?;
        harness.run_until_painted("tool-card-50");
    }
    Ok(())
}

fn shared_agent() -> FakeAgent {
    agent_sharing(None)
}

fn quoting_agent(item: &str) -> FakeAgent {
    agent_sharing(Some(ChatChannelAnchor { item: item.to_owned(), excerpt: "Read".to_owned() }))
}

fn agent_sharing(anchor: Option<ChatChannelAnchor>) -> FakeAgent {
    let maya = Participant { id: "maya".to_owned(), name: "Maya".to_owned() };
    let message = ChatMessage {
        seq: 1,
        author: maya.id.clone(),
        body: "Cap it at 30s".to_owned(),
        sent_at_ms: 0,
        kind: ChatMessageKind::Post { key: "1".to_owned(), anchor, after_item: None },
    };
    FakeAgent::new("alpha-session").chat(ChatChannelJoinResponse {
        present: vec![maya.id.clone()],
        members: vec![ChatChannelMember { participant: maya, joined_at_ms: 0 }],
        read: 0,
        page: ChatChannelPage { messages: vec![message], more: false },
    })
}
