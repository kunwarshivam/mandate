//! An equity opening on a profile that offers no bracket and no OCO carries its protection
//! (E7-23 B3, E7-4; [DEC-966](../../../docs/project/decisions/DEC-966.md); trading-domain spec
//! §5.2's Robinhood table and §5.4's "Equities on a profile with no OCO or bracket";
//! [DEC-529](../../../docs/project/decisions/DEC-529.md) item 7; `AGENTS.md` rules 3, 5, 12
//! and 13).
//!
//! The entry goes as a plain limit order. Once it has filled completely, or ends partly filled,
//! or its remainder is cancelled at `bracket_partial_fill_timeout_s`, one GTC stop-limit sell is
//! journaled and then sent for the filled quantity, at the intent's stop and stop × (1 −
//! `stop_limit_offset`); the take-profit is never a resting order beside it, since the two would
//! sell the position twice. If no stop rests by `max_unprotected_s`, the executor exits the held
//! quantity as its own `risk_exit` and alerts the owner.
//!
//! The oracles are written here from the spec, over orders and fills the test itself feeds: the
//! position is the test's own sum of the fills it reported, never the executor's.

mod common;

use common::{
    FixedInstruments, FixedMandate, Ran, Shell, TestIds, broker_fill, broker_order, clock, config,
    handoff, ports, price, protected_opening, qty, quote, risk_exit, stream_opened,
};
use mandate_accounting::Side;
use mandate_canon::Value;
use mandate_domain::{
    AssetClass, CapabilityProfile, Cell, Idempotency, MarketSession, OrderType as Kind,
    ProtectionForm as Form, QuantityForm as Qf, Retry, Row, TimeInForce as Tif,
};
use mandate_executor::{
    BrokerOutcome, BrokerRequest, BrokerUpdate, Effect, Input, OrderState, OrderType, Ports,
    Purpose, SubmitOrder, TimeInForce,
};
use mandate_num::Qty;
use proptest::prelude::*;

const AAPL: &str = FixedInstruments::LIQUID_EQUITY;
const INTENT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ0";

/// Trading spec §5.2's Robinhood table, written here as data: every equity cell `day` and `gtc`,
/// `market` in every quantity form, `limit`, `stop` and `stop_limit` in whole shares, and the
/// whole-share stop-limit the one protective form, with no bracket and no OCO.
fn no_bracket_profile() -> CapabilityProfile {
    let cell = |order_type, quantity_form, forms: &[Form]| Cell {
        order_type,
        quantity_form,
        times_in_force: [Tif::Day, Tif::Gtc].into(),
        protection_forms: forms.iter().copied().collect(),
    };
    let rows = vec![Row {
        asset_class: AssetClass::UsEquity,
        session: MarketSession::Regular,
        cells: vec![
            cell(Kind::Market, Qf::Whole, &[]),
            cell(Kind::Market, Qf::Fractional, &[]),
            cell(Kind::Market, Qf::Notional, &[]),
            cell(Kind::Limit, Qf::Whole, &[]),
            cell(Kind::Stop, Qf::Whole, &[]),
            cell(Kind::StopLimit, Qf::Whole, &[Form::StopLimit]),
        ],
    }];
    let idempotency = Idempotency {
        client_order_id: true,
        retry: Retry::Unknown,
        query_by_client_order_id: false,
    };
    CapabilityProfile::new(1, rows, idempotency).expect("a valid profile")
}

/// A ready executor on the no-bracket profile, as the shell starts one.
fn ready(ports: &Ports<'_>) -> Shell {
    let mut shell = Shell::new(1);
    shell.profile = Some(no_bracket_profile());
    shell.fold_one(&stream_opened()).expect("the stream opens");
    shell.restart_ready(ports)
}

/// The entry an opening of `quantity` AAPL at 150, stop 140 and take-profit `take_profit`, sends.
fn entry(
    shell: &mut Shell,
    ports: &Ports<'_>,
    quantity: &str,
    take_profit: Option<&str>,
) -> SubmitOrder {
    let ran = shell.run(
        handoff(
            INTENT,
            common::AGENT,
            protected_opening(AAPL, quantity, "150", "140", take_profit),
        ),
        ports,
    );
    let sent = ran.submissions();
    let [entry] = sent.as_slice() else {
        panic!("the opening sends one order: {:?}", ran.requests);
    };
    (*entry).clone()
}

fn report(shell: &mut Shell, ports: &Ports<'_>, order: mandate_executor::BrokerOrder) -> Ran {
    shell.run(Input::BrokerUpdate(BrokerUpdate::Order(order)), ports)
}

fn entry_state(
    id: &str,
    quantity: &str,
    filled: &str,
    status: &str,
) -> mandate_executor::BrokerOrder {
    broker_order("e-1", Some(id), AAPL, Side::Buy, quantity, filled, status)
}

fn fill(shell: &mut Shell, ports: &Ports<'_>, fill_id: &str, id: &str, quantity: &str) -> Ran {
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            fill_id,
            Some(id),
            quantity,
            "150",
        ))),
        ports,
    )
}

/// The protective stop-limits `ran` sent.
fn stops(ran: &Ran) -> Vec<SubmitOrder> {
    ran.submissions()
        .into_iter()
        .filter(|order| order.purpose == Purpose::Protective)
        .cloned()
        .collect()
}

/// Whether the `OrderSubmitted` naming `id` comes before the request that sends it (rule 5).
fn journaled_before_sent(ran: &Ran, id: &str) -> bool {
    let journaled = ran.effects.iter().position(|effect| {
        matches!(effect, Effect::Journal(draft) if draft.event_type == "OrderSubmitted"
            && matches!(draft.payload.get("client_order_id"), Some(Value::Str(named)) if named == id))
    });
    let sent = ran.effects.iter().position(|effect| {
        matches!(effect, Effect::Broker(BrokerRequest::Submit(order)) if order.client_order_id.as_str() == id)
    });
    matches!((journaled, sent), (Some(first), Some(then)) if first < then)
}

/// §5.4, DEC-529 item 7: on a profile with no bracket the entry goes plain, a limit order for the
/// day, whether or not the mandate sets a take-profit; never a bracket the broker cannot take,
/// and a stop with no take-profit is no longer refused.
#[test]
fn a_protected_entry_goes_plain_on_a_profile_with_no_bracket() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = ports(&ids, &mandates, &instruments, &config);
    for take_profit in [Some("170"), None] {
        let mut shell = ready(&ports);
        let sent = entry(&mut shell, &ports, "10", take_profit);
        assert_eq!(
            (
                sent.order_type,
                sent.tif,
                sent.bracket.is_some(),
                sent.oco.is_some(),
                sent.side
            ),
            (OrderType::Limit, TimeInForce::Day, false, false, Side::Buy),
            "take-profit {take_profit:?}: a plain day limit buy, no bracket and no OCO"
        );
        assert_eq!(sent.limit_price, Some(price("150")));
    }
}

/// §5.4: once the entry has filled completely, one GTC stop-limit sell covers the whole 10, its
/// stop the intent's 140 and its limit 140 × (1 − 0.005) = 139.3; the take-profit is not a
/// resting order. Its `OrderSubmitted` is journaled before it is sent (rule 5), and the placement
/// records the stop and no take-profit.
#[test]
fn a_complete_fill_places_one_gtc_stop_limit_journaled_before_it_is_sent() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = ready(&ports);
    let sent = entry(&mut shell, &ports, "10", Some("170"));
    let id = sent.client_order_id.as_str().to_owned();
    let partly = fill(&mut shell, &ports, "f-1", &id, "4");
    assert!(
        stops(&partly).is_empty(),
        "nothing is placed on a working entry"
    );
    let completing = fill(&mut shell, &ports, "f-2", &id, "6");
    let reported = report(&mut shell, &ports, entry_state(&id, "10", "10", "filled"));
    let ticked = shell.run(Input::Tick(clock(1)), &ports);
    let mut placed: Vec<SubmitOrder> = Vec::new();
    for ran in [&completing, &reported, &ticked] {
        placed.extend(stops(ran));
    }
    let [stop] = placed.as_slice() else {
        panic!("one protective order for the filled entry: {placed:?}");
    };
    assert_eq!(
        (stop.side, stop.order_type, stop.tif, stop.qty),
        (
            Side::Sell,
            OrderType::StopLimit,
            TimeInForce::Gtc,
            qty("10")
        ),
        "one GTC stop-limit sell for the whole position"
    );
    assert_eq!(
        (
            stop.stop_price,
            stop.limit_price,
            stop.oco.is_some(),
            stop.bracket.is_some()
        ),
        (Some(price("140")), Some(price("139.3")), false, false),
        "at the intent's stop, limit stop × (1 − offset), and no take-profit leg"
    );
    let ran = [&completing, &reported, &ticked]
        .into_iter()
        .find(|ran| !stops(ran).is_empty())
        .expect("one step sent the stop");
    assert!(
        journaled_before_sent(ran, stop.client_order_id.as_str()),
        "the stop's OrderSubmitted precedes its request (rule 5)"
    );
    let placement = ran
        .drafts
        .iter()
        .find(|draft| {
            draft.event_type == "ProtectionChanged"
                && matches!(draft.payload.get("action"), Some(Value::Str(action)) if action == "placed")
        })
        .expect("the placement is journaled");
    assert_eq!(placement.payload.get("take_profit"), Some(&Value::Null));
    assert_eq!(
        stop.client_order_id
            .protected_entry()
            .map(|e| e.as_str().to_owned()),
        Some(id),
        "named for the entry it protects (§2.3)"
    );
}

/// §5.4 and DEC-620 item 3: an entry still part filled at a 30-second
/// `bracket_partial_fill_timeout_s` has its remainder cancelled; once the cancel is confirmed,
/// inside the 60-second bound, the stop covers the 4 that filled, and no more.
#[test]
fn a_part_filled_entry_is_cancelled_at_the_timeout_and_its_fill_gets_the_stop() {
    let config = mandate_executor::ExecutorConfig {
        bracket_partial_fill_timeout_s: 30,
        ..config()
    };
    let (ids, mandates, instruments) = (TestIds, FixedMandate::covering(&[AAPL]), FixedInstruments);
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = ready(&ports);
    let id = entry(&mut shell, &ports, "10", Some("170"))
        .client_order_id
        .as_str()
        .to_owned();
    let filled = fill(&mut shell, &ports, "f-1", &id, "4");
    let reported = report(
        &mut shell,
        &ports,
        entry_state(&id, "10", "4", "partially_filled"),
    );
    let early = shell.run(Input::Tick(clock(20)), &ports);
    for ran in [&filled, &reported, &early] {
        assert!(
            !ran.requests
                .iter()
                .any(|r| matches!(r, BrokerRequest::Cancel { .. })),
            "nothing is cancelled before the timeout: {:?}",
            ran.requests
        );
    }
    let due = shell.run(Input::Tick(clock(31)), &ports);
    assert!(
        due.requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::Cancel { client_order_id } if client_order_id.as_str() == id)),
        "at the timeout the entry's remainder is cancelled: {:?}",
        due.requests
    );
    let after = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: id.clone(),
        })),
        &ports,
    );
    let placed = stops(&after);
    let [stop] = placed.as_slice() else {
        panic!(
            "one stop once the remainder's cancel is confirmed: {:?}",
            after.requests
        );
    };
    assert_eq!(
        (stop.order_type, stop.qty, stop.stop_price),
        (OrderType::StopLimit, qty("4"), Some(price("140"))),
        "the stop covers exactly what filled"
    );
}

/// §5.4's bound on a profile with no OCO: the stop is refused by the broker, so nothing rests;
/// at `max_unprotected_s` the owner is alerted and the executor exits the 10 held as its own
/// `risk_exit` (rule 13), never leaving the position unprotected past the bound.
#[test]
fn a_stop_that_never_rests_is_flattened_at_the_bound() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = ready(&ports);
    shell.run(Input::Market(quote(AAPL, "151", "151.1", 0)), &ports);
    let id = entry(&mut shell, &ports, "10", None)
        .client_order_id
        .as_str()
        .to_owned();
    let filled = fill(&mut shell, &ports, "f-1", &id, "10");
    let reported = report(&mut shell, &ports, entry_state(&id, "10", "10", "filled"));
    let mut placed = stops(&filled);
    placed.extend(stops(&reported));
    let [stop] = placed.as_slice() else {
        panic!("the stop is sent: {placed:?}");
    };
    report(
        &mut shell,
        &ports,
        broker_order(
            "s-1",
            Some(stop.client_order_id.as_str()),
            AAPL,
            Side::Sell,
            "10",
            "0",
            "rejected",
        ),
    );
    let inside = shell.run(Input::Tick(clock(30)), &ports);
    assert!(
        inside.submissions().is_empty(),
        "inside the bound nothing exits yet"
    );
    shell.run(Input::Market(quote(AAPL, "151", "151.1", 59)), &ports);
    let bound = shell.run(Input::Tick(clock(61)), &ports);
    assert!(
        bound.notifications.contains(&"protection"),
        "the owner is alerted at the bound: {:?}",
        bound.notifications
    );
    let exits: Vec<&SubmitOrder> = bound
        .submissions()
        .into_iter()
        .filter(|order| order.side == Side::Sell && order.purpose == Purpose::RiskExit)
        .collect();
    let [exit] = exits.as_slice() else {
        panic!(
            "one risk exit of the held quantity at the bound: {:?}",
            bound.requests
        );
    };
    assert_eq!(exit.qty, qty("10"), "the whole position is exited");
}

/// Whether `ran` handed the executor its own `risk_exit` (`w-<record>`) of `quantity`.
fn flattens(ran: &Ran, quantity: &str) -> bool {
    ran.drafts.iter().any(|draft| {
        draft.event_type == "IntentReceived"
            && matches!(draft.payload.get("intent_id"), Some(Value::Str(intent)) if intent.starts_with("w-"))
            && matches!(draft.payload.get("purpose"), Some(Value::Str(purpose)) if purpose == "risk_exit")
            && matches!(draft.payload.get("qty"), Some(Value::Str(shares)) if qty(shares) == qty(quantity))
    })
}

/// DEC-966 item 4: an entry still part filled at the bound is alerted and its remainder cancelled,
/// with nothing exited yet; when the cancel is confirmed after the bound, the 4 it filled are
/// exited whole as the executor's own `risk_exit`, and no stop is placed for them.
#[test]
fn an_entry_ending_after_its_bound_is_exited_whole_with_no_stop() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = ready(&ports);
    shell.run(Input::Market(quote(AAPL, "151", "151.1", 0)), &ports);
    let id = entry(&mut shell, &ports, "10", None)
        .client_order_id
        .as_str()
        .to_owned();
    fill(&mut shell, &ports, "f-1", &id, "4");
    report(
        &mut shell,
        &ports,
        entry_state(&id, "10", "4", "partially_filled"),
    );
    shell.run(Input::Market(quote(AAPL, "151", "151.1", 59)), &ports);
    let bound = shell.run(Input::Tick(clock(61)), &ports);
    assert!(bound.notifications.contains(&"protection"));
    assert!(
        bound
            .requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::Cancel { client_order_id } if client_order_id.as_str() == id)),
        "the working entry is cancelled at the bound: {:?}",
        bound.requests
    );
    assert!(
        !flattens(&bound, "4"),
        "nothing is exited while the entry may still fill"
    );
    let ended = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: id.clone(),
        })),
        &ports,
    );
    assert!(
        stops(&ended).is_empty(),
        "past its bound, no stop is placed"
    );
    assert!(
        flattens(&ended, "4"),
        "the 4 it filled are exited whole: {:?}",
        ended.draft_types()
    );
}

/// The review's case (DEC-966 item 7): entry A's stop is rejected, then entry B fills and its stop
/// is accepted. B's start never ends A's interval, so at A's bound the owner is alerted and the
/// whole position is exited, B's resting stop cancelled first (§5.4's marketable sequence).
#[test]
fn a_second_entry_never_ends_the_interval_of_the_first() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = ready(&ports);
    shell.run(Input::Market(quote(AAPL, "151", "151.1", 0)), &ports);
    let first = entry(&mut shell, &ports, "5", None)
        .client_order_id
        .as_str()
        .to_owned();
    let filled = fill(&mut shell, &ports, "f-1", &first, "5");
    let [first_stop] = stops(&filled).try_into().expect("the first entry's stop");
    report(
        &mut shell,
        &ports,
        broker_order(
            "s-1",
            Some(first_stop.client_order_id.as_str()),
            AAPL,
            Side::Sell,
            "5",
            "0",
            "rejected",
        ),
    );
    let second = shell.run(
        handoff(
            "01JABCDEFGHJKMNPQRSTVWXYZ1",
            common::AGENT,
            protected_opening(AAPL, "5", "150", "140", None),
        ),
        &ports,
    );
    let second = second
        .submissions()
        .first()
        .map(|order| order.client_order_id.as_str().to_owned())
        .expect("the second entry is sent");
    let filled = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-2",
            Some(&second),
            "5",
            "150",
        ))),
        &ports,
    );
    let [second_stop] = stops(&filled).try_into().expect("the second entry's stop");
    report(
        &mut shell,
        &ports,
        broker_order(
            "s-2",
            Some(second_stop.client_order_id.as_str()),
            AAPL,
            Side::Sell,
            "5",
            "0",
            "accepted",
        ),
    );
    shell.run(Input::Market(quote(AAPL, "151", "151.1", 59)), &ports);
    let bound = shell.run(Input::Tick(clock(61)), &ports);
    assert!(
        bound.notifications.contains(&"protection"),
        "the first entry's interval reaches its bound: {:?}",
        bound.notifications
    );
    assert!(
        flattens(&bound, "10"),
        "the whole position is exited: {:?}",
        bound.draft_types()
    );
    assert!(
        bound
            .requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::Cancel { client_order_id }
            if client_order_id == &second_stop.client_order_id)),
        "the resting stop is cancelled before the exit sells: {:?}",
        bound.requests
    );
}

/// A first entry of 5 filled, its stop then answered `first` (or left unanswered with `None`),
/// an agent's risk exit of 1 that cancels that stop, then a second entry of 5 that fills and ends
/// while the stop's cancel is unanswered: the second entry's id and what its fill and its end
/// sent.
fn a_second_fill_beside_a_stop_being_cancelled(first: Option<&str>) -> (String, [Ran; 2]) {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = ready(&ports);
    shell.run(Input::Market(quote(AAPL, "151", "151.1", 0)), &ports);
    let entry_id = entry(&mut shell, &ports, "5", None)
        .client_order_id
        .as_str()
        .to_owned();
    let filled = fill(&mut shell, &ports, "f-1", &entry_id, "5");
    let [first_stop] = stops(&filled).try_into().expect("the first entry's stop");
    if let Some(status) = first {
        report(
            &mut shell,
            &ports,
            broker_order(
                "s-1",
                Some(first_stop.client_order_id.as_str()),
                AAPL,
                Side::Sell,
                "5",
                "0",
                status,
            ),
        );
    }
    let exiting = shell.run(
        handoff(
            "01JABCDEFGHJKMNPQRSTVWXYZ8",
            common::AGENT,
            risk_exit(AAPL, "1", "150"),
        ),
        &ports,
    );
    assert!(
        exiting
            .requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::Cancel { client_order_id }
            if client_order_id == &first_stop.client_order_id)),
        "the exit asks the first stop's cancel: {:?}",
        exiting.requests
    );
    let second = shell
        .run(
            handoff(
                "01JABCDEFGHJKMNPQRSTVWXYZ1",
                common::AGENT,
                protected_opening(AAPL, "5", "150", "140", None),
            ),
            &ports,
        )
        .submissions()
        .first()
        .map(|order| order.client_order_id.as_str().to_owned())
        .expect("the second entry is sent");
    let filled = fill(&mut shell, &ports, "f-2", &second, "5");
    let ended = report(&mut shell, &ports, entry_state(&second, "5", "5", "filled"));
    (second, [filled, ended])
}

/// Whether any of `ran` sent a stop named for `entry` (§2.3's `{entry}-p…`).
fn stops_for(ran: &[Ran], entry: &str) -> bool {
    let named = format!("{entry}-p");
    ran.iter()
        .flat_map(stops)
        .any(|stop| stop.client_order_id.as_str().starts_with(&named))
}

/// The independent review of #2395, S5: a fill gets its own stop even while another protective
/// order's cancel, asked before the broker acknowledged it, is unanswered. Holding the stop back
/// there would leave the fill to the bound's exit (DEC-966 items 2 and 4).
#[test]
fn a_fill_beside_an_unanswered_stop_being_cancelled_gets_its_own_stop() {
    let (second, ran) = a_second_fill_beside_a_stop_being_cancelled(None);
    assert!(
        stops_for(&ran, &second),
        "{:?}",
        ran.map(|ran| ran.requests)
    );
}

/// The independent review of #2395, S6: the same with the stop being cancelled one the broker
/// accepted.
#[test]
fn a_fill_beside_an_accepted_stop_being_cancelled_gets_its_own_stop() {
    let (second, ran) = a_second_fill_beside_a_stop_being_cancelled(Some("accepted"));
    assert!(
        stops_for(&ran, &second),
        "{:?}",
        ran.map(|ran| ran.requests)
    );
}

/// The review's round-2 case (DEC-966 item 7): two entries fill and both stops are sent before
/// the broker accepts either; once both are accepted the whole position rests on its own stops,
/// so the bound alerts nothing and exits nothing.
#[test]
fn two_entries_whose_stops_are_both_accepted_are_never_flattened() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = ready(&ports);
    shell.run(Input::Market(quote(AAPL, "151", "151.1", 0)), &ports);
    let first = entry(&mut shell, &ports, "5", None)
        .client_order_id
        .as_str()
        .to_owned();
    let filled = fill(&mut shell, &ports, "f-1", &first, "5");
    let [first_stop] = stops(&filled).try_into().expect("the first entry's stop");
    let second = shell
        .run(
            handoff(
                "01JABCDEFGHJKMNPQRSTVWXYZ1",
                common::AGENT,
                protected_opening(AAPL, "5", "150", "140", None),
            ),
            &ports,
        )
        .submissions()
        .first()
        .map(|order| order.client_order_id.as_str().to_owned())
        .expect("the second entry is sent");
    let filled = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-2",
            Some(&second),
            "5",
            "150",
        ))),
        &ports,
    );
    let [second_stop] = stops(&filled).try_into().expect("the second entry's stop");
    for (broker_id, stop) in [("s-1", &first_stop), ("s-2", &second_stop)] {
        report(
            &mut shell,
            &ports,
            broker_order(
                broker_id,
                Some(stop.client_order_id.as_str()),
                AAPL,
                Side::Sell,
                "5",
                "0",
                "accepted",
            ),
        );
    }
    shell.run(Input::Market(quote(AAPL, "151", "151.1", 59)), &ports);
    for at in [61, 121, 181] {
        let ran = shell.run(Input::Tick(clock(at)), &ports);
        assert!(
            ran.notifications.is_empty(),
            "at {at}s: {:?}",
            ran.notifications
        );
        assert!(ran.requests.is_empty(), "at {at}s: {:?}", ran.requests);
    }
}

/// LT-14: on Alpaca's profile a part-filled bracket's interval reaching the bound is alerted as
/// before and never flattened; the bound cancels nothing of its own.
#[test]
fn a_bracket_at_the_bound_is_alerted_and_never_flattened() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("the stream opens");
    let mut shell = shell.restart_ready(&ports);
    shell.run(Input::Market(quote(AAPL, "151", "151.1", 0)), &ports);
    let sent = entry(&mut shell, &ports, "10", Some("170"));
    assert!(
        sent.bracket.is_some(),
        "Alpaca's profile offers the bracket"
    );
    let id = sent.client_order_id.as_str().to_owned();
    fill(&mut shell, &ports, "f-1", &id, "4");
    let ended = report(&mut shell, &ports, entry_state(&id, "10", "4", "canceled"));
    assert!(ended.submissions().iter().any(|order| order.oco.is_some()));
    shell.run(Input::Market(quote(AAPL, "151", "151.1", 59)), &ports);
    let bound = shell.run(Input::Tick(clock(61)), &ports);
    assert!(bound.notifications.contains(&"protection"));
    assert!(
        !flattens(&bound, "4"),
        "a bracket is never flattened at the bound"
    );
    assert!(
        !bound.draft_types().contains(&"IntentReceived"),
        "{:?}",
        bound.draft_types()
    );
}

/// One thing the broker or the clock does next, as the property draws it.
#[derive(Debug, Clone)]
enum Step {
    /// A fill of the working entry, of this share of what is still unfilled, out of 4.
    EntryFill(u8),
    /// The working entry ends: the broker reports it cancelled with what has filled.
    EntryEnds,
    /// The broker confirms a cancel of the working entry the executor asked for.
    CancelConfirmed,
    /// The broker accepts every protective order it has not answered.
    StopsAccepted,
    /// The broker rejects every protective order it has not answered.
    StopsRejected,
    /// The broker confirms the cancel of every stop it accepted that the executor asked to
    /// cancel, so the wait that cancel held ends here.
    StopCancelConfirmed,
    /// Another protected opening of 4, once the working entry has ended (at most three in all).
    Add,
    /// The clock moves on this many seconds.
    Wait(u8),
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        (1u8..=4).prop_map(Step::EntryFill),
        Just(Step::EntryEnds),
        Just(Step::CancelConfirmed),
        Just(Step::StopsAccepted),
        Just(Step::StopsRejected),
        Just(Step::StopCancelConfirmed),
        Just(Step::Add),
        (1u8..=40).prop_map(Step::Wait),
    ]
}

/// The intents the property's openings carry, in order.
const INTENTS: [&str; 3] = [
    INTENT,
    "01JABCDEFGHJKMNPQRSTVWXYZ1",
    "01JABCDEFGHJKMNPQRSTVWXYZ2",
];

/// One opening as the property records it: its order, its quantity, what has filled, whether it
/// has ended, and when it first filled.
struct Opening {
    id: String,
    quantity: u64,
    filled: u64,
    done: bool,
    first_fill_at: Option<i64>,
}

/// The property's own record of the run: the position is the sum of the fills it reported, and
/// the sells are read off the requests the executor sent and the broker answers it gave.
#[derive(Default)]
struct Book {
    openings: Vec<Opening>,
    held: u64,
    now: i64,
    cancels: Vec<String>,
    unanswered: Vec<(String, u64)>,
    resting: Vec<(String, u64)>,
    exits: u64,
    flattened: u64,
    alerted: bool,
    fills: u32,
}

impl Book {
    fn sent(&mut self, ran: &Ran) {
        for request in &ran.requests {
            match request {
                BrokerRequest::Submit(order) if order.purpose == Purpose::Protective => {
                    let shares = whole(order.qty);
                    self.unanswered
                        .push((order.client_order_id.as_str().to_owned(), shares));
                }
                BrokerRequest::Submit(order) if order.side == Side::Sell => {
                    self.exits = self.exits.saturating_add(whole(order.qty));
                }
                BrokerRequest::Cancel { client_order_id } => {
                    self.cancels.push(client_order_id.as_str().to_owned());
                }
                _ => {}
            }
        }
        if ran.notifications.contains(&"protection") {
            self.alerted = true;
        }
        for draft in &ran.drafts {
            let flatten = draft.event_type == "IntentReceived"
                && matches!(draft.payload.get("intent_id"), Some(Value::Str(intent)) if intent.starts_with("w-"))
                && matches!(draft.payload.get("purpose"), Some(Value::Str(purpose)) if purpose == "risk_exit");
            if let (true, Some(Value::Str(shares))) = (flatten, draft.payload.get("qty")) {
                self.flattened = self.flattened.saturating_add(whole(qty(shares)));
            }
        }
    }

    fn covered(&self) -> u64 {
        self.resting.iter().map(|(_, shares)| *shares).sum()
    }

    /// What the stops the broker accepted for `opening` itself cover: each is named for the entry
    /// it protects (§2.3).
    fn covered_for(&self, opening: &Opening) -> u64 {
        let named = format!("{}-p", opening.id);
        self.resting
            .iter()
            .filter(|(stop, _)| stop.starts_with(&named))
            .map(|(_, shares)| *shares)
            .sum()
    }

    fn open_sells(&self) -> u64 {
        self.covered()
            .saturating_add(self.unanswered.iter().map(|(_, shares)| *shares).sum())
            .saturating_add(self.exits)
    }

    /// Whether every opening that filled anything is covered by its own accepted stops, with
    /// nothing unanswered: a position no bound can be due on.
    fn wholly_covered(&self) -> bool {
        self.unanswered.is_empty()
            && self
                .openings
                .iter()
                .all(|opening| self.covered_for(opening) >= opening.filled)
    }

    /// Whether the executor has asked the broker to cancel an order the broker has not answered:
    /// an opening still working, a stop never acknowledged, or a stop the broker accepted whose
    /// cancel no `StopCancelConfirmed` step has confirmed yet. An exit waits on that cancel (§5.3
    /// rule 5, §5.4's marketable sequence), which is the broker's hold `AGENTS.md` rule 13 allows;
    /// once the broker confirms it, the wait ends and the bound holds the executor to it.
    fn waiting_on_broker(&self) -> bool {
        let working = self
            .openings
            .iter()
            .filter(|opening| !opening.done)
            .map(|opening| opening.id.as_str());
        let unanswered = self.unanswered.iter().map(|(stop, _)| stop.as_str());
        let resting = self.resting.iter().map(|(stop, _)| stop.as_str());
        working
            .chain(unanswered)
            .chain(resting)
            .any(|id| self.cancels.iter().any(|asked| asked == id))
    }

    fn working(&mut self) -> Option<&mut Opening> {
        self.openings.last_mut().filter(|opening| !opening.done)
    }
}

fn whole(quantity: Qty) -> u64 {
    let text = quantity.to_string();
    let integral = text.split('.').next().unwrap_or_default();
    assert_eq!(
        qty(integral),
        quantity,
        "a protective or exit sell is in whole shares"
    );
    integral.parse().expect("whole shares")
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 96,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    /// Over any sequence of up to three openings, their fills, part fills, ends and cancels, stop
    /// acceptances, rejects and confirmed cancels, and waits:
    /// - the protective and exit sells the executor sent never exceed the position the test
    ///   itself summed (rule 12, §5.3 rule 4);
    /// - once an opening's shares have been held past `max_unprotected_s` + one wait, they are
    ///   covered by the stops the broker accepted for that opening, or the owner has been alerted
    ///   and the executor has handed itself a `risk_exit` of at least those shares, or is waiting
    ///   on a cancel the broker has not answered (§5.4's bound, rule 13's broker hold);
    /// - a position every opening of which is covered by its own accepted stops is never
    ///   flattened (DEC-966 item 7).
    ///
    /// What an exit's own sequence does next is §5.4's, which the executor's other suites pin.
    #[test]
    fn the_position_is_never_oversold_nor_unprotected_past_the_bound(
        steps in proptest::collection::vec(step(), 1..32),
    ) {
        holds(steps)?;
    }
}

/// A case a random seed drew in CI (#2370's mutants baseline): the bound's exit for the first
/// opening cancels the second opening's stop of 3 in the step the broker then accepts it, so the
/// exit waits on that cancel, which no step confirms, while the bound passes for the second
/// opening's 4 shares. That wait is the broker's hold (rule 13), not an unprotected position.
#[test]
fn an_accepted_stop_whose_cancel_is_unanswered_is_the_brokers_hold() {
    let steps = vec![
        Step::EntryFill(1),
        Step::Wait(25),
        Step::EntryEnds,
        Step::Wait(35),
        Step::Add,
        Step::EntryFill(2),
        Step::Wait(21),
        Step::EntryFill(1),
        Step::EntryFill(1),
        Step::StopsAccepted,
        Step::Wait(40),
        Step::Wait(1),
    ];
    assert!(holds(steps).is_ok());
}

/// The property's run over `steps` (see the property's doc).
fn holds(steps: Vec<Step>) -> Result<(), TestCaseError> {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = ready(&ports);
    shell.run(Input::Market(quote(AAPL, "151", "151.1", 0)), &ports);
    let mut book = Book::default();
    let first = entry(&mut shell, &ports, "4", Some("170"))
        .client_order_id
        .as_str()
        .to_owned();
    report(
        &mut shell,
        &ports,
        entry_state(&first, "4", "0", "accepted"),
    );
    book.openings.push(Opening {
        id: first,
        quantity: 4,
        filled: 0,
        done: false,
        first_fill_at: None,
    });
    for step in steps {
        let covered_before = book.wholly_covered();
        let flattened_before = book.flattened;
        let now = book.now;
        let ran = match step {
            Step::EntryFill(share) => {
                let Some(opening) = book
                    .working()
                    .filter(|opening| opening.filled < opening.quantity)
                else {
                    continue;
                };
                let left = opening.quantity - opening.filled;
                let shares = (left * u64::from(share)).div_ceil(4).max(1);
                opening.filled += shares;
                opening.first_fill_at.get_or_insert(now);
                if opening.filled == opening.quantity {
                    opening.done = true;
                }
                let (id, filled, done) = (opening.id.clone(), opening.filled, opening.done);
                book.held += shares;
                book.fills += 1;
                let fill_id = format!("f-{}", book.fills);
                let filling = fill(&mut shell, &ports, &fill_id, &id, &shares.to_string());
                book.sent(&filling);
                let status = if done { "filled" } else { "partially_filled" };
                report(
                    &mut shell,
                    &ports,
                    entry_state(&id, "4", &filled.to_string(), status),
                )
            }
            Step::EntryEnds => {
                let Some(opening) = book.working() else {
                    continue;
                };
                opening.done = true;
                let (id, filled) = (opening.id.clone(), opening.filled);
                report(
                    &mut shell,
                    &ports,
                    entry_state(&id, "4", &filled.to_string(), "canceled"),
                )
            }
            Step::CancelConfirmed => {
                let cancels = book.cancels.clone();
                let Some(opening) = book
                    .working()
                    .filter(|opening| cancels.contains(&opening.id))
                else {
                    continue;
                };
                opening.done = true;
                let id = opening.id.clone();
                shell.run(
                    Input::Broker(Ok(BrokerOutcome::CancelAccepted {
                        client_order_id: id,
                    })),
                    &ports,
                )
            }
            Step::StopsAccepted | Step::StopsRejected if !book.unanswered.is_empty() => {
                let accepted = matches!(step, Step::StopsAccepted);
                for (stop, shares) in std::mem::take(&mut book.unanswered) {
                    let status = if accepted { "accepted" } else { "rejected" };
                    let order = broker_order(
                        "s-1",
                        Some(&stop),
                        AAPL,
                        Side::Sell,
                        &shares.to_string(),
                        "0",
                        status,
                    );
                    let answered = report(&mut shell, &ports, order);
                    book.sent(&answered);
                    if accepted {
                        book.resting.push((stop, shares));
                    }
                }
                Ran::default()
            }
            Step::StopCancelConfirmed => {
                let (asked, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut book.resting)
                    .into_iter()
                    .partition(|(stop, _)| book.cancels.contains(stop));
                book.resting = kept;
                if asked.is_empty() {
                    continue;
                }
                for (stop, _) in asked {
                    let confirmed = shell.run(
                        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
                            client_order_id: stop,
                        })),
                        &ports,
                    );
                    book.sent(&confirmed);
                }
                Ran::default()
            }
            Step::Add => {
                let next = book.openings.len();
                let (Some(intent), true) = (INTENTS.get(next), book.working().is_none()) else {
                    continue;
                };
                let ran = shell.run(
                    handoff(
                        intent,
                        common::AGENT,
                        protected_opening(AAPL, "4", "150", "140", Some("170")),
                    ),
                    &ports,
                );
                let sent = ran
                    .submissions()
                    .into_iter()
                    .find(|order| order.side == Side::Buy)
                    .map(|order| order.client_order_id.as_str().to_owned());
                if let Some(id) = sent {
                    book.sent(&ran);
                    let acknowledged =
                        report(&mut shell, &ports, entry_state(&id, "4", "0", "accepted"));
                    book.openings.push(Opening {
                        id,
                        quantity: 4,
                        filled: 0,
                        done: false,
                        first_fill_at: None,
                    });
                    acknowledged
                } else {
                    ran
                }
            }
            Step::Wait(seconds) => {
                book.now += i64::from(seconds);
                shell.run(Input::Tick(clock(book.now)), &ports)
            }
            _ => continue,
        };
        book.sent(&ran);
        prop_assert!(
            book.open_sells() <= book.held,
            "the sells sent ({}) exceed the {} held",
            book.open_sells(),
            book.held
        );
        prop_assert!(
            !(covered_before && book.flattened > flattened_before),
            "a position wholly covered by its own accepted stops is flattened at {}s",
            book.now
        );
        let Step::Wait(seconds) = step else {
            continue;
        };
        let checked = book.now - i64::from(seconds);
        for opening in &book.openings {
            let Some(first) = opening.first_fill_at else {
                continue;
            };
            if checked <= first + config.max_unprotected_s || opening.filled == 0 {
                continue;
            }
            let protected = book.covered_for(opening) >= opening.filled;
            let cancelling = book.waiting_on_broker();
            let flattening = book.alerted && (book.flattened >= opening.filled || cancelling);
            prop_assert!(
                protected || flattening,
                "{} held {} at {}s, first fill at {}s: covered {}, alerted {}, flattened {}",
                opening.id,
                opening.filled,
                book.now,
                first,
                book.covered_for(opening),
                book.alerted,
                book.flattened
            );
        }
    }
    let orders = shell.state.orders();
    let unknown = orders
        .values()
        .any(|order| order.state == OrderState::Unknown);
    prop_assert!(!unknown, "nothing here leaves an order in doubt");
    Ok(())
}

/// A sequence the property found: the first opening's flatten waits on a stop the broker never
/// answers, while a second opening fills and reaches its own bound.
#[test]
fn a_second_opening_beside_a_waiting_flatten_reaches_its_bound() -> Result<(), TestCaseError> {
    holds(vec![
        Step::EntryFill(1),
        Step::Wait(12),
        Step::Wait(13),
        Step::EntryEnds,
        Step::Wait(35),
        Step::Add,
        Step::EntryFill(4),
        Step::Wait(26),
        Step::Wait(35),
        Step::Wait(1),
    ])
}
