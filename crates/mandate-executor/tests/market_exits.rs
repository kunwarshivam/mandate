//! E7-29, DEC-2037: an exit handed at the market, with no limit. The core receives it as a market
//! order, sends it at the market only in the regular session on a fresh, sane quote with nothing
//! resting (trading spec §5.1), prices it by the exit ladder everywhere else (§5.6), holds it
//! `exit_unpriced` when there is nothing to price from (DEC-2037 item 5), and never refuses it
//! (`AGENTS.md` rule 13). An opening or an increase with no limit is non-canonical (rule 12).
//!
//! Every expected price is the ladder's first rung computed here from the quote: the liquid tier's
//! `exit_offset` of 0.5% off the reference bid (§5.6's table, `FixedInstruments`).

mod common;

use common::{
    ACCOUNT_STREAM, AGENT, ALLOWING_BINDING_GATE, BINDING_EXITS, FixedInstruments, FixedMandate,
    MISSING_BINDING_GATE, Ran, Shell, TestIds, clock, config, event, handoff, instrument, price,
    qty, quote, stale_quote, stream_opened, text, with_clock,
};
use mandate_accounting::Side;
use mandate_canon::Value;
use mandate_executor::{
    BindingGateSource, BrokerOutcome, BrokerUnknown, ExecutorError, Input, IntentBody, OrderType,
    Ports, Purpose, SubmitOrder,
};
use mandate_num::Price;

const AAPL: &str = FixedInstruments::LIQUID_EQUITY;
const INTENT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ0";
const BID: &str = "150";
/// 2026-09-22 08:00 ET, pre-market on the committed calendar; clock-0 instants read as the
/// regular session (DEC-260 (13), DEC-506).
const PRE_MARKET: i64 = 1_790_078_400;

/// The ladder's first rung for a sell against a whole-dollar `bid` in the liquid tier, computed in
/// hundredths of a dollar: bid × (1 − 0.005), which the tier's tick needs no rounding for.
fn first_rung(bid: &str) -> Price {
    let cents = bid
        .parse::<u64>()
        .map(|dollars| dollars * 100)
        .unwrap_or_default();
    let rung = cents * 995 / 1000;
    price(&format!("{}.{:02}", rung / 100, rung % 100))
}

/// A sell of ten `AAPL` at the market: no limit (DEC-2037 item 1).
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

fn member<'a>(ran: &'a Ran, event_type: &str, name: &str) -> Option<&'a Value> {
    ran.draft(event_type)
        .and_then(|draft| draft.payload.get(name))
}

fn only_submission(ran: &Ran) -> SubmitOrder {
    match ran.submissions().as_slice() {
        [one] => (*one).clone(),
        other => panic!("one submission, not {other:?}"),
    }
}

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

#[test]
fn an_exit_at_the_market_is_received_as_a_market_order_with_no_limit() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = common::ports(&ids, &mandates, &instruments, &config);
    let mut shell = held(&ports, 10);
    shell.run(Input::Market(quote(AAPL, BID, "150.2", 20)), &ports);
    let ran = shell.run(
        handoff(INTENT, AGENT, market_exit(Purpose::RiskExit)),
        &ports,
    );
    assert_eq!(
        member(&ran, "IntentReceived", "type"),
        Some(&text("market")),
        "received as proposed (journal spec §9.5, DEC-2037 item 2)"
    );
    assert_eq!(
        member(&ran, "IntentReceived", "limit_price"),
        Some(&Value::Null)
    );
}

/// §5.1: in the regular session, outside the close window, on a fresh, sane quote, with nothing
/// resting, an exit at the market whose gate decision read a binding snapshot goes at the market
/// (DEC-2037 item 4).
#[test]
fn an_exit_at_the_market_goes_at_the_market_on_a_fresh_quote_in_the_regular_session() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = common::ports(&ids, &mandates, &instruments, &config);
    for purpose in [Purpose::RiskExit, Purpose::OwnerExit] {
        let mut shell = held(&ports, 10);
        shell.run(Input::Market(quote(AAPL, BID, "150.2", 20)), &ports);
        let ran = shell.run_with_binding(
            handoff(INTENT, AGENT, market_exit(purpose)),
            &ports,
            &BINDING_EXITS,
        );
        let sent = only_submission(&ran);
        assert_eq!(
            (sent.order_type, sent.limit_price, sent.qty),
            (OrderType::Market, None, qty("10")),
            "{purpose:?}"
        );
        assert_eq!(
            member(&ran, "OrderSubmitted", "type"),
            Some(&text("market"))
        );
        assert_eq!(
            member(&ran, "OrderSubmitted", "limit_price"),
            Some(&Value::Null),
            "{purpose:?}: journaled before it is sent, as sent (rule 5)"
        );
    }
}

/// §5.1, DEC-2037 items 3 and 4: where the gate's decision on an exit read no binding snapshot (no
/// source, or one that answers nothing for it), nothing checked a market order's participation
/// caps, marketable-limit rule or collar, so even on a fresh regular quote the exit goes as a limit
/// at the ladder's first rung, never at the market.
#[test]
fn an_exit_at_the_market_whose_gate_read_no_snapshot_goes_by_the_ladder() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = common::ports(&ids, &mandates, &instruments, &config);
    let sources: [(&str, &dyn BindingGateSource); 2] = [
        ("no source", &MISSING_BINDING_GATE),
        ("no answer for an exit", &ALLOWING_BINDING_GATE),
    ];
    for purpose in [Purpose::RiskExit, Purpose::OwnerExit] {
        for (name, source) in sources {
            let mut shell = held(&ports, 10);
            shell.run(Input::Market(quote(AAPL, BID, "150.2", 20)), &ports);
            let ran = shell.run_with_binding(
                handoff(INTENT, AGENT, market_exit(purpose)),
                &ports,
                source,
            );
            let sent = only_submission(&ran);
            assert_eq!(
                (sent.order_type, sent.limit_price),
                (OrderType::Limit, Some(first_rung(BID))),
                "{purpose:?}, {name}"
            );
            assert_eq!(
                member(&ran, "OrderRequestRecorded", "rung").and_then(Value::as_int),
                Some(0),
                "{purpose:?}, {name}: the first rung of a ladder that steps, not a limit left alone"
            );
        }
    }
}

/// Trading spec §5.7, DEC-2037 items 3 and 4: an exit sent at the market and lost is resent under
/// its own id once its absence is confirmed. If the re-check reads no binding snapshot, nothing
/// checked a market order's caps, so it is resent as a limit at the ladder's first rung; with one,
/// at the market as before.
#[test]
fn a_lost_exit_at_the_market_is_resent_at_the_market_only_on_a_binding_recheck() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = common::ports(&ids, &mandates, &instruments, &config);
    let sources: [(&str, &dyn BindingGateSource, OrderType, Option<Price>); 2] = [
        ("binding", &BINDING_EXITS, OrderType::Market, None),
        (
            "unbound",
            &MISSING_BINDING_GATE,
            OrderType::Limit,
            Some(first_rung(BID)),
        ),
    ];
    for (name, source, resent_as, resent_at) in sources {
        let mut shell = held(&ports, 10);
        shell.run(Input::Market(quote(AAPL, BID, "150.2", 20)), &ports);
        let first = shell.run_with_binding(
            handoff(INTENT, AGENT, market_exit(Purpose::RiskExit)),
            &ports,
            &BINDING_EXITS,
        );
        let sent = only_submission(&first);
        assert_eq!(sent.order_type, OrderType::Market, "{name}");
        let id = sent.client_order_id.as_str().to_owned();
        let absent = || {
            Input::Broker(Ok(BrokerOutcome::Absent {
                client_order_id: id.clone(),
            }))
        };
        shell.run_with_binding(Input::Broker(Err(BrokerUnknown::Timeout)), &ports, source);
        shell.run_with_binding(absent(), &ports, source);
        shell.run_with_binding(Input::Market(quote(AAPL, BID, "150.2", 22)), &ports, source);
        shell.run_with_binding(Input::Tick(clock(28)), &ports, source);
        shell.run_with_binding(absent(), &ports, source);
        shell.run_with_binding(Input::Market(quote(AAPL, BID, "150.2", 36)), &ports, source);
        shell.run_with_binding(Input::Tick(clock(36)), &ports, source);
        let resent = shell.run_with_binding(absent(), &ports, source);
        let again = only_submission(&resent);
        assert_eq!(
            (
                again.client_order_id.as_str(),
                again.order_type,
                again.limit_price
            ),
            (id.as_str(), resent_as, resent_at),
            "{name}"
        );
    }
}

/// §5.6: with the latest quote not sane (a presumed halt), a market order is barred and the exit
/// goes as a limit at the ladder's first rung, priced from the last sane bid.
#[test]
fn an_exit_at_the_market_on_a_stale_quote_is_priced_by_the_ladder() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = common::ports(&ids, &mandates, &instruments, &config);
    for purpose in [Purpose::RiskExit, Purpose::OwnerExit] {
        let mut shell = held(&ports, 10);
        shell.run(Input::Market(quote(AAPL, BID, "150.2", 20)), &ports);
        shell.run(Input::Market(stale_quote(AAPL, BID, 25)), &ports);
        let sent =
            only_submission(&shell.run(handoff(INTENT, AGENT, market_exit(purpose)), &ports));
        assert_eq!(
            (sent.order_type, sent.limit_price),
            (OrderType::Limit, Some(first_rung(BID))),
            "{purpose:?}"
        );
    }
}

/// §5.1, §5.6: outside the regular session a market order is barred, and a risk exit goes in the
/// extended session as a limit at the ladder's first rung.
#[test]
fn a_risk_exit_at_the_market_before_the_open_is_priced_by_the_ladder() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = common::ports(&ids, &mandates, &instruments, &config);
    let mut shell = held(&ports, PRE_MARKET - 100);
    shell.run(Input::Market(quote(AAPL, BID, "150.2", PRE_MARKET)), &ports);
    let sent = only_submission(&shell.run(
        handoff(INTENT, AGENT, market_exit(Purpose::RiskExit)),
        &ports,
    ));
    assert_eq!(
        (sent.order_type, sent.limit_price, sent.extended_hours),
        (OrderType::Limit, Some(first_rung(BID)), true)
    );
}

/// §5.4, §5.6: with protection resting, the exit's sequence cancels it first, and the exit then
/// goes by the ladder, never at the market (DEC-2037 item 4).
#[test]
fn an_exit_at_the_market_beside_resting_protection_goes_by_the_ladder() {
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
        "the protection is cancelled first"
    );
    let sent = only_submission(&shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    ));
    assert_eq!(
        (sent.order_type, sent.limit_price),
        (OrderType::Limit, Some(first_rung(BID)))
    );
}

/// DEC-2037 item 5: with nothing to price from, an exit at the market is held `exit_unpriced`,
/// journaled and alerted once, never refused; a tick re-checks it silently, and the first tick
/// after a fresh quote releases it.
#[test]
fn an_exit_at_the_market_with_nothing_to_price_from_is_held_then_goes() {
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
        let held = shell.run(handoff(INTENT, AGENT, market_exit(purpose)), &ports);
        assert!(
            held.submissions().is_empty(),
            "{purpose:?}: nothing is sent"
        );
        assert_eq!(
            verdict(&held),
            Some(("hold".to_owned(), "exit_unpriced".to_owned())),
            "{purpose:?}"
        );
        assert_eq!(held.notifications, ["exit_stalled"], "{purpose:?}");
        let quiet = shell.run(Input::Tick(clock(15)), &ports);
        assert!(
            quiet.is_empty(),
            "{purpose:?}: journaled once: {:?}",
            quiet.effects
        );
        shell.run(Input::Market(quote(AAPL, BID, "150.2", 20)), &ports);
        let released = shell.run(Input::Tick(clock(21)), &ports);
        assert_eq!(
            released.submissions().len(),
            1,
            "{purpose:?}: released by the first priceable quote: {:?}",
            released.effects
        );
    }
}

/// Journal spec §9.5: an exit received at the market folds back as one, so a restart keeps
/// holding it and a later quote still releases it at the market.
#[test]
fn an_exit_received_at_the_market_survives_a_restart() {
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
    let (mut restarted, _) = shell.restart(&ports);
    restarted.run(Input::Market(quote(AAPL, BID, "150.2", 20)), &ports);
    let released = restarted.run_with_binding(Input::Tick(clock(21)), &ports, &BINDING_EXITS);
    let sent = only_submission(&released);
    assert_eq!(
        (sent.order_type, sent.limit_price),
        (OrderType::Market, None)
    );
}

/// Rule 12, DEC-2037 item 1: an opening or an increase with no limit is non-canonical, refused
/// before anything is journaled or sent.
#[test]
fn an_opening_or_an_increase_with_no_limit_is_refused_before_anything_is_journaled() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = common::ports(&ids, &mandates, &instruments, &config);
    for purpose in [Purpose::Open, Purpose::Increase] {
        let mut shell = held(&ports, 10);
        shell.run(Input::Market(quote(AAPL, BID, "150.2", 20)), &ports);
        let head = shell.head();
        let mut buy = market_exit(purpose);
        if let IntentBody::Order { side, .. } = &mut buy {
            *side = Side::Buy;
        }
        let refused = shell.step(handoff(INTENT, AGENT, buy), &ports);
        assert!(
            matches!(
                &refused,
                Err(ExecutorError::NonCanonicalPayload { field }) if field == "limit_price"
            ),
            "{purpose:?}: {refused:?}"
        );
        assert_eq!(shell.head(), head, "{purpose:?}: nothing journaled or sent");
    }
}

/// Rule 13 over every exit purpose, market situation and gate source: an exit at the market is never
/// refused and never denied; it is sent, or held for a reason rule 13 allows (nothing to price
/// from, or a discretionary exit waiting for the session); and it goes at the market only in the
/// regular session on a fresh, sane quote with a binding gate decision, never as a limit with no
/// price.
#[test]
fn no_exit_at_the_market_is_refused_and_only_a_fresh_regular_quote_sends_it_at_the_market() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = common::ports(&ids, &mandates, &instruments, &config);
    let situations = ["fresh", "stale", "none", "pre_market"];
    let sources: [(&str, &dyn BindingGateSource); 2] = [
        ("binding", &BINDING_EXITS),
        ("unbound", &MISSING_BINDING_GATE),
    ];
    let allowed_holds = ["exit_unpriced", "session_closed", "session_unknown"];
    let cases = [
        Purpose::RiskExit,
        Purpose::DiscretionaryExit,
        Purpose::OwnerExit,
    ]
    .into_iter()
    .flat_map(|purpose| situations.map(|situation| (purpose, situation)))
    .flat_map(|(purpose, situation)| sources.map(|source| (purpose, situation, source)));
    for (purpose, situation, (name, source)) in cases {
        let start = if situation == "pre_market" {
            PRE_MARKET
        } else {
            20
        };
        let mut shell = held(&ports, start - 10);
        match situation {
            "fresh" | "pre_market" => {
                shell.run(Input::Market(quote(AAPL, BID, "150.2", start)), &ports);
            }
            "stale" => {
                shell.run(Input::Market(quote(AAPL, BID, "150.2", start)), &ports);
                shell.run(Input::Market(stale_quote(AAPL, BID, start + 5)), &ports);
            }
            _ => {}
        }
        let at = format!("{purpose:?} {situation} {name}");
        let ran = shell
            .step_with_binding(handoff(INTENT, AGENT, market_exit(purpose)), &ports, source)
            .unwrap_or_else(|e| panic!("{at}: refused with {}: {e}", e.code()));
        let decided = verdict(&ran).unwrap_or_else(|| panic!("{at}: no gate decision"));
        assert_ne!(decided.0, "deny", "{at}: denied {decided:?}");
        match ran.submissions().as_slice() {
            [] => {
                assert_eq!(decided.0, "hold", "{at}: neither sent nor held");
                assert!(
                    allowed_holds.contains(&decided.1.as_str()),
                    "{at}: held for {:?}, which rule 13 does not allow",
                    decided.1
                );
            }
            [sent] => {
                let market = sent.order_type == OrderType::Market;
                assert_eq!(market, sent.limit_price.is_none(), "{at}: {sent:?}");
                assert!(
                    !market || situation == "fresh" && name == "binding",
                    "{at}: at the market outside a fresh regular quote and a binding gate"
                );
            }
            more => panic!("{at}: {more:?}"),
        }
    }
}
