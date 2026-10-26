//! A second, independent oracle for DEC-2833's counters (trading spec §9.6, mandate spec §5.3):
//! random account streams that mix the agent's orders of every purpose with another agent's, partly
//! and repeatedly filled orders, fills that name no order (a broker's leg), orders left working,
//! protective legs placed and cancelled, an owner's open order the broker lists, and instants that
//! cross the New York midnight on the risk clock and the recorded instant apart.
//! Each counter is derived here from the specs' definitions and DEC-2833 item 3's readings with
//! its own accumulators, never from the fold's types, and compared with `Journaled::counted` and
//! `Journaled::flat`.

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
const PURPOSES: [&str; 6] = [
    "open",
    "increase",
    "discretionary_exit",
    "risk_exit",
    "owner_exit",
    "protective",
];
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

/// One step of a generated account stream. Instants are seconds after the step before.
#[derive(Debug, Clone)]
enum Op {
    /// An order by the agent or the other agent, in an instrument, with a purpose; its rows are
    /// recorded `delay` seconds after its risk clock.
    Order {
        mine: bool,
        instrument: usize,
        purpose: usize,
        after: i64,
        delay: i64,
    },
    /// A fill of the `order`-th order placed so far (modulo their count), or of none. `dated`
    /// picks its trade date: its risk clock's day, today whatever its clock, or the day before,
    /// so a fill booked to today on yesterday's clock is generated too.
    Fill {
        order: Option<usize>,
        instrument: usize,
        buy: bool,
        qty: u64,
        after: i64,
        delay: i64,
        dated: usize,
    },
    /// The `order`-th order so far (modulo their count) reaches a terminal state.
    End {
        order: usize,
        state: usize,
        after: i64,
    },
    /// A protective leg placed in an instrument (`ProtectionChanged` `placed`), working until a
    /// `Cancel` ends it.
    Leg { instrument: usize, after: i64 },
    /// The `leg`-th leg placed so far (modulo their count) is cancelled.
    Cancel { leg: usize, after: i64 },
    /// The broker lists an open order of the owner's (`ExternalActivityIngested` `open_order`),
    /// whose end the account stream never journals.
    OwnerOrder { after: i64 },
}

fn ops() -> impl Strategy<Value = Vec<Op>> {
    let gap = 0i64..1_800;
    let delay = 0i64..300;
    let op = prop_oneof![
        (
            any::<bool>(),
            0usize..2,
            0usize..6,
            gap.clone(),
            delay.clone()
        )
            .prop_map(|(mine, instrument, purpose, after, delay)| Op::Order {
                mine,
                instrument,
                purpose,
                after,
                delay,
            }),
        (
            proptest::option::weighted(0.8, 0usize..64),
            0usize..2,
            any::<bool>(),
            1u64..4,
            gap.clone(),
            delay,
            0usize..3,
        )
            .prop_map(
                |(order, instrument, buy, qty, after, delay, dated)| Op::Fill {
                    order,
                    instrument,
                    buy,
                    qty,
                    after,
                    delay,
                    dated,
                }
            ),
        (0usize..64, 0usize..4, gap.clone()).prop_map(|(order, state, after)| Op::End {
            order,
            state,
            after,
        }),
        (0usize..2, gap.clone()).prop_map(|(instrument, after)| Op::Leg { instrument, after }),
        (0usize..64, gap.clone()).prop_map(|(leg, after)| Op::Cancel { leg, after }),
        gap.prop_map(|after| Op::OwnerOrder { after }),
    ];
    proptest::collection::vec(op, 0..40)
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

/// Folds `ops` into a fresh `Journaled` and, beside it, derives what each counter must be. Returns
/// the fold, the oracle's counters and the risk clock to read them at.
fn laid(ops: &[Op]) -> (Journaled, Expected, i64) {
    let mut fold = Journaled::new(ACCOUNT, CLOCK, AGENT);
    let mut put = |stream: &str, event_type: &str, payload: Value, recorded: Option<i64>| {
        fold.fold_recorded(
            Row {
                stream,
                event_type,
                payload: &payload,
            },
            recorded.map(instant),
        )
        .unwrap();
    };
    put(
        CONTROL,
        "ConnectionEstablished",
        object(&[("connection_id", text("conn-1"))]),
        None,
    );
    let mut placed: Vec<Placed> = Vec::new();
    let mut legs: Vec<(String, usize, bool)> = Vec::new();
    let mut owner_order = false;
    let mut held = [0u64; 2];
    let mut filled_orders: BTreeSet<String> = BTreeSet::new();
    let mut expected = Expected::default();
    let mut at = FIRST;
    for (step, op) in ops.iter().enumerate() {
        match *op {
            Op::Order {
                mine,
                instrument,
                purpose,
                after,
                delay,
            } => {
                at += after;
                let id = format!("o{step}");
                let purpose = PURPOSES[purpose];
                let recorded = at + delay;
                put(
                    ACCOUNT,
                    "OrderRequestRecorded",
                    object(&[
                        ("agent_id", text(if mine { AGENT } else { OTHER })),
                        ("purpose", text(purpose)),
                        ("risk_clock", clock(at)),
                    ]),
                    Some(recorded),
                );
                put(
                    ACCOUNT,
                    "OrderSubmitted",
                    object(&[
                        ("client_order_id", text(&id)),
                        ("instrument_id", text(INSTRUMENTS[instrument])),
                        ("risk_clock", clock(at)),
                    ]),
                    Some(recorded),
                );
                placed.push(Placed {
                    id,
                    mine,
                    instrument,
                    purpose,
                    at,
                    recorded,
                    ended: false,
                });
            }
            Op::Fill {
                order,
                instrument,
                buy,
                qty,
                after,
                delay,
                dated,
            } => {
                at += after;
                let of = order
                    .filter(|_| !placed.is_empty())
                    .map(|index| index % placed.len());
                let instrument = of.map_or(instrument, |index| placed[index].instrument);
                let buy = buy || held[instrument] < qty;
                held[instrument] = if buy {
                    held[instrument] + qty
                } else {
                    held[instrument] - qty
                };
                let recorded = at + delay;
                let trade_date = match dated {
                    0 if today(at) => TODAY,
                    0 | 2 => BEFORE,
                    _ => TODAY,
                };
                put(
                    ACCOUNT,
                    "FillApplied",
                    object(&[
                        ("fill_id", text(&format!("f{step}"))),
                        (
                            "client_order_id",
                            of.map_or(Value::Null, |index| text(&placed[index].id)),
                        ),
                        ("instrument_id", text(INSTRUMENTS[instrument])),
                        ("side", text(if buy { "buy" } else { "sell" })),
                        ("qty_gross", text(&qty.to_string())),
                        ("price", text("500")),
                        ("trade_date", text(trade_date)),
                        ("risk_clock", clock(at)),
                        ("fees", Value::Array(Vec::new())),
                    ]),
                    Some(recorded),
                );
                let name = INSTRUMENTS[instrument].to_owned();
                let instant = at.max(recorded);
                let side = expected
                    .last_side_fill
                    .entry((name.clone(), buy))
                    .or_insert(instant);
                *side = (*side).max(instant);
                if !buy {
                    let exit = expected
                        .last_exit_fill
                        .entry(name.clone())
                        .or_insert(instant);
                    *exit = (*exit).max(instant);
                }
                if trade_date == TODAY || today(at) || today(recorded) {
                    *expected.participation.entry(name.clone()).or_default() += qty;
                }
                if let Some(index) = of
                    && placed[index].mine
                    && trade_date == TODAY
                    && today(at)
                    && filled_orders.insert(placed[index].id.clone())
                {
                    *expected.filled.entry(name).or_default() += 1;
                }
            }
            Op::End {
                order,
                state,
                after,
            } => {
                if placed.is_empty() {
                    continue;
                }
                let index = order % placed.len();
                if placed[index].ended {
                    continue;
                }
                at += after;
                put(
                    ACCOUNT,
                    "OrderStateChanged",
                    object(&[
                        ("client_order_id", text(&placed[index].id)),
                        ("state", text(TERMINAL[state])),
                        ("ignored", Value::Bool(false)),
                        ("replaced_by", Value::Null),
                        ("risk_clock", clock(at)),
                    ]),
                    Some(at),
                );
                placed[index].ended = true;
            }
            Op::Leg { instrument, after } => {
                at += after;
                let id = format!("leg{step}");
                put(
                    ACCOUNT,
                    "ProtectionChanged",
                    object(&[
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
                legs.push((id, instrument, false));
            }
            Op::Cancel { leg, after } => {
                if legs.is_empty() {
                    continue;
                }
                let index = leg % legs.len();
                if legs[index].2 {
                    continue;
                }
                at += after;
                put(
                    ACCOUNT,
                    "ProtectionChanged",
                    object(&[
                        ("action", text("cancelled")),
                        ("instrument_id", text(INSTRUMENTS[legs[index].1])),
                        ("orders", Value::Array(vec![text(&legs[index].0)])),
                        ("qty", text("1")),
                        ("risk_clock", clock(at)),
                    ]),
                    Some(at),
                );
                legs[index].2 = true;
            }
            Op::OwnerOrder { after } => {
                at += after;
                put(
                    ACCOUNT,
                    "ExternalActivityIngested",
                    object(&[("source", text("open_order")), ("risk_clock", clock(at))]),
                    Some(at),
                );
                owner_order = true;
            }
        }
    }
    for order in placed.iter().filter(|order| order.mine) {
        if today(order.at) || today(order.recorded) {
            *expected
                .orders_per_instrument
                .entry(INSTRUMENTS[order.instrument].to_owned())
                .or_default() += 1;
            if matches!(order.purpose, "open" | "increase") {
                expected.orders_today += 1;
            }
        }
    }
    expected.flat = held == [0, 0]
        && placed.iter().all(|order| order.ended)
        && legs.iter().all(|leg| leg.2)
        && !owner_order;
    let now = (at + 300).max(MIDNIGHT + 36_000);
    (fold, expected, now)
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

    /// Over any mix of both agents' orders, partial and unattributed fills, working orders and
    /// instants across the risk day's midnight, every counter and the flatness are the specs'.
    #[test]
    fn every_counter_matches_an_oracle_read_from_the_specs(ops in ops()) {
        let (fold, expected, now) = laid(&ops);
        let counted = fold.counted(instant(now));
        prop_assert!(counted.is_some(), "every row read, so the counters read");
        let counted = counted.unwrap();
        prop_assert!(counted.conduct.resting_since.is_empty());
        prop_assert_eq!(observed(&counted, fold.flat()), without_zeros(expected));
    }
}
