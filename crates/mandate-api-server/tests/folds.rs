//! DEC-2145 item 10: the account reads' folds are held per stream and agent and resumed past the
//! rows they folded, and every read serves exactly what the fold of the whole stream serves.

use std::collections::BTreeMap;
use std::sync::Mutex;

use mandate_api_server::folds::{Folds, HELD};
use mandate_api_server::reads::{
    AccountFold, OrdersFold, PositionsFold, account_positions, agent_orders, agent_positions,
};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_journal::StoredEvent;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestCaseError, TestError, TestRunner};

const A: &str = "acct:01JB3K6F0C9R2V7N4M8Q1T5W3Y:acct_1";
const B: &str = "acct:01JB3K6F0C9R2V7N4M8Q1T5W3Z:acct_1";
const AGENTS: [&str; 2] = ["agt_a", "agt_b"];
const INSTRUMENTS: [&str; 2] = [
    "8ccae427-5dd0-45b3-b5fe-7c0c3a8ee5c4",
    "9ddbf538-6ee1-46c4-c6ff-8d1d4b9ff6d5",
];

/// One record of an account stream before it is chained: its type, the record it names as its
/// cause, and its payload.
#[derive(Debug, Clone)]
struct Spec {
    id: String,
    kind: String,
    causation: Option<String>,
    payload: String,
}

/// An account stream as records, chained into rows on each read: each row's body names the hash
/// of the row before, so changing one record changes every hash from it on, as in the journal.
#[derive(Debug, Clone)]
struct Account {
    stream: &'static str,
    specs: Vec<Spec>,
    orders: Vec<(String, String, String)>,
    nonce: u64,
}

impl Account {
    fn new(stream: &'static str) -> Self {
        Self {
            stream,
            specs: Vec::new(),
            orders: Vec::new(),
            nonce: 0,
        }
    }

    fn push(&mut self, kind: &str, causation: Option<String>, payload: String) -> String {
        self.nonce += 1;
        let id = format!("01JBH3BTZ8K2M4N6P8R{:07}", self.nonce);
        self.specs.push(Spec {
            id: id.clone(),
            kind: kind.to_owned(),
            causation,
            payload,
        });
        id
    }

    fn order(&mut self, agent: &str, instrument: &str, side: &str) {
        let id = format!("o-{}", self.nonce);
        let companion = self.push(
            "OrderRequestRecorded",
            None,
            format!(
                r#"{{"agent_id":"{agent}","intent_id":null,"purpose":"open","extended_hours":false,"stop_price":null,"order_class":null,"take_profit":null,"stop":null,"rung":null,"at_floor":null,"risk_clock":"2026-10-19T13:00:00Z"}}"#
            ),
        );
        self.push(
            "OrderSubmitted",
            Some(companion),
            format!(
                r#"{{"client_order_id":"{id}","attempt":1,"instrument_id":"{instrument}","side":"{side}","type":"limit","tif":"day","qty":"100","limit_price":"10","risk_clock":"2026-10-19T13:00:00Z"}}"#
            ),
        );
        self.orders
            .push((id, instrument.to_owned(), side.to_owned()));
    }

    fn fill(&mut self, order: usize, qty: &str) {
        let Some((id, instrument, side)) =
            self.orders.get(order % self.orders.len().max(1)).cloned()
        else {
            return;
        };
        let fill = format!("f-{}", self.nonce);
        self.push(
            "FillApplied",
            None,
            format!(
                r#"{{"fill_id":"{fill}","client_order_id":"{id}","instrument_id":"{instrument}","side":"{side}","qty_gross":"{qty}","price":"10","trade_date":"2026-10-19","risk_clock":"2026-10-19T13:00:00Z","fees":[]}}"#
            ),
        );
    }

    fn state(&mut self, order: usize, state: &str) {
        let Some((id, _, _)) = self.orders.get(order % self.orders.len().max(1)).cloned() else {
            return;
        };
        self.push(
            "OrderStateChanged",
            None,
            format!(
                r#"{{"client_order_id":"{id}","state":"{state}","filled_qty":"1","ignored":false}}"#
            ),
        );
    }

    fn observe(&mut self, instrument: &str, qty: &str) {
        self.push(
            "BrokerPositionObserved",
            None,
            format!(
                r#"{{"instrument":"{instrument}","model_qty":"{qty}","broker_qty":"{qty}","mismatch":false}}"#
            ),
        );
    }

    /// A fill no fold can read: its side is neither buy nor sell.
    fn unreadable(&mut self) {
        if let Some((id, instrument, _)) = self.orders.first().cloned() {
            self.push(
                "FillApplied",
                None,
                format!(
                    r#"{{"fill_id":"f-bad-{}","client_order_id":"{id}","instrument_id":"{instrument}","side":"short","qty_gross":"1","price":"10","trade_date":"2026-10-19","risk_clock":"2026-10-19T13:00:00Z","fees":[]}}"#,
                    self.nonce
                ),
            );
        }
    }

    /// Replaces record `at` with a different one, as rewritten history would.
    fn rewrite(&mut self, at: usize) {
        if self.specs.is_empty() {
            return;
        }
        let at = at % self.specs.len();
        self.nonce += 1;
        let spec = &mut self.specs[at];
        spec.kind = "StreamNoted".to_owned();
        spec.causation = None;
        spec.payload = format!(r#"{{"note":{}}}"#, self.nonce);
    }

    fn truncate(&mut self, keep: usize) {
        self.specs.truncate(keep);
    }

    fn rows(&self) -> Vec<StoredEvent> {
        let mut prev = Digest::ZERO;
        let mut rows = Vec::new();
        for (index, spec) in self.specs.iter().enumerate() {
            let seq = u64::try_from(index).unwrap() + 1;
            let causation = spec
                .causation
                .as_ref()
                .map_or("null".to_owned(), |id| format!(r#""{id}""#));
            let text = format!(
                r#"{{"causation_id":{causation},"environment":"paper","event_id":"{}","event_type":"{}","payload":{},"prev_hash":"{}","seq":{seq},"stream_id":"{}"}}"#,
                spec.id,
                spec.kind,
                spec.payload,
                prev.to_hex(),
                self.stream
            );
            let body = to_canonical(&parse(text.as_bytes()).expect("JSON"));
            let hash = Digest::of(&body);
            rows.push(StoredEvent {
                stream_id: self.stream.to_owned(),
                seq,
                event_id: spec.id.clone(),
                event_type: spec.kind.clone(),
                schema_version: if spec.kind == "OrderSubmitted" { 2 } else { 1 },
                environment: "paper".to_owned(),
                recorded_at: format!(
                    "2026-10-19T13:{:02}:{:02}.000000000Z",
                    seq / 60 % 60,
                    seq % 60
                ),
                prev_hash: prev,
                hash,
                body,
            });
            prev = hash;
        }
        rows
    }
}

/// What the three reads serve: the agent's orders, its positions, and the account's holdings.
type Served = (Vec<Value>, Option<Vec<Value>>, Vec<Value>);

/// The seeded folds held per stream and agent, with the count of rows each has folded.
type Held<F> = Mutex<BTreeMap<(String, String), (F, usize)>>;

/// What every read serves, by the fold of the whole stream.
fn whole(rows: &[StoredEvent], agent: &str) -> Served {
    (
        agent_orders(rows, agent),
        agent_positions(rows, agent),
        account_positions(rows),
    )
}

fn cached(folds: &Folds, rows: &[StoredEvent], agent: &str) -> Served {
    (
        folds.orders(rows, agent),
        folds.positions(rows, agent),
        folds.holdings(rows),
    )
}

fn grown(account: &mut Account, count: usize) {
    for n in 0..count {
        match n % 4 {
            0 => account.order(AGENTS[n % 2], INSTRUMENTS[n % 2], "buy"),
            1 => account.fill(n, "2"),
            2 => account.observe(INSTRUMENTS[n % 2], "2"),
            _ => account.state(n, "filled"),
        }
    }
}

#[test]
fn a_second_read_past_n_new_rows_folds_exactly_those_n() {
    let mut account = Account::new(A);
    grown(&mut account, 30);
    let folds = Folds::default();
    let rows = account.rows();
    let served = whole(&rows, "agt_a");
    assert!(
        !served.0.is_empty()
            && served.1.as_ref().is_some_and(|p| !p.is_empty())
            && !served.2.is_empty(),
        "every fold serves something here: {served:?}"
    );
    assert_eq!(cached(&folds, &rows, "agt_a"), served);
    let first = u64::try_from(rows.len()).unwrap();
    assert_eq!(folds.folded(), [first; 3]);
    grown(&mut account, 7);
    let rows = account.rows();
    let added = u64::try_from(rows.len()).unwrap() - first;
    assert!(added >= 7);
    assert_eq!(cached(&folds, &rows, "agt_a"), whole(&rows, "agt_a"));
    assert_eq!(
        folds.folded(),
        [first + added; 3],
        "each fold folded only the rows added"
    );
    assert_eq!(cached(&folds, &rows, "agt_a"), whole(&rows, "agt_a"));
    assert_eq!(
        folds.folded(),
        [first + added; 3],
        "nothing new folds nothing"
    );
}

#[test]
fn two_workspaces_and_two_agents_each_get_only_their_own_fold() {
    let mut a = Account::new(A);
    let mut b = Account::new(B);
    grown(&mut a, 20);
    a.order("agt_a", INSTRUMENTS[0], "buy");
    a.fill(10, "3");
    b.order("agt_a", INSTRUMENTS[1], "buy");
    b.fill(0, "1");
    b.observe(INSTRUMENTS[1], "1");
    let folds = Folds::default();
    for _ in 0..2 {
        for agent in AGENTS {
            let (rows_a, rows_b) = (a.rows(), b.rows());
            assert_eq!(cached(&folds, &rows_a, agent), whole(&rows_a, agent));
            assert_eq!(cached(&folds, &rows_b, agent), whole(&rows_b, agent));
        }
        a.fill(3, "1");
        b.fill(0, "2");
    }
    assert_ne!(whole(&a.rows(), "agt_a"), whole(&b.rows(), "agt_a"));
    assert_eq!(
        folds.held(),
        [4, 4, 2],
        "a fold per stream and agent; holdings per stream"
    );
}

#[test]
fn a_rewrite_or_a_cut_at_or_before_the_held_row_folds_from_the_first_row() {
    let mut account = Account::new(A);
    grown(&mut account, 24);
    let folds = Folds::default();
    let rows = account.rows();
    cached(&folds, &rows, "agt_a");
    let n = rows.len();
    account.rewrite(3);
    let rows = account.rows();
    assert_eq!(rows.len(), n, "the same length");
    assert_eq!(cached(&folds, &rows, "agt_a"), whole(&rows, "agt_a"));
    assert_eq!(
        folds.folded(),
        [u64::try_from(2 * n).unwrap(); 3],
        "the held row's hash changed, so every fold began again"
    );
    account.truncate(n - 5);
    let rows = account.rows();
    assert_eq!(cached(&folds, &rows, "agt_a"), whole(&rows, "agt_a"));
}

#[test]
fn a_row_no_fold_can_read_serves_no_position_and_holds_nothing() {
    let mut account = Account::new(A);
    grown(&mut account, 8);
    let folds = Folds::default();
    cached(&folds, &account.rows(), "agt_a");
    account.unreadable();
    let rows = account.rows();
    assert_eq!(folds.positions(&rows, "agt_a"), None);
    assert_eq!(folds.held()[1], 0, "the positions fold is dropped");
    assert_eq!(folds.positions(&rows, "agt_a"), None, "and stays unserved");
}

#[test]
fn at_most_held_folds_are_held_and_the_least_recently_read_goes_first() {
    let folds = Folds::default();
    let mut account = Account::new(A);
    grown(&mut account, 4);
    let rows = account.rows();
    for n in 0..=HELD {
        folds.orders(&rows, &format!("agt_{n}"));
    }
    assert_eq!(folds.held()[0], HELD);
    let before = folds.folded()[0];
    folds.orders(&rows, &format!("agt_{HELD}"));
    assert_eq!(folds.folded()[0], before, "the latest is held");
    folds.orders(&rows, "agt_0");
    assert!(folds.folded()[0] > before, "the first read was dropped");
}

#[test]
fn an_empty_stream_folds_to_nothing() {
    let folds = Folds::default();
    assert_eq!(cached(&folds, &[], "agt_a"), whole(&[], "agt_a"));
    assert_eq!(folds.held(), [0, 0, 0]);
}

#[derive(Debug, Clone)]
enum Op {
    Order(usize, usize, bool),
    Fill(usize, usize),
    State(usize),
    Observe(usize, usize),
    Unreadable,
    Rewrite(usize),
    Truncate(usize),
    Read(usize),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (0..2usize, 0..2usize, any::<bool>()).prop_map(|(a, i, s)| Op::Order(a, i, s)),
        4 => (0..8usize, 1..4usize).prop_map(|(o, q)| Op::Fill(o, q)),
        1 => (0..8usize).prop_map(Op::State),
        2 => (0..2usize, 0..3usize).prop_map(|(i, q)| Op::Observe(i, q)),
        1 => Just(Op::Unreadable),
        1 => (0..40usize).prop_map(Op::Rewrite),
        1 => (0..40usize).prop_map(Op::Truncate),
        6 => (0..2usize).prop_map(Op::Read),
    ]
}

/// The reads under test, as an oracle sees them.
trait Reads {
    fn read(&self, rows: &[StoredEvent], agent: &str) -> Served;
}

impl Reads for Folds {
    fn read(&self, rows: &[StoredEvent], agent: &str) -> Served {
        cached(self, rows, agent)
    }
}

/// The oracle: after any sequence of orders, fills, observations, unreadable rows, rewrites and
/// cuts, across two workspaces and two agents, every read is exactly the fold of the whole stream.
fn agrees(reads: &dyn Reads, ops: &[Op]) -> Result<(), TestCaseError> {
    let mut accounts = [Account::new(A), Account::new(B)];
    for (n, op) in ops.iter().enumerate() {
        let account = &mut accounts[n % 2];
        match *op {
            Op::Order(a, i, buy) => {
                account.order(AGENTS[a], INSTRUMENTS[i], if buy { "buy" } else { "sell" })
            }
            Op::Fill(o, q) => account.fill(o, &q.to_string()),
            Op::State(o) => account.state(o, "partially_filled"),
            Op::Observe(i, q) => account.observe(INSTRUMENTS[i], &q.to_string()),
            Op::Unreadable => account.unreadable(),
            Op::Rewrite(at) => account.rewrite(at),
            Op::Truncate(keep) => account.truncate(keep),
            Op::Read(agent) => {
                let rows = account.rows();
                prop_assert_eq!(
                    reads.read(&rows, AGENTS[agent]),
                    whole(&rows, AGENTS[agent]),
                    "{:?}",
                    op
                );
            }
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(Config { cases: 200, ..Config::default() })]
    #[test]
    fn every_resumed_fold_is_the_fold_of_the_whole_stream(ops in prop::collection::vec(op(), 1..60)) {
        agrees(&Folds::default(), &ops)?;
    }
}

/// A seeded bug: folds resumed at the held seq without checking that row's hash.
#[derive(Default)]
struct Unchecked {
    orders: Held<OrdersFold>,
    positions: Held<PositionsFold>,
}

fn resume<F: AccountFold>(
    held: &Held<F>,
    rows: &[StoredEvent],
    agent: &str,
    fresh: F,
) -> Option<F> {
    let Some(last) = rows.last() else {
        return Some(fresh);
    };
    let key = (last.stream_id.clone(), agent.to_owned());
    let (mut fold, from) = held
        .lock()
        .unwrap()
        .get(&key)
        .cloned()
        .filter(|(_, at)| *at <= rows.len())
        .unwrap_or((fresh, 0));
    for row in &rows[from..] {
        fold.fold(row)?;
    }
    held.lock().unwrap().insert(key, (fold.clone(), rows.len()));
    Some(fold)
}

impl Reads for Unchecked {
    fn read(&self, rows: &[StoredEvent], agent: &str) -> Served {
        (
            resume(&self.orders, rows, agent, OrdersFold::new(agent))
                .map_or_else(Vec::new, |f| f.finish()),
            resume(&self.positions, rows, agent, PositionsFold::new(agent))
                .and_then(|f| f.finish()),
            account_positions(rows),
        )
    }
}

#[test]
fn the_oracle_catches_folds_resumed_without_their_rows_hash() {
    let mut runner = TestRunner::new(Config {
        cases: 300,
        failure_persistence: None,
        ..Config::default()
    });
    let found = runner.run(&prop::collection::vec(op(), 1..60), |ops| {
        agrees(&Unchecked::default(), &ops)
    });
    assert!(
        matches!(found, Err(TestError::Fail(..))),
        "the oracle must fail folds resumed unchecked: {found:?}"
    );
}
