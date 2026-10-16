//! The tool seam over the client (DEC-860 item 2, DEC-928 item 1): what [`Tools::call_tool`]
//! answers is the server's `result` byte for byte, the class it is handed is the budget it draws
//! on, and every refusal of the client is the seam's, with nothing sent.

use std::time::Duration;

use serde_json::json;

use super::contract::{OK, base, connected, listing, roomy, sent, session};
use super::server::reply;
use crate::TransportConfig;
use crate::{BucketConfig, BudgetConfig, CallClass, ContractHash, McpError, Tools};

/// A result whose spacing and member order no re-serialization would keep.
const SPACED: &str = r#"{ "structuredContent" : {"z":1,"a":"x"} ,"isError":false}"#;

#[tokio::test]
async fn the_seam_answers_the_result_exactly_as_the_server_wrote_it() {
    let answer = reply(&format!(r#""result":{SPACED}"#));
    let (server, client) = connected(&base(), None, vec![answer]).await;
    let arguments = json!({"account_number": "5QR00001", "symbol": "SPY"});
    let text = Tools::call_tool(&client, CallClass::Ordinary, "get_accounts", &arguments).await;
    assert_eq!(text.unwrap(), SPACED);
    let call = sent(&server, 3);
    assert_eq!(call["method"], "tools/call");
    assert_eq!(call["params"]["name"], "get_accounts");
    assert_eq!(call["params"]["arguments"], arguments);
}

#[tokio::test]
async fn the_seam_draws_on_the_class_it_is_handed() {
    let spent = BucketConfig {
        capacity: 3,
        refill_every: Duration::from_secs(3600),
    };
    let reserved = BucketConfig {
        capacity: 1,
        refill_every: Duration::from_secs(3600),
    };
    let config = TransportConfig {
        budget: BudgetConfig {
            ordinary: spent,
            reserved,
        },
        ..roomy()
    };
    let (server, client) = session(vec![listing(&base()), reply(OK)], None, config).await;
    let client = client.unwrap();
    let ordinary = Tools::call_tool(&client, CallClass::Ordinary, "get_accounts", &json!({})).await;
    assert!(matches!(ordinary, Err(McpError::Throttled)), "{ordinary:?}");
    let exit = Tools::call_tool(
        &client,
        CallClass::RiskReducing,
        "cancel_equity_order",
        &json!({}),
    )
    .await;
    assert_eq!(exit.unwrap(), r#"{"content":[]}"#);
    assert_eq!(server.seen().len(), 4, "the throttled call sent nothing");
}

#[tokio::test]
async fn every_refusal_of_the_client_is_the_seams_with_nothing_sent() {
    let pin = Some(ContractHash::from_bytes([7; 32]));
    let (server, client) = connected(&base(), pin, vec![]).await;
    assert!(client.openings_halted().unwrap());
    let outside =
        Tools::call_tool(&client, CallClass::Ordinary, "get_watchlists", &json!({})).await;
    assert!(
        matches!(outside, Err(McpError::ToolNotAllowed)),
        "{outside:?}"
    );
    let opening = Tools::call_tool(
        &client,
        CallClass::Ordinary,
        "place_equity_order",
        &json!({}),
    )
    .await;
    assert!(
        matches!(opening, Err(McpError::ContractDrift)),
        "{opening:?}"
    );
    assert_eq!(server.seen().len(), 3);
}

#[tokio::test]
async fn a_json_rpc_error_is_the_seams_error_and_never_a_result() {
    let error = reply(r#""error":{"code":-32602,"message":"no"}"#);
    let (_server, client) = connected(&base(), None, vec![error]).await;
    let answer = Tools::call_tool(&client, CallClass::Ordinary, "get_portfolio", &json!({})).await;
    assert!(
        matches!(answer, Err(McpError::Rpc { code: -32602, .. })),
        "{answer:?}"
    );
}
