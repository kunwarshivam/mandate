//! E7-30, DEC-2040: an owner's exit handed with the owner's confirmed bid. The core journals the
//! confirmation as `OwnerConfirmationReceived` immediately after the intent (journal spec §9.5
//! rule 248). Outside the regular session it sells the exit as an extended-hours limit, priced from
//! the confirmed bid and never below the floor (trading spec §5.5). In the regular session the floor
//! binds too, so a confirmed exit is never sent at the market (DEC-2040 item 6). An unconfirmed
//! owner exit still waits for the session, and a confirmation on anything else, or with a limit
//! below its floor, is non-canonical.
//!
//! Every expected price is the ladder's first rung computed here: the liquid tier's `exit_offset`
//! of 0.5% off the confirmed bid, rounded up to the cent (§2.1, DEC-260 (6)).

mod common;

use common::{
    ACCOUNT_STREAM, AGENT, FixedInstruments, FixedMandate, Ran, Shell, TestIds, broker_order,
    clock, config, event, instrument, price, qty, quote, stream_opened, text, with_clock,
};
use mandate_accounting::Side;
use mandate_canon::Value;
use mandate_executor::{
    BrokerOutcome, BrokerRequest, EventId, ExecutorError, HandedConfirmation, Input, IntentBody,
    IntentHandoff, IntentId, OrderType, Ports, Purpose, SubmitOrder, TimeInForce,
};
use mandate_num::Price;

const AAPL: &str = FixedInstruments::LIQUID_EQUITY;
const INTENT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ0";
const REQUEST: &str = "01JABCDEFGHJKMNPQRSTVWXYZR";
/// The bid on the feed, which the owner never saw: a confirmed exit outside the session is not
/// priced from it.
const FEED_BID: &str = "150";
/// 2026-09-22 08:00 ET, pre-market on the committed calendar; clock-0 instants read as the
/// regular session (DEC-260 (13), DEC-506).
const PRE_MARKET: i64 = 1_790_078_400;
/// 2026-09-22 04:00 ET, the pre-market open.
const PRE_OPEN: i64 = PRE_MARKET - 4 * 3_600;
/// One minute before the pre-market open: no v1 session is open, so an equity exit is held
/// `session_closed` until the open (DEC-260 (13)).
const OVERNIGHT: i64 = PRE_OPEN - 60;

/// A confirmation's expiry for a case handed between `PRE_MARKET - 100` and `PRE_MARKET`: inside
/// the five minutes DEC-2040 item 7 gives it from the handing instant, and past every instant
/// those cases reach, unless one says otherwise.
const LATER: i64 = PRE_MARKET + 180;

fn confirmation(bid: &str, floor: &str) -> HandedConfirmation {
    expiring(bid, floor, LATER)
}

fn expiring(bid: &str, floor: &str, at: i64) -> HandedConfirmation {
    HandedConfirmation {
        bid: price(bid),
        bid_size: qty("300"),
        floor: price(floor),
        expires_at: clock(at),
        copied_from: EventId(REQUEST.to_owned()),
    }
}

/// A sell of ten `AAPL` for `purpose`, at `limit` or at the market.
fn exit(purpose: Purpose, limit: Option<&str>) -> IntentBody {
    IntentBody::Order {
        instrument: instrument(AAPL),
        side: Side::Sell,
        qty: qty("10"),
        limit: limit.map(price),
        purpose,
        protection: None,
    }
}

fn handed(body: IntentBody, confirmed: Option<HandedConfirmation>) -> Input {
    Input::Intent(IntentHandoff {
        intent_id: IntentId(EventId(INTENT.to_owned())),
        agent: mandate_executor::AgentId(AGENT.to_owned()),
        tif: Some(TimeInForce::Day),
        body,
        confirmation: confirmed,
    })
}

/// Ten `AAPL` bought by `AGENT` and filled, with nothing resting, folded at `at`.
fn held(ports: &Ports<'_>, at: i64) -> Shell {
    let mut shell = Shell::new(1);
    let order = || text("md-held-1");
    let events = [
        (
            "OrderSubmitted",
            vec![
                ("client_order_id", order()),
                ("agent", text(AGENT)),
                ("instrument", text(AAPL)),
                ("side", text("buy")),
                ("qty", text("10")),
                ("limit", text(FEED_BID)),
            ],
        ),
        (
            "FillApplied",
            vec![
                ("fill_id", text("f-0")),
                ("client_order_id", order()),
                ("instrument", text(AAPL)),
                ("side", text("buy")),
                ("qty_gross", text("10")),
                ("price", text(FEED_BID)),
            ],
        ),
        (
            "OrderStateChanged",
            vec![("client_order_id", order()), ("state", text("filled"))],
        ),
    ];
    shell.fold_one(&stream_opened()).expect("the stream opens");
    for (seq, (event_type, pairs)) in (2_u64..).zip(events) {
        shell
            .fold_one(&event(
                ACCOUNT_STREAM,
                seq,
                event_type,
                with_clock(&pairs, at),
            ))
            .expect("the position folds");
    }
    shell.restart_ready(ports)
}

fn only_submission(ran: &Ran) -> SubmitOrder {
    match ran.submissions().as_slice() {
        [one] => (*one).clone(),
        other => panic!("one submission, not {other:?}"),
    }
}

fn member<'a>(ran: &'a Ran, event_type: &str, name: &str) -> Option<&'a Value> {
    ran.draft(event_type)
        .and_then(|draft| draft.payload.get(name))
}

/// Before the open the confirmed exit goes extended-hours at 155 × 0.995 = 154.225, rounded up to
/// 154.23, from the confirmed bid and not the feed's 150. The confirmation is journaled
/// immediately after the intent, as rule 248 requires.
#[test]
fn a_confirmed_owner_exit_before_the_open_sells_in_extended_hours_from_the_confirmed_bid() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = common::ports(&ids, &mandates, &instruments, &config);
    let mut shell = held(&ports, PRE_MARKET - 100);
    shell.run(
        Input::Market(quote(AAPL, FEED_BID, "150.2", PRE_MARKET)),
        &ports,
    );
    let ran = shell.run(
        handed(
            exit(Purpose::OwnerExit, None),
            Some(confirmation("155", "150.35")),
        ),
        &ports,
    );
    let types = ran.draft_types();
    let at = types
        .iter()
        .position(|t| *t == "IntentReceived")
        .expect("the intent is received");
    assert_eq!(
        types.get(at + 1),
        Some(&"OwnerConfirmationReceived"),
        "immediately after its intent (rule 248): {types:?}"
    );
    assert_eq!(
        member(&ran, "OwnerConfirmationReceived", "expires_at").and_then(Value::as_int),
        u64::try_from(LATER).ok(),
        "the expiry, in risk-clock seconds"
    );
    for (name, value) in [
        ("intent_id", INTENT),
        ("instrument_id", AAPL),
        ("qty", "10"),
        ("bid", "155"),
        ("bid_size", "300"),
        ("floor", "150.35"),
        ("copied_from", REQUEST),
    ] {
        assert_eq!(
            member(&ran, "OwnerConfirmationReceived", name),
            Some(&text(value)),
            "{name}"
        );
    }
    let sent = only_submission(&ran);
    assert_eq!(
        (
            sent.order_type,
            sent.limit_price,
            sent.extended_hours,
            sent.qty
        ),
        (OrderType::Limit, Some(price("154.23")), true, qty("10"))
    );
}

/// A rung the floor clamps rests at the floor: 151 × 0.995 = 150.245 is below the floor of 150.8.
#[test]
fn the_floor_binds_a_confirmed_owner_exit_before_the_open() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = common::ports(&ids, &mandates, &instruments, &config);
    let mut shell = held(&ports, PRE_MARKET - 100);
    shell.run(
        Input::Market(quote(AAPL, FEED_BID, "150.2", PRE_MARKET)),
        &ports,
    );
    let sent = only_submission(&shell.run(
        handed(
            exit(Purpose::OwnerExit, None),
            Some(confirmation("151", "150.8")),
        ),
        &ports,
    ));
    let floor: Price = price("150.8");
    assert_eq!(
        (sent.limit_price, sent.extended_hours),
        (Some(floor), true),
        "never below the floor the owner confirmed (§5.5)"
    );
}

/// Without a confirmation the owner's exit waits for the regular session, as before (§5.5,
/// DEC-260 (13)): nothing goes extended-hours.
#[test]
fn an_unconfirmed_owner_exit_before_the_open_waits_for_the_session() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = common::ports(&ids, &mandates, &instruments, &config);
    let mut shell = held(&ports, PRE_MARKET - 100);
    shell.run(
        Input::Market(quote(AAPL, FEED_BID, "150.2", PRE_MARKET)),
        &ports,
    );
    let ran = shell.run(handed(exit(Purpose::OwnerExit, None), None), &ports);
    let sent = only_submission(&ran);
    assert!(
        !sent.extended_hours,
        "an unconfirmed owner exit never trades outside the session: {sent:?}"
    );
    assert!(ran.draft("OwnerConfirmationReceived").is_none());
}

/// In the regular session the confirmed floor binds too (DEC-2040 item 6, Proposed, its default):
/// on a fresh quote at 150 the exit is not sent at the market but as a limit, priced by the ladder
/// from the market (150 × 0.995 = 149.25) and clamped at the floor of 150.35.
#[test]
fn a_confirmed_owner_exit_in_the_regular_session_never_sells_below_its_floor() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = common::ports(&ids, &mandates, &instruments, &config);
    let mut shell = held(&ports, 10);
    shell.run(Input::Market(quote(AAPL, FEED_BID, "150.2", 20)), &ports);
    let ran = shell.run(
        handed(
            exit(Purpose::OwnerExit, None),
            Some(expiring("155", "150.35", 200)),
        ),
        &ports,
    );
    assert!(ran.draft("OwnerConfirmationReceived").is_some());
    let sent = only_submission(&ran);
    assert_eq!(
        (sent.order_type, sent.limit_price, sent.extended_hours),
        (OrderType::Limit, Some(price("150.35")), false),
        "never at the market, never below the floor"
    );
}

/// The fold reads the confirmation back: an exit held in the minute before the pre-market open,
/// across a restart, sells at the open from the confirmed bid, extended-hours, while its
/// confirmation is still inside its five minutes (DEC-2040 item 7).
#[test]
fn a_restart_keeps_the_confirmation_of_an_exit_held_overnight() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = common::ports(&ids, &mandates, &instruments, &config);
    let mut shell = held(&ports, OVERNIGHT - 100);
    shell.run(
        Input::Market(quote(AAPL, FEED_BID, "150.2", OVERNIGHT)),
        &ports,
    );
    let overnight = shell.run(
        handed(
            exit(Purpose::OwnerExit, None),
            Some(expiring("155", "150.35", OVERNIGHT + 180)),
        ),
        &ports,
    );
    assert!(
        overnight.submissions().is_empty(),
        "no session is open before 04:00"
    );
    let (mut restarted, _) = shell.restart(&ports);
    restarted.run(
        Input::Market(quote(AAPL, FEED_BID, "150.2", PRE_OPEN)),
        &ports,
    );
    let opened = restarted.run(Input::Tick(clock(PRE_OPEN + 1)), &ports);
    let sent = only_submission(&opened);
    assert_eq!(
        (sent.limit_price, sent.extended_hours),
        (Some(price("154.23")), true),
        "the confirmed bid and floor survive the restart"
    );
}

/// DEC-2040 item 7: a confirmation is judged against the core's journaled event time, and at
/// exactly `expires_at` it has expired. Handed at its expiry, the exit takes the unconfirmed path
/// and nothing goes extended-hours; handed one second before, it sells extended-hours from the
/// confirmed bid. Either way the confirmation is journaled as it was handed.
#[test]
fn a_confirmation_at_its_expiry_has_expired() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = common::ports(&ids, &mandates, &instruments, &config);
    for (expires_at, confirmed) in [(PRE_MARKET, false), (PRE_MARKET + 1, true)] {
        let mut shell = held(&ports, PRE_MARKET - 100);
        shell.run(Input::Tick(clock(PRE_MARKET)), &ports);
        shell.run(
            Input::Market(quote(AAPL, FEED_BID, "150.2", PRE_MARKET)),
            &ports,
        );
        let ran = shell.run(
            handed(
                exit(Purpose::OwnerExit, None),
                Some(expiring("155", "150.35", expires_at)),
            ),
            &ports,
        );
        assert!(
            ran.draft("OwnerConfirmationReceived").is_some(),
            "{expires_at}: journaled as handed"
        );
        let sent = only_submission(&ran);
        if confirmed {
            assert_eq!(
                (sent.order_type, sent.limit_price, sent.extended_hours),
                (OrderType::Limit, Some(price("154.23")), true),
                "one second before"
            );
        } else {
            assert_eq!(
                (sent.order_type, sent.extended_hours),
                (OrderType::Limit, false),
                "at its expiry, the unconfirmed path: {sent:?}"
            );
        }
    }
}

/// DEC-2040 item 7: expiry never cancels a working order. The first rung, sent extended-hours
/// before the confirmation expires, keeps working past the expiry; the ladder steps it only after
/// `exit_step_s` (5 s), as any rung, and the next rung, sent after the expiry, takes the
/// unconfirmed path: a regular-session limit, priced from the feed's last sane bid,
/// 150 × (1 − 0.01) = 148.50, and clamped at the floor of 150.35, which still binds.
#[test]
fn a_rung_after_the_confirmation_expires_takes_the_unconfirmed_path() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = common::ports(&ids, &mandates, &instruments, &config);
    let mut shell = held(&ports, PRE_MARKET - 100);
    shell.run(Input::Tick(clock(PRE_MARKET)), &ports);
    shell.run(
        Input::Market(quote(AAPL, FEED_BID, "150.2", PRE_MARKET)),
        &ports,
    );
    let first = only_submission(&shell.run(
        handed(
            exit(Purpose::OwnerExit, None),
            Some(expiring("155", "150.35", PRE_MARKET + 2)),
        ),
        &ports,
    ));
    assert!(first.extended_hours, "before the expiry");
    shell.run(
        Input::Broker(Ok(BrokerOutcome::Submitted(broker_order(
            "b-1",
            Some(first.client_order_id.as_str()),
            AAPL,
            Side::Sell,
            "10",
            "0",
            "new",
        )))),
        &ports,
    );
    let expired = shell.run(Input::Tick(clock(PRE_MARKET + 3)), &ports);
    assert!(
        expired.requests.is_empty(),
        "the expiry cancels nothing: {:?}",
        expired.requests
    );
    let due = shell.run(Input::Tick(clock(PRE_MARKET + 5)), &ports);
    assert!(
        due.requests
            .iter()
            .any(|request| matches!(request, BrokerRequest::Cancel { .. })),
        "the ladder steps the rung after exit_step_s: {:?}",
        due.requests
    );
    let next = only_submission(&shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: first.client_order_id.as_str().to_owned(),
        })),
        &ports,
    ));
    assert_eq!(
        (next.extended_hours, next.limit_price),
        (false, Some(price("150.35"))),
        "after the expiry: a regular-session limit, never below the floor"
    );
}

fn refused(input: Input) -> ExecutorError {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = common::ports(&ids, &mandates, &instruments, &config);
    let mut shell = held(&ports, PRE_MARKET - 100);
    let head = shell.head();
    let err = shell
        .step(input, &ports)
        .expect_err("a non-canonical handoff is refused");
    assert_eq!(shell.head(), head, "before anything is journaled");
    err
}

/// DEC-2040 item 1: a confirmation on any purpose but an owner exit, a bid size that is not
/// positive, or a limit below the floor is non-canonical, refused before anything is journaled.
#[test]
fn a_confirmation_that_does_not_fit_its_exit_is_non_canonical() {
    let field = |name: &str| ExecutorError::NonCanonicalPayload {
        field: name.to_owned(),
    };
    for purpose in [Purpose::RiskExit, Purpose::DiscretionaryExit] {
        assert_eq!(
            refused(handed(
                exit(purpose, None),
                Some(confirmation("155", "150.35"))
            )),
            field("confirmation"),
            "{purpose:?}"
        );
    }
    let mut empty = confirmation("155", "150.35");
    empty.bid_size = qty("0");
    assert_eq!(
        refused(handed(exit(Purpose::OwnerExit, None), Some(empty))),
        field("bid_size")
    );
    assert_eq!(
        refused(handed(
            exit(Purpose::OwnerExit, None),
            Some(expiring("155", "150.35", 0))
        )),
        field("expires_at"),
        "an expiry at the epoch confirms nothing"
    );
    assert_eq!(
        refused(handed(
            exit(Purpose::OwnerExit, Some("150.34")),
            Some(confirmation("155", "150.35"))
        )),
        field("limit_price"),
        "a limit one cent below the floor"
    );
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = common::ports(&ids, &mandates, &instruments, &config);
    let mut shell = held(&ports, PRE_MARKET - 100);
    assert!(
        shell
            .step(
                handed(
                    exit(Purpose::OwnerExit, Some("150.35")),
                    Some(confirmation("155", "150.35"))
                ),
                &ports
            )
            .is_ok(),
        "a limit at the floor fits"
    );
}
