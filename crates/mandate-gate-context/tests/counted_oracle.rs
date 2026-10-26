//! A second, independent oracle for DEC-2833's counters (trading spec §9.6, mandate spec §5.3):
//! random account streams that mix the agent's orders of every purpose with another agent's, partly
//! and repeatedly filled orders, fills that name no order or an order no request named, the same
//! fill journaled twice, orders left working, replaced, abandoned or ignored, protective legs placed
//! and cancelled, and instants on the risk clock and the recorded instant that cross the New York
//! midnight in either order. Most streams are then settled flat, and some given one blocker, so the
//! flatness is checked both ways. Each counter is derived here from the specs' definitions and
//! DEC-2833 item 3's readings with its own accumulators, never from the fold's types, and compared
//! with `Journaled::counted` and `Journaled::flat`.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Key, Value};
use mandate_gate_context::{Counted, Journaled, Row};
use mandate_risk::{AssetId, RestingSide};
use mandate_time::UtcNanos;
use proptest::prelude::*;

const ACCOUNT: &str = "acct:ws1:alpaca:paper1";
const CONTROL: &str = "ctl:ws1";
const CLOCK: &str = "clock:ws1";
const AGENT: &str = "agent-1";
const OTHER: &str = "agent-2";
const INSTRUMENTS: [&str; 2] = [
    "b28f4066-5c6d-479b-a2af-85dc1a8f16fb",
    "eb82ffb0-efd6-4f4e-9307-8fead28e04f6",
];
/// Every purpose an order may have (trading spec §6.1); the first two open or add risk (mandate
/// spec §5.3's orders per day).
const PURPOSES: [&str; 7] = [
    "open",
    "increase",
    "discretionary_exit",
    "risk_exit",
    "owner_exit",
    "protective",
    "flatten",
];
/// The order states that end an order (trading spec §5.7) the oracle writes as a state change.
const TERMINAL: [&str; 4] = ["filled", "canceled", "rejected", "expired"];
/// 2026-09-21 00:00 America/New_York (04:00 UTC, daylight time): the risk day the oracle's `now`
/// falls in starts here and lasts 86 400 s (mandate spec §5.3).
const MIDNIGHT: i64 = 1_789_963_200;
const DAY: i64 = 86_400;
const TODAY: &str = "2026-09-21";
/// The trading day before, a Friday, for fills booked to it.
const BEFORE: &str = "2026-09-18";
/// The streams start four hours before the risk day's midnight.
const FIRST: i64 = MIDNIGHT - 4 * 3_600;

fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

fn object(members: &[(&str, Value)]) -> Value {
    Value::Object(
        members
            .iter()
            .map(|(key, value)| (Key::new(key).unwrap(), value.clone()))
            .collect(),
    )
}

fn instant(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap()
}

fn clock(secs: i64) -> Value {
    text(&instant(secs).to_string())
}

fn today(secs: i64) -> bool {
    (MIDNIGHT..MIDNIGHT + DAY).contains(&secs)
}

/// Whom a fill names: an order placed so far (modulo their count), an order no request named (a
/// leg the broker created), or no order.
#[derive(Debug, Clone, Copy)]
enum Named {
    Placed(usize),
    Unknown,
    Nothing,
}

/// What happens to the `order`-th order placed so far (modulo their count).
#[derive(Debug, Clone, Copy)]
enum Change {
    /// A terminal state of [`TERMINAL`], which ends it.
    Ends(usize),
    /// `OrderAbandoned`, which ends it.
    Abandoned,
    /// A state that does not end it (`partially_filled`).
    Works,
    /// A terminal state the executor marks `ignored`, which changes nothing.
    Ignored,
    /// `replaced`, naming a successor that counts as an order of its own and works until ended.
    Replaced,
}

/// One step of a generated account stream. Instants are seconds after the step before; `delay`
/// is the recorded instant's offset from the risk clock, before or after it.
#[derive(Debug, Clone)]
enum Op {
    Order {
        mine: bool,
        instrument: usize,
        purpose: usize,
        after: i64,
        delay: i64,
    },
    /// A fill. `dated` picks its trade date: its risk clock's day, today whatever its clock, or
    /// the day before.
    Fill {
        named: Named,
        instrument: usize,
        buy: bool,
        qty: u64,
        after: i64,
        delay: i64,
        dated: usize,
    },
    /// The `fill`-th fill so far (modulo their count) journaled again, the same `fill_id` at the
    /// current risk clock, which the counters read once.
    Again { fill: usize, after: i64 },
    Change {
        order: usize,
        change: Change,
        after: i64,
        delay: i64,
    },
    /// A protective leg placed in an instrument, working until a `Cancel` ends it.
    Leg { instrument: usize, after: i64 },
    /// The `leg`-th leg placed so far (modulo their count) is cancelled.
    Cancel { leg: usize, after: i64 },
}

/// What a settled stream is left with, if anything: none, an owner's open order the broker lists,
/// one working order, one open leg, a position, a request whose submission never came, or a
/// working order whose only terminal state the executor marked `ignored`.
const BLOCKERS: usize = 7;

fn ops() -> impl Strategy<Value = (Vec<Op>, Option<usize>)> {
    let gap = 0i64..1_800;
    let delay = -300i64..300;
    let named = prop_oneof![
        8 => (0usize..64).prop_map(Named::Placed),
        1 => Just(Named::Unknown),
        1 => Just(Named::Nothing),
    ];
    let change = prop_oneof![
        4 => (0usize..4).prop_map(Change::Ends),
        1 => Just(Change::Abandoned),
        1 => Just(Change::Works),
        1 => Just(Change::Ignored),
        1 => Just(Change::Replaced),
    ];
    let op = prop_oneof![
        3 => (any::<bool>(), 0usize..2, 0usize..7, gap.clone(), delay.clone()).prop_map(
            |(mine, instrument, purpose, after, delay)| Op::Order {
                mine,
                instrument,
                purpose,
                after,
                delay,
            }
        ),
        4 => (
            named,
            0usize..2,
            any::<bool>(),
            1u64..4,
            gap.clone(),
            delay.clone(),
            0usize..3,
        )
            .prop_map(
                |(named, instrument, buy, qty, after, delay, dated)| Op::Fill {
                    named,
                    instrument,
                    buy,
                    qty,
                    after,
                    delay,
                    dated,
                }
            ),
        1 => (0usize..64, gap.clone()).prop_map(|(fill, after)| Op::Again { fill, after }),
        3 => (0usize..64, change, gap.clone(), delay).prop_map(|(order, change, after, delay)| {
            Op::Change {
                order,
                change,
                after,
                delay,
            }
        }),
        1 => (0usize..2, gap.clone()).prop_map(|(instrument, after)| Op::Leg { instrument, after }),
        1 => (0usize..64, gap).prop_map(|(leg, after)| Op::Cancel { leg, after }),
    ];
    (
        proptest::collection::vec(op, 0..40),
        proptest::option::weighted(0.75, 0usize..BLOCKERS),
    )
}

/// An order the oracle placed: its owner, instrument, purpose and instants.
struct Placed {
    id: String,
    mine: bool,
    instrument: usize,
    purpose: &'static str,
    at: i64,
    recorded: i64,
    ended: bool,
}

/// What the specs say the counters are, read from the generated steps alone.
#[derive(Debug, Default, PartialEq, Eq)]
struct Expected {
    orders_today: u32,
    orders_per_instrument: BTreeMap<String, u32>,
    filled: BTreeMap<String, u32>,
    last_side_fill: BTreeMap<(String, bool), i64>,
    participation: BTreeMap<String, u64>,
    last_exit_fill: BTreeMap<String, i64>,
    flat: bool,
}

/// The oracle's account stream: the rows it folds into the fold under test, and its own reading
/// of them.
struct Oracle {
    fold: Journaled,
    at: i64,
    placed: Vec<Placed>,
    legs: Vec<(String, usize, bool)>,
    held: [u64; 2],
    fills: Vec<Value>,
    filled_orders: BTreeSet<String>,
    owner_order: bool,
    asked: bool,
    expected: Expected,
}

impl Oracle {
    fn new() -> Self {
        let mut oracle = Self {
            fold: Journaled::new(ACCOUNT, CLOCK, AGENT),
            at: FIRST,
            placed: Vec::new(),
            legs: Vec::new(),
            held: [0, 0],
            fills: Vec::new(),
            filled_orders: BTreeSet::new(),
            owner_order: false,
            asked: false,
            expected: Expected::default(),
        };
        oracle.put(
            CONTROL,
            "ConnectionEstablished",
            &object(&[("connection_id", text("conn-1"))]),
            None,
        );
        oracle
    }

    fn put(&mut self, stream: &str, event_type: &str, payload: &Value, recorded: Option<i64>) {
        self.fold
            .fold_recorded(
                Row {
                    stream,
                    event_type,
                    payload,
                },
                recorded.map(instant),
            )
            .unwrap();
    }

    fn request(&mut self, mine: bool, purpose: &str, delay: i64) {
        let at = self.at;
        self.put(
            ACCOUNT,
            "OrderRequestRecorded",
            &object(&[
                ("agent_id", text(if mine { AGENT } else { OTHER })),
                ("purpose", text(purpose)),
                ("risk_clock", clock(at)),
            ]),
            Some(at + delay),
        );
    }

    fn order(&mut self, id: String, mine: bool, instrument: usize, purpose: usize, delay: i64) {
        let purpose = PURPOSES[purpose];
        self.request(mine, purpose, delay);
        let at = self.at;
        self.put(
            ACCOUNT,
            "OrderSubmitted",
            &object(&[
                ("client_order_id", text(&id)),
                ("instrument_id", text(INSTRUMENTS[instrument])),
                ("risk_clock", clock(at)),
            ]),
            Some(at + delay),
        );
        self.placed.push(Placed {
            id,
            mine,
            instrument,
            purpose,
            at,
            recorded: at + delay,
            ended: false,
        });
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "each member of the fill a step generates is its own argument"
    )]
    fn fill(
        &mut self,
        id: String,
        named: Named,
        instrument: usize,
        buy: bool,
        qty: u64,
        delay: i64,
        dated: usize,
    ) {
        let at = self.at;
        let of = match named {
            Named::Placed(index) if !self.placed.is_empty() => Some(index % self.placed.len()),
            _ => None,
        };
        let instrument = of.map_or(instrument, |index| self.placed[index].instrument);
        let buy = buy || self.held[instrument] < qty;
        self.held[instrument] = if buy {
            self.held[instrument] + qty
        } else {
            self.held[instrument] - qty
        };
        let recorded = at + delay;
        let trade_date = match dated {
            0 if today(at) => TODAY,
            0 | 2 => BEFORE,
            _ => TODAY,
        };
        let order = match (of, named) {
            (Some(index), _) => text(&self.placed[index].id),
            (None, Named::Unknown) => text(&format!("broker-leg-{id}")),
            (None, _) => Value::Null,
        };
        let payload = object(&[
            ("fill_id", text(&id)),
            ("client_order_id", order),
            ("instrument_id", text(INSTRUMENTS[instrument])),
            ("side", text(if buy { "buy" } else { "sell" })),
            ("qty_gross", text(&qty.to_string())),
            ("price", text("500")),
            ("trade_date", text(trade_date)),
            ("risk_clock", clock(at)),
            ("fees", Value::Array(Vec::new())),
        ]);
        self.put(ACCOUNT, "FillApplied", &payload, Some(recorded));
        self.fills.push(payload);
        let name = INSTRUMENTS[instrument].to_owned();
        let when = at.max(recorded);
        let side = self
            .expected
            .last_side_fill
            .entry((name.clone(), buy))
            .or_insert(when);
        *side = (*side).max(when);
        if !buy {
            let exit = self
                .expected
                .last_exit_fill
                .entry(name.clone())
                .or_insert(when);
            *exit = (*exit).max(when);
        }
        if trade_date == TODAY || today(at) || today(recorded) {
            *self.expected.participation.entry(name.clone()).or_default() += qty;
        }
        if let Some(index) = of
            && self.placed[index].mine
            && trade_date == TODAY
            && today(at)
            && self.filled_orders.insert(self.placed[index].id.clone())
        {
            *self.expected.filled.entry(name).or_default() += 1;
        }
    }

    fn change(&mut self, step: usize, index: usize, change: Change, delay: i64) {
        if self.placed[index].ended {
            return;
        }
        let at = self.at;
        let id = self.placed[index].id.clone();
        let state = |state: &str, ignored: bool, successor: Value| {
            object(&[
                ("client_order_id", text(&id)),
                ("state", text(state)),
                ("ignored", Value::Bool(ignored)),
                ("replaced_by", successor),
                ("risk_clock", clock(at)),
            ])
        };
        match change {
            Change::Ends(terminal) => {
                self.put(
                    ACCOUNT,
                    "OrderStateChanged",
                    &state(TERMINAL[terminal], false, Value::Null),
                    Some(at + delay),
                );
                self.placed[index].ended = true;
            }
            Change::Abandoned => {
                self.put(
                    ACCOUNT,
                    "OrderAbandoned",
                    &object(&[("client_order_id", text(&id)), ("risk_clock", clock(at))]),
                    Some(at + delay),
                );
                self.placed[index].ended = true;
            }
            Change::Works => self.put(
                ACCOUNT,
                "OrderStateChanged",
                &state("partially_filled", false, Value::Null),
                Some(at + delay),
            ),
            Change::Ignored => self.put(
                ACCOUNT,
                "OrderStateChanged",
                &state("filled", true, Value::Null),
                Some(at + delay),
            ),
            Change::Replaced => {
                let successor = format!("r{step}");
                self.put(
                    ACCOUNT,
                    "OrderStateChanged",
                    &state("replaced", false, text(&successor)),
                    Some(at + delay),
                );
                self.placed[index].ended = true;
                let replaced = &self.placed[index];
                let next = Placed {
                    id: successor,
                    mine: replaced.mine,
                    instrument: replaced.instrument,
                    purpose: replaced.purpose,
                    at,
                    recorded: at + delay,
                    ended: false,
                };
                self.placed.push(next);
            }
        }
    }

    fn leg(&mut self, id: String, instrument: usize) {
        let at = self.at;
        self.put(
            ACCOUNT,
            "ProtectionChanged",
            &object(&[
                ("action", text("placed")),
                ("instrument_id", text(INSTRUMENTS[instrument])),
                ("orders", Value::Array(vec![text(&id)])),
                ("qty", text("1")),
                ("stop", text("490")),
                ("take_profit", Value::Null),
                ("risk_clock", clock(at)),
            ]),
            Some(at),
        );
        self.legs.push((id, instrument, false));
    }

    fn cancel(&mut self, index: usize) {
        if self.legs[index].2 {
            return;
        }
        let at = self.at;
        let (id, instrument) = (self.legs[index].0.clone(), self.legs[index].1);
        self.put(
            ACCOUNT,
            "ProtectionChanged",
            &object(&[
                ("action", text("cancelled")),
                ("instrument_id", text(INSTRUMENTS[instrument])),
                ("orders", Value::Array(vec![text(&id)])),
                ("qty", text("1")),
                ("risk_clock", clock(at)),
            ]),
            Some(at),
        );
        self.legs[index].2 = true;
    }

    fn step(&mut self, step: usize, op: &Op) {
        match *op {
            Op::Order {
                mine,
                instrument,
                purpose,
                after,
                delay,
            } => {
                self.at += after;
                self.order(format!("o{step}"), mine, instrument, purpose, delay);
            }
            Op::Fill {
                named,
                instrument,
                buy,
                qty,
                after,
                delay,
                dated,
            } => {
                self.at += after;
                self.fill(
                    format!("f{step}"),
                    named,
                    instrument,
                    buy,
                    qty,
                    delay,
                    dated,
                );
            }
            Op::Again { fill, after } => {
                if self.fills.is_empty() {
                    return;
                }
                self.at += after;
                let at = self.at;
                let mut payload = self.fills[fill % self.fills.len()].clone();
                if let Value::Object(members) = &mut payload {
                    members.insert(Key::new("risk_clock").unwrap(), clock(at));
                }
                self.put(ACCOUNT, "FillApplied", &payload, Some(at));
            }
            Op::Change {
                order,
                change,
                after,
                delay,
            } => {
                if self.placed.is_empty() {
                    return;
                }
                self.at += after;
                let index = order % self.placed.len();
                self.change(step, index, change, delay);
            }
            Op::Leg { instrument, after } => {
                self.at += after;
                self.leg(format!("leg{step}"), instrument);
            }
            Op::Cancel { leg, after } => {
                if self.legs.is_empty() {
                    return;
                }
                self.at += after;
                let index = leg % self.legs.len();
                self.cancel(index);
            }
        }
    }

    /// Settles the stream flat, a minute a step: every order ended, every leg cancelled, every
    /// position sold; then leaves `blocker` behind (see [`BLOCKERS`]).
    fn settle(&mut self, blocker: usize) {
        for index in 0..self.placed.len() {
            self.at += 60;
            self.change(1_000 + index, index, Change::Ends(1), 0);
        }
        for index in 0..self.legs.len() {
            self.at += 60;
            self.cancel(index);
        }
        for instrument in 0..2 {
            let qty = self.held[instrument];
            if qty > 0 {
                self.at += 60;
                let id = format!("flat{instrument}");
                self.fill(id, Named::Nothing, instrument, false, qty, 0, 0);
            }
        }
        self.at += 60;
        match blocker {
            1 => {
                let at = self.at;
                self.put(
                    ACCOUNT,
                    "ExternalActivityIngested",
                    &object(&[("source", text("open_order")), ("risk_clock", clock(at))]),
                    Some(at),
                );
                self.owner_order = true;
            }
            2 => self.order("working".to_owned(), true, 0, 2, 0),
            3 => self.leg("open-leg".to_owned(), 1),
            4 => self.fill("residual".to_owned(), Named::Nothing, 0, true, 1, 0, 0),
            5 => {
                self.request(true, "open", 0);
                self.asked = true;
            }
            6 => {
                self.order("ignored".to_owned(), true, 1, 0, 0);
                let index = self.placed.len() - 1;
                self.at += 60;
                self.change(2_000, index, Change::Ignored, 0);
            }
            _ => {}
        }
    }

    /// The fold, and the oracle's counters read in the risk day the oracle defines.
    fn expected(mut self) -> (Journaled, Expected) {
        for order in self.placed.iter().filter(|order| order.mine) {
            if today(order.at) || today(order.recorded) {
                *self
                    .expected
                    .orders_per_instrument
                    .entry(INSTRUMENTS[order.instrument].to_owned())
                    .or_default() += 1;
                if matches!(order.purpose, "open" | "increase") {
                    self.expected.orders_today += 1;
                }
            }
        }
        self.expected.flat = self.held == [0, 0]
            && self.placed.iter().all(|order| order.ended)
            && self.legs.iter().all(|leg| leg.2)
            && !self.owner_order
            && !self.asked;
        (self.fold, self.expected)
    }
}

/// The fold's counters in the oracle's shape, zero entries dropped from both.
fn observed(counted: &Counted, flat: bool) -> Expected {
    let name = |id: &AssetId| id.as_str().to_owned();
    Expected {
        orders_today: counted.orders_today,
        orders_per_instrument: counted
            .conduct
            .orders_today_per_instrument
            .iter()
            .filter(|(_, count)| **count > 0)
            .map(|(id, count)| (name(id), *count))
            .collect(),
        filled: counted
            .conduct
            .filled_today
            .iter()
            .filter(|(_, count)| **count > 0)
            .map(|(id, count)| (name(id), *count))
            .collect(),
        last_side_fill: counted
            .conduct
            .last_opposite_fill_at
            .iter()
            .map(|((id, side), at)| ((name(id), *side == RestingSide::Buy), at.secs()))
            .collect(),
        participation: counted
            .conduct
            .participation_today
            .iter()
            .map(|(id, qty)| (name(id), qty.to_string().parse::<u64>().unwrap()))
            .filter(|(_, qty)| *qty > 0)
            .collect(),
        last_exit_fill: counted
            .last_exit_fill_at
            .iter()
            .map(|(id, at)| (name(id), at.secs()))
            .collect(),
        flat,
    }
}

fn without_zeros(mut expected: Expected) -> Expected {
    expected.orders_per_instrument.retain(|_, count| *count > 0);
    expected.filled.retain(|_, count| *count > 0);
    expected.participation.retain(|_, qty| *qty > 0);
    expected
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    /// Over any mix of both agents' orders and their changes, partial, unattributed and repeated
    /// fills, legs, and instants across the risk day's midnight in either order, every counter and
    /// the flatness are the specs'. A stream settled with no blocker is flat by construction, and
    /// one with a blocker is not, so both readings of the flatness are checked.
    #[test]
    fn every_counter_matches_an_oracle_read_from_the_specs((ops, settle) in ops()) {
        let mut oracle = Oracle::new();
        for (step, op) in ops.iter().enumerate() {
            oracle.step(step, op);
        }
        if let Some(blocker) = settle {
            oracle.settle(blocker);
        }
        let now = (oracle.at + 300).max(MIDNIGHT + 36_000);
        prop_assert!(today(now), "the counters are read in the risk day the oracle defines");
        let (fold, expected) = oracle.expected();
        if let Some(blocker) = settle {
            prop_assert_eq!(expected.flat, blocker == 0, "settled with blocker {}", blocker);
        }
        let counted = fold.counted(instant(now));
        prop_assert!(counted.is_some(), "every row read, so the counters read");
        let counted = counted.unwrap();
        prop_assert!(counted.conduct.resting_since.is_empty());
        prop_assert_eq!(observed(&counted, fold.flat()), without_zeros(expected));
    }
}
