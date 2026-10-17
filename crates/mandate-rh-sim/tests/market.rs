//! The rehearsal's market (R0's step 2t, DEC-1152 item 3): a rehearsal server takes
//! `POST /rehearsal/market` with `{"quotes": {symbol: price}}`. It moves each quote, which
//! triggers a stop it reaches, and then offers the script's unused fills to the working orders
//! it can now fill, as a place does for the order it creates. It answers every order's id,
//! symbol, side and state. A server started outside a rehearsal has no such route, and a body
//! that is not exactly that shape changes nothing. Oracles: the core read directly.

mod common;

use common::wire::http;
use common::{AGENTIC, price, qty};
use mandate_rh_sim::{OrderRequest, Script, SimServer, State, Variant};
use serde_json::{Value, json};

const MARKET: &str = "/rehearsal/market";

/// One agentic account in the regular session, SPY at 500, and one scripted sell fill at 480.
fn script() -> Script {
    let text = json!({
        "accounts": [{"number": AGENTIC, "agentic_allowed": true, "cash": "10000",
            "buying_power": "10000", "pattern_day_trader": false}],
        "session": "regular",
        "quotes": {"SPY": "500"},
        "fills": [{"symbol": "SPY", "side": "sell", "price": "480"}],
    });
    Script::parse(&text.to_string()).unwrap()
}

/// A rehearsal server holding one SPY share bought at 500, and a GTC stop-limit sell of it at
/// stop 481 and limit 478, working and not triggered.
fn protected() -> (SimServer, String) {
    let server = SimServer::rehearse(script(), 0).unwrap();
    let stop = server
        .drive(|sim| {
            let entry = sim.place(&common::limit("buy", "1", "500", 1))?;
            sim.fill(&entry.id, qty("1"), price("500"))?;
            sim.place(&OrderRequest {
                order_type: "stop_limit".to_owned(),
                stop_price: Some("481".to_owned()),
                time_in_force: Some("gtc".to_owned()),
                ..common::limit("sell", "1", "478", 2)
            })
        })
        .unwrap()
        .unwrap();
    (server, stop.id)
}

fn market(server: &SimServer, body: &str) -> (u16, Value) {
    let url = server.url().unwrap().replace("/mcp", MARKET);
    let reply = http("POST", &url, Some("application/json"), body);
    let answer = serde_json::from_str(&reply.body).unwrap_or(Value::Null);
    (reply.status, answer)
}

fn state(server: &SimServer, id: &str) -> (State, mandate_num::Qty) {
    let orders = server.drive(|sim| sim.orders(AGENTIC)).unwrap().unwrap();
    let order = orders.iter().find(|order| order.id == id).unwrap();
    (order.state, order.filled_quantity)
}

/// A quote at the stop triggers it, the script's sell fill takes it at 480, inside its limit, and
/// the position is flat; the answer names the stop `filled`.
#[test]
fn a_quote_through_the_stop_fills_it_with_the_scripted_fill() {
    let (server, stop) = protected();
    let (status, answer) = market(&server, r#"{"quotes":{"SPY":"480"}}"#);
    assert_eq!(status, 200, "{answer}");
    assert_eq!(state(&server, &stop), (State::Filled, qty("1")));
    assert_eq!(
        server.drive(|sim| sim.position(AGENTIC, "SPY")).unwrap(),
        Ok(qty("0")),
        "the position is flat"
    );
    let named: Vec<(String, String)> = answer["orders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            (
                o["id"].as_str().unwrap().to_owned(),
                o["state"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert!(
        named.contains(&(stop.clone(), "filled".to_owned())),
        "{named:?}"
    );
}

/// A quote above the stop moves the market and fills nothing: the stop keeps working and the
/// scripted fill waits for a later move that reaches it.
#[test]
fn a_quote_above_the_stop_fills_nothing_and_the_fill_waits() {
    let (server, stop) = protected();
    let (status, _) = market(&server, r#"{"quotes":{"SPY":"490"}}"#);
    assert_eq!(status, 200);
    assert_eq!(state(&server, &stop), (State::Confirmed, qty("0")));
    market(&server, r#"{"quotes":{"SPY":"481"}}"#);
    assert_eq!(state(&server, &stop), (State::Filled, qty("1")));
}

/// Anything but `{"quotes": {symbol: decimal text}}` is refused with 400 and changes nothing;
/// a server outside a rehearsal has no market route at all.
#[test]
fn a_body_of_another_shape_changes_nothing_and_only_a_rehearsal_has_the_route() {
    let (server, stop) = protected();
    for body in [
        "not json",
        r#"{"quotes":{"SPY":480}}"#,
        r#"{"quotes":{"SPY":"480"},"fills":[]}"#,
        r#"{"quotes":[]}"#,
        r#"{}"#,
    ] {
        let (status, _) = market(&server, body);
        assert_eq!(status, 400, "{body}");
        assert_eq!(
            state(&server, &stop),
            (State::Confirmed, qty("0")),
            "{body}"
        );
    }
    let plain = SimServer::start(common::sim().unwrap(), Variant::Honest).unwrap();
    let (status, _) = market(&plain, r#"{"quotes":{"SPY":"480"}}"#);
    assert_eq!(status, 404, "no market outside a rehearsal");
}
