//! The allowlist, the pinned contract hash, drift, and the fund-movement refusal, against the
//! loopback server (connections spec §6.2 rules 2 to 4, CN-2, CN-9, DEC-441 item 8, DEC-839).

use std::time::Duration;

use serde_json::{Value, json};

use super::server::{Answer, Loopback, reply, serve, with_type};
use crate::{ALLOWLIST, BucketConfig, BudgetConfig, CallClass, ContractHash, McpClient, McpError};
use crate::{McpTransport, TransportConfig};

const NINE: [&str; 9] = [
    "get_accounts",
    "get_portfolio",
    "get_equity_positions",
    "get_equity_quotes",
    "get_equity_tradability",
    "get_equity_orders",
    "review_equity_order",
    "place_equity_order",
    "cancel_equity_order",
];
const INPUT: &str = r#"{"type":"object","properties":{"account_number":{"type":"string"}},"required":["account_number"]}"#;
const OK: &str = r#""result":{"content":[]}"#;

fn tool(name: &str) -> Value {
    let mut tool = json!({"name": name, "description": "d", "inputSchema": serde_json::from_str::<Value>(INPUT).unwrap()});
    if name == "get_accounts" {
        tool["outputSchema"] =
            json!({"type": "object", "properties": {"accounts": {"type": "array"}}});
    }
    tool
}

fn base() -> Vec<Value> {
    NINE.iter().map(|name| tool(name)).collect()
}

fn cat(first: Vec<Answer>, second: Vec<Answer>) -> Vec<Answer> {
    first.into_iter().chain(second).collect()
}

fn listing(tools: &[Value]) -> Answer {
    reply(&format!(r#""result":{}"#, json!({ "tools": tools })))
}

fn page(tools: &[Value], next: &str) -> Answer {
    reply(&format!(
        r#""result":{}"#,
        json!({"tools": tools, "nextCursor": next})
    ))
}

fn handshake() -> Vec<Answer> {
    vec![
        reply(
            r#""result":{"protocolVersion":"2025-06-18","capabilities":{},"serverInfo":{"name":"s","version":"1"}}"#,
        ),
        with_type(202, "application/json", ""),
    ]
}

fn roomy() -> TransportConfig {
    let bucket = BucketConfig {
        capacity: 100,
        refill_every: Duration::from_secs(1),
    };
    TransportConfig {
        budget: BudgetConfig {
            ordinary: bucket,
            reserved: bucket,
        },
        ..TransportConfig::CONSERVATIVE
    }
}

async fn session(
    tail: Vec<Answer>,
    pin: Option<ContractHash>,
    config: TransportConfig,
) -> (Loopback, Result<McpClient, McpError>) {
    let server = serve(cat(handshake(), tail)).await;
    let transport: McpTransport = server.transport(config);
    let client = McpClient::connect(transport, pin).await;
    (server, client)
}

async fn connected(
    tools: &[Value],
    pin: Option<ContractHash>,
    more: Vec<Answer>,
) -> (Loopback, McpClient) {
    let (server, client) = session(cat(vec![listing(tools)], more), pin, roomy()).await;
    (server, client.unwrap())
}

fn sent(server: &Loopback, index: usize) -> Value {
    let request = &server.seen()[index];
    serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap()
}

fn sent_names(server: &Loopback) -> Vec<String> {
    (3..server.seen().len())
        .map(|i| {
            sent(server, i)["params"]["name"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect()
}

fn without(name: &str) -> Vec<Value> {
    base().into_iter().filter(|t| t["name"] != name).collect()
}

#[test]
fn the_allowlist_is_the_nine_tools_of_the_contract() {
    assert_eq!(ALLOWLIST, NINE);
}

#[tokio::test]
#[ignore = "pending E7-16"]
async fn a_tool_outside_the_allowlist_is_refused_before_anything_is_sent() {
    let (server, client) = connected(&base(), None, vec![]).await;
    let outside = [
        "place_option_order",
        "place_crypto_order",
        "cancel_crypto_order",
        "exercise_option",
        "create_watchlist",
        "GET_ACCOUNTS",
        "get_accounts ",
        " get_accounts",
        "get_accounts\n",
        "get_account",
        "",
    ];
    for name in outside {
        let refused = client
            .call_tool(CallClass::RiskReducing, name, &json!({}))
            .await;
        assert!(
            matches!(refused, Err(McpError::ToolNotAllowed)),
            "{name:?}: {refused:?}"
        );
    }
    assert_eq!(server.seen().len(), 3, "a refused call must send nothing");
}

#[tokio::test]
#[ignore = "pending E7-16"]
async fn a_refused_call_draws_no_budget_and_a_throttled_class_spends_its_own() {
    let bucket = |capacity| BucketConfig {
        capacity,
        refill_every: Duration::from_secs(3600),
    };
    let config = TransportConfig {
        budget: BudgetConfig {
            ordinary: bucket(3),
            reserved: bucket(1),
        },
        ..TransportConfig::CONSERVATIVE
    };
    let (server, client) = session(vec![listing(&base()), reply(OK)], None, config).await;
    let client = client.unwrap();
    let outside = client
        .call_tool(CallClass::RiskReducing, "place_option_order", &json!({}))
        .await;
    assert!(
        matches!(outside, Err(McpError::ToolNotAllowed)),
        "{outside:?}"
    );
    let spent = client
        .call_tool(CallClass::Ordinary, "get_accounts", &json!({}))
        .await;
    assert!(matches!(spent, Err(McpError::Throttled)), "{spent:?}");
    let exit = client
        .call_tool(CallClass::RiskReducing, "cancel_equity_order", &json!({}))
        .await;
    assert!(exit.is_ok(), "{exit:?}");
    assert_eq!(sent_names(&server), ["cancel_equity_order"]);
}

#[tokio::test]
#[ignore = "pending E7-16"]
async fn with_no_stored_pin_a_listed_fund_movement_tool_is_refused() {
    let names = [
        "transfer_funds",
        "withdraw_cash",
        "sendMoney",
        "initiate-ach-transfer",
        "wire_transfer",
        "crypto_withdrawal",
        "TRANSFER",
        "create_ach_relationship_transfer",
        "get_transfers",
    ];
    for name in names {
        let mut tools = base();
        tools.push(json!({"name": name, "description": "d", "inputSchema": {"type": "object"}}));
        let (_server, client) = session(vec![listing(&tools)], None, roomy()).await;
        assert!(
            matches!(client, Err(McpError::FundMovementTool)),
            "{name}: {client:?}"
        );
    }
    let mut both = without("get_equity_tradability");
    both.push(json!({"name": "transfer_funds", "inputSchema": {"type": "object"}}));
    let (_server, client) = session(vec![listing(&both)], None, roomy()).await;
    assert!(
        matches!(client, Err(McpError::FundMovementTool)),
        "fund-movement is checked before missing: {client:?}"
    );
    let mut hidden = base();
    hidden.push(json!({"name": "transfer_funds", "inputSchema": {"type": "object"}}));
    let tail = vec![page(&base(), "c2"), listing(&hidden[9..])];
    let (_server, client) = session(tail, None, roomy()).await;
    assert!(
        matches!(client, Err(McpError::FundMovementTool)),
        "page 2: {client:?}"
    );
}

#[tokio::test]
#[ignore = "pending E7-16"]
async fn with_no_stored_pin_a_missing_allowlisted_tool_is_refused() {
    for name in NINE {
        let (_server, client) = session(vec![listing(&without(name))], None, roomy()).await;
        assert!(
            matches!(client, Err(McpError::ContractMissingTool)),
            "{name}: {client:?}"
        );
    }
}

#[tokio::test]
#[ignore = "pending E7-16"]
async fn with_a_stored_pin_a_fund_tool_or_a_missing_tool_halts_openings_and_never_the_exit() {
    let extra = |name: &str| {
        let mut tools = base();
        tools.push(json!({"name": name, "inputSchema": {"type": "object"}}));
        tools
    };
    let lists = [
        ("transfer_funds", extra("transfer_funds")),
        ("send_feedback", extra("send_feedback")),
        ("missing", without("get_equity_tradability")),
    ];
    for (label, tools) in lists {
        let pin = Some(ContractHash::from_bytes([7; 32]));
        let (server, client) =
            session(vec![listing(&tools), reply(OK), reply(OK)], pin, roomy()).await;
        let client = client.unwrap();
        assert!(client.openings_halted().unwrap(), "{label}");
        let opening = client
            .call_tool(CallClass::Ordinary, "place_equity_order", &json!({}))
            .await;
        assert!(
            matches!(opening, Err(McpError::ContractDrift)),
            "{label}: {opening:?}"
        );
        let listed = client
            .call_tool(CallClass::RiskReducing, "transfer_funds", &json!({}))
            .await;
        assert!(
            matches!(listed, Err(McpError::ToolNotAllowed)),
            "{label}: {listed:?}"
        );
        for name in ["place_equity_order", "cancel_equity_order"] {
            let exit = client
                .call_tool(CallClass::RiskReducing, name, &json!({}))
                .await;
            assert!(exit.is_ok(), "{label}: {name}: {exit:?}");
        }
        let exits = ["place_equity_order", "cancel_equity_order"];
        assert_eq!(sent_names(&server), exits, "{label}");
    }
}

#[tokio::test]
#[ignore = "pending E7-16"]
async fn a_benign_extra_tool_connects_and_is_never_called() {
    let mut tools = base();
    tools.push(json!({"name": "get_watchlists", "inputSchema": {"type": "object"}}));
    let (server, client) = connected(&tools, None, vec![]).await;
    let refused = client
        .call_tool(CallClass::Ordinary, "get_watchlists", &json!({}))
        .await;
    assert!(
        matches!(refused, Err(McpError::ToolNotAllowed)),
        "{refused:?}"
    );
    assert_eq!(server.seen().len(), 3);
}

#[test]
fn a_contract_hash_keeps_its_bytes_and_prints_them_as_hex() {
    let ascending: [u8; 32] = std::array::from_fn(|i| u8::try_from(i).unwrap());
    for bytes in [[0xab; 32], ascending] {
        let hash = ContractHash::from_bytes(bytes);
        assert_eq!(hash.as_bytes(), &bytes);
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(format!("{hash:?}"), format!("ContractHash({hex})"));
    }
}
