//! An agent's positions folded from its account stream (`read-models/positions`; workspace API
//! spec §4.7 D3; journal spec §9.5; DEC-2111 items 4 and 5): the net filled quantity of its own
//! orders per instrument, never another agent's, and nothing served from part of its fills.

use mandate_api_server::reads::agent_positions;
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_journal::StoredEvent;
use mandate_num::{Price, SignedQty};
use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestError, TestRunner};

const ACCT: &str = "acct:01JB3K6F0C9R2V7N4M8Q1T5W3Y:acct_1";
const SPY: &str = "8ccae427-5dd0-45b3-b5fe-7c0c3a8ee5c4";
const QQQ: &str = "9ddbf538-6ee1-46c4-c6ff-8d1d4b9ff6d5";

#[derive(Default)]
struct Account {
    rows: Vec<StoredEvent>,
}

impl Account {
    fn push(&mut self, kind: &str, causation: Option<&str>, payload: &str) -> String {
        let seq = u64::try_from(self.rows.len()).expect("a count") + 1;
        let event_id = format!("01JBH3BTZ8K2M4N6P8R0W{seq:05}");
        let causation = causation.map_or("null".to_owned(), |id| format!(r#""{id}""#));
        let text = format!(
            r#"{{"causation_id":{causation},"environment":"paper","event_id":"{event_id}","event_type":"{kind}","payload":{payload},"stream_id":"{ACCT}"}}"#
        );
        let body = to_canonical(&parse(text.as_bytes()).expect("JSON"));
        self.rows.push(StoredEvent {
            stream_id: ACCT.to_owned(),
            seq,
            event_id: event_id.clone(),
            event_type: kind.to_owned(),
            schema_version: if kind == "OrderSubmitted" { 2 } else { 1 },
            environment: "paper".to_owned(),
            recorded_at: format!("2026-10-19T13:{:02}:{:02}.000000000Z", seq / 60, seq % 60),
            prev_hash: Digest::ZERO,
            hash: Digest::of(&body),
            body,
        });
        event_id
    }

    /// `agent`'s order `id` in `instrument`, with its companion.
    fn order(&mut self, agent: &str, id: &str, instrument: &str, side: &str) {
        let companion = self.push(
            "OrderRequestRecorded",
            None,
            &format!(
                r#"{{"agent_id":"{agent}","intent_id":null,"purpose":"open","extended_hours":false,"stop_price":null,"order_class":null,"take_profit":null,"stop":null,"rung":null,"at_floor":null,"risk_clock":"2026-10-19T13:00:00Z"}}"#
            ),
        );
        self.push(
            "OrderSubmitted",
            Some(&companion),
            &format!(
                r#"{{"client_order_id":"{id}","attempt":1,"instrument_id":"{instrument}","side":"{side}","type":"limit","tif":"day","qty":"100","limit_price":"10","risk_clock":"2026-10-19T13:00:00Z"}}"#
            ),
        );
    }

    fn fill(&mut self, kind: &str, id: &str, instrument: &str, side: &str, qty: &str) {
        let fill_id = format!("f-{}", self.rows.len());
        self.push(
            kind,
            None,
            &format!(
                r#"{{"fill_id":"{fill_id}","client_order_id":"{id}","instrument_id":"{instrument}","side":"{side}","qty_gross":"{qty}","price":"10","trade_date":"2026-10-19","risk_clock":"2026-10-19T13:00:00Z","fees":[]}}"#
            ),
        );
    }
}

/// A position every fill of which was at 10 (`Account::fill`'s price), so trading spec §8.1's
/// basis is the quantity × 10 however it was reached; with no mark there is no value, and with no
/// `ProtectionChanged` no protection (DEC-2136).
fn position(instrument: &str, qty: &str) -> Value {
    let basis = SignedQty::parse(qty)
        .expect("a quantity")
        .value_at(Price::parse("10").expect("a price"))
        .expect("in range");
    let text = format!(
        r#"{{"instrument_id":"{instrument}","filled_qty":"{qty}","cost_basis":{{"qty":"{qty}","basis":"{basis}"}},"risk_mark":null,"market_value":null,"unrealized_pnl":null,"protection":{{"orders":[],"covered_qty":"0","stop":null,"take_profit":null,"unprotected_since":null}}}}"#
    );
    parse(text.as_bytes()).expect("JSON")
}

#[test]
fn positions_net_the_agents_own_fills_per_instrument_in_instrument_order() {
    let mut a = Account::default();
    a.push("StreamOpened", None, "{}");
    a.order("agt_a", "o-1", SPY, "buy");
    a.order("agt_b", "o-2", SPY, "buy");
    a.order("agt_a", "o-3", QQQ, "buy");
    a.order("agt_a", "o-4", SPY, "sell");
    a.fill("FillApplied", "o-1", SPY, "buy", "10");
    a.fill("FillApplied", "o-2", SPY, "buy", "7");
    a.fill("LateFillApplied", "o-1", SPY, "buy", "2.5");
    a.fill("FillApplied", "o-4", SPY, "sell", "4");
    a.fill("FillApplied", "o-3", QQQ, "buy", "5");
    a.fill("FillApplied", "o-3", QQQ, "sell", "5");
    a.fill("FillApplied", "o-stray", SPY, "buy", "50");
    assert_eq!(
        agent_positions(&a.rows, "agt_a"),
        Some(vec![position(SPY, "8.5")]),
        "agt_b's and an unattributed order's fills are not agt_a's, and QQQ nets to zero"
    );
    assert_eq!(
        agent_positions(&a.rows, "agt_b"),
        Some(vec![position(SPY, "7")])
    );
    assert_eq!(agent_positions(&a.rows, "agt_c"), Some(vec![]));
    let mut short = Account::default();
    short.order("agt_a", "o-1", QQQ, "sell");
    short.order("agt_a", "o-2", SPY, "buy");
    short.fill("FillApplied", "o-1", QQQ, "sell", "3");
    short.fill("FillApplied", "o-2", SPY, "buy", "1");
    assert_eq!(
        agent_positions(&short.rows, "agt_a"),
        Some(vec![position(SPY, "1"), position(QQQ, "-3")]),
        "a net sale is served negative, never dropped; instruments in byte order"
    );
}

#[test]
fn an_own_fill_that_cannot_be_read_serves_no_position() {
    for (side, qty) in [("short", "1"), ("buy", "1.50"), ("buy", "x")] {
        let mut a = Account::default();
        a.order("agt_a", "o-1", SPY, "buy");
        a.fill("FillApplied", "o-1", SPY, "buy", "2");
        a.fill("FillApplied", "o-1", SPY, side, qty);
        assert_eq!(
            agent_positions(&a.rows, "agt_a"),
            None,
            "side {side}, qty {qty}"
        );
    }
    let mut other = Account::default();
    other.order("agt_b", "o-1", SPY, "buy");
    other.fill("FillApplied", "o-1", SPY, "short", "x");
    assert_eq!(
        agent_positions(&other.rows, "agt_a"),
        Some(vec![]),
        "another agent's unreadable fill is not this agent's to refuse"
    );
}

/// One generated fill: whose order, which instrument, which side, how many.
type Fill = (bool, bool, bool, u8);

/// The rows of `fills`, one order per fill, and the oracle's positions of `agt_a`, summed as
/// integers on their own.
fn replay(fills: &[Fill]) -> (Vec<StoredEvent>, Vec<Value>) {
    let mut a = Account::default();
    let (mut spy, mut qqq) = (0_i64, 0_i64);
    for (n, (mine, in_spy, buy, qty)) in fills.iter().enumerate() {
        let id = format!("o-{n}");
        let (agent, instrument) = (
            if *mine { "agt_a" } else { "agt_b" },
            if *in_spy { SPY } else { QQQ },
        );
        let side = if *buy { "buy" } else { "sell" };
        a.order(agent, &id, instrument, side);
        let kind = if n % 3 == 0 {
            "LateFillApplied"
        } else {
            "FillApplied"
        };
        a.fill(kind, &id, instrument, side, &qty.to_string());
        if *mine {
            let signed = if *buy {
                i64::from(*qty)
            } else {
                -i64::from(*qty)
            };
            *(if *in_spy { &mut spy } else { &mut qqq }) += signed;
        }
    }
    let expected = [(SPY, spy), (QQQ, qqq)]
        .into_iter()
        .filter(|(_, net)| *net != 0)
        .map(|(instrument, net)| position(instrument, &net.to_string()))
        .collect();
    (a.rows, expected)
}

type Fold = fn(&[StoredEvent]) -> Option<Vec<Value>>;

fn real(rows: &[StoredEvent]) -> Option<Vec<Value>> {
    agent_positions(rows, "agt_a")
}

fn counts_every_agent(rows: &[StoredEvent]) -> Option<Vec<Value>> {
    let relabelled: Vec<StoredEvent> = rows
        .iter()
        .map(|row| {
            let body = String::from_utf8_lossy(&row.body).replace("agt_b", "agt_a");
            StoredEvent {
                body: body.into_bytes(),
                ..row.clone()
            }
        })
        .collect();
    agent_positions(&relabelled, "agt_a")
}

fn drops_late_fills(rows: &[StoredEvent]) -> Option<Vec<Value>> {
    let kept: Vec<StoredEvent> = rows
        .iter()
        .filter(|r| r.event_type != "LateFillApplied")
        .cloned()
        .collect();
    agent_positions(&kept, "agt_a")
}

fn agrees(fills: &[Fill], fold: Fold) -> Result<(), TestCaseError> {
    let (rows, expected) = replay(fills);
    prop_assert_eq!(fold(&rows), Some(expected));
    Ok(())
}

fn runner() -> TestRunner {
    TestRunner::new(ProptestConfig {
        cases: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    })
}

#[test]
fn the_fold_matches_an_independent_sum_and_catches_seeded_bugs() {
    let fills = proptest::collection::vec(
        (any::<bool>(), any::<bool>(), any::<bool>(), 1_u8..20),
        0..20,
    );
    if let Err(failure) = runner().run(&fills, |fills| agrees(&fills, real)) {
        panic!("{failure}");
    }
    for (name, bug) in [
        ("counts every agent's fills", counts_every_agent as Fold),
        ("drops late fills", drops_late_fills),
    ] {
        let caught = runner().run(&fills, |fills| agrees(&fills, bug));
        assert!(
            matches!(caught, Err(TestError::Fail(..))),
            "the oracle catches the bug that {name}"
        );
    }
}

/// A net sale is marked at the ask, which a risk mark does not carry, so neither its value nor its
/// P&L is served at the bid (DEC-2136 item 3).
#[test]
fn a_short_sub_ledger_is_never_valued_at_the_bid() {
    let mut a = Account::default();
    a.order("agt_a", "o-1", QQQ, "sell");
    a.fill("FillApplied", "o-1", QQQ, "sell", "3");
    a.push(
        "MarkUpdated",
        None,
        &format!(
            r#"{{"instrument_id":"{QQQ}","price":"11","source":"quote","feed":"iex","bid":"11","sane":true,"session":"regular","risk_clock":"2026-10-19T13:00:05.000000000Z"}}"#
        ),
    );
    let served = agent_positions(&a.rows, "agt_a").expect("readable");
    assert_eq!(served.len(), 1);
    assert_eq!(served[0].get("market_value"), Some(&Value::Null));
    assert_eq!(served[0].get("unrealized_pnl"), Some(&Value::Null));
    assert_eq!(
        served[0].get("cost_basis").and_then(|b| b.get("qty")),
        Some(&Value::Str("-3".to_owned()))
    );
}
