//! DEC-2833: an opening on an account that has traded may be answered only while the account is
//! flat, the workspace relates no other account, and every counter the gate reads beside the
//! agent's risk is folded from the account stream. `counted` answers exactly then. Each counter is checked against an oracle that reads the
//! generated round trips its own way, from mandate spec §5.3 and trading spec §9.6.

use std::collections::BTreeMap;

use mandate_canon::{Key, Value};
use mandate_gate_context::{Counted, Journaled, OpeningHold, Row};
use mandate_num::Qty;
use mandate_risk::{AssetId, RestingSide};
use mandate_time::UtcNanos;
use proptest::prelude::*;

const ACCOUNT: &str = "acct:ws1:alpaca:paper1";
const CONTROL: &str = "ctl:ws1";
const CLOCK: &str = "clock:ws1";
const AGENT: &str = "agent-1";
const SPY: &str = "b28f4066-5c6d-479b-a2af-85dc1a8f16fb";
const QQQ: &str = "eb82ffb0-efd6-4f4e-9307-8fead28e04f6";
/// Monday 2026-09-21 at 09:33:20 New York.
const START: i64 = 1_790_000_000;
/// 2026-09-21 00:00 New York: the start of `START`'s risk day.
const MIDNIGHT: i64 = 1_789_963_200;
const TODAY: &str = "2026-09-21";
const YESTERDAY: &str = "2026-09-18";

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

fn asset(id: &str) -> AssetId {
    AssetId::new(id).unwrap()
}

/// The fold under test, with the rows it has been handed.
struct Rig(Journaled);

impl Rig {
    /// The agent deployed on the account's one connection.
    fn new() -> Self {
        let mut rig = Self(Journaled::new(ACCOUNT, CLOCK, AGENT));
        rig.connection("ConnectionEstablished", "conn-1");
        rig.on(
            CONTROL,
            "AgentDeployed",
            &object(&[
                ("agent_id", text(AGENT)),
                ("mandate_version", text("sha256:v1")),
            ]),
            None,
        );
        rig
    }

    fn on(&mut self, stream: &str, event_type: &str, payload: &Value, recorded: Option<i64>) {
        self.0
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

    fn connection(&mut self, event_type: &str, id: &str) {
        self.on(
            CONTROL,
            event_type,
            &object(&[("connection_id", text(id))]),
            None,
        );
    }

    /// The agent's order `order` in `instrument` with `purpose`, requested and submitted at `at`
    /// and recorded at `recorded`.
    fn order(&mut self, order: &str, instrument: &str, purpose: &str, at: i64, recorded: i64) {
        self.on(
            ACCOUNT,
            "OrderRequestRecorded",
            &object(&[
                ("agent_id", text(AGENT)),
                ("purpose", text(purpose)),
                ("risk_clock", clock(at)),
            ]),
            Some(recorded),
        );
        self.on(
            ACCOUNT,
            "OrderSubmitted",
            &object(&[
                ("client_order_id", text(order)),
                ("instrument_id", text(instrument)),
                ("risk_clock", clock(at)),
            ]),
            Some(recorded),
        );
    }

    fn state(&mut self, order: &str, state: &str, at: i64) {
        self.on(
            ACCOUNT,
            "OrderStateChanged",
            &object(&[
                ("client_order_id", text(order)),
                ("state", text(state)),
                ("ignored", Value::Bool(false)),
                ("replaced_by", Value::Null),
                ("risk_clock", clock(at)),
            ]),
            Some(at),
        );
    }

    /// A fill `fill` of `order` (or of none) on `side`, of `qty`, at the risk clock `at` with
    /// `trade_date`, recorded at `recorded`.
    #[allow(
        clippy::too_many_arguments,
        reason = "each member of the fill a test names is its own argument"
    )]
    fn fill(
        &mut self,
        fill: &str,
        order: Option<&str>,
        instrument: &str,
        side: &str,
        qty: &str,
        (at, trade_date, recorded): (i64, &str, Option<i64>),
    ) {
        self.on(
            ACCOUNT,
            "FillApplied",
            &object(&[
                ("fill_id", text(fill)),
                ("client_order_id", order.map_or(Value::Null, text)),
                ("instrument_id", text(instrument)),
                ("side", text(side)),
                ("qty_gross", text(qty)),
                ("price", text("500")),
                ("trade_date", text(trade_date)),
                ("risk_clock", clock(at)),
                ("fees", Value::Array(Vec::new())),
            ]),
            recorded,
        );
    }

    /// A round trip in `instrument`: an opening bought and filled at `at`, then a risk exit sold
    /// and filled 100 s later, each recorded a second after its risk clock.
    fn round_trip(&mut self, n: usize, instrument: &str, at: i64) {
        let (open, exit) = (format!("o{n}"), format!("x{n}"));
        self.order(&open, instrument, "open", at, at + 1);
        self.fill(
            &format!("fb{n}"),
            Some(&open),
            instrument,
            "buy",
            "2",
            (at + 10, day(at + 10).1, Some(at + 11)),
        );
        self.state(&open, "filled", at + 10);
        self.order(&exit, instrument, "risk_exit", at + 100, at + 101);
        self.fill(
            &format!("fx{n}"),
            Some(&exit),
            instrument,
            "sell",
            "2",
            (at + 110, day(at + 110).1, Some(at + 111)),
        );
        self.state(&exit, "filled", at + 110);
    }
}

/// Whether an opening on the traded account may be answered from the fold: the account is flat
/// and its counters read, at a tick later the same day.
fn answers(rig: &Rig) -> bool {
    rig.0.flat() && rig.0.counted(instant(START + 600)).is_some()
}

/// After one round trip the account is flat, and an opening is held for nothing the fold reads:
/// the counters are each the round trip's, every instant the later of the fill's risk clock and the
/// instant the store recorded it.
#[test]
fn a_flat_account_after_a_round_trip_reads_every_counter() {
    let mut rig = Rig::new();
    rig.round_trip(0, SPY, START);
    assert!(rig.0.flat());
    assert!(answers(&rig));
    assert_eq!(
        rig.0.opening_hold(false),
        Some(OpeningHold::ResearchAgentTraded),
        "an unpinned agent stays held (E6-15m)"
    );
    let counted = rig.0.counted(instant(START + 600)).unwrap();
    let spy = asset(SPY);
    assert_eq!(counted.orders_today, 1, "the opening alone adds risk");
    assert_eq!(
        counted.conduct.orders_today_per_instrument,
        BTreeMap::from([(spy.clone(), 2)])
    );
    assert_eq!(
        counted.conduct.filled_today,
        BTreeMap::from([(spy.clone(), 2)])
    );
    assert_eq!(
        counted.conduct.last_opposite_fill_at,
        BTreeMap::from([
            ((spy.clone(), RestingSide::Buy), instant(START + 11)),
            ((spy.clone(), RestingSide::Sell), instant(START + 111)),
        ])
    );
    assert_eq!(
        counted.conduct.participation_today,
        BTreeMap::from([(spy.clone(), Qty::parse("4").unwrap())])
    );
    assert_eq!(counted.conduct.resting_since, BTreeMap::new());
    assert_eq!(
        counted.last_exit_fill_at,
        BTreeMap::from([(spy, instant(START + 111))])
    );
}

/// A position, an order still working, or a broker-created leg that has not ended holds every
/// opening, as before.
#[test]
fn a_position_or_a_working_order_holds_the_opening() {
    let mut rig = Rig::new();
    rig.order("o0", SPY, "open", START, START + 1);
    rig.fill(
        "fb0",
        Some("o0"),
        SPY,
        "buy",
        "2",
        (START + 10, TODAY, Some(START + 11)),
    );
    rig.state("o0", "filled", START + 10);
    assert!(!rig.0.flat(), "a position");
    assert!(!answers(&rig));

    let mut rig = Rig::new();
    rig.round_trip(0, SPY, START);
    rig.order("o1", QQQ, "open", START + 200, START + 201);
    assert!(!rig.0.flat(), "an order working");
    assert!(!answers(&rig));
    rig.state("o1", "rejected", START + 202);
    assert!(rig.0.flat(), "a rejected order ends");
    assert!(answers(&rig));

    rig.state("leg-1", "accepted", START + 300);
    assert!(
        !rig.0.flat(),
        "a leg the broker created works until it ends"
    );
    rig.on(
        ACCOUNT,
        "OrderAbandoned",
        &object(&[("client_order_id", text("leg-1"))]),
        Some(START + 301),
    );
    assert!(rig.0.flat());

    rig.fill(
        "fz",
        None,
        QQQ,
        "buy",
        "1",
        (START + 400, TODAY, Some(START + 401)),
    );
    assert!(
        !rig.0.flat(),
        "an unattributed fill moves the account's position"
    );

    let mut rig = Rig::new();
    rig.round_trip(0, SPY, START);
    rig.on(
        ACCOUNT,
        "OrderRequestRecorded",
        &object(&[
            ("agent_id", text("agent-2")),
            ("purpose", text("open")),
            ("risk_clock", clock(START + 500)),
        ]),
        Some(START + 500),
    );
    assert!(
        !rig.0.flat(),
        "a request whose submission is not journaled may be working"
    );
    assert!(!answers(&rig));
}

/// A second live connection relates another account the counters do not read, so the opening is
/// held until it is revoked (DEC-2833 item 4).
#[test]
fn a_second_live_connection_holds_until_it_is_revoked() {
    let mut rig = Rig::new();
    rig.round_trip(0, SPY, START);
    rig.connection("ConnectionEstablished", "conn-2");
    assert!(!answers(&rig));
    assert_eq!(rig.0.counted(instant(START + 600)), None);
    rig.connection("ConnectionRevoked", "conn-2");
    assert!(answers(&rig));
    assert!(rig.0.counted(instant(START + 600)).is_some());
}

/// A fill folded without the instant its row was recorded, an order of a purpose the fold does
/// not know, or a fill whose side is not a side leaves the counters unread, which holds the
/// opening; the facts are read as before.
#[test]
fn a_fill_without_its_recorded_instant_or_an_unknown_purpose_leaves_the_counters_unread() {
    let mut rig = Rig::new();
    rig.round_trip(0, SPY, START);
    rig.fill("fz", None, QQQ, "buy", "1", (START + 400, TODAY, None));
    rig.fill(
        "fz2",
        None,
        QQQ,
        "sell",
        "1",
        (START + 401, TODAY, Some(START + 402)),
    );
    assert!(rig.0.flat());
    assert_eq!(rig.0.counted(instant(START + 600)), None);
    assert!(!answers(&rig));

    let mut rig = Rig::new();
    rig.round_trip(0, SPY, START);
    rig.order("o1", SPY, "rebalance", START + 200, START + 201);
    rig.state("o1", "canceled", START + 202);
    assert!(rig.0.flat());
    assert_eq!(rig.0.counted(instant(START + 600)), None);
    assert!(!answers(&rig));
}

/// Orders and fills of an earlier risk day count toward no daily counter; their instants still
/// bind the cooldown and the opposite-side interval, which never reset. An order or a fill that
/// straddles midnight counts today by either of its instants, except that a fill counts toward
/// `filled_today` only when its trade date and its risk clock are both today, since counting it
/// more would loosen the order-to-fill ratio.
#[test]
fn the_risk_day_counts_by_either_instant_and_filled_today_by_both() {
    let mut rig = Rig::new();
    let before = MIDNIGHT - 3_600;
    rig.round_trip(0, SPY, before);
    let counted = rig.0.counted(instant(START)).unwrap();
    assert_eq!(counted.orders_today, 0);
    assert_eq!(counted.conduct.orders_today_per_instrument, BTreeMap::new());
    assert_eq!(counted.conduct.filled_today, BTreeMap::new());
    assert_eq!(counted.conduct.participation_today, BTreeMap::new());
    assert_eq!(
        counted.last_exit_fill_at,
        BTreeMap::from([(asset(SPY), instant(before + 111))])
    );
    assert_eq!(counted.conduct.last_opposite_fill_at.len(), 2);

    let mut rig = Rig::new();
    let late = MIDNIGHT - 1;
    rig.order("o0", QQQ, "open", late, MIDNIGHT + 1);
    rig.fill(
        "fb0",
        Some("o0"),
        QQQ,
        "buy",
        "3",
        (late, YESTERDAY, Some(MIDNIGHT + 1)),
    );
    rig.state("o0", "filled", MIDNIGHT + 2);
    rig.order("x0", QQQ, "discretionary_exit", MIDNIGHT + 5, MIDNIGHT + 5);
    rig.fill(
        "fx0",
        Some("x0"),
        QQQ,
        "sell",
        "3",
        (MIDNIGHT + 6, TODAY, Some(MIDNIGHT + 6)),
    );
    rig.state("x0", "filled", MIDNIGHT + 6);
    let counted = rig.0.counted(instant(START)).unwrap();
    let qqq = asset(QQQ);
    assert_eq!(counted.orders_today, 1, "recorded today");
    assert_eq!(
        counted.conduct.orders_today_per_instrument,
        BTreeMap::from([(qqq.clone(), 2)])
    );
    assert_eq!(
        counted.conduct.filled_today,
        BTreeMap::from([(qqq.clone(), 1)]),
        "the opening's fill was dated yesterday"
    );
    assert_eq!(
        counted.conduct.participation_today,
        BTreeMap::from([(qqq, Qty::parse("6").unwrap())])
    );
}

/// A fill the broker dated today, on a risk clock still before midnight, counts toward the day's
/// participation but not toward `filled_today`, whose fills must be today by both.
#[test]
fn a_fill_dated_today_on_yesterday_s_risk_clock_is_not_filled_today() {
    let mut rig = Rig::new();
    rig.order("o0", SPY, "open", MIDNIGHT - 2, MIDNIGHT - 2);
    rig.fill(
        "fb0",
        Some("o0"),
        SPY,
        "buy",
        "1",
        (MIDNIGHT - 1, TODAY, Some(MIDNIGHT - 1)),
    );
    rig.state("o0", "filled", MIDNIGHT - 1);
    rig.order("x0", SPY, "risk_exit", MIDNIGHT - 1, MIDNIGHT - 1);
    rig.fill(
        "fx0",
        Some("x0"),
        SPY,
        "sell",
        "1",
        (MIDNIGHT - 1, YESTERDAY, Some(MIDNIGHT - 1)),
    );
    rig.state("x0", "filled", MIDNIGHT - 1);
    let counted = rig.0.counted(instant(START)).unwrap();
    assert_eq!(counted.conduct.filled_today, BTreeMap::new());
    assert_eq!(
        counted.conduct.participation_today,
        BTreeMap::from([(asset(SPY), Qty::parse("1").unwrap())]),
        "dated today by the broker"
    );
}

/// The agent's order folded without the instant the store recorded it leaves the counters unread;
/// an order with no `risk_clock` counts by its recorded instant alone.
#[test]
fn an_order_without_its_recorded_instant_leaves_the_counters_unread() {
    let request = object(&[("agent_id", text(AGENT)), ("purpose", text("increase"))]);
    let submitted = object(&[
        ("client_order_id", text("o0")),
        ("instrument_id", text(SPY)),
    ]);
    let mut rig = Rig::new();
    rig.on(ACCOUNT, "OrderRequestRecorded", &request, None);
    rig.on(ACCOUNT, "OrderSubmitted", &submitted, None);
    rig.state("o0", "canceled", START);
    assert_eq!(rig.0.counted(instant(START + 10)), None);

    let mut rig = Rig::new();
    rig.on(ACCOUNT, "OrderRequestRecorded", &request, Some(START));
    rig.on(ACCOUNT, "OrderSubmitted", &submitted, Some(START));
    rig.state("o0", "canceled", START);
    assert_eq!(rig.0.counted(instant(START + 10)).unwrap().orders_today, 1);
    assert_eq!(
        rig.0.counted(instant(START + 86_400)).unwrap().orders_today,
        0
    );
}

/// A row the facts refuse, a corporate action among them, leaves the account not flat whatever its
/// fills net to, and the counters unread.
#[test]
fn a_row_the_facts_refuse_is_never_flat() {
    let mut rig = Rig::new();
    rig.round_trip(0, SPY, START);
    assert!(answers(&rig));
    let refused = rig.0.fold_recorded(
        Row {
            stream: ACCOUNT,
            event_type: "CorporateActionApplied",
            payload: &object(&[("risk_clock", clock(START + 300))]),
        },
        Some(instant(START + 300)),
    );
    assert!(refused.is_err());
    assert!(!rig.0.flat());
    assert_eq!(rig.0.counted(instant(START + 600)), None);
}

/// An open order of the owner's that the broker listed, whose end the account stream never
/// journals, leaves the account not flat; a protective leg placed is working until it is
/// cancelled or reaches a terminal state; a malformed record of either leaves the counters unread.
#[test]
fn an_owner_s_open_order_or_a_placed_leg_is_working() {
    let mut rig = Rig::new();
    rig.round_trip(0, SPY, START);
    let external = |source: &str| {
        object(&[
            ("source", text(source)),
            ("broker_order_id", text("b-1")),
            ("fill_id", Value::Null),
            ("client_order_id", Value::Null),
            ("instrument", text("SPY")),
            ("side", text("buy")),
            ("qty", text("1")),
            ("risk_clock", clock(START + 300)),
        ])
    };
    rig.on(
        ACCOUNT,
        "ExternalActivityIngested",
        &external("fill"),
        Some(START + 300),
    );
    assert!(
        answers(&rig),
        "an unattributed fill's record alone is no order"
    );
    rig.on(
        ACCOUNT,
        "ExternalActivityIngested",
        &external("open_order"),
        Some(START + 300),
    );
    assert!(!rig.0.flat());

    let protection = |action: &str, orders: &[&str]| {
        object(&[
            ("instrument_id", text(SPY)),
            ("action", text(action)),
            (
                "orders",
                Value::Array(orders.iter().map(|order| text(order)).collect()),
            ),
            ("risk_clock", clock(START + 400)),
        ])
    };
    let mut rig = Rig::new();
    rig.round_trip(0, SPY, START);
    rig.on(
        ACCOUNT,
        "ProtectionChanged",
        &protection("placed", &["leg-1", "leg-2"]),
        Some(START + 400),
    );
    assert!(!rig.0.flat(), "the bracket's legs rest");
    rig.on(
        ACCOUNT,
        "ProtectionChanged",
        &protection("cancelled", &["leg-1"]),
        Some(START + 401),
    );
    assert!(!rig.0.flat(), "one leg still rests");
    rig.state("leg-2", "filled", START + 402);
    assert!(answers(&rig));

    let mut rig = Rig::new();
    rig.round_trip(0, SPY, START);
    rig.on(
        ACCOUNT,
        "ExternalActivityIngested",
        &external("rumour"),
        Some(START + 300),
    );
    assert_eq!(rig.0.counted(instant(START + 600)), None);
    let mut rig = Rig::new();
    rig.round_trip(0, SPY, START);
    rig.on(
        ACCOUNT,
        "ProtectionChanged",
        &object(&[("action", text("placed")), ("orders", text("leg-1"))]),
        Some(START + 400),
    );
    assert_eq!(rig.0.counted(instant(START + 600)), None);
}

/// Another agent's orders on the account count toward none of this agent's order counters, and a
/// replacement of one of this agent's orders is an order of its own.
#[test]
fn only_the_agent_s_orders_count_and_a_replacement_is_one() {
    let mut rig = Rig::new();
    rig.on(
        ACCOUNT,
        "OrderRequestRecorded",
        &object(&[
            ("agent_id", text("agent-2")),
            ("purpose", text("open")),
            ("risk_clock", clock(START)),
        ]),
        Some(START),
    );
    rig.on(
        ACCOUNT,
        "OrderSubmitted",
        &object(&[
            ("client_order_id", text("theirs")),
            ("instrument_id", text(SPY)),
            ("risk_clock", clock(START)),
        ]),
        Some(START),
    );
    rig.state("theirs", "canceled", START + 1);
    rig.order("o0", SPY, "open", START + 2, START + 2);
    rig.on(
        ACCOUNT,
        "OrderStateChanged",
        &object(&[
            ("client_order_id", text("o0")),
            ("state", text("replaced")),
            ("ignored", Value::Bool(false)),
            ("replaced_by", text("o0-r")),
            ("risk_clock", clock(START + 3)),
        ]),
        Some(START + 3),
    );
    rig.state("o0-r", "canceled", START + 4);
    let counted = rig.0.counted(instant(START + 10)).unwrap();
    assert_eq!(counted.orders_today, 2);
    assert_eq!(
        counted.conduct.orders_today_per_instrument,
        BTreeMap::from([(asset(SPY), 2)])
    );
}

/// One generated event: a round trip in SPY (`false`) or QQQ, starting `offset` seconds after the
/// risk day began (negative is yesterday), each fill recorded `lag` seconds after its risk clock;
/// or an opening that is rejected.
#[derive(Debug, Clone)]
enum Event {
    RoundTrip { qqq: bool, offset: i64, lag: i64 },
    Rejected { qqq: bool, offset: i64 },
}

fn events() -> impl Strategy<Value = Vec<Event>> {
    prop::collection::vec(
        prop_oneof![
            3 => (any::<bool>(), -40_000i64..21_000, 0i64..5).prop_map(|(qqq, offset, lag)| Event::RoundTrip { qqq, offset, lag }),
            1 => (any::<bool>(), -40_000i64..21_000).prop_map(|(qqq, offset)| Event::Rejected { qqq, offset }),
        ],
        1..8,
    )
}

/// What the oracle expects, read from the events alone, by the specs' definitions.
#[derive(Debug, Default)]
struct Expected {
    orders_today: u32,
    per_instrument: BTreeMap<AssetId, u32>,
    filled_today: BTreeMap<AssetId, u32>,
    opposite: BTreeMap<(AssetId, RestingSide), UtcNanos>,
    participation: BTreeMap<AssetId, u64>,
    exits: BTreeMap<AssetId, UtcNanos>,
}

fn day(secs: i64) -> (i64, &'static str) {
    if secs >= MIDNIGHT {
        (1, TODAY)
    } else {
        (0, YESTERDAY)
    }
}

/// Lays the events down in time order and computes the oracle as it goes.
fn laid(events: &[Event]) -> (Rig, Expected) {
    let mut ordered: Vec<(usize, &Event)> = events.iter().enumerate().collect();
    ordered.sort_by_key(|(n, event)| {
        let offset = match event {
            Event::RoundTrip { offset, .. } | Event::Rejected { offset, .. } => *offset,
        };
        (offset, *n)
    });
    let mut rig = Rig::new();
    let mut expected = Expected::default();
    let mut free = i64::MIN;
    let mut at_of = |offset: i64| {
        let at = (MIDNIGHT + offset * 4).max(free);
        free = at + 3;
        at
    };
    for (n, event) in ordered {
        match event {
            Event::Rejected { qqq, offset } => {
                let (instrument, at) = (if *qqq { QQQ } else { SPY }, at_of(*offset));
                let order = format!("r{n}");
                rig.order(&order, instrument, "open", at, at);
                rig.state(&order, "rejected", at);
                if day(at).0 == 1 {
                    expected.orders_today += 1;
                    *expected
                        .per_instrument
                        .entry(asset(instrument))
                        .or_default() += 1;
                }
            }
            Event::RoundTrip { qqq, offset, lag } => {
                let (instrument, at) = (if *qqq { QQQ } else { SPY }, at_of(*offset));
                let (open, exit) = (format!("o{n}"), format!("x{n}"));
                rig.order(&open, instrument, "open", at, at);
                let (buy_at, sell_at) = (at + 1, at + 2);
                rig.fill(
                    &format!("fb{n}"),
                    Some(&open),
                    instrument,
                    "buy",
                    "1",
                    (buy_at, day(buy_at).1, Some(buy_at + lag)),
                );
                rig.state(&open, "filled", buy_at);
                rig.order(&exit, instrument, "risk_exit", sell_at, sell_at);
                rig.fill(
                    &format!("fx{n}"),
                    Some(&exit),
                    instrument,
                    "sell",
                    "1",
                    (sell_at, day(sell_at).1, Some(sell_at + lag)),
                );
                rig.state(&exit, "filled", sell_at);
                let id = asset(instrument);
                if day(at).0 == 1 {
                    expected.orders_today += 1;
                    *expected.per_instrument.entry(id.clone()).or_default() += 1;
                }
                for (side, fill_at) in [(RestingSide::Buy, buy_at), (RestingSide::Sell, sell_at)] {
                    let recorded = fill_at + lag;
                    if day(fill_at).0 == 1 {
                        *expected.filled_today.entry(id.clone()).or_default() += 1;
                    }
                    if day(fill_at).0 == 1 || day(recorded).0 == 1 {
                        *expected.participation.entry(id.clone()).or_default() += 1;
                    }
                    later(
                        &mut expected.opposite,
                        (id.clone(), side),
                        instant(recorded),
                    );
                }
                if day(sell_at).0 == 1 {
                    *expected.per_instrument.entry(id.clone()).or_default() += 1;
                }
                later(&mut expected.exits, id, instant(sell_at + lag));
            }
        }
    }
    (rig, expected)
}

fn later<K: Ord>(map: &mut BTreeMap<K, UtcNanos>, key: K, at: UtcNanos) {
    let entry = map.entry(key).or_insert(at);
    *entry = (*entry).max(at);
}

fn qtys(map: &BTreeMap<AssetId, Qty>) -> BTreeMap<AssetId, u64> {
    map.iter()
        .map(|(id, qty)| (id.clone(), qty.to_string().parse::<u64>().unwrap()))
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, failure_persistence: None, ..ProptestConfig::default() })]

    /// Over any sequence of round trips and rejected openings across two risk days, every counter
    /// is the oracle's, and the flat account holds no opening.
    #[test]
    fn every_counter_is_the_specs_reading_of_the_rows(events in events()) {
        let (rig, expected) = laid(&events);
        prop_assert!(rig.0.flat());
        prop_assert!(answers(&rig));
        let now = instant(MIDNIGHT + 86_399);
        let Counted { conduct, orders_today, last_exit_fill_at } = rig.0.counted(now).unwrap();
        prop_assert_eq!(orders_today, expected.orders_today);
        prop_assert_eq!(conduct.orders_today_per_instrument, expected.per_instrument);
        prop_assert_eq!(conduct.filled_today, expected.filled_today);
        prop_assert_eq!(conduct.last_opposite_fill_at, expected.opposite);
        prop_assert_eq!(qtys(&conduct.participation_today), expected.participation);
        prop_assert_eq!(last_exit_fill_at, expected.exits);
        prop_assert_eq!(conduct.resting_since, BTreeMap::new());
    }
}
