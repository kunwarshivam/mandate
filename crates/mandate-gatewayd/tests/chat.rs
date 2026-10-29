//! The live registry file's chat entries (DEC-2293 item 13): the committed file names Sonnet and
//! Haiku, each reads strictly, and each is offered on its one endpoint with no start probe, beside
//! research entries that stay `evaluating`.

use std::path::PathBuf;

use mandate_canon::parse;
use mandate_gateway::registry::{Registry, Status};
use mandate_gatewayd::live::{chat_entries, chat_upstreams, read_chat, register_chat};

fn committed() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../deploy/cell/model-gateway/registry.json")
}

#[test]
fn the_committed_registry_offers_both_chat_models() {
    let chats = read_chat(&committed()).unwrap();
    let ids: Vec<&str> = chats.iter().map(|c| c.entry.model_id.as_str()).collect();
    assert_eq!(ids, ["llm.chat_sonnet", "llm.chat_haiku"]);
    let mut registry = Registry::new();
    assert!(register_chat(&mut registry, &chats).is_empty(), "every chat entry registers");
    for chat in &chats {
        let pin = chat.entry.content().model_ref();
        let entry = registry.entry(&pin).unwrap();
        assert_eq!(entry.status(), Status::Offered, "{} is offered", pin.model_id);
        assert_eq!(entry.endpoints().count(), 1, "{} has its endpoint", pin.model_id);
    }
    assert_eq!(
        chat_upstreams(&chats).into_iter().collect::<Vec<_>>(),
        ["google-vertex/global"]
    );
}

#[test]
fn a_file_without_chat_has_none_and_a_broken_entry_refuses_the_file() {
    let none = parse(br#"{"entries":[]}"#).unwrap();
    assert_eq!(chat_entries(&none), Some(Vec::new()));
    let broken = parse(br#"{"chat":[{"entry":{"model_id":"llm.chat_x"}}],"entries":[]}"#).unwrap();
    assert_eq!(chat_entries(&broken), None);
}
