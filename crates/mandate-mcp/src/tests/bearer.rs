//! The login's token on the MCP session (O2, DEC-947 item 4): once a transport is authorized with
//! the [`AccessToken`] the code exchange returned, every request it sends carries that token as
//! `authorization: Bearer`, and nothing else does; the token stays out of every printout (LT-9).

use secrecy::SecretString;
use serde_json::json;

use super::server::{json, serve};
use crate::{AccessToken, CallClass, McpError, TransportConfig};

const ANSWER: &str = r#"{"jsonrpc":"2.0","id":1,"result":{}}"#;

fn token(secret: &str) -> AccessToken {
    AccessToken {
        secret: SecretString::from(secret.to_owned()),
    }
}

/// Every request an authorized transport sends names the token as its bearer, once.
#[tokio::test]
async fn an_authorized_transport_sends_the_token_as_its_bearer() {
    let server = serve(vec![json(ANSWER), json(ANSWER)]).await;
    let transport = server
        .transport(TransportConfig::CONSERVATIVE)
        .authorized(token("canary-token"))
        .unwrap();
    for _ in 0..2 {
        transport
            .request(CallClass::Ordinary, "tools/list", &json!({}))
            .await
            .unwrap();
    }
    let seen = server.seen();
    assert_eq!(seen.len(), 2);
    for sent in seen {
        let head = sent.split("\r\n\r\n").next().unwrap();
        let bearers: Vec<&str> = head
            .lines()
            .filter(|line| line.starts_with("authorization:"))
            .collect();
        assert_eq!(bearers, ["authorization: Bearer canary-token"], "{head}");
    }
}

/// A transport that was never authorized sends no bearer at all.
#[tokio::test]
async fn an_unauthorized_transport_sends_no_bearer() {
    let server = serve(vec![json(ANSWER)]).await;
    let transport = server.transport(TransportConfig::CONSERVATIVE);
    transport
        .request(CallClass::Ordinary, "tools/list", &json!({}))
        .await
        .unwrap();
    assert!(!server.seen()[0].contains("authorization"), "{:?}", server.seen());
}

/// A token no header can carry is refused, never sent mangled.
#[tokio::test]
async fn a_token_no_header_can_carry_is_refused() {
    let server = serve(vec![]).await;
    let refused = server
        .transport(TransportConfig::CONSERVATIVE)
        .authorized(token("canary\ntoken"))
        .err();
    assert!(matches!(refused, Some(McpError::Malformed)), "{refused:?}");
}

/// The authorized transport's printout names the endpoint and never the token (LT-9).
#[tokio::test]
async fn an_authorized_transport_never_prints_its_token() {
    let server = serve(vec![]).await;
    let transport = server
        .transport(TransportConfig::CONSERVATIVE)
        .authorized(token("canary-token"))
        .unwrap();
    let printed = format!("{transport:?}");
    assert!(!printed.contains("canary"), "{printed}");
}
