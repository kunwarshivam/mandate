//! A Robinhood sell's fill booked at its order's limit price (DEC-2300 items 1 to 3, DEC-2395):
//! Robinhood publishes no sell price, so a risk-reducing sell its broker record reports filled is
//! booked at the worst price the order allowed, its journaled limit, never its stop trigger, under
//! an id derived from the order and the cumulative quantity, when the ledger holds the shares and
//! the broker's position fell by exactly the increment. Anything else derives nothing, and the
//! reconciliation still pauses the agent.
//!
//! Every case runs on a profile like Robinhood's: whole shares, one GTC stop-limit as protection,
//! and no query by client order id, so the executor itself places the resting stop after an entry
//! fills and a listing is how a record reaches it. Each expectation is computed here from the
//! case's own inputs, never from the crate's predicates (`AGENTS.md`, independent oracles), and
//! every edited journal first asserts that its edit took effect.

mod common;

use common::{
    AGENT, FixedInstruments, FixedMandate, Shell, TestIds, broker_fill, broker_order, config, date,
    discretionary_exit, handoff, instrument, key as member, opening, ports, price,
    protected_opening, qty, risk_exit, signed_qty, snapshot, text,
};
use mandate_accounting::Side;
use mandate_canon::Value;
use mandate_domain::{
    AssetClass, CapabilityProfile, Cell, Idempotency, MarketSession, OrderType as Kind,
    ProtectionForm, QuantityForm, Retry, Row, TimeInForce as Tif,
};
use mandate_executor::{
    BrokerFill, BrokerOutcome, BrokerPosition, BrokerUpdate, ClientOrderId, FillId, Input,
    ListedOrder, OrderType, Ports, Purpose, ReconcileReason, RiskClock, SubmitOrder, TimeInForce,
    limit_fills,
};
use mandate_num::{Price, Usd};
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
/// The exit's limit, which differs from the entry's 150 and the broker record's 155, so the oracle
/// can tell which price a fill was booked at.
const EXIT_LIMIT: &str = "148.25";
/// The protective stop's trigger the protected entries carry.
const STOP: &str = "147";

fn key(id: &str) -> ClientOrderId {
    ClientOrderId::parse(id).expect("an id this platform derived")
}

/// Robinhood's shape: whole shares, limit orders, one stop-limit as protection, `day` and `gtc`,
/// and no query by client order id (DEC-529 items 2, 4 and 7).
fn profile() -> CapabilityProfile {
    let cell = |order_type, forms: &[ProtectionForm]| Cell {
        order_type,
        quantity_form: QuantityForm::Whole,
        times_in_force: [Tif::Day, Tif::Gtc].into_iter().collect(),
        protection_forms: forms.iter().copied().collect(),
    };
    let row = Row {
        asset_class: AssetClass::UsEquity,
        session: MarketSession::Regular,
        cells: vec![
            cell(Kind::Limit, &[]),
            cell(Kind::StopLimit, &[ProtectionForm::StopLimit]),
        ],
    };
    let idempotency = Idempotency {
        client_order_id: true,
        retry: Retry::Unknown,
        query_by_client_order_id: false,
    };
    CapabilityProfile::new(1, vec![row], idempotency).expect("a valid profile")
}

fn tick(shell: &mut Shell, ports: &Ports<'_>, at: &str) {
    let secs = UtcNanos::parse_rfc3339(at).expect("a timestamp").secs();
    shell.run(Input::Tick(RiskClock::from_secs(secs)), ports);
}

/// A stream opened, a process started on it under [`profile`], at [`TUESDAY`].
fn started(ports: &Ports<'_>) -> Shell {
    let mut shell = Shell::new(1);
    shell.fold_one(&common::stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(ports);
    shell.state = shell.state.clone().with_profile(profile());
    tick(&mut shell, ports, TUESDAY);
    shell
}

/// `body`, handed off as `intent`, sent and acknowledged as `broker_id`; answers its request.
fn sent(
    shell: &mut Shell,
    ports: &Ports<'_>,
    intent: &str,
    body: mandate_executor::IntentBody,
    broker_id: &str,
) -> SubmitOrder {
    let ran = shell.run(handoff(intent, AGENT, body), ports);
    let request = ran
        .submissions()
        .first()
        .map(|order| (*order).clone())
        .expect("the order is sent");
    acknowledged(shell, ports, &request, broker_id);
    request
}

fn acknowledged(shell: &mut Shell, ports: &Ports<'_>, request: &SubmitOrder, broker_id: &str) {
    shell.run(
        Input::Broker(Ok(BrokerOutcome::Submitted(broker_order(
            broker_id,
            Some(request.client_order_id.as_str()),
            AAPL,
            request.side,
            &request.qty.to_string(),
            "0",
            "accepted",
        )))),
        ports,
    );
}

/// `held` shares of AAPL bought with no protection and booked from a broker fill report.
fn holding(ports: &Ports<'_>, held: &str) -> Shell {
    let mut shell = started(ports);
    let entry = sent(
        &mut shell,
        ports,
        ENTRY,
        opening(AAPL, held, "150"),
        "b-entry",
    );
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-entry",
            Some(entry.client_order_id.as_str()),
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

/// One share bought with protection at [`STOP`]: once the entry fills, the executor itself
/// places the resting GTC stop-limit the profile offers, which the broker acknowledges. Answers
/// the shell and the stop's request.
fn protected(ports: &Ports<'_>) -> (Shell, SubmitOrder) {
    let mut shell = started(ports);
    let entry = sent(
        &mut shell,
        ports,
        ENTRY,
        protected_opening(AAPL, "1", "150", STOP, None),
        "b-entry",
    );
    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-entry",
            Some(entry.client_order_id.as_str()),
            "1",
            "150",
        ))),
        ports,
    );
    let stop = ran
        .submissions()
        .into_iter()
        .find(|order| order.purpose == Purpose::Protective)
        .cloned()
        .expect("the executor places the protective stop");
    assert_eq!(
        (stop.order_type, stop.tif, stop.side, stop.stop_price),
        (
            OrderType::StopLimit,
            TimeInForce::Gtc,
            Side::Sell,
            Some(price(STOP))
        ),
        "the profile's protection: one resting GTC stop-limit sell"
    );
    let limit = stop.limit_price.expect("a stop-limit carries its limit");
    assert_ne!(limit, price(STOP), "the limit is not the trigger");
    acknowledged(&mut shell, ports, &stop, "b-stop");
    (shell, stop)
}

/// The broker's record of `request` as a listing answers it: ours by id, `filled` of `filled`,
/// with `stop` as its stop price and the request's own limit, type and time in force.
fn listed(
    shell: &mut Shell,
    ports: &Ports<'_>,
    request: &SubmitOrder,
    filled: &str,
    stop: Option<Price>,
) {
    let mut order = broker_order(
        "b-stop",
        Some(request.client_order_id.as_str()),
        AAPL,
        Side::Sell,
        &request.qty.to_string(),
        filled,
        "filled",
    );
    order.limit_price = request.limit_price;
    order.stop_price = stop;
    shell.run(
        Input::Broker(Ok(BrokerOutcome::Listed {
            client_order_id: request.client_order_id.as_str().to_owned(),
            orders: vec![ListedOrder {
                order,
                order_type: request.order_type,
                tif: request.tif,
            }],
        })),
        ports,
    );
}

/// The broker's record of the sell `id`, `filled` of `quantity`.
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

/// `held_qty` shares booked, a `day` risk exit of `exiting` at [`EXIT_LIMIT`] sent on Tuesday, and
/// its record read back `filled` of `filled`. Answers the shell and the exit's client order id.
fn exited(ports: &Ports<'_>, held_qty: &str, exiting: &str, filled: &str) -> (Shell, String) {
    let mut shell = holding(ports, held_qty);
    let exit = sent(
        &mut shell,
        ports,
        EXIT,
        risk_exit(AAPL, exiting, EXIT_LIMIT),
        "b-exit",
    );
    let id = exit.client_order_id.as_str().to_owned();
    let status = if filled == exiting {
        "filled"
    } else {
        "partially_filled"
    };
    reported(&mut shell, ports, &id, exiting, filled, status);
    (shell, id)
}

/// One fill of the sell `id`, written out field by field from the case's own inputs.
fn expected(id: &str, cumulative: &str, increment: &str, limit: Price, day: Date) -> BrokerFill {
    BrokerFill {
        fill_id: FillId(format!("limit:{id}:{cumulative}")),
        client_order_id: Some(id.to_owned()),
        broker_order_id: None,
        instrument: instrument(AAPL),
        side: Side::Sell,
        qty: qty(increment),
        price: limit,
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

/// The account stream of `shell` folded afresh under [`profile`], each record's payload passed
/// through `edit` with its event type first, so one member alone differs.
fn replayed(shell: &Shell, edit: impl Fn(&str, &mut Value)) -> Shell {
    let mut replay = Shell::new(1);
    replay.state = replay.state.clone().with_profile(profile());
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

fn member_is(payload: &Value, name: &str, value: &str) -> bool {
    payload.get(name).and_then(Value::as_str) == Some(value)
}

/// The exit's own `OrderSubmitted`, which names it by client order id.
fn exit_submitted(kind: &str, payload: &Value, id: &str) -> bool {
    kind == "OrderSubmitted" && member_is(payload, "client_order_id", id)
}

/// The exit's `OrderRequestRecorded` companion, which names its intent, not its client order id:
/// the purpose and the stop price are folded from it.
fn exit_companion(kind: &str, payload: &Value) -> bool {
    kind == "OrderRequestRecorded" && member_is(payload, "intent_id", EXIT)
}

fn tuesday() -> Date {
    date("2026-10-20")
}

fn wednesday() -> Date {
    date("2026-10-21")
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
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, id) = exited(&ports, "2", "2", "2");

    let fills = derived(&shell, &[]);
    assert_eq!(
        fills,
        vec![expected(&id, "2", "2", price(EXIT_LIMIT), tuesday())]
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

/// DEC-2395 item 3: a `day` sell sent on Tuesday whose record is first read on Wednesday is dated
/// Tuesday, its submission's date: a `day` order can fill on no other.
#[test]
#[ignore = "pending E7-6"]
fn a_day_exit_read_a_day_later_is_dated_by_its_submission() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = holding(&ports, "1");
    let exit = sent(
        &mut shell,
        &ports,
        EXIT,
        risk_exit(AAPL, "1", EXIT_LIMIT),
        "b-exit",
    );
    tick(&mut shell, &ports, WEDNESDAY);
    let id = exit.client_order_id.as_str().to_owned();
    reported(&mut shell, &ports, &id, "1", "1", "filled");
    assert_eq!(
        derived(&shell, &[]),
        vec![expected(&id, "1", "1", price(EXIT_LIMIT), tuesday())]
    );
}

/// A broker that lists a sold-out position as a row of quantity zero reads as the same zero
/// (DEC-2390 item 1).
#[test]
#[ignore = "pending E7-6"]
fn a_sold_out_row_of_zero_reads_as_the_reduced_position() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (shell, id) = exited(&ports, "1", "1", "1");
    assert_eq!(
        derived(&shell, &[held(AAPL, "0")]),
        vec![expected(&id, "1", "1", price(EXIT_LIMIT), tuesday())]
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
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, id) = exited(&ports, "3", "3", "1");
    let first = derived(&shell, &[held(AAPL, "2")]);
    assert_eq!(
        first,
        vec![expected(&id, "1", "1", price(EXIT_LIMIT), tuesday())]
    );
    let (types, paused) = reconcile(&mut shell, &ports, vec![held(AAPL, "2")], first);
    assert!(!paused, "{types:?}");
    reported(&mut shell, &ports, &id, "3", "3", "filled");
    assert_eq!(
        derived(&shell, &[]),
        vec![expected(&id, "3", "2", price(EXIT_LIMIT), tuesday())]
    );
}

/// A discretionary exit, which a test can hand off directly, is booked at its own limit.
#[test]
#[ignore = "pending E7-6"]
fn a_discretionary_exit_is_booked_at_its_limit() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = holding(&ports, "1");
    let exit = sent(
        &mut shell,
        &ports,
        EXIT,
        discretionary_exit(AAPL, "1", "151.5"),
        "b-exit",
    );
    let id = exit.client_order_id.as_str().to_owned();
    reported(&mut shell, &ports, &id, "1", "1", "filled");
    assert_eq!(
        derived(&shell, &[]),
        vec![expected(&id, "1", "1", price("151.5"), tuesday())]
    );
}

/// An owner exit is one of DEC-2300's four purposes: the risk exit's journal with its companion's
/// purpose edited to `owner_exit` is booked the same.
#[test]
#[ignore = "pending E7-6"]
fn an_owner_exit_is_booked_at_its_limit() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (shell, id) = exited(&ports, "1", "1", "1");
    let owner = replayed(&shell, |kind, payload| {
        if exit_companion(kind, payload) {
            set(payload, "purpose", text("owner_exit"));
        }
    });
    assert_eq!(
        owner.state.order(&key(&id)).map(|order| order.purpose),
        Some(Purpose::OwnerExit),
        "the edit took effect"
    );
    assert_eq!(
        derived(&owner, &[]),
        vec![expected(&id, "1", "1", price(EXIT_LIMIT), tuesday())]
    );
}

/// The resting protective stop the executor placed: once a listing reports it filled, it is booked
/// at its limit, never its stop trigger, and dated by the executor's clock when it is booked,
/// Wednesday, not by its Tuesday submission.
#[test]
#[ignore = "pending E7-6"]
fn a_resting_stop_is_booked_at_its_limit_and_dated_when_booked() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, stop) = protected(&ports);
    tick(&mut shell, &ports, WEDNESDAY);
    listed(&mut shell, &ports, &stop, "1", stop.stop_price);
    let limit = stop.limit_price.expect("a stop-limit carries its limit");
    assert_eq!(
        derived(&shell, &[]),
        vec![expected(
            stop.client_order_id.as_str(),
            "1",
            "1",
            limit,
            wednesday()
        )]
    );
}

/// DEC-2395 item 2: a listed record of the stop whose stop price is not the journaled one is not
/// the order we placed (it may have been edited in the app), so it is never adopted and nothing is
/// booked; the same record with the journaled stop is, as the control.
#[test]
#[ignore = "pending E7-6"]
fn a_record_whose_stop_differs_is_never_booked() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut edited, stop) = protected(&ports);
    listed(&mut edited, &ports, &stop, "1", Some(price("140")));
    assert!(
        derived(&edited, &[]).is_empty(),
        "a record with another stop derives nothing"
    );
    let (mut control, stop) = protected(&ports);
    listed(&mut control, &ports, &stop, "1", stop.stop_price);
    assert_eq!(
        derived(&control, &[]).len(),
        1,
        "the record with the journaled stop is booked"
    );
}

/// The control for every refusal below: with no fill derived, the sold-out position pauses the
/// agent, as on `main`.
#[test]
fn without_a_derived_fill_the_sold_out_position_still_pauses() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
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
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (shell, id) = exited(&ports, "3", "2", "2");
    assert_eq!(
        derived(&shell, &[held(AAPL, "1")]),
        vec![expected(&id, "2", "2", price(EXIT_LIMIT), tuesday())],
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

/// DEC-2300 item 3: a ledger that does not hold the increment derives nothing. The journal is the
/// control's, with the entry's booked fill edited down to one share.
#[test]
#[ignore = "pending E7-6"]
fn a_ledger_short_of_the_increment_derives_nothing() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (shell, id) = exited(&ports, "2", "2", "2");
    assert_eq!(
        derived(&replayed(&shell, |_, _| {}), &[]),
        vec![expected(&id, "2", "2", price(EXIT_LIMIT), tuesday())],
        "the control: 2 held, 2 sold"
    );
    let short = replayed(&shell, |kind, payload| {
        if kind == "FillApplied" && member_is(payload, "fill_id", "f-entry") {
            set(payload, "qty_gross", text("1"));
        }
    });
    assert_eq!(
        short.state.positions().get(&instrument(AAPL)).copied(),
        Some(signed_qty("1")),
        "the edit took effect: the ledger holds one share"
    );
    for positions in [Vec::new(), vec![held(AAPL, "-1")]] {
        assert!(
            derived(&short, &positions).is_empty(),
            "1 held, 2 reported sold: nothing is derived"
        );
    }
}

/// DEC-2300 items 1 and 2, one guard at a time on the same journal: the unedited replay derives
/// the fill, each edit is shown to have taken effect, and each alone refuses it.
#[test]
#[ignore = "pending E7-6"]
fn each_guard_alone_refuses_the_fill() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (shell, id) = exited(&ports, "2", "2", "2");
    let control = replayed(&shell, |_, _| {});
    assert_eq!(
        derived(&control, &[]),
        vec![expected(&id, "2", "2", price(EXIT_LIMIT), tuesday())],
        "the unedited journal derives the fill"
    );
    let purpose = |shell: &Shell| shell.state.order(&key(&id)).map(|order| order.purpose);
    let request = |shell: &Shell| shell.state.request_of(&key(&id)).cloned();

    let flatten = replayed(&shell, |kind, payload| {
        if exit_companion(kind, payload) {
            set(payload, "purpose", text("flatten"));
        }
    });
    assert_eq!(
        purpose(&flatten),
        Some(Purpose::Flatten),
        "the edit took effect"
    );
    assert!(
        derived(&flatten, &[]).is_empty(),
        "a flatten, which DEC-2300 does not name: nothing is derived"
    );

    let adds = replayed(&shell, |kind, payload| {
        if exit_companion(kind, payload) {
            set(payload, "purpose", text("open"));
        }
    });
    assert_eq!(purpose(&adds), Some(Purpose::Open), "the edit took effect");
    assert!(
        derived(&adds, &[]).is_empty(),
        "a purpose that adds risk: nothing is derived"
    );

    let unbounded = replayed(&shell, |kind, payload| {
        if exit_submitted(kind, payload, &id) {
            set(payload, "limit_price", Value::Null);
        }
    });
    assert_eq!(
        request(&unbounded).map(|sent| sent.limit_price),
        Some(None),
        "the edit took effect"
    );
    assert!(
        derived(&unbounded, &[]).is_empty(),
        "a sell with no limit price (fill_price_unbounded): nothing is derived"
    );

    let bought = replayed(&shell, |kind, payload| {
        if exit_submitted(kind, payload, &id) {
            set(payload, "side", text("buy"));
        }
    });
    assert_eq!(
        bought.state.order(&key(&id)).map(|order| order.side),
        Some(Side::Buy),
        "the edit took effect"
    );
    assert!(
        derived(&bought, &[]).is_empty(),
        "a buy: nothing is derived"
    );
}

/// `shell`'s journal with every filled quantity reported for the order `id` edited to `filled`.
fn reporting(shell: &Shell, id: &str, filled: &str) -> Shell {
    replayed(shell, |kind, payload| {
        let reports = payload
            .get("filled_qty")
            .is_some_and(|reported| *reported != Value::Null);
        if kind == "OrderStateChanged" && member_is(payload, "client_order_id", id) && reports {
            set(payload, "filled_qty", text(filled));
        }
    })
}

/// DEC-2300 item 1's cap alone: 4 held, a sell of 2, and its record's report edited. Reported 1,
/// with 3 listed, the increment fits every other guard and is derived, which shows the edit
/// reaches the fold; reported 3, more than the order's 2, with 1 listed, the ledger still holds the
/// 3 and the position is reduced by exactly 3, so only the order's quantity refuses it.
#[test]
#[ignore = "pending E7-6"]
fn a_report_beyond_the_orders_quantity_derives_nothing() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (shell, id) = exited(&ports, "4", "2", "2");
    assert_eq!(
        derived(&reporting(&shell, &id, "1"), &[held(AAPL, "3")]),
        vec![expected(&id, "1", "1", price(EXIT_LIMIT), tuesday())],
        "the edit reaches the fold: a report of 1 of 2 is derived"
    );
    assert!(
        derived(&reporting(&shell, &id, "3"), &[held(AAPL, "1")]).is_empty(),
        "a record reporting 3 of an order of 2: nothing is derived"
    );
}

/// An unattributed sell fill in the instrument (a sale the ledger booked with no order named)
/// leaves the position's history ambiguous, so nothing is derived though the arithmetic would fit.
#[test]
#[ignore = "pending E7-6"]
fn an_unattributed_fill_in_the_instrument_derives_nothing() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = holding(&ports, "3");
    let mut unnamed = broker_fill("f-unnamed", None, "1", "149");
    unnamed.side = Side::Sell;
    shell.run(Input::BrokerUpdate(BrokerUpdate::Fill(unnamed)), &ports);
    assert_eq!(
        shell.state.positions().get(&instrument(AAPL)).copied(),
        Some(signed_qty("2")),
        "the unnamed sale is booked"
    );
    let exit = sent(
        &mut shell,
        &ports,
        EXIT,
        risk_exit(AAPL, "2", EXIT_LIMIT),
        "b-exit",
    );
    reported(
        &mut shell,
        &ports,
        exit.client_order_id.as_str(),
        "2",
        "2",
        "filled",
    );
    assert!(
        derived(&shell, &[]).is_empty(),
        "an unattributed fill in the instrument: nothing is derived"
    );
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
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, _) = exited(&ports, "3", "1", "1");
    let other = sent(
        &mut shell,
        &ports,
        OTHER,
        risk_exit(AAPL, "1", EXIT_LIMIT),
        "b-other",
    );
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-other",
            Some(other.client_order_id.as_str()),
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

/// Positions in another instrument derive nothing for it and block nothing here: the order's own
/// instrument alone is read.
#[test]
#[ignore = "pending E7-6"]
fn the_order_is_read_against_its_own_instrument_alone() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, CPHC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (shell, id) = exited(&ports, "1", "1", "1");
    assert_eq!(
        derived(&shell, &[held(CPHC, "7")]),
        vec![expected(&id, "1", "1", price(EXIT_LIMIT), tuesday())]
    );
}

proptest! {
    /// Over any sequence of growing reports of one sell, each booked before the next is read,
    /// the booked increments sum to the last report, each id is booked once, every price is the
    /// limit, and nothing is booked beyond the order's quantity; a report beyond the quantity
    /// derives nothing. The oracle is a separate accumulator.
    #[test]
    #[ignore = "pending E7-6"]
    fn increments_sum_to_the_last_report_and_never_exceed_the_order(
        steps in proptest::collection::vec(1u32..3, 1..5),
        overshoot in 1u32..3,
    ) {
        let ids = TestIds;
        let mandates = FixedMandate::covering(&[AAPL]);
        let instruments = FixedInstruments;
        let config = config();
        let ports = ports(&ids, &mandates, &instruments, &config);
        let total: u32 = 6;
        let owned: u32 = total + 3;
        let mut shell = holding(&ports, &owned.to_string());
        let exit = sent(&mut shell, &ports, EXIT, risk_exit(AAPL, &total.to_string(), EXIT_LIMIT), "b-exit");
        let id = exit.client_order_id.as_str().to_owned();
        let mut cumulative = 0u32;
        let mut booked_sum = 0u32;
        let mut seen = std::collections::BTreeSet::new();
        for step in steps {
            cumulative = (cumulative + step).min(total);
            let status = if cumulative == total { "filled" } else { "partially_filled" };
            reported(&mut shell, &ports, &id, &total.to_string(), &cumulative.to_string(), status);
            let left = owned - cumulative;
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
        if cumulative < total {
            let beyond = total + overshoot;
            reported(&mut shell, &ports, &id, &total.to_string(), &beyond.to_string(), "filled");
            let listed = owned - beyond;
            prop_assert!(
                derived(&shell, &[held(AAPL, &listed.to_string())]).is_empty(),
                "a report beyond the order's quantity derives nothing, though the ledger holds it"
            );
        }
    }
}
