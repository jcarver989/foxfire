use agent_client_protocol::v2::schema::Meta;
use foxfire_acp::chat::{
    ChatChannelAnchor, ChatChannelEvent, ChatChannelHistoryFrom, ChatChannelMember, ChatMessage, ChatMessageKind,
    ClientMeta, FOXFIRE_META_NAMESPACE, FoxfireMeta, Participant, PromptMeta,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Error, Value, json};
use std::fmt::Debug;

#[test]
fn events_keep_the_form_they_are_sent_and_stored_in() -> Result<(), Error> {
    let anchor = Some(ChatChannelAnchor { item: "call".to_owned(), excerpt: "cargo test".to_owned() });
    round_trips(
        &message(ChatMessageKind::Post { key: "key".to_owned(), anchor, after_item: Some("last".to_owned()) }),
        json!({ "type": "message", "message": {
            "seq": 7, "author": "maya", "body": "hello", "sentAtMs": 1000,
            "kind": "post", "key": "key", "afterItem": "last", "anchor": { "item": "call", "excerpt": "cargo test" },
        }}),
    )?;
    round_trips(
        &message(ChatMessageKind::Ask { message_id: "prompt".into(), sources: vec![3, 5] }),
        json!({ "type": "message", "message": {
            "seq": 7, "author": "maya", "body": "hello", "sentAtMs": 1000,
            "kind": "ask", "messageId": "prompt", "sources": [3, 5],
        }}),
    )?;
    round_trips(
        &ChatChannelEvent::Member { member: ChatChannelMember { participant: maya(), joined_at_ms: 1000 } },
        json!({ "type": "member", "member": { "id": "maya", "name": "Maya", "joinedAtMs": 1000 } }),
    )?;
    round_trips(
        &ChatChannelEvent::Read { participant: "maya".to_owned(), seq: 7 },
        json!({ "type": "read", "participant": "maya", "seq": 7 }),
    )?;
    round_trips(&ChatChannelHistoryFrom::Before(7), json!({ "before": 7 }))
}

#[test]
fn metadata_is_namespaced_and_absent_by_default() {
    let client = ClientMeta { participant: Some(maya()) };
    let meta = client.to_meta();
    let expected = json!({ "foxfire": { "participant": { "id": "maya", "name": "Maya" } } });
    assert_eq!(Value::Object(meta.clone()), expected);
    assert_eq!(ClientMeta::from_meta(Some(&meta)), client);

    assert_eq!(PromptMeta::default().to_meta(), Meta::new());
    for malformed in [json!(null), json!({ "participant": { "id": "maya" } })] {
        let meta = Meta::from_iter([(FOXFIRE_META_NAMESPACE.to_owned(), malformed)]);
        assert_eq!(ClientMeta::from_meta(Some(&meta)), ClientMeta::default());
    }
}

fn maya() -> Participant {
    Participant { id: "maya".to_owned(), name: "Maya".to_owned() }
}

fn message(kind: ChatMessageKind) -> ChatChannelEvent {
    let message = ChatMessage { seq: 7, author: "maya".to_owned(), body: "hello".to_owned(), sent_at_ms: 1000, kind };
    ChatChannelEvent::Message { message }
}

fn round_trips<T: Serialize + DeserializeOwned + Debug + PartialEq>(value: &T, json: Value) -> Result<(), Error> {
    assert_eq!(serde_json::to_value(value)?, json);
    assert_eq!(&serde_json::from_value::<T>(json)?, value);
    Ok(())
}
