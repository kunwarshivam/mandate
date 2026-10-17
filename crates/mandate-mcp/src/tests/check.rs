//! The session the connect check reads through (connections spec §8.1, journal spec §9.8; R0
//! blocker R1, DEC-946): the login's token as the bearer of every request, and a first connect
//! that reports the listed contract to the permission checks instead of refusing it, so a failed
//! check is journaled rather than lost.

use std::collections::BTreeSet;

use secrecy::SecretString;
use serde_json::json;

use super::contract::{OK, base, cat, handshake, listing, roomy, sent_names, without};
use super::server::{reply, serve};
use crate::{AccessToken, CallClass, McpClient, McpError};

fn token(secret: &str) -> AccessToken {
    AccessToken {
        secret: SecretString::from(secret.to_owned()),
    }
}

fn names(listed: &[serde_json::Value]) -> BTreeSet<String> {
    listed
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn the_bearer_is_sent_on_every_request_of_the_session() {
    let server = serve(cat(
        handshake(),
        vec![listing(&base()), reply(OK), reply(OK)],
    ))
    .await;
    let transport = server
        .transport(roomy())
        .with_bearer(token("canary-bearer"));
    let client = McpClient::connect(transport, None).await.unwrap();
    client
        .call_tool(CallClass::Ordinary, "get_accounts", &json!({}))
        .await
        .unwrap();
    client
        .call_tool(CallClass::RiskReducing, "cancel_equity_order", &json!({}))
        .await
        .unwrap();
    let seen = server.seen();
    assert_eq!(seen.len(), 5, "{seen:?}");
    for request in &seen {
        let head = request.split_once("\r\n\r\n").unwrap().0;
        let bearers: Vec<&str> = head
            .lines()
            .filter(|line| line.starts_with("authorization:"))
            .collect();
        assert_eq!(bearers, ["authorization: bearer canary-bearer"], "{head}");
    }
}

#[tokio::test]
async fn a_transport_without_a_bearer_sends_no_authorization() {
    let server = serve(cat(handshake(), vec![listing(&base())])).await;
    let transport = server.transport(roomy());
    McpClient::connect(transport, None).await.unwrap();
    for request in server.seen() {
        assert!(!request.contains("authorization"), "{request}");
    }
}

#[tokio::test]
async fn the_bearer_never_prints() {
    let server = serve(cat(handshake(), vec![listing(&base())])).await;
    let transport = server
        .transport(roomy())
        .with_bearer(token("canary-bearer"));
    assert!(
        !format!("{transport:?}").contains("canary"),
        "{transport:?}"
    );
    let (client, listed) = McpClient::connect_to_check(transport).await.unwrap();
    assert!(!format!("{client:?}").contains("canary"), "{client:?}");
    assert!(!format!("{listed:?}").contains("canary"), "{listed:?}");
}

#[tokio::test]
async fn a_clean_contract_is_reported_whole_and_pinned_as_connect_pins_it() {
    let (_plain, pinned) = {
        let server = serve(cat(handshake(), vec![listing(&base())])).await;
        let client = McpClient::connect(server.transport(roomy()), None)
            .await
            .unwrap();
        let hash = client.contract().unwrap().clone();
        (server, hash)
    };
    let server = serve(cat(handshake(), vec![listing(&base()), reply(OK)])).await;
    let (client, listed) = McpClient::connect_to_check(server.transport(roomy()))
        .await
        .unwrap();
    assert_eq!(listed.names, names(&base()));
    assert!(listed.allowlisted_present);
    assert_eq!(client.contract().unwrap(), &pinned);
    assert!(!client.openings_halted().unwrap());
    client
        .call_tool(CallClass::Ordinary, "place_equity_order", &json!({}))
        .await
        .unwrap();
    assert_eq!(sent_names(&server), ["place_equity_order"]);
}

#[tokio::test]
async fn a_fund_movement_tool_is_reported_and_halts_openings_instead_of_refusing() {
    let mut listed_tools = base();
    listed_tools.push(json!({"name": "withdraw_funds", "inputSchema": {"type": "object"}}));
    let server = serve(cat(handshake(), vec![listing(&listed_tools), reply(OK)])).await;
    let (client, listed) = McpClient::connect_to_check(server.transport(roomy()))
        .await
        .unwrap();
    assert_eq!(listed.names, names(&listed_tools));
    assert!(listed.names.contains("withdraw_funds"));
    assert!(listed.allowlisted_present);
    assert!(client.openings_halted().unwrap());
    let refused = client
        .call_tool(CallClass::Ordinary, "place_equity_order", &json!({}))
        .await;
    assert!(
        matches!(refused, Err(McpError::ContractDrift)),
        "{refused:?}"
    );
    client
        .call_tool(CallClass::Ordinary, "get_accounts", &json!({}))
        .await
        .unwrap();
    assert_eq!(sent_names(&server), ["get_accounts"]);
}

#[tokio::test]
async fn a_missing_allowlisted_tool_is_reported_and_halts_openings() {
    let listed_tools = without("get_equity_orders");
    let server = serve(cat(handshake(), vec![listing(&listed_tools)])).await;
    let (client, listed) = McpClient::connect_to_check(server.transport(roomy()))
        .await
        .unwrap();
    assert_eq!(listed.names, names(&listed_tools));
    assert!(!listed.allowlisted_present);
    assert!(client.openings_halted().unwrap());
}

#[tokio::test]
async fn an_unreadable_listing_gives_no_client() {
    let server = serve(cat(handshake(), vec![reply(r#""result":{"tools":7}"#)])).await;
    let refused = McpClient::connect_to_check(server.transport(roomy())).await;
    assert!(matches!(refused, Err(McpError::Malformed)), "{refused:?}");
}
