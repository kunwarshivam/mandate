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
    handoff, ports, price, protected_opening, qty, quote, stream_opened,
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
    fill(&mut shell, &ports, "f-2", &id, "6");
    let ran = report(&mut shell, &ports, entry_state(&id, "10", "10", "filled"));
    let mut placed: Vec<SubmitOrder> = stops(&ran);
    placed.extend(stops(&shell.run(Input::Tick(clock(1)), &ports)));
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
    assert!(
        journaled_before_sent(&ran, stop.client_order_id.as_str()),
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

/// §5.4 and DEC-620 item 3: an entry still part filled at the timeout has its remainder
/// cancelled; once the cancel is confirmed the stop covers the 4 that filled, and no more.
#[test]
fn a_part_filled_entry_is_cancelled_at_the_timeout_and_its_fill_gets_the_stop() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = ready(&ports);
    let id = entry(&mut shell, &ports, "10", Some("170"))
        .client_order_id
        .as_str()
        .to_owned();
    fill(&mut shell, &ports, "f-1", &id, "4");
    report(
        &mut shell,
        &ports,
        entry_state(&id, "10", "4", "partially_filled"),
    );
    let early = shell.run(Input::Tick(clock(30)), &ports);
    assert!(
        !early
            .requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::Cancel { .. }))
    );
    let due = shell.run(Input::Tick(clock(61)), &ports);
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
    fill(&mut shell, &ports, "f-1", &id, "10");
    let ran = report(&mut shell, &ports, entry_state(&id, "10", "10", "filled"));
    let placed = stops(&ran);
    let [stop] = placed.as_slice() else {
        panic!("the stop is sent: {:?}", ran.requests);
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
        bound.notifications.contains(&"unprotected_interval_limit"),
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

/// One thing the broker or the clock does next, as the property draws it.
#[derive(Debug, Clone)]
enum Step {
    /// A fill of the entry, of this share of what is still unfilled, out of 4.
    EntryFill(u8),
    /// The entry ends: the broker reports it cancelled with what has filled.
    EntryEnds,
    /// The broker confirms a cancel the executor asked for.
    CancelConfirmed,
    /// The broker accepts every protective order it has not answered.
    StopsAccepted,
    /// The broker rejects every protective order it has not answered.
    StopsRejected,
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
        (1u8..=40).prop_map(Step::Wait),
    ]
}

/// The property's own record of the run: the position is the sum of the fills it reported, and
/// the sells are read off the requests the executor sent and the broker answers it gave.
#[derive(Default)]
struct Book {
    held: u64,
    filled_entry: u64,
    entry_done: bool,
    first_fill_at: Option<i64>,
    now: i64,
    cancels: Vec<String>,
    unanswered: Vec<(String, u64)>,
    resting: Vec<(String, u64)>,
    exits: u64,
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
        if ran.notifications.contains(&"unprotected_interval_limit") {
            self.alerted = true;
        }
    }

    fn covered(&self) -> u64 {
        self.resting.iter().map(|(_, shares)| *shares).sum()
    }

    fn open_sells(&self) -> u64 {
        self.covered()
            .saturating_add(self.unanswered.iter().map(|(_, shares)| *shares).sum())
            .saturating_add(self.exits)
    }

    /// Whether the exit sequence has asked the broker to cancel every stop it has not answered,
    /// its step before the exit is sent (§5.4's marketable exit sequence).
    fn cancelling_unanswered(&self) -> bool {
        !self.unanswered.is_empty()
            && self
                .unanswered
                .iter()
                .all(|(stop, _)| self.cancels.contains(stop))
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

    /// Over any sequence of entry fills, part fills, cancels, stop acceptances and rejects, and
    /// waits: the protective and exit sells the executor sent never exceed the position the test
    /// itself summed (rule 12, §5.3 rule 4); and once the position has been held past
    /// `max_unprotected_s` + one wait, it is either covered by stops the broker accepted, or the
    /// owner has been alerted and an exit of what is held has been sent, or, where a stop is still
    /// unanswered, its cancel asked as the exit's first step (§5.4's bound, rule 13).
    #[test]
    fn the_position_is_never_oversold_nor_unprotected_past_the_bound(
        steps in proptest::collection::vec(step(), 1..24),
    ) {
        let (ids, mandates, instruments, config) =
            (TestIds, FixedMandate::covering(&[AAPL]), FixedInstruments, config());
        let ports = ports(&ids, &mandates, &instruments, &config);
        let mut shell = ready(&ports);
        shell.run(Input::Market(quote(AAPL, "151", "151.1", 0)), &ports);
        let id = entry(&mut shell, &ports, "8", Some("170")).client_order_id.as_str().to_owned();
        let mut book = Book::default();
        let quantity = 8u64;
        for step in steps {
            let ran = match step {
                Step::EntryFill(share) if !book.entry_done && book.filled_entry < quantity => {
                    let left = quantity - book.filled_entry;
                    let shares = (left * u64::from(share)).div_ceil(4).max(1);
                    book.fills += 1;
                    book.filled_entry += shares;
                    book.held += shares;
                    book.first_fill_at.get_or_insert(book.now);
                    let fill_id = format!("f-{}", book.fills);
                    let mut ran = fill(&mut shell, &ports, &fill_id, &id, &shares.to_string());
                    let status = if book.filled_entry == quantity {
                        book.entry_done = true;
                        "filled"
                    } else {
                        "partially_filled"
                    };
                    book.sent(&ran);
                    ran = report(
                        &mut shell,
                        &ports,
                        entry_state(&id, "8", &book.filled_entry.to_string(), status),
                    );
                    ran
                }
                Step::EntryEnds if !book.entry_done => {
                    book.entry_done = true;
                    report(
                        &mut shell,
                        &ports,
                        entry_state(&id, "8", &book.filled_entry.to_string(), "canceled"),
                    )
                }
                Step::CancelConfirmed if book.cancels.contains(&id) && !book.entry_done => {
                    book.entry_done = true;
                    shell.run(
                        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
                            client_order_id: id.clone(),
                        })),
                        &ports,
                    )
                }
                Step::StopsAccepted if !book.unanswered.is_empty() => {
                    let mut last = Ran::default();
                    for (stop, shares) in std::mem::take(&mut book.unanswered) {
                        let order = broker_order(
                            "s-1", Some(&stop), AAPL, Side::Sell, &shares.to_string(), "0", "accepted",
                        );
                        last = report(&mut shell, &ports, order);
                        book.sent(&last);
                        book.resting.push((stop, shares));
                    }
                    last
                }
                Step::StopsRejected if !book.unanswered.is_empty() => {
                    let mut last = Ran::default();
                    for (stop, shares) in std::mem::take(&mut book.unanswered) {
                        let order = broker_order(
                            "s-1", Some(&stop), AAPL, Side::Sell, &shares.to_string(), "0", "rejected",
                        );
                        last = report(&mut shell, &ports, order);
                        book.sent(&last);
                    }
                    last
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
                "the stops sent ({}) exceed the {} held", book.open_sells(), book.held
            );
            if let (Some(first), Step::Wait(seconds)) = (book.first_fill_at, &step) {
                let bound = first + i64::try_from(config.max_unprotected_s).unwrap_or(i64::MAX);
                if book.now - i64::from(*seconds) > bound && book.held > 0 {
                    let protected = book.covered() >= book.held;
                    let flattening = book.alerted
                        && (book.exits >= book.held || book.cancelling_unanswered());
                    prop_assert!(
                        protected || flattening,
                        "held {} at {}s, first fill at {}s: covered {}, alerted {}, exits {}",
                        book.held, book.now, first, book.covered(), book.alerted, book.exits
                    );
                }
            }
        }
        let orders = shell.state.orders();
        let unknown = orders.values().any(|order| order.state == OrderState::Unknown);
        prop_assert!(!unknown, "nothing here leaves an order in doubt");
    }
}
