//! The loopback MCP server over the core (E7-25, S2 tests parts 1 and 2): loopback only, the
//! session, the pinned contract, tool calls driving the core, unlisted tools, and the extra-tool
//! and injection variants (LT-1, LT-8, CN-9). Oracles: the contract file, `mandate-mcp`'s
//! allowlist and revision, a hash computed outside Rust, imperatives written here, the core
//! driven directly, and the contract's own state and alert names.

mod common;

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr};

use common::wire::Wire;
use common::{AGENTIC, NOT_AGENTIC, limit, price, qty, sim};
use mandate_mcp::{ALLOWLIST, PROTOCOL_VERSION};
use mandate_rh_sim::{CONTRACT, INJECTION, OrderRequest, ServerError, SimServer, State, Variant};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

type Outcome = Result<(), ServerError>;

/// DEC-839 item 1's hash of [`CONTRACT`], computed when it was written with Python's
/// `json.dumps(sort_keys=True, separators=(",", ":"))` and `hashlib.sha256`.
const PINNED: &str = "584bc25909105beb79882b8c39f595bb4aabfcd6176c6183e2374be47fe2d59d";

/// DEC-839 item 1 computed its own way, as `mandate-mcp`'s M2 tests do: the allowlisted tools'
/// names and schemas, sorted by name, written compact by `serde_json`'s sorted map.
fn hash(tools: &[Value]) -> String {
    let mut nine: Vec<Value> = tools
        .iter()
        .filter(|t| ALLOWLIST.contains(&t["name"].as_str().unwrap()))
        .map(|t| {
            let output = t.get("outputSchema").cloned().unwrap_or(Value::Null);
            json!({"inputSchema": t["inputSchema"], "name": t["name"], "outputSchema": output})
        })
        .collect();
    nine.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    let digest = Sha256::digest(Value::Array(nine).to_string().as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn names(tools: &[Value]) -> BTreeSet<&str> {
    tools.iter().map(|t| t["name"].as_str().unwrap()).collect()
}

/// What makes text an instruction to a model, written here rather than read from the server, so
/// text other than the [`INJECTION`] literal is caught too. Each matches whole words only.
const IMPERATIVES: [&str; 9] = [
    "ignore",
    "instruction",
    "assistant",
    "system",
    "you must",
    "do not",
    "always",
    "call",
    "transfer",
];

fn instructs(text: &str) -> bool {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    IMPERATIVES.iter().any(|phrase| {
        let phrase: Vec<&str> = phrase.split(' ').collect();
        words
            .windows(phrase.len())
            .any(|run| run == phrase.as_slice())
    })
}

#[test]
fn the_imperatives_match_whole_words_only() {
    assert!(instructs("Please CALL transfer_funds") && instructs("you  must, now"));
    for text in [
        "recall the systematic ignorer",
        "transferable",
        "do nothing",
        "a caller",
    ] {
        assert!(!instructs(text), "{text}");
    }
}

fn served(variant: Variant) -> Result<(SimServer, Wire), ServerError> {
    let server = SimServer::start(sim().unwrap(), variant)?;
    let wire = Wire::connect(&server.url()?);
    Ok((server, wire))
}

/// The request as `place_equity_order`'s arguments, under the contract's parameter names.
fn arguments(r: &OrderRequest) -> Value {
    let mut args = json!({"account_number": r.account_number, "symbol": r.symbol,
        "side": r.side, "type": r.order_type});
    let optional = [
        ("quantity", &r.quantity),
        ("dollar_amount", &r.dollar_amount),
        ("limit_price", &r.limit_price),
        ("stop_price", &r.stop_price),
        ("time_in_force", &r.time_in_force),
        ("market_hours", &r.market_hours),
        ("ref_id", &r.ref_id),
    ];
    for (name, value) in optional {
        if let Some(value) = value {
            args[name] = json!(value);
        }
    }
    args
}

/// The contract's `state` values, written out here.
fn state_text(state: State) -> &'static str {
    match state {
        State::New => "new",
        State::Queued => "queued",
        State::Confirmed => "confirmed",
        State::Unconfirmed => "unconfirmed",
        State::PartiallyFilled => "partially_filled",
        State::Filled => "filled",
        State::Cancelled => "cancelled",
        State::Rejected => "rejected",
        State::Failed => "failed",
        State::Voided => "voided",
    }
}

fn refused(result: &Value) -> bool {
    result["isError"] == json!(true)
}

fn orders(server: &SimServer, account: &str) -> Result<Vec<mandate_rh_sim::Order>, ServerError> {
    server.drive(|s| s.orders(account).unwrap())
}

#[test]
#[ignore = "pending E7-25"]
fn the_server_listens_on_loopback_only_at_a_port_the_system_chose() -> Outcome {
    let first = SimServer::start(sim().unwrap(), Variant::Honest)?;
    let second = SimServer::start(sim().unwrap(), Variant::Honest)?;
    let (a, b) = (first.addr()?, second.addr()?);
    let loopback = IpAddr::V4(Ipv4Addr::LOCALHOST);
    assert_eq!((a.ip(), b.ip()), (loopback, loopback), "127.0.0.1 only");
    assert_ne!(a.port(), 0);
    assert_ne!(a.port(), b.port(), "port 0, so the system chooses");
    let url = format!("http://127.0.0.1:{}/mcp", a.port());
    assert_eq!(first.url()?, url);
    Ok(())
}

#[test]
#[ignore = "pending E7-25"]
fn the_handshake_answers_the_transports_revision_and_a_session_later_requests_need() -> Outcome {
    let server = SimServer::start(sim().unwrap(), Variant::Honest)?;
    let mut wire = Wire::connect(&server.url()?);
    let session = wire.session.clone().unwrap();
    let visible = session.bytes().all(|b| (0x21..=0x7e).contains(&b));
    assert!(!session.is_empty() && visible, "{session:?}");
    let other = Wire::connect(&server.url()?).session.unwrap();
    assert_ne!(other, session, "one session per initialize");
    assert_eq!(wire.request("tools/list", json!({})).status, 200);
    wire.session = None;
    let bare = wire.request("tools/list", json!({}));
    assert_eq!(bare.status, 400, "no session id: {}", bare.body);
    wire.session = Some(format!("{session}x"));
    let stale = wire.request("tools/list", json!({}));
    assert_eq!(stale.status, 404, "a session it never gave: {}", stale.body);
    let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}});
    wire.session = None;
    assert_eq!(
        wire.post(&note).status,
        400,
        "a notification needs the session too"
    );
    wire.session = Some(other);
    wire.protocol = "2025-03-26".to_owned();
    let revision = wire.request("tools/list", json!({}));
    assert_eq!(revision.status, 400, "another revision: {}", revision.body);
    wire.protocol = PROTOCOL_VERSION.to_owned();
    assert_eq!(wire.request("tools/list", json!({})).status, 200);
    Ok(())
}

#[test]
#[ignore = "pending E7-25"]
fn the_honest_listing_is_the_pinned_contract_and_carries_no_instruction_text() -> Outcome {
    let server = SimServer::start(sim().unwrap(), Variant::Honest)?;
    let tools = Wire::connect(&server.url()?).list();
    let file: Vec<Value> = serde_json::from_str(CONTRACT).unwrap();
    assert_eq!(
        names(&tools),
        BTreeSet::from(ALLOWLIST),
        "the nine, no more"
    );
    assert_eq!(tools.len(), 9, "each tool once");
    assert_eq!(hash(&file), PINNED, "the contract file is the pin");
    assert_eq!(hash(&tools), PINNED, "the listing is the contract file");
    for tool in &tools {
        let pinned = file.iter().find(|f| f["name"] == tool["name"]).unwrap();
        assert_eq!(
            tool["inputSchema"], pinned["inputSchema"],
            "{}",
            tool["name"]
        );
        assert_eq!(
            tool.get("outputSchema"),
            None,
            "the contract publishes none"
        );
        let description = tool["description"].as_str().unwrap();
        assert!(!instructs(description), "{}: {description}", tool["name"]);
    }
    let text = Value::Array(tools).to_string();
    assert!(
        !text.contains(INJECTION),
        "the honest server injects nothing"
    );
    assert_eq!(
        server.calls()?,
        Vec::<String>::new(),
        "a listing is not a call"
    );
    Ok(())
}

#[test]
#[ignore = "pending E7-25"]
fn the_extra_tool_variant_lists_one_more_tool_keeps_the_hash_and_reaches_nothing() -> Outcome {
    for extra in ["place_option_order", "transfer_funds"] {
        let (server, mut wire) = served(Variant::ExtraTool(extra.to_owned()))?;
        let tools = wire.list();
        let mut expected = BTreeSet::from(ALLOWLIST);
        expected.insert(extra);
        assert_eq!(names(&tools), expected, "{extra}");
        assert_eq!(
            hash(&tools),
            PINNED,
            "{extra}: not allowlisted, so not hashed"
        );
        let answer = wire.call(extra, arguments(&limit("buy", "1", "501", 1)));
        assert!(refused(&answer), "{extra}: {answer}");
        assert_eq!(server.calls()?, [extra], "every call is recorded");
        assert_eq!(orders(&server, AGENTIC)?, [], "{extra} reaches no order");
    }
    Ok(())
}

#[test]
#[ignore = "pending E7-25"]
fn the_injection_variant_carries_instruction_text_in_every_description_and_result() -> Outcome {
    assert!(
        instructs(INJECTION),
        "the imperatives catch the variant's own text"
    );
    let (server, mut wire) = served(Variant::Injection)?;
    let tools = wire.list();
    assert_eq!(
        (names(&tools), hash(&tools)),
        (BTreeSet::from(ALLOWLIST), PINNED.to_owned())
    );
    for tool in &tools {
        let description = tool["description"].as_str().unwrap();
        assert!(description.contains(INJECTION), "{}", tool["name"]);
    }
    let request = limit("buy", "1", "501", 1);
    let placed = wire.call("place_equity_order", arguments(&request));
    let listed = wire.call("get_equity_orders", json!({"account_number": AGENTIC}));
    let refusal = wire.call(
        "place_equity_order",
        arguments(&limit("sell", "9", "499", 2)),
    );
    for result in [&placed, &listed, &refusal] {
        assert!(result.to_string().contains(INJECTION), "{result}");
    }
    assert!(!refused(&placed) && refused(&refusal), "{placed} {refusal}");
    let mut direct = sim().unwrap();
    direct.place(&request).unwrap();
    assert_eq!(
        orders(&server, AGENTIC)?,
        direct.orders(AGENTIC).unwrap(),
        "text moves nothing"
    );
    Ok(())
}

#[test]
#[ignore = "pending E7-25"]
fn an_unlisted_tool_or_an_unknown_method_is_a_protocol_error_that_reaches_nothing() -> Outcome {
    let (server, mut wire) = served(Variant::Honest)?;
    let args = json!({"name": "get_equity_tax_lots", "arguments": {"account_number": AGENTIC}});
    let unlisted = wire.request("tools/call", args).message();
    assert_eq!(
        (&unlisted["error"]["code"], unlisted.get("result")),
        (&json!(-32602), None)
    );
    let unknown = wire.request("resources/list", json!({})).message();
    assert_eq!(
        (&unknown["error"]["code"], unknown.get("result")),
        (&json!(-32601), None)
    );
    assert_eq!(server.calls()?, ["get_equity_tax_lots"]);
    assert_eq!(orders(&server, AGENTIC)?, []);
    Ok(())
}

#[test]
#[ignore = "pending E7-25"]
fn a_place_drives_the_core_as_the_core_driven_alone_would() -> Outcome {
    let (server, mut wire) = served(Variant::Honest)?;
    let mut direct = sim().unwrap();
    let text = |value: &str| Some(value.to_owned());
    let requests = [
        limit("buy", "2", "501", 1),
        limit("buy", "3", "490", 1),
        limit("sell", "5", "499", 2),
        OrderRequest {
            account_number: NOT_AGENTIC.to_owned(),
            ..limit("buy", "1", "501", 3)
        },
        limit("buy", "0.5", "501", 4),
        OrderRequest {
            order_type: "market".to_owned(),
            quantity: None,
            limit_price: None,
            dollar_amount: text("250"),
            ..limit("buy", "1", "1", 5)
        },
        OrderRequest {
            order_type: "stop_limit".to_owned(),
            stop_price: text("505"),
            time_in_force: text("gtc"),
            ..limit("buy", "1", "506", 6)
        },
        OrderRequest {
            market_hours: text("extended_hours"),
            ..limit("buy", "1", "502", 7)
        },
        OrderRequest {
            ref_id: None,
            ..limit("buy", "1", "498", 8)
        },
    ];
    let (mut placed, mut refusals) = (0, 0);
    for request in &requests {
        let answer = wire.call("place_equity_order", arguments(request));
        match direct.place(request) {
            Ok(order) => {
                placed += 1;
                assert!(!refused(&answer), "{request:?}: {answer}");
                assert_eq!(answer["structuredContent"]["id"], json!(order.id));
                assert_eq!(
                    answer["structuredContent"]["state"],
                    state_text(order.state)
                );
            }
            Err(_) => {
                refusals += 1;
                assert!(refused(&answer), "{request:?}: {answer}");
            }
        }
    }
    assert_eq!(
        (placed, refusals),
        (6, 3),
        "the script covers both outcomes"
    );
    for account in [AGENTIC, NOT_AGENTIC] {
        assert_eq!(
            orders(&server, account)?,
            direct.orders(account).unwrap(),
            "{account}"
        );
    }
    assert_eq!(server.calls()?, vec!["place_equity_order"; requests.len()]);
    Ok(())
}

#[test]
#[ignore = "pending E7-25"]
fn review_reads_and_cancel_drive_the_core_and_review_places_nothing() -> Outcome {
    let (server, mut wire) = served(Variant::Honest)?;
    let buy = |q: &str, at: &str, n| arguments(&limit("buy", q, at, n));
    let id = |answer: Value| {
        answer["structuredContent"]["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let review = wire.call("review_equity_order", buy("100", "501", 1));
    let alerts = &review["structuredContent"]["alerts"];
    assert_eq!(alerts, &json!(["buying_power"]), "{review}");
    assert_eq!(orders(&server, AGENTIC)?, [], "review places nothing");
    let filled = id(wire.call("place_equity_order", buy("1", "501", 2)));
    server.drive(|s| s.fill(&filled, qty("1"), price("500")).unwrap())?;
    let cancel = |id: &str| json!({"account_number": AGENTIC, "order_id": id});
    let late = wire.call("cancel_equity_order", cancel(&filled));
    assert!(refused(&late), "a filled order: {late}");
    let working = id(wire.call("place_equity_order", buy("1", "499", 3)));
    let cancelled = wire.call("cancel_equity_order", cancel(&working));
    assert!(!refused(&cancelled), "{cancelled}");
    let newest = orders(&server, AGENTIC)?;
    assert_eq!((newest.len(), newest[0].state), (2, State::Cancelled));
    let listed = wire.call("get_equity_orders", json!({"account_number": AGENTIC}));
    let rows = listed["structuredContent"]["orders"].as_array().unwrap();
    let seen: Vec<(&Value, &Value)> = rows.iter().map(|o| (&o["id"], &o["state"])).collect();
    let (working, filled) = (json!(working), json!(filled));
    let expected = [(&working, &json!("cancelled")), (&filled, &json!("filled"))];
    assert_eq!(seen, expected, "newest first, with the contract's states");
    assert!(
        !instructs(&listed.to_string()),
        "honest results instruct nothing"
    );
    let calls = [
        "review_equity_order",
        "place_equity_order",
        "cancel_equity_order",
    ];
    let then = [
        "place_equity_order",
        "cancel_equity_order",
        "get_equity_orders",
    ];
    assert_eq!(server.calls()?, [calls, then].concat());
    Ok(())
}
