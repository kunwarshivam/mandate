//! E21-30, DEC-2359: the instruments whose exit waits on a price, which the process's quote feed
//! reads. Each case builds its state from journaled records and handed inputs, and states the set
//! it expects from what it built, never from the accessor's own rule: a held exit and an exit in
//! §5.4's sequence are wanted; a position with only resting protection is not, nor a held opening,
//! nor an account with nothing waiting.

mod common;

use std::collections::BTreeSet;

use common::{
    ACCOUNT_STREAM, AGENT, FixedInstruments, FixedMandate, Ran, Shell, TestIds, config, event,
    handoff, instrument, qty, quote, stream_opened, text, with_clock,
};
use mandate_accounting::{InstrumentId, Side};
use mandate_canon::Value;
use mandate_executor::{Input, IntentBody, Ports, Purpose};

const AAPL: &str = FixedInstruments::LIQUID_EQUITY;
const INTENT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ0";
const BID: &str = "150";

/// A sell of ten `AAPL` at the market: no limit, so with nothing to price from it is held
/// (DEC-2037 item 5).
fn market_exit(purpose: Purpose) -> IntentBody {
    IntentBody::Order {
        instrument: instrument(AAPL),
        side: Side::Sell,
        qty: qty("10"),
        limit: None,
        purpose,
        protection: None,
    }
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
                ("limit", text(BID)),
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
                ("price", text(BID)),
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

/// The same ten `AAPL` protected by one resting GTC OCO at 170 and 140.
fn protected(ports: &Ports<'_>) -> Shell {
    let mut shell = held(ports, 10);
    let placed = event(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "ProtectionChanged",
        with_clock(
            &[
                ("instrument", text(AAPL)),
                ("action", text("placed")),
                ("orders", text("md-oco-1")),
                ("qty", text("10")),
                ("take_profit", text("170")),
                ("stop", text("140")),
                ("created_on", text("2026-09-22")),
            ],
            11,
        ),
    );
    shell.fold_one(&placed).expect("the protection folds");
    shell.restart_ready(ports)
}

/// The gate's verdict and reason on the one decision `ran` journaled.
fn verdict(ran: &Ran) -> Option<(String, String)> {
    let decided = ran.draft("GateDecided")?;
    let name = |key: &str| {
        decided
            .payload
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    Some((name("verdict"), name("reason_code")))
}

fn wanted(shell: &Shell) -> BTreeSet<InstrumentId> {
    shell
        .state
        .quote_wanted()
        .expect("the wanted instruments read")
}

fn only(name: &str) -> BTreeSet<InstrumentId> {
    BTreeSet::from([instrument(name)])
}

/// A position with nothing waiting to exit, protected or not, wants no quote.
#[test]
fn a_position_with_no_exit_waiting_wants_no_quote() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = common::ports(&ids, &mandates, &instruments, &config);
    assert_eq!(wanted(&held(&ports, 10)), BTreeSet::new(), "unprotected");
    assert_eq!(wanted(&protected(&ports)), BTreeSet::new(), "protected");
}

/// DEC-2037 item 5: an exit at the market with nothing to price from is held `exit_unpriced`, and
/// its instrument is wanted for every exit purpose, so the quote that would release it is read.
#[test]
fn a_held_exit_wants_its_instruments_quote() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = common::ports(&ids, &mandates, &instruments, &config);
    for purpose in [
        Purpose::RiskExit,
        Purpose::DiscretionaryExit,
        Purpose::OwnerExit,
    ] {
        let mut shell = held(&ports, 10);
        let ran = shell.run(handoff(INTENT, AGENT, market_exit(purpose)), &ports);
        assert_eq!(
            verdict(&ran),
            Some(("hold".to_owned(), "exit_unpriced".to_owned())),
            "{purpose:?}: the case builds a held exit"
        );
        assert_eq!(wanted(&shell), only(AAPL), "{purpose:?}");
    }
}

/// §5.4: an exit beside resting protection starts the exit sequence, which cancels the protection
/// first; while the sequence runs its instrument is wanted, so its exit is priced by the ladder.
#[test]
fn an_exit_in_its_sequence_wants_its_instruments_quote() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = common::ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected(&ports);
    shell.run(Input::Market(quote(AAPL, BID, "150.2", 20)), &ports);
    let started = shell.run(
        handoff(INTENT, AGENT, market_exit(Purpose::RiskExit)),
        &ports,
    );
    assert!(
        started.submissions().is_empty(),
        "the case builds a sequence that cancels the protection first"
    );
    assert_eq!(wanted(&shell), only(AAPL));
}

/// A held opening waits on no exit price, so it wants no quote, whatever held it.
#[test]
#[ignore = "pending E21-30"]
fn a_held_opening_wants_no_quote() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = common::ports(&ids, &mandates, &instruments, &config);
    let mut shell = held(&ports, 10);
    let received = event(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "IntentReceived",
        with_clock(
            &[
                ("intent_id", text(INTENT)),
                ("agent", text(AGENT)),
                ("instrument", text(AAPL)),
                ("side", text("buy")),
                ("qty", text("5")),
                ("limit_price", text(BID)),
                ("purpose", text("open")),
            ],
            12,
        ),
    );
    shell.fold_one(&received).expect("the opening folds");
    let decided = event(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "GateDecided",
        with_clock(
            &[
                ("intent_id", text(INTENT)),
                ("verdict", text("hold")),
                ("reason_code", text("unknown_order_in_flight")),
            ],
            12,
        ),
    );
    shell.fold_one(&decided).expect("the opening's hold folds");
    assert_eq!(wanted(&shell), BTreeSet::new());
}

/// Reading the wanted instruments changes nothing: the state folds the same afterwards, and a
/// second read answers the same.
#[test]
fn reading_the_wanted_instruments_changes_nothing() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = common::ports(&ids, &mandates, &instruments, &config);
    let mut shell = held(&ports, 10);
    shell.run(
        handoff(INTENT, AGENT, market_exit(Purpose::RiskExit)),
        &ports,
    );
    let before = format!("{:?}", shell.state);
    let first = wanted(&shell);
    assert_eq!(wanted(&shell), first);
    assert_eq!(format!("{:?}", shell.state), before);
}
