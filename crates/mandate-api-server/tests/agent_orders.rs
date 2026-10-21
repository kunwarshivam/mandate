//! An agent's orders folded from its account stream (`read-models/orders`; workspace API spec §4.7
//! D3; journal spec §9.5; DEC-2111 item 3): its own orders only, by the companion its submission
//! names; the request as first submitted; state and filled quantity from the last state change.

use mandate_api_server::reads::agent_orders;
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_journal::StoredEvent;
use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestError, TestRunner};

const ACCT: &str = "acct:01JB3K6F0C9R2V7N4M8Q1T5W3Y:acct_1";
const SPY: &str = "8ccae427-5dd0-45b3-b5fe-7c0c3a8ee5c4";

/// An account stream built row by row, `seq` from 1.
#[derive(Default)]
struct Account {
    rows: Vec<StoredEvent>,
}

impl Account {
    fn push(&mut self, kind: &str, causation: Option<&str>, payload: &str) -> String {
        let seq = u64::try_from(self.rows.len()).expect("a count") + 1;
        let event_id = format!("01JBH3BTZ8K2M4N6P8R0V{seq:05}");
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
            recorded_at: format!("2026-10-19T12:{:02}:{:02}.000000000Z", seq / 60, seq % 60),
            prev_hash: Digest::ZERO,
            hash: Digest::of(&body),
            body,
        });
        event_id
    }

    fn recorded_at(&self) -> String {
        self.rows
            .last()
            .map(|row| row.recorded_at.clone())
            .unwrap_or_default()
    }

    /// `agent`'s companion and the first submission of order `id`, a buy of `qty` at `limit`.
    fn submit(&mut self, agent: &str, id: &str, qty: &str, limit: &str) {
        let companion = self.push(
            "OrderRequestRecorded",
            None,
            &format!(
                r#"{{"agent_id":"{agent}","intent_id":null,"purpose":"open","extended_hours":false,"stop_price":null,"order_class":null,"take_profit":null,"stop":null,"rung":null,"at_floor":null,"risk_clock":"2026-10-19T12:00:00Z"}}"#
            ),
        );
        self.resubmit(&companion, id, 1, qty, limit);
    }

    fn resubmit(&mut self, companion: &str, id: &str, attempt: u64, qty: &str, limit: &str) {
        self.push(
            "OrderSubmitted",
            Some(companion),
            &format!(
                r#"{{"client_order_id":"{id}","attempt":{attempt},"instrument_id":"{SPY}","side":"buy","type":"limit","tif":"day","qty":"{qty}","limit_price":"{limit}","risk_clock":"2026-10-19T12:00:00Z"}}"#
            ),
        );
    }

    fn state(&mut self, id: &str, state: &str, filled: Option<&str>) {
        let filled = filled.map_or("null".to_owned(), |q| format!(r#""{q}""#));
        self.push(
            "OrderStateChanged",
            None,
            &format!(
                r#"{{"client_order_id":"{id}","state":"{state}","filled_qty":{filled},"attempted":null,"broker_status":null,"reject_code":null,"replaces":null,"replaced_by":null,"replaced_by_broker_order_id":null,"lookup":null,"ignored":false,"cancel_requested":false,"cancel_confirmed":false,"cancel_overdue":false,"adopted":false,"ladder_step":false,"risk_clock":"2026-10-19T12:00:00Z"}}"#
            ),
        );
    }
}

/// The served order, as `read-models/orders`'s `Order` states it.
#[expect(
    clippy::too_many_arguments,
    reason = "each argument is one member of the served order"
)]
fn order(
    id: &str,
    qty: &str,
    limit: &str,
    state: Option<&str>,
    filled: Option<&str>,
    submitted_at: &str,
    updated_at: &str,
    purpose: &str,
) -> Value {
    let quoted = |v: Option<&str>| v.map_or("null".to_owned(), |v| format!(r#""{v}""#));
    let text = format!(
        r#"{{"client_order_id":"{id}","intent_id":null,"purpose":"{purpose}","instrument_id":"{SPY}","side":"buy","type":"limit","tif":"day","qty":"{qty}","limit_price":"{limit}","stop_price":null,"state":{},"filled_qty":{},"submitted_at":"{submitted_at}","updated_at":"{updated_at}"}}"#,
        quoted(state),
        quoted(filled)
    );
    parse(text.as_bytes()).expect("JSON")
}

#[test]
fn only_the_agents_own_orders_are_listed_in_the_order_first_submitted() {
    let mut a = Account::default();
    a.push("StreamOpened", None, "{}");
    a.submit("agt_a", "o-1", "3", "101.5");
    let first = a.recorded_at();
    a.submit("agt_b", "o-2", "5", "99");
    a.submit("agt_a", "o-3", "1", "100");
    let third = a.recorded_at();
    assert_eq!(
        agent_orders(&a.rows, "agt_a"),
        vec![
            order("o-1", "3", "101.5", None, None, &first, &first, "open"),
            order("o-3", "1", "100", None, None, &third, &third, "open"),
        ],
        "agt_b's order on the same account is not agt_a's"
    );
    assert_eq!(
        agent_orders(&a.rows, "agt_c"),
        vec![],
        "an agent with no order"
    );
    assert_eq!(agent_orders(&[], "agt_a"), vec![], "an empty stream");
}

#[test]
fn state_and_filled_qty_are_the_last_state_changes_and_null_before_any() {
    let mut a = Account::default();
    a.submit("agt_a", "o-1", "3", "101.5");
    let submitted = a.recorded_at();
    a.state("o-1", "accepted", None);
    a.state("o-1", "partially_filled", Some("1"));
    a.state("o-1", "unknown", Some("1"));
    let unknown = a.recorded_at();
    assert_eq!(
        agent_orders(&a.rows, "agt_a"),
        vec![order(
            "o-1",
            "3",
            "101.5",
            Some("unknown"),
            Some("1"),
            &submitted,
            &unknown,
            "open"
        )],
        "unknown is served as unknown, never as its last known state"
    );
    let companion = a.rows[0].event_id.clone();
    a.resubmit(&companion, "o-1", 2, "3", "101.5");
    let resubmitted = a.recorded_at();
    a.state("o-1", "filled", Some("3"));
    let filled = a.recorded_at();
    assert_eq!(
        agent_orders(&a.rows, "agt_a"),
        vec![order(
            "o-1",
            "3",
            "101.5",
            Some("filled"),
            Some("3"),
            &submitted,
            &filled,
            "open"
        )],
        "a resubmission under the same id keeps the first request and time"
    );
    assert_ne!(resubmitted, submitted);
}

#[test]
fn an_order_no_companion_names_is_no_agents() {
    let mut a = Account::default();
    a.push(
        "OrderSubmitted",
        None,
        &format!(
            r#"{{"client_order_id":"o-v1","attempt":1,"instrument_id":"{SPY}","side":"buy","type":"limit","tif":"day","qty":"1","limit_price":"1","risk_clock":"2026-10-19T12:00:00Z"}}"#
        ),
    );
    let opened = a.push("StreamOpened", None, "{}");
    a.resubmit(&opened, "o-stray", 1, "1", "1");
    a.state("o-ghost", "filled", Some("1"));
    assert_eq!(
        agent_orders(&a.rows, "agt_a"),
        vec![],
        "no companion, a cause that is no companion, or no submission at all"
    );
}

/// One generated step: an agent's new order, a resubmission, or a state change of an order so far.
#[derive(Debug, Clone)]
enum Step {
    Submit {
        agent: bool,
        qty: u8,
    },
    Resubmit(u8),
    State {
        order: u8,
        state: u8,
        filled: Option<u8>,
    },
}

const STATES: [&str; 6] = [
    "accepted",
    "partially_filled",
    "unknown",
    "filled",
    "canceled",
    "rejected",
];

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        (any::<bool>(), 1_u8..9).prop_map(|(agent, qty)| Step::Submit { agent, qty }),
        any::<u8>().prop_map(Step::Resubmit),
        (any::<u8>(), 0_u8..6, proptest::option::of(0_u8..9)).prop_map(|(order, state, filled)| {
            Step::State {
                order,
                state,
                filled,
            }
        }),
    ]
}

/// The oracle's record of one order.
struct Kept {
    id: String,
    mine: bool,
    qty: String,
    companion: String,
    submitted_at: String,
    state: Option<String>,
    filled: Option<String>,
    updated_at: String,
}

/// The rows of `steps`, and the oracle's orders of `agt_a`: its own replay of the steps, kept as
/// the expected values in first-submitted order.
fn replay(steps: &[Step]) -> (Vec<StoredEvent>, Vec<Value>) {
    let mut a = Account::default();
    let mut orders: Vec<Kept> = Vec::new();
    for step in steps {
        match step {
            Step::Submit { agent, qty } => {
                let id = format!("o-{}", orders.len());
                let owner = if *agent { "agt_a" } else { "agt_b" };
                a.submit(owner, &id, &qty.to_string(), "10");
                let companion = a.rows[a.rows.len() - 2].event_id.clone();
                let at = a.recorded_at();
                orders.push(Kept {
                    id,
                    mine: *agent,
                    qty: qty.to_string(),
                    companion,
                    submitted_at: at.clone(),
                    state: None,
                    filled: None,
                    updated_at: at,
                });
            }
            Step::Resubmit(n) if !orders.is_empty() => {
                let at = usize::from(*n) % orders.len();
                let o = &mut orders[at];
                a.resubmit(&o.companion, &o.id, 2, "8", "11");
                o.updated_at = a.recorded_at();
            }
            Step::State {
                order,
                state,
                filled,
            } if !orders.is_empty() => {
                let at = usize::from(*order) % orders.len();
                let o = &mut orders[at];
                let filled = filled.map(|q| q.to_string());
                a.state(&o.id, STATES[usize::from(*state)], filled.as_deref());
                o.state = Some(STATES[usize::from(*state)].to_owned());
                o.filled = filled;
                o.updated_at = a.recorded_at();
            }
            _ => {}
        }
    }
    let expected = orders
        .iter()
        .filter(|o| o.mine)
        .map(|o| {
            order(
                &o.id,
                &o.qty,
                "10",
                o.state.as_deref(),
                o.filled.as_deref(),
                &o.submitted_at,
                &o.updated_at,
                "open",
            )
        })
        .collect();
    (a.rows, expected)
}

type Fold = fn(&[StoredEvent]) -> Vec<Value>;

fn real(rows: &[StoredEvent]) -> Vec<Value> {
    agent_orders(rows, "agt_a")
}

fn lists_every_agent(rows: &[StoredEvent]) -> Vec<Value> {
    let mut all = agent_orders(rows, "agt_a");
    all.extend(agent_orders(rows, "agt_b"));
    all
}

fn misses_the_last_record(rows: &[StoredEvent]) -> Vec<Value> {
    agent_orders(rows.split_last().map_or(rows, |(_, rest)| rest), "agt_a")
}

fn reverses(rows: &[StoredEvent]) -> Vec<Value> {
    agent_orders(rows, "agt_a").into_iter().rev().collect()
}

fn agrees(steps: &[Step], fold: Fold) -> Result<(), TestCaseError> {
    let (rows, expected) = replay(steps);
    let served = fold(&rows);
    prop_assert_eq!(served, expected);
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
fn the_fold_matches_an_independent_replay_and_catches_seeded_bugs() {
    let steps = proptest::collection::vec(step(), 0..24);
    if let Err(failure) = runner().run(&steps, |steps| agrees(&steps, real)) {
        panic!("{failure}");
    }
    for (name, bug) in [
        ("lists every agent's orders", lists_every_agent as Fold),
        ("misses the last record", misses_the_last_record),
        ("reverses the order", reverses),
    ] {
        let caught = runner().run(&steps, |steps| agrees(&steps, bug));
        assert!(
            matches!(caught, Err(TestError::Fail(..))),
            "the oracle catches the bug that {name}"
        );
    }
}
