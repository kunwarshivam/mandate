//! A Robinhood sell's fill booked at its order's limit price (DEC-2300 items 1 to 3, DEC-2391):
//! Robinhood publishes no sell price, so a risk-reducing sell its broker record reports filled is
//! booked at the worst price the order allowed, its journaled limit, never its stop trigger, under
//! an id derived from the order and the cumulative quantity, when the ledger holds the shares and
//! the broker's position fell by exactly the increment. Anything else derives nothing, and the
//! reconciliation still pauses the agent.
//!
//! Each expectation is computed here from the case's own inputs, never from the crate's
//! predicates (`AGENTS.md`, independent oracles).

mod common;

use common::{
    AGENT, FixedInstruments, FixedMandate, Shell, TestIds, broker_fill, broker_order, config, date,
    discretionary_exit, handoff, instrument, key as member, opening, ports, price, qty, risk_exit,
    signed_qty, snapshot, text,
};
use mandate_accounting::Side;
use mandate_canon::Value;
use mandate_executor::{
    BrokerFill, BrokerOutcome, BrokerPosition, BrokerUpdate, FillId, Input, Ports, ReconcileReason,
    RiskClock, limit_fills,
};
use mandate_num::Usd;
use mandate_time::{Date, UtcNanos};
use proptest::prelude::*;

const AAPL: &str = FixedInstruments::LIQUID_EQUITY;
const CPHC: &str = FixedInstruments::THIN_EQUITY;
const ENTRY: &str = "01JABCDEFGHJKMNPQRSTVWXYZ0";
const EXIT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ1";
const OTHER: &str = "01JABCDEFGHJKMNPQRSTVWXYZ2";
/// Tuesday 2026-10-20 at 11:00 in New York.
const TUESDAY: &str = "2026-10-20T15:00:00Z";
/// Wednesday 2026-10-21 at 11:00 in New York.
const WEDNESDAY: &str = "2026-10-21T15:00:00Z";
/// The exit's limit, which differs from the entry's 150, the broker record's 155 and any stop, so
/// the oracle can tell which price a fill was booked at.
const EXIT_LIMIT: &str = "148.25";

fn secs(at: &str) -> i64 {
    UtcNanos::parse_rfc3339(at).expect("a timestamp").secs()
}

fn tick(shell: &mut Shell, ports: &Ports<'_>, at: &str) {
    shell.run(Input::Tick(RiskClock::from_secs(secs(at))), ports);
}

/// A stream opened, a process started on it at [`TUESDAY`], and `held` shares of AAPL bought and
/// booked from a broker fill report, so the ledger holds them.
fn holding(ports: &Ports<'_>, held: &str) -> Shell {
    let mut shell = Shell::new(1);
    shell.fold_one(&common::stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(ports);
    tick(&mut shell, ports, TUESDAY);
    let ran = shell.run(handoff(ENTRY, AGENT, opening(AAPL, held, "150")), ports);
    let entry = ran
        .submissions()
        .first()
        .map(|order| order.client_order_id.as_str().to_owned())
        .expect("the opening is sent");
    shell.run(
        Input::Broker(Ok(BrokerOutcome::Submitted(broker_order(
            "b-entry",
            Some(&entry),
            AAPL,
            Side::Buy,
            held,
            "0",
            "accepted",
        )))),
        ports,
    );
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-entry",
            Some(&entry),
            held,
            "150",
        ))),
        ports,
    );
    assert_eq!(
        shell.state.positions().get(&instrument(AAPL)).copied(),
        Some(signed_qty(held)),
        "the entry is booked"
    );
    shell
}

/// `body`, a sell of `AGENT`'s, handed off as `intent`, sent and acknowledged as `broker_id`;
/// answers its client order id.
fn sent(
    shell: &mut Shell,
    ports: &Ports<'_>,
    intent: &str,
    body: mandate_executor::IntentBody,
    broker_id: &str,
    quantity: &str,
) -> String {
    let ran = shell.run(handoff(intent, AGENT, body), ports);
    let id = ran
        .submissions()
        .first()
        .map(|order| order.client_order_id.as_str().to_owned())
        .expect("the sell is sent");
    shell.run(
        Input::Broker(Ok(BrokerOutcome::Submitted(broker_order(
            broker_id,
            Some(&id),
            AAPL,
            Side::Sell,
            quantity,
            "0",
            "accepted",
        )))),
        ports,
    );
    id
}

/// The broker's record of the sell `id`, `filled` of `quantity`, as a listing reads it.
fn reported(
    shell: &mut Shell,
    ports: &Ports<'_>,
    id: &str,
    quantity: &str,
    filled: &str,
    status: &str,
) {
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-exit",
            Some(id),
            AAPL,
            Side::Sell,
            quantity,
            filled,
            status,
        ))),
        ports,
    );
}

fn held(name: &str, quantity: &str) -> BrokerPosition {
    BrokerPosition {
        instrument: instrument(name),
        qty: signed_qty(quantity),
        avg_entry_price: price("150"),
    }
}

/// The R0 state after a risk exit: `held` shares booked, a risk exit of `exiting` at
/// [`EXIT_LIMIT`] sent on Tuesday, and its record read back as `filled` of `filled`.
fn exited(ports: &Ports<'_>, held_qty: &str, exiting: &str, filled: &str) -> (Shell, String) {
    let mut shell = holding(ports, held_qty);
    let id = sent(
        &mut shell,
        ports,
        EXIT,
        risk_exit(AAPL, exiting, EXIT_LIMIT),
        "b-exit",
        exiting,
    );
    let status = if filled == exiting {
        "filled"
    } else {
        "partially_filled"
    };
    reported(&mut shell, ports, &id, exiting, filled, status);
    (shell, id)
}

/// One fill of the sell `id`, written out field by field from the case's own inputs.
fn expected(id: &str, cumulative: &str, increment: &str, limit: &str, day: Date) -> BrokerFill {
    BrokerFill {
        fill_id: FillId(format!("limit:{id}:{cumulative}")),
        client_order_id: Some(id.to_owned()),
        broker_order_id: None,
        instrument: instrument(AAPL),
        side: Side::Sell,
        qty: qty(increment),
        price: price(limit),
        fees: Usd::ZERO,
        trade_date: day,
    }
}

fn derived(shell: &Shell, positions: &[BrokerPosition]) -> Vec<BrokerFill> {
    limit_fills(&shell.state, positions).expect("the limit fills are derived")
}

/// The reconciliation of a snapshot holding `positions` and `fills` at the shell's head: its
/// drafts' types, and whether it paused the agent on a position mismatch.
fn reconcile(
    shell: &mut Shell,
    ports: &Ports<'_>,
    positions: Vec<BrokerPosition>,
    fills: Vec<BrokerFill>,
) -> (Vec<String>, bool) {
    let mut taken = snapshot(shell.head().0, ReconcileReason::Startup);
    taken.positions = positions;
    taken.fills = fills;
    let ran = shell.run(Input::BrokerSnapshot(taken), ports);
    let types: Vec<String> = ran.draft_types().into_iter().map(str::to_owned).collect();
    let paused = ran
        .draft("BrokerPositionObserved")
        .is_some_and(|draft| draft.payload.get("mismatch") == Some(&Value::Bool(true)))
        && types.iter().any(|kind| kind == "AgentModeApplied");
    (types, paused)
}

/// The account stream of `shell` folded afresh, each record's payload passed through `edit` with
/// its event type first, so one member alone differs.
fn replayed(shell: &Shell, edit: impl Fn(&str, &mut Value)) -> Shell {
    let mut replay = Shell::new(1);
    for event in &shell.account_journal {
        let mut event = event.clone();
        edit(&event.event_type, &mut event.payload);
        replay.fold_one(&event).expect("the edited journal folds");
    }
    replay
}

fn set(payload: &mut Value, name: &str, value: Value) {
    let Value::Object(members) = payload else {
        panic!("a payload is an object");
    };
    members.insert(member(name), value);
}

/// Whether `payload` is about the order `id`.
fn about(payload: &Value, id: &str) -> bool {
    payload.get("client_order_id").and_then(Value::as_str) == Some(id)
}

fn ports_for<'a>(
    ids: &'a TestIds,
    mandates: &'a FixedMandate,
    instruments: &'a FixedInstruments,
    config: &'a mandate_executor::ExecutorConfig,
) -> Ports<'a> {
    ports(ids, mandates, instruments, config)
}

/// DEC-2300 item 1: a `day` risk exit its record reports wholly filled is booked once, at its
/// journaled limit (not the broker record's 155 nor the entry's 150), dated by its submission, and
/// the reconciliation then reads the sold-out position as explained; a second read books nothing.
#[test]
#[ignore = "pending E7-6"]
fn a_filled_risk_exit_is_booked_once_at_its_limit() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports_for(&ids, &mandates, &instruments, &config);
    let (mut shell, id) = exited(&ports, "2", "2", "2");

    let fills = derived(&shell, &[]);
    assert_eq!(
        fills,
        vec![expected(&id, "2", "2", EXIT_LIMIT, date("2026-10-20"))]
    );

    let (types, paused) = reconcile(&mut shell, &ports, Vec::new(), fills.clone());
    assert!(
        !paused,
        "the fill explains the sold-out position: {types:?}"
    );
    assert_eq!(
        shell.state.positions().get(&instrument(AAPL)).copied(),
        Some(signed_qty("0")),
        "the ledger is flat"
    );
    assert!(
        derived(&shell, &[]).is_empty(),
        "once booked, nothing more is derived"
    );
    let (again, paused) = reconcile(&mut shell, &ports, Vec::new(), fills);
    assert!(!paused, "{again:?}");
    assert!(
        !again.iter().any(|kind| kind.ends_with("FillApplied")),
        "the same id is never booked twice: {again:?}"
    );
}

/// A broker that lists a sold-out position as a row of quantity zero reads as the same zero.
#[test]
#[ignore = "pending E7-6"]
fn a_sold_out_row_of_zero_reads_as_the_reduced_position() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports_for(&ids, &mandates, &instruments, &config);
    let (shell, id) = exited(&ports, "1", "1", "1");
    assert_eq!(
        derived(&shell, &[held(AAPL, "0")]),
        vec![expected(&id, "1", "1", EXIT_LIMIT, date("2026-10-20"))]
    );
}

/// Partial fills are booked as increments over what is booked, each under its cumulative
/// quantity's id: 1 of 3, then the remaining 2 once the record reports 3.
#[test]
#[ignore = "pending E7-6"]
fn partial_fills_are_booked_as_increments() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports_for(&ids, &mandates, &instruments, &config);
    let (mut shell, id) = exited(&ports, "3", "3", "1");
    let first = derived(&shell, &[held(AAPL, "2")]);
    assert_eq!(
        first,
        vec![expected(&id, "1", "1", EXIT_LIMIT, date("2026-10-20"))]
    );
    let (types, paused) = reconcile(&mut shell, &ports, vec![held(AAPL, "2")], first);
    assert!(!paused, "{types:?}");
    reported(&mut shell, &ports, &id, "3", "3", "filled");
    assert_eq!(
        derived(&shell, &[]),
        vec![expected(&id, "3", "2", EXIT_LIMIT, date("2026-10-20"))]
    );
}

/// Each purpose DEC-2300 names is booked; a discretionary exit stands for the ones a test can
/// hand off directly, and the protective stop is the next test's.
#[test]
#[ignore = "pending E7-6"]
fn a_discretionary_exit_is_booked_at_its_limit() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports_for(&ids, &mandates, &instruments, &config);
    let mut shell = holding(&ports, "1");
    let id = sent(
        &mut shell,
        &ports,
        EXIT,
        discretionary_exit(AAPL, "1", "151.5"),
        "b-exit",
        "1",
    );
    reported(&mut shell, &ports, &id, "1", "1", "filled");
    assert_eq!(
        derived(&shell, &[]),
        vec![expected(&id, "1", "1", "151.5", date("2026-10-20"))]
    );
}

/// The resting protective stop: a GTC stop-limit is booked at its limit, never its stop trigger,
/// and dated by the executor's clock when it is booked, Wednesday, not by its Tuesday submission.
#[test]
#[ignore = "pending E7-6"]
fn a_resting_stop_is_booked_at_its_limit_and_dated_when_booked() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports_for(&ids, &mandates, &instruments, &config);
    let mut shell = holding(&ports, "1");
    let id = sent(
        &mut shell,
        &ports,
        EXIT,
        risk_exit(AAPL, "1", EXIT_LIMIT),
        "b-exit",
        "1",
    );
    tick(&mut shell, &ports, WEDNESDAY);
    reported(&mut shell, &ports, &id, "1", "1", "filled");
    let stop = replayed(&shell, |kind, payload| {
        if !about(payload, &id) {
            return;
        }
        if kind == "OrderSubmitted" {
            set(payload, "tif", text("gtc"));
            set(payload, "order_type", text("stop_limit"));
            set(payload, "stop_price", text("149"));
        }
        if matches!(kind, "OrderSubmitted" | "OrderRequestRecorded") {
            set(payload, "purpose", text("protective"));
        }
    });
    assert_eq!(
        derived(&stop, &[]),
        vec![expected(&id, "1", "1", EXIT_LIMIT, date("2026-10-21"))]
    );
}

/// The control for every case below: with no fill derived, the sold-out position pauses the
/// agent, as on `main`.
#[test]
fn without_a_derived_fill_the_sold_out_position_still_pauses() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports_for(&ids, &mandates, &instruments, &config);
    let (mut shell, _) = exited(&ports, "1", "1", "1");
    let (types, paused) = reconcile(&mut shell, &ports, Vec::new(), Vec::new());
    assert!(
        paused,
        "a position the ledger cannot explain pauses: {types:?}"
    );
}

/// DEC-2300 item 3: a position that is not the ledger's less exactly the increment derives
/// nothing: unchanged, reduced by more or by less, two rows, a short row, or a row in another
/// instrument only.
#[test]
#[ignore = "pending E7-6"]
fn a_position_not_reduced_by_exactly_the_increment_derives_nothing() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, CPHC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports_for(&ids, &mandates, &instruments, &config);
    let (shell, id) = exited(&ports, "3", "2", "2");
    assert_eq!(
        derived(&shell, &[held(AAPL, "1")]),
        vec![expected(&id, "2", "2", EXIT_LIMIT, date("2026-10-20"))],
        "the control: 3 held, 2 sold, 1 listed"
    );
    for (positions, why) in [
        (vec![held(AAPL, "3")], "unchanged"),
        (vec![held(AAPL, "2")], "reduced by less"),
        (Vec::new(), "reduced by more"),
        (vec![held(AAPL, "0")], "a zero row, reduced by more"),
        (vec![held(AAPL, "-1")], "short"),
        (vec![held(AAPL, "1"), held(AAPL, "0")], "two rows"),
        (vec![held(CPHC, "1")], "only another instrument"),
    ] {
        assert!(
            derived(&shell, &positions).is_empty(),
            "{why}: nothing is derived"
        );
    }
}

/// DEC-2300 item 3: a ledger that does not hold the increment derives nothing, since the sell
/// would then be selling what the agent never held. The journal is the control's, with the entry's
/// booked fill edited down to one share, so the ledger's shortfall is the only difference.
#[test]
#[ignore = "pending E7-6"]
fn a_ledger_short_of_the_increment_derives_nothing() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports_for(&ids, &mandates, &instruments, &config);
    let (shell, id) = exited(&ports, "2", "2", "2");
    assert_eq!(
        derived(&replayed(&shell, |_, _| {}), &[]),
        vec![expected(&id, "2", "2", EXIT_LIMIT, date("2026-10-20"))],
        "the control: 2 held, 2 sold"
    );
    let short = replayed(&shell, |kind, payload| {
        if kind == "FillApplied"
            && payload.get("fill_id").and_then(Value::as_str) == Some("f-entry")
        {
            set(payload, "qty_gross", text("1"));
        }
    });
    assert_eq!(
        short.state.positions().get(&instrument(AAPL)).copied(),
        Some(signed_qty("1")),
        "the edited ledger holds one share"
    );
    for positions in [Vec::new(), vec![held(AAPL, "-1")]] {
        assert!(
            derived(&short, &positions).is_empty(),
            "1 held, 2 reported sold: nothing is derived"
        );
    }
}

/// DEC-2300 items 1 and 2, one guard at a time on the same journal: the unedited replay derives
/// the fill, and each edit alone refuses it.
#[test]
#[ignore = "pending E7-6"]
fn each_guard_alone_refuses_the_fill() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports_for(&ids, &mandates, &instruments, &config);
    let (shell, id) = exited(&ports, "2", "2", "2");
    let control = replayed(&shell, |_, _| {});
    assert_eq!(
        derived(&control, &[]),
        vec![expected(&id, "2", "2", EXIT_LIMIT, date("2026-10-20"))],
        "the unedited journal derives the fill"
    );
    let cases: [(&[&str], &str, Value, &str); 5] = [
        (
            &["OrderSubmitted", "OrderRequestRecorded"],
            "purpose",
            text("flatten"),
            "a flatten, which DEC-2300 does not name",
        ),
        (
            &["OrderSubmitted", "OrderRequestRecorded"],
            "purpose",
            text("open"),
            "a purpose that adds risk",
        ),
        (
            &["OrderSubmitted"],
            "limit",
            Value::Null,
            "a sell with no limit price (fill_price_unbounded)",
        ),
        (
            &["OrderStateChanged"],
            "filled_qty",
            text("3"),
            "a record reporting more than the order's quantity",
        ),
        (&["OrderSubmitted"], "side", text("buy"), "a buy"),
    ];
    for (event_types, name, value, why) in cases {
        let edited = replayed(&shell, |kind, payload| {
            if event_types.contains(&kind) && about(payload, &id) {
                set(payload, name, value.clone());
            }
        });
        assert!(
            limit_fills(&edited.state, &[])
                .expect("the limit fills are derived")
                .is_empty(),
            "{why}: nothing is derived"
        );
    }
}

/// Another order in the instrument whose record reported more than it booked leaves the
/// position unattributable, so nothing is derived for either.
#[test]
#[ignore = "pending E7-6"]
fn a_second_unbooked_report_in_the_instrument_derives_nothing() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports_for(&ids, &mandates, &instruments, &config);
    let (mut shell, _) = exited(&ports, "3", "1", "1");
    let other = sent(
        &mut shell,
        &ports,
        OTHER,
        risk_exit(AAPL, "1", EXIT_LIMIT),
        "b-other",
        "1",
    );
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-other",
            Some(&other),
            AAPL,
            Side::Sell,
            "1",
            "1",
            "filled",
        ))),
        &ports,
    );
    for positions in [vec![held(AAPL, "2")], vec![held(AAPL, "1")]] {
        assert!(
            derived(&shell, &positions).is_empty(),
            "two orders with unbooked reports in the instrument"
        );
    }
}

/// Positions and reports in another instrument derive nothing for it and block nothing here:
/// the order's own instrument alone is read.
#[test]
#[ignore = "pending E7-6"]
fn the_order_is_read_against_its_own_instrument_alone() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, CPHC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports_for(&ids, &mandates, &instruments, &config);
    let (shell, id) = exited(&ports, "1", "1", "1");
    assert_eq!(
        derived(&shell, &[held(CPHC, "7")]),
        vec![expected(&id, "1", "1", EXIT_LIMIT, date("2026-10-20"))]
    );
}

proptest! {
    /// Over any sequence of growing reports of one sell, each booked before the next is read,
    /// the booked increments sum to the last report, each id is booked once, and nothing is ever
    /// booked beyond the order's quantity. The oracle is a separate accumulator.
    #[test]
    #[ignore = "pending E7-6"]
    fn increments_sum_to_the_last_report_and_never_exceed_the_order(
        steps in proptest::collection::vec(1u32..3, 1..5),
    ) {
        let ids = TestIds;
        let mandates = FixedMandate::covering(&[AAPL]);
        let instruments = FixedInstruments;
        let config = config();
        let ports = ports_for(&ids, &mandates, &instruments, &config);
        let total: u32 = 6;
        let mut shell = holding(&ports, &total.to_string());
        let id = sent(&mut shell, &ports, EXIT, risk_exit(AAPL, &total.to_string(), EXIT_LIMIT), "b-exit", &total.to_string());
        let mut cumulative = 0u32;
        let mut booked_sum = 0u32;
        let mut seen = std::collections::BTreeSet::new();
        for step in steps {
            cumulative = (cumulative + step).min(total);
            let status = if cumulative == total { "filled" } else { "partially_filled" };
            reported(&mut shell, &ports, &id, &total.to_string(), &cumulative.to_string(), status);
            let left = total - cumulative;
            let positions = vec![held(AAPL, &left.to_string())];
            let fills = derived(&shell, &positions);
            for one in &fills {
                prop_assert!(seen.insert(one.fill_id.clone()), "an id booked twice");
                prop_assert_eq!(one.price, price(EXIT_LIMIT));
                booked_sum += one.qty.to_string().parse::<u32>().expect("a whole quantity");
            }
            let (types, paused) = reconcile(&mut shell, &ports, positions, fills);
            prop_assert!(!paused, "{:?}", types);
            prop_assert_eq!(booked_sum, cumulative);
        }
        prop_assert!(booked_sum <= total);
    }
}
