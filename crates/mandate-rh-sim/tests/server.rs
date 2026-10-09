//! The loopback MCP server over the core (E7-25, S2 tests parts 1 and 2, and the tests
//! correction): loopback only, the session, the pinned contract, tool calls driving the core,
//! unlisted tools, the extra-tool and injection variants (LT-1, LT-8, CN-9), and the transport's
//! edges: the listener closing on drop, the status lines, a parse error, an unknown session, a
//! wrong path, and filters the core does not model. Oracles: the contract file, `mandate-mcp`'s
//! allowlist and revision, a hash computed outside Rust, imperatives written here, the core
//! driven directly, the contract's own state and alert names, and HTTP's and JSON-RPC's own
//! reason phrases and codes.

mod common;

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::thread;
use std::time::Duration;

use common::wire::Wire;
use common::{AGENTIC, DAY_TRADER, NOT_AGENTIC, limit, price, qty, ref_id, sim};
use mandate_mcp::{ALLOWLIST, PROTOCOL_VERSION};
use mandate_rh_sim::{CONTRACT, Event, Fault, Garble, INJECTION, Order, OrderRequest, OrderType};
use mandate_rh_sim::{ServerError, Side, SimError, SimServer, State, TimeInForce, Variant};
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
const IMPERATIVES: [&str; 16] = [
    "ignore",
    "ignores",
    "ignored",
    "instruction",
    "instructions",
    "assistant",
    "system",
    "you must",
    "do not",
    "always",
    "call",
    "calls",
    "calling",
    "transfer",
    "transfers",
    "transferring",
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
    let inflected = [
        "Ignores prior instructions",
        "it calls",
        "transfers the cash",
    ];
    assert!(
        inflected.iter().all(|text| instructs(text)),
        "inflected forms count"
    );
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

/// An order record as `get_equity_orders` must show it, built here from the core's order: every
/// field list-and-match reads (connections spec §6.2), under the contract's parameter names, with
/// numbers as decimal text (LT-12).
fn row(o: &Order) -> Value {
    let side = match o.side {
        Side::Buy => "buy",
        Side::Sell => "sell",
    };
    let kind = match o.order_type {
        OrderType::Market => "market",
        OrderType::Limit => "limit",
        OrderType::StopMarket => "stop_market",
        OrderType::StopLimit => "stop_limit",
    };
    let tif = match o.time_in_force {
        TimeInForce::Gfd => "gfd",
        TimeInForce::Gtc => "gtc",
    };
    json!({"id": o.id, "symbol": o.symbol, "side": side, "type": kind, "time_in_force": tif,
        "quantity": o.quantity.to_string(), "limit_price": o.limit_price.map(|p| p.to_string()),
        "state": state_text(o.state)})
}

/// The account's records over MCP, each checked against [`row`] of the core's order.
fn listed(wire: &mut Wire, held: &[Order]) -> Vec<Value> {
    let answer = wire.call("get_equity_orders", json!({"account_number": AGENTIC}));
    let rows = answer["structuredContent"]["orders"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(rows.len(), held.len(), "{answer}");
    for (served, order) in rows.iter().zip(held) {
        for (key, value) in row(order).as_object().unwrap() {
            assert_eq!(&served[key], value, "{key} of {served}");
        }
    }
    rows
}

const STATES: [State; 10] = [
    State::New,
    State::Queued,
    State::Confirmed,
    State::Unconfirmed,
    State::PartiallyFilled,
    State::Filled,
    State::Cancelled,
    State::Rejected,
    State::Failed,
    State::Voided,
];

fn refused(result: &Value) -> bool {
    result["isError"] == json!(true)
}

fn orders(server: &SimServer, account: &str) -> Result<Vec<Order>, ServerError> {
    server.drive(|s| s.orders(account).unwrap())
}

#[test]
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
        OrderRequest {
            account_number: DAY_TRADER.to_owned(),
            ..limit("buy", "1", "500", 9)
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
        (7, 3),
        "the script covers both outcomes"
    );
    for account in [AGENTIC, DAY_TRADER] {
        let served = orders(&server, account)?;
        assert!(!served.is_empty(), "{account} places an order");
        assert_eq!(served, direct.orders(account).unwrap(), "{account}");
    }
    assert_eq!(
        orders(&server, NOT_AGENTIC)?,
        [],
        "a refused account holds nothing"
    );
    assert_eq!(server.calls()?, vec!["place_equity_order"; requests.len()]);
    Ok(())
}

#[test]
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

#[test]
fn a_lost_answer_leaves_one_order_the_client_finds_only_by_its_fields() -> Outcome {
    let (server, mut wire) = served(Variant::Honest)?;
    let mut direct = sim().unwrap();
    let lose = Event::Script(Fault::LoseAnswer);
    server.drive(|s| s.apply(lose.clone()).unwrap())?;
    direct.apply(lose).unwrap();
    let request = limit("buy", "2", "501", 1);
    let answer = wire.call_raw("place_equity_order", arguments(&request));
    assert!(answer.is_none(), "no byte of an answer arrives");
    assert_eq!(direct.place(&request), Err(SimError::AnswerLost));
    let held = direct.orders(AGENTIC).unwrap();
    assert_eq!(orders(&server, AGENTIC)?, held, "the core acted");
    let rows = listed(&mut wire, &held);
    assert_eq!(
        rows[0].get("ref_id"),
        None,
        "records do not echo ref_id unless scripted"
    );
    let resent = wire.call(
        "place_equity_order",
        arguments(&limit("buy", "3", "490", 1)),
    );
    assert_eq!(
        resent["structuredContent"]["id"],
        json!(held[0].id),
        "dedup by ref_id"
    );
    assert_eq!(
        orders(&server, AGENTIC)?,
        held,
        "a re-send makes no second order"
    );
    server.drive(|s| s.apply(Event::EchoRefId(true)).unwrap())?;
    let echoed = listed(&mut wire, &held);
    assert_eq!(echoed[0]["ref_id"], json!(ref_id(1)));
    Ok(())
}

#[test]
fn two_lost_answers_for_one_body_leave_records_nothing_tells_apart() -> Outcome {
    let (server, mut wire) = served(Variant::Honest)?;
    for n in [1, 2] {
        server.drive(|s| s.apply(Event::Script(Fault::LoseAnswer)).unwrap())?;
        let answer = wire.call_raw(
            "place_equity_order",
            arguments(&limit("buy", "1", "501", n)),
        );
        assert!(answer.is_none(), "{n}");
    }
    let held = orders(&server, AGENTIC)?;
    let mut rows = listed(&mut wire, &held);
    assert_ne!(rows[0]["id"], rows[1]["id"], "two orders");
    for row in &mut rows {
        assert_eq!(row.get("ref_id"), None);
        row.as_object_mut().unwrap().remove("id");
    }
    assert_eq!(
        rows[0], rows[1],
        "every field list-and-match reads is the same"
    );
    Ok(())
}

#[test]
fn a_garbled_answer_follows_an_order_the_core_placed_and_only_that_answer_is_bent() -> Outcome {
    let states: Vec<&str> = STATES.iter().map(|s| state_text(*s)).collect();
    let all = [
        Garble::UnknownState,
        Garble::MissingId,
        Garble::NumberQuantity,
        Garble::NotJson,
    ];
    for garble in all {
        let (server, mut wire) = served(Variant::Honest)?;
        server.garble_next(garble)?;
        let request = limit("buy", "1", "501", 1);
        let reply = wire
            .call_raw("place_equity_order", arguments(&request))
            .unwrap();
        assert_eq!(reply.status, 200, "{garble:?}");
        assert_eq!(reply.header("content-type"), Some("application/json"));
        let mut direct = sim().unwrap();
        direct.place(&request).unwrap();
        let held = direct.orders(AGENTIC).unwrap();
        let bent = match garble {
            Garble::UnknownState => "state",
            Garble::MissingId => "id",
            Garble::NumberQuantity => "quantity",
            Garble::NotJson => {
                assert!(serde_json::from_str::<Value>(&reply.body).is_err());
                ""
            }
        };
        if garble != Garble::NotJson {
            let result = reply.result();
            assert!(
                !refused(&result),
                "{garble:?}: bent, never a refusal: {result}"
            );
            let content = &result["structuredContent"];
            let expected = row(&held[0]);
            for (key, value) in expected.as_object().unwrap() {
                if key != bent {
                    assert_eq!(&content[key], value, "{garble:?}: {key} of {content}");
                }
            }
            match garble {
                Garble::UnknownState => {
                    let state = content["state"].as_str().unwrap();
                    assert!(!states.contains(&state), "{state}");
                }
                Garble::MissingId => assert_eq!(content.get("id"), None),
                _ => {
                    assert!(content["quantity"].is_number(), "{content}");
                    let quantity = content["quantity"].to_string();
                    assert_eq!(
                        quantity,
                        held[0].quantity.to_string(),
                        "the core's quantity"
                    );
                }
            }
        }
        assert_eq!(
            orders(&server, AGENTIC)?,
            held,
            "{garble:?}: the core acted"
        );
        listed(&mut wire, &held);
    }
    Ok(())
}

/// A request's head and body, written here byte for byte.
fn request(addr: SocketAddr, line: &str, session: Option<&str>, body: &[u8]) -> Vec<u8> {
    let mut head = format!(
        "{line}\r\nhost: {addr}\r\nconnection: close\r\ncontent-type: application/json\r\n\
         accept: application/json, text/event-stream\r\nmcp-protocol-version: {PROTOCOL_VERSION}\r\n"
    );
    if let Some(session) = session {
        head.push_str(&format!("mcp-session-id: {session}\r\n"));
    }
    head.push_str(&format!("content-length: {}\r\n\r\n", body.len()));
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

/// Writes `bytes` split at `splits`, pausing between the parts, and returns the whole answer as
/// text, or `None` when the server closes or resets the connection with no byte of an answer.
fn send(addr: SocketAddr, bytes: &[u8], splits: &[usize]) -> Option<String> {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut from = 0;
    for &to in splits.iter().chain([&bytes.len()]) {
        let _written = stream.write_all(&bytes[from..to]);
        thread::sleep(Duration::from_millis(if to == bytes.len() {
            0
        } else {
            50
        }));
        from = to;
    }
    let mut answer = Vec::new();
    let _read = stream.read_to_end(&mut answer);
    (!answer.is_empty()).then(|| String::from_utf8(answer).unwrap())
}

fn exchange(addr: SocketAddr, line: &str, session: Option<&str>, body: &[u8]) -> Option<String> {
    send(addr, &request(addr, line, session, body), &[])
}

const POST: &str = "POST /mcp HTTP/1.1";

fn list_body() -> Vec<u8> {
    json!({"jsonrpc": "2.0", "id": 7, "method": "tools/list", "params": {}})
        .to_string()
        .into_bytes()
}

/// The JSON-RPC message after the head of an answer.
fn body_of(answer: &str) -> Value {
    serde_json::from_str(answer.split_once("\r\n\r\n").unwrap().1).unwrap()
}

#[test]
fn dropping_the_server_closes_its_listener() -> Outcome {
    let addr = {
        let server = SimServer::start(sim().unwrap(), Variant::Honest)?;
        let addr = server.addr()?;
        assert!(
            TcpStream::connect(addr).is_ok(),
            "it listens while it lives"
        );
        addr
    };
    let wait = Duration::from_millis(20);
    let accepted = (0..50).take_while(|_| {
        thread::sleep(wait);
        TcpStream::connect_timeout(&addr, wait).is_ok()
    });
    assert!(
        accepted.count() < 50,
        "{addr} still accepts a second after drop"
    );
    Ok(())
}

#[test]
fn each_status_line_carries_its_standard_reason_phrase() -> Outcome {
    let server = SimServer::start(sim().unwrap(), Variant::Honest)?;
    let addr = server.addr()?;
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params":
        {"protocolVersion": PROTOCOL_VERSION, "capabilities": {},
         "clientInfo": {"name": "mandate-mcp", "version": "0.0.0"}}});
    let ok = exchange(addr, POST, None, init.to_string().as_bytes()).unwrap();
    assert!(ok.starts_with("HTTP/1.1 200 OK\r\n"), "{ok}");
    let session = Wire::connect(&server.url()?).session.unwrap();
    let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}});
    let accepted = exchange(addr, POST, Some(&session), note.to_string().as_bytes()).unwrap();
    assert!(
        accepted.starts_with("HTTP/1.1 202 Accepted\r\n"),
        "{accepted}"
    );
    assert!(
        accepted.ends_with("\r\n\r\n"),
        "202 carries no body: {accepted}"
    );
    let bad = exchange(addr, POST, None, &list_body()).unwrap();
    assert!(bad.starts_with("HTTP/1.1 400 Bad Request\r\n"), "{bad}");
    let gone = exchange(addr, POST, Some("never-given"), &list_body()).unwrap();
    assert!(gone.starts_with("HTTP/1.1 404 Not Found\r\n"), "{gone}");
    Ok(())
}

#[test]
fn a_body_that_is_not_json_is_a_parse_error_under_400() -> Outcome {
    let server = SimServer::start(sim().unwrap(), Variant::Honest)?;
    let session = Wire::connect(&server.url()?).session.unwrap();
    let answer = exchange(
        server.addr()?,
        POST,
        Some(&session),
        b"{\"jsonrpc\": \"2.0\", ",
    )
    .unwrap();
    assert!(answer.starts_with("HTTP/1.1 400 "), "{answer}");
    let message = body_of(&answer);
    assert_eq!(message["error"]["code"], json!(-32700), "{message}");
    assert_eq!(
        (message.get("id"), message.get("result")),
        (Some(&Value::Null), None)
    );
    assert_eq!(server.calls()?, Vec::<String>::new());
    Ok(())
}

#[test]
fn a_session_the_server_never_gave_is_404_with_code_32001_and_reaches_nothing() -> Outcome {
    let (server, wire) = served(Variant::Honest)?;
    let stale = format!("{}x", wire.session.unwrap());
    let args = arguments(&limit("buy", "1", "501", 1));
    let call = json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "place_equity_order", "arguments": args}});
    let answer = exchange(
        server.addr()?,
        POST,
        Some(&stale),
        call.to_string().as_bytes(),
    );
    let answer = answer.unwrap();
    assert!(answer.starts_with("HTTP/1.1 404 "), "{answer}");
    let message = body_of(&answer);
    assert_eq!(message["error"]["code"], json!(-32001), "{message}");
    assert_eq!(message.get("result"), None);
    assert_eq!(server.calls()?, Vec::<String>::new(), "no call is recorded");
    assert_eq!(orders(&server, AGENTIC)?, [], "no order is placed");
    Ok(())
}

#[test]
fn every_initialize_gets_a_session_of_its_own() -> Outcome {
    let server = SimServer::start(sim().unwrap(), Variant::Honest)?;
    let url = server.url()?;
    let sessions: BTreeSet<String> = (0..4)
        .map(|_| Wire::connect(&url).session.unwrap())
        .collect();
    assert_eq!(sessions.len(), 4, "{sessions:?}");
    Ok(())
}

#[test]
fn get_equity_orders_refuses_every_filter_it_does_not_model() -> Outcome {
    let (_server, mut wire) = served(Variant::Honest)?;
    let placed = wire.call(
        "place_equity_order",
        arguments(&limit("buy", "1", "499", 1)),
    );
    assert!(!refused(&placed), "{placed}");
    let filters = [
        ("order_id", "nothing"),
        ("symbol", "QQQ"),
        ("state", "filled"),
        ("created_at_gte", "2099-01-01T00:00:00Z"),
        ("placed_agent", "user"),
        ("cursor", "next"),
    ];
    for (key, value) in filters {
        let mut args = json!({"account_number": AGENTIC});
        args[key] = json!(value);
        let answer = wire.call("get_equity_orders", args);
        assert!(refused(&answer), "{key} is never ignored: {answer}");
        assert_eq!(answer.get("structuredContent"), None, "{key}: {answer}");
    }
    let plain = wire.call("get_equity_orders", json!({"account_number": AGENTIC}));
    assert_eq!(
        plain["structuredContent"]["orders"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    Ok(())
}

#[test]
fn anything_but_post_to_mcp_is_404_never_400() -> Outcome {
    let server = SimServer::start(sim().unwrap(), Variant::Honest)?;
    let session = Wire::connect(&server.url()?).session.unwrap();
    let addr = server.addr()?;
    let served = exchange(addr, POST, Some(&session), &list_body()).unwrap();
    assert!(
        served.starts_with("HTTP/1.1 200 "),
        "the same request on POST /mcp: {served}"
    );
    let lines = [
        "GET /mcp HTTP/1.1",
        "PUT /mcp HTTP/1.1",
        "POST / HTTP/1.1",
        "POST /mcpx HTTP/1.1",
        "POST /mcp/tools HTTP/1.1",
        "POST /MCP HTTP/1.1",
    ];
    for line in lines {
        let answer = exchange(addr, line, Some(&session), &list_body()).unwrap();
        assert!(answer.starts_with("HTTP/1.1 404 "), "{line}: {answer}");
    }
    let unparsed = exchange(addr, "POST /nowhere HTTP/1.1", Some(&session), b"not json").unwrap();
    assert!(
        unparsed.starts_with("HTTP/1.1 404 "),
        "the path before the body: {unparsed}"
    );
    Ok(())
}

/// `MAX_REQUEST`: "the most bytes one request may hold; past that the connection closes
/// unanswered". A request is its whole head and body, so that is what is counted here.
const MIB: usize = 1_048_576;

/// A `tools/list` request of exactly `total` bytes, its body padded with JSON whitespace.
fn request_of(total: usize, addr: SocketAddr, session: &str) -> Vec<u8> {
    let mut pad = total - 512;
    loop {
        let mut body = list_body();
        let close = body.pop().unwrap();
        body.extend(std::iter::repeat_n(b' ', pad));
        body.push(close);
        let bytes = request(addr, POST, Some(session), &body);
        match bytes.len() {
            len if len == total => return bytes,
            len => pad = pad + total - len,
        }
    }
}

#[test]
fn a_request_of_one_mib_is_answered_and_one_byte_more_is_closed_unanswered() -> Outcome {
    let server = SimServer::start(sim().unwrap(), Variant::Honest)?;
    let session = Wire::connect(&server.url()?).session.unwrap();
    let addr = server.addr()?;
    let fits = request_of(MIB, addr, &session);
    assert_eq!(fits.len(), MIB);
    for splits in [vec![], vec![MIB - 1]] {
        let answer = send(addr, &fits, &splits).unwrap_or_default();
        assert!(
            answer.starts_with("HTTP/1.1 200 OK\r\n"),
            "{splits:?}: {answer:.80}"
        );
        assert!(body_of(&answer)["result"]["tools"].is_array(), "{splits:?}");
    }
    let over = request_of(MIB + 1, addr, &session);
    assert_eq!(over.len(), MIB + 1);
    let splits = [
        vec![],
        vec![MIB],
        vec![1],
        vec![4097, MIB / 2 + 3],
        vec![MIB - 4095],
    ];
    for splits in splits {
        let answer = send(addr, &over, &splits);
        assert_eq!(
            answer.as_deref().map(|a| &a[..a.len().min(80)]),
            None,
            "{splits:?}"
        );
    }
    let after = exchange(addr, POST, Some(&session), &list_body()).unwrap();
    assert!(after.starts_with("HTTP/1.1 200 "), "it serves on: {after}");
    Ok(())
}
