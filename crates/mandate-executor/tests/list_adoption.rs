//! An `Unknown` order on a profile with no query by client order id (E7-23 B2b; DEC-529 item 4,
//! DEC-862; trading-domain spec §5.3 rule 9, §5.7; connections spec §6.2; first-live-trade brief
//! LT-2, LT-6; `AGENTS.md` rules 3 and 13).
//!
//! The oracle is written here: a record matches only when its instrument, side, quantity, limit
//! price, order type and time in force are the order's as sent, and it carries no client order id
//! or ours. Exactly one match is adopted; zero or several leave the order `Unknown`, its
//! instrument blocked, with nothing sent, cancelled or filled. Part 1 pins the listing and the
//! cases that adopt nothing; part 2 pins the adoptions and DEC-863's exclusion of a record that
//! looks like another journaled order of ours.

mod common;

use common::{
    AGENT, FixedInstruments, FixedMandate, OTHER_AGENT, Ran, Shell, TestIds, broker_fill,
    broker_order, config, handoff, key, opening, ports, price, qty, risk_exit, snapshot,
    stream_opened, text,
};
use mandate_accounting::Side;
use mandate_canon::Value;
use mandate_domain::{
    AssetClass, CapabilityProfile, Cell, Idempotency, MarketSession, OrderType as Kind,
    ProtectionForm, QuantityForm, Retry, Row, TimeInForce as Tif,
};
use mandate_executor::{
    BrokerOutcome, BrokerRequest, BrokerUnknown, BrokerUpdate, ClientOrderId, ExecutorConfig,
    FoldedEvent, Input, ListedOrder, OrderListing, OrderOrigin, OrderState, OrderType, Ports,
    ReconcileReason, RiskClock, TimeInForce,
};
use mandate_num::{Price, Qty};
use mandate_time::UtcNanos;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use std::cell::Cell as Tally;

const AAPL: &str = "AAPL";
const OTHER: &str = "FRAC";
/// DEC-862 item 2: the listing starts this long before the order's `OrderSubmitted`.
const MARGIN_S: i64 = 300;
const INTENT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ0";
const AGAIN: &str = "01JABCDEFGHJKMNPQRSTVWXYZ2";
const STALE: &str = "stale-1";
const OTHER_INTENT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ1";

type Matched = (Side, Qty, Option<Price>, OrderType, TimeInForce);

/// The members DEC-529 item 4 matches on, as the order is sent: 10 AAPL bought at 150, day.
fn matched() -> Matched {
    let day = TimeInForce::Day;
    (
        Side::Buy,
        qty("10"),
        Some(price("150")),
        OrderType::Limit,
        day,
    )
}

struct Fixture(TestIds, FixedMandate, FixedInstruments, ExecutorConfig);

impl Fixture {
    fn new() -> Self {
        let universe = FixedMandate::covering(&[AAPL, OTHER]);
        Self(TestIds, universe, FixedInstruments, config())
    }

    fn ports(&self) -> Ports<'_> {
        ports(&self.0, &self.1, &self.2, &self.3)
    }
}

fn profile(query_by_client_order_id: bool) -> CapabilityProfile {
    let cell = |order_type, protection_forms: &[ProtectionForm]| Cell {
        order_type,
        quantity_form: QuantityForm::Whole,
        times_in_force: [Tif::Day, Tif::Gtc].into_iter().collect(),
        protection_forms: protection_forms.iter().copied().collect(),
    };
    let row = Row {
        asset_class: AssetClass::UsEquity,
        session: MarketSession::Regular,
        cells: vec![
            cell(Kind::Limit, &[]),
            cell(Kind::StopLimit, &[ProtectionForm::StopLimit]),
        ],
    };
    let (client_order_id, retry) = (true, Retry::Unknown);
    let idempotency = Idempotency {
        client_order_id,
        retry,
        query_by_client_order_id,
    };
    CapabilityProfile::new(1, vec![row], idempotency).expect("a valid profile")
}

/// What the account stream holds before the order whose answer is lost.
#[derive(Clone, Copy, PartialEq)]
enum Before {
    Nothing,
    /// `OTHER_AGENT`'s accepted order in another instrument.
    OtherAgent,
    /// `AGENT`'s own earlier order with the same matched members, accepted as broker order
    /// `STALE` and then cancelled.
    Lookalike,
}

/// A started shell on `profile` holding `before`, then `AGENT`'s opening of 10 AAPL at 150 sent
/// and its answer lost.
fn lost(ports: &Ports<'_>, query: bool, before: Before) -> (Shell, ClientOrderId, Ran) {
    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(ports);
    shell.state = shell.state.clone().with_profile(profile(query));
    if before == Before::Lookalike {
        let first = shell.run(
            handoff(OTHER_INTENT, AGENT, opening(AAPL, "10", "150")),
            ports,
        );
        let sent = first.submissions()[0].client_order_id.as_str().to_owned();
        for status in ["accepted", "canceled"] {
            let mut answer = exact(STALE, status).order;
            answer.client_order_id = Some(sent.clone());
            shell.run(Input::Broker(Ok(BrokerOutcome::Order(answer))), ports);
        }
        let ended = shell
            .state
            .order(&ClientOrderId::parse(&sent).expect("ours"));
        assert_eq!(
            ended.map(|o| o.state),
            Some(OrderState::Canceled),
            "the lookalike ended"
        );
    }
    if before == Before::OtherAgent {
        let other = shell.run(
            handoff(OTHER_INTENT, OTHER_AGENT, opening(OTHER, "1", "20")),
            ports,
        );
        let sent = other.submissions()[0].client_order_id.as_str().to_owned();
        let mut answer = broker_order("b-1", Some(&sent), OTHER, Side::Buy, "1", "0", "accepted");
        answer.limit_price = Some(price("20"));
        shell.run(Input::Broker(Ok(BrokerOutcome::Submitted(answer))), ports);
    }
    let sent = shell.run(handoff(INTENT, AGENT, opening(AAPL, "10", "150")), ports);
    let o = sent.submissions()[0].clone();
    assert_eq!(
        (o.side, o.qty, o.limit_price, o.order_type, o.tif),
        matched(),
        "as sent"
    );
    let lost = shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), ports);
    (shell, o.client_order_id, lost)
}

/// The `risk_clock` the journal stamped on `id`'s `OrderSubmitted`, read from the journal.
fn submitted_at(shell: &Shell, id: &ClientOrderId) -> RiskClock {
    let payload = &shell
        .account_journal
        .iter()
        .find(|event| {
            event.event_type == "OrderSubmitted"
                && event.payload.get("client_order_id") == Some(&Value::Str(id.as_str().into()))
        })
        .expect("the order's OrderSubmitted is journaled")
        .payload;
    let Some(Value::Str(stamp)) = payload.get("risk_clock") else {
        panic!("the batch stamps a §4.7 risk_clock (DEC-306 item 2)");
    };
    RiskClock::from_secs(UtcNanos::parse(stamp).expect("a timestamp").secs())
}

/// The record that matches the order as sent, with `status` and no client order id.
fn exact(broker_id: &str, status: &str) -> ListedOrder {
    let mut order = broker_order(broker_id, None, AAPL, Side::Buy, "10", "0", status);
    order.limit_price = Some(price("150"));
    ListedOrder {
        order,
        order_type: OrderType::Limit,
        tif: TimeInForce::Day,
    }
}

/// One record per matched member, each differing from the order in that member alone.
fn near_misses() -> Vec<ListedOrder> {
    let miss = |n: usize| {
        let mut record = exact(&format!("miss-{n}"), "accepted");
        match n {
            0 => record.order.qty = qty("9"),
            1 => record.order.limit_price = Some(price("149")),
            2 => record.order.limit_price = None,
            3 => record.order.side = Side::Sell,
            4 => record.order_type = OrderType::StopLimit,
            5 => record.tif = TimeInForce::Gtc,
            6 => record.order.instrument = common::instrument(OTHER),
            _ => record.order.client_order_id = Some("md-someone-else".to_owned()),
        }
        record
    };
    (0..8).map(miss).collect()
}

fn listed(id: &ClientOrderId, orders: Vec<ListedOrder>) -> Input {
    Input::Broker(Ok(BrokerOutcome::Listed {
        client_order_id: id.as_str().to_owned(),
        orders,
    }))
}

/// Nothing in `ran` reaches the broker: no submit, cancel, query, or second listing (rule 13,
/// LT-6, DEC-862 item 5).
fn sends_nothing(ran: &Ran, what: &str) {
    let sent = &ran.requests;
    assert!(
        sent.is_empty(),
        "{what}: nothing reaches the broker: {sent:?}"
    );
}

fn unknown(shell: &Shell, id: &ClientOrderId, why: &str) {
    assert_state(shell, id, OrderState::Unknown, why);
}

fn assert_state(shell: &Shell, id: &ClientOrderId, expected: OrderState, why: &str) {
    let order = shell.state.order(id).expect("the order is in the fold");
    let got = (order.state, order.filled_qty);
    assert_eq!(got, (expected, Qty::ZERO), "{why}; no invented fill");
}

/// Stays `Unknown` after `answer`, and a second opening in the instrument is not sent.
fn stays_unknown_and_blocks(
    shell: &mut Shell,
    id: &ClientOrderId,
    ports: &Ports<'_>,
    answer: Vec<ListedOrder>,
) {
    let ran = shell.run(listed(id, answer), ports);
    sends_nothing(&ran, "an unadopted listing");
    unknown(shell, id, "DEC-529 item 4");
    let sent = submitted_at(shell, id).secs();
    for after in [30, 3600] {
        let ticked = shell.run(Input::Tick(RiskClock::from_secs(sent + after)), ports);
        sends_nothing(&ticked, "a later tick");
    }
    let again = shell.run(handoff(AGAIN, AGENT, opening(AAPL, "1", "150")), ports);
    assert!(
        again.submissions().is_empty(),
        "§5.3 rule 9: its instrument is blocked"
    );
    unknown(shell, id, "only the owner ends it");
}

#[test]
fn a_lost_answer_lists_by_instrument_origin_and_creation_time() {
    let fixture = Fixture::new();
    let ports = fixture.ports();
    let (shell, id, lost) = lost(&ports, false, Before::Nothing);
    let since = RiskClock::from_secs(submitted_at(&shell, &id).secs() - MARGIN_S);
    let listing = OrderListing {
        client_order_id: id.clone(),
        instrument: common::instrument(AAPL),
        origin: OrderOrigin::Agentic,
        created_since: since,
    };
    assert_eq!(
        lost.requests,
        vec![BrokerRequest::ListOrders(listing)],
        "one listing in place of a query by client order id, and never a re-send (LT-6)"
    );
    unknown(&shell, &id, "§5.7");
}

#[test]
fn no_matching_record_leaves_the_order_unknown_and_its_instrument_blocked() {
    let fixture = Fixture::new();
    let ports = fixture.ports();
    let (mut shell, id, _) = lost(&ports, false, Before::Nothing);
    let empty = shell.run(listed(&id, Vec::new()), &ports);
    sends_nothing(&empty, "an empty listing");
    unknown(&shell, &id, "zero records");
    stays_unknown_and_blocks(&mut shell, &id, &ports, near_misses());
}

#[test]
fn an_account_another_agent_trades_on_is_never_listed() {
    let fixture = Fixture::new();
    let ports = fixture.ports();
    let (_, _, alone) = lost(&ports, false, Before::Nothing);
    let listings = alone
        .requests
        .iter()
        .filter(|r| matches!(r, BrokerRequest::ListOrders(_)));
    assert_eq!(
        listings.count(),
        1,
        "the control: a dedicated account lists"
    );
    let (mut shell, id, lost) = lost(&ports, false, Before::OtherAgent);
    assert!(
        lost.requests.is_empty(),
        "DEC-529 item 4: only on an account dedicated to one agent, got {:?}",
        lost.requests
    );
    unknown(&shell, &id, "never listed");
    let ran = shell.run(listed(&id, vec![exact("the-one", "accepted")]), &ports);
    sends_nothing(&ran, "an unasked listing");
    unknown(&shell, &id, "an unasked listing");
}

#[test]
fn a_profile_that_queries_by_client_order_id_never_lists() {
    let fixture = Fixture::new();
    let ports = fixture.ports();
    let (shell, id, lost) = lost(&ports, true, Before::Nothing);
    assert_eq!(
        lost.requests,
        vec![BrokerRequest::GetOrderByClientId(id.clone())],
        "§5.2: reconciliation queries by client order id when the profile can"
    );
    unknown(&shell, &id, "§5.7");
}

#[test]
fn exactly_one_matching_record_is_adopted() {
    let fixture = Fixture::new();
    let ports = fixture.ports();
    let (mut shell, id, _) = lost(&ports, false, Before::Nothing);
    let mut records = near_misses();
    records.insert(3, exact("the-one", "accepted"));
    let ran = shell.run(listed(&id, records), &ports);
    sends_nothing(&ran, "an adoption");
    assert_state(
        &shell,
        &id,
        OrderState::Accepted,
        "§5.7: Unknown → Accepted, found",
    );
    let adopted = &ran.draft("OrderStateChanged").expect("journaled").payload;
    let named = [adopted.get("client_order_id"), adopted.get("state")];
    let expected = [id.as_str(), "accepted"].map(|v| Value::Str(v.into()));
    assert_eq!(named, [Some(&expected[0]), Some(&expected[1])], "§5.7");
}

#[test]
fn two_matching_records_leave_the_order_unknown_and_its_instrument_blocked() {
    let fixture = Fixture::new();
    let ports = fixture.ports();
    let (mut shell, id, _) = lost(&ports, false, Before::Nothing);
    let answer = vec![exact("first", "accepted"), exact("second", "filled")];
    stays_unknown_and_blocks(&mut shell, &id, &ports, answer);
}

/// DEC-863: the broker still lists our cancelled order `STALE`, which carries no client order id
/// and every matched member of the lost one. It cannot be told apart, so nothing is adopted.
#[test]
fn a_stale_lookalike_of_a_journaled_order_is_never_adopted() {
    let fixture = Fixture::new();
    let ports = fixture.ports();
    let (mut shell, id, lost) = lost(&ports, false, Before::Lookalike);
    let listings = lost
        .requests
        .iter()
        .filter(|r| matches!(r, BrokerRequest::ListOrders(_)));
    assert_eq!(
        listings.count(),
        1,
        "the account is still dedicated to one agent"
    );
    stays_unknown_and_blocks(&mut shell, &id, &ports, vec![exact(STALE, "canceled")]);
}

/// Over generated listings of exact records (with no client order id or ours) and near misses in
/// any order, the order is adopted exactly when one record is exact; the oracle counts what it
/// generated, never the matcher's verdict.
#[test]
fn over_generated_listings_exactly_one_exact_record_is_adopted() {
    let fixture = Fixture::new();
    let ports = fixture.ports();
    let mut runner = TestRunner::new(Config {
        cases: 64,
        failure_persistence: None,
        ..Config::default()
    });
    let record = prop_oneof![(0..8_usize).prop_map(Some), Just(None)];
    let picks = prop::collection::vec((record, any::<bool>()), 0..5);
    let counts = Tally::new([0_u32; 3]);
    let outcome = runner.run(&picks, |picks| {
        let (mut shell, id, _) = lost(&ports, false, Before::Nothing);
        let misses = near_misses();
        let records: Vec<ListedOrder> = picks
            .iter()
            .enumerate()
            .map(|(n, (pick, carries_ours))| match pick {
                Some(miss) => misses[*miss].clone(),
                None => {
                    let mut record = exact(&format!("exact-{n}"), "accepted");
                    record.order.client_order_id = carries_ours.then(|| id.as_str().to_owned());
                    record
                }
            })
            .collect();
        let exact_count = picks.iter().filter(|(pick, _)| pick.is_none()).count();
        let ran = shell
            .step(listed(&id, records), &ports)
            .map_err(|e| TestCaseError::fail(format!("step refused with {}: {e}", e.code())))?;
        prop_assert!(ran.requests.is_empty(), "nothing sent: {:?}", ran.requests);
        let expected = if exact_count == 1 {
            OrderState::Accepted
        } else {
            OrderState::Unknown
        };
        let order = shell.state.order(&id).map(|o| (o.state, o.filled_qty));
        prop_assert_eq!(order, Some((expected, Qty::ZERO)));
        let mut seen = counts.get();
        seen[exact_count.min(2)] = seen[exact_count.min(2)].saturating_add(1);
        counts.set(seen);
        Ok(())
    });
    if let Err(failure) = outcome {
        panic!("{failure}");
    }
    assert!(
        counts.get().iter().all(|n| *n > 0),
        "zero, one and several exact records each reached"
    );
}

/// A dedicated no-query shell whose journal holds one earlier order of `AGENT`'s, 10 AAPL at 150
/// day, cancelled (or, with `replaced`, replaced by a linked order the fold holds no request
/// for, which is then cancelled), edited by `edit` and folded again; then the order whose answer
/// is lost, as [`lost`] sends it.
fn after_prior(
    ports: &Ports<'_>,
    replaced: bool,
    edit: impl Fn(&mut FoldedEvent),
) -> (Shell, ClientOrderId, Ran) {
    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(ports);
    let first = shell.run(
        handoff(OTHER_INTENT, AGENT, opening(AAPL, "10", "150")),
        ports,
    );
    let prior = first.submissions()[0].client_order_id.clone();
    let mut answer = exact(STALE, "accepted").order;
    answer.client_order_id = Some(prior.as_str().to_owned());
    shell.run(
        Input::Broker(Ok(BrokerOutcome::Order(answer.clone()))),
        ports,
    );
    let mut last = prior.clone();
    if replaced {
        answer.status = "replaced".to_owned();
        answer.replaced_by_broker_order_id = Some("linked-1".to_owned());
        shell.run(
            Input::Broker(Ok(BrokerOutcome::Order(answer.clone()))),
            ports,
        );
        last = shell
            .state
            .order(&prior)
            .and_then(|o| o.replaced_by.clone())
            .expect("linked");
        answer.broker_order_id = "linked-1".to_owned();
    }
    answer.client_order_id = Some(last.as_str().to_owned());
    answer.status = "canceled".to_owned();
    shell.run(Input::Broker(Ok(BrokerOutcome::Order(answer))), ports);
    let ended = shell.state.order(&last).map(|o| o.state);
    assert_eq!(ended, Some(OrderState::Canceled), "the earlier order ended");
    shell.account_journal.iter_mut().for_each(edit);
    let mut shell = shell.restart_ready(ports);
    shell.state = shell.state.clone().with_profile(profile(false));
    let sent = shell.run(handoff(INTENT, AGENT, opening(AAPL, "10", "150")), ports);
    let id = sent.submissions()[0].client_order_id.clone();
    let lost = shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), ports);
    (shell, id, lost)
}

/// An edit setting `field` to `value` in every `event_type` the journal holds so far.
fn set(
    event_type: &'static str,
    field: &'static str,
    value: &'static str,
) -> impl Fn(&mut FoldedEvent) {
    move |event| {
        if let (true, Value::Object(members)) = (event.event_type == event_type, &mut event.payload)
        {
            members.insert(key(field), text(value));
        }
    }
}

fn listings(ran: &Ran) -> usize {
    ran.requests
        .iter()
        .filter(|r| matches!(r, BrokerRequest::ListOrders(_)))
        .count()
}

/// The only request in `ran` is one listing for `id`, never a lookup by client order id.
fn one_listing(ran: &Ran, id: &ClientOrderId, path: &str) {
    let named = ran.requests.iter().filter_map(|request| match request {
        BrokerRequest::ListOrders(listing) => Some(Ok(&listing.client_order_id)),
        BrokerRequest::GetOrderByClientId(asked) => Some(Err(asked)),
        _ => None,
    });
    assert_eq!(
        named.collect::<Vec<_>>(),
        vec![Ok(id)],
        "{path}: one listing, no lookup by id"
    );
}

/// DEC-862 item 3, each half alone: an intent of another agent with no order, then an order of
/// another agent with no intent of its own.
#[test]
#[ignore = "pending E7-23"]
fn another_agents_intent_or_order_alone_stops_the_listing() {
    let fixture = Fixture::new();
    let ports = fixture.ports();
    for event_type in ["IntentReceived", "OrderRequestRecorded"] {
        let (shell, id, lost) =
            after_prior(&ports, false, set(event_type, "agent_id", OTHER_AGENT));
        assert_eq!(listings(&lost), 0, "{event_type} names another agent");
        unknown(&shell, &id, event_type);
    }
}

/// DEC-863 item 3: an earlier order of ours differing from the lost one in any one matched member
/// is no lookalike, so the one exact record is adopted.
#[test]
#[ignore = "pending E7-23"]
fn an_earlier_order_differing_in_one_member_is_no_lookalike() {
    let fixture = Fixture::new();
    let ports = fixture.ports();
    let members = [
        ("instrument_id", OTHER),
        ("side", "sell"),
        ("qty", "9"),
        ("limit_price", "149"),
        ("type", "stop_limit"),
        ("tif", "gtc"),
    ];
    for (field, value) in members {
        let (mut shell, id, lost) = after_prior(&ports, false, set("OrderSubmitted", field, value));
        assert_eq!(listings(&lost), 1, "{field}: listed");
        let ran = shell.run(listed(&id, vec![exact("the-one", "accepted")]), &ports);
        sends_nothing(&ran, field);
        assert_state(&shell, &id, OrderState::Accepted, field);
    }
}

/// DEC-863 item 3: an order of ours whose request the fold does not hold (a replacement the
/// broker linked) counts as a lookalike on instrument, side and quantity alone.
#[test]
#[ignore = "pending E7-23"]
fn an_order_with_no_folded_request_is_a_lookalike_on_instrument_side_and_quantity() {
    let fixture = Fixture::new();
    let ports = fixture.ports();
    let (mut shell, id, lost) =
        after_prior(&ports, true, set("OrderSubmitted", "limit_price", "149"));
    assert_eq!(listings(&lost), 1, "listed");
    let ran = shell.run(listed(&id, vec![exact("the-one", "accepted")]), &ports);
    sends_nothing(&ran, "a lookalike");
    unknown(&shell, &id, "DEC-863 item 3");
}

/// DEC-862 item 3: a listing answer on a profile that queries by client order id was never asked.
#[test]
#[ignore = "pending E7-23"]
fn a_listing_under_a_queryable_profile_adopts_nothing() {
    let fixture = Fixture::new();
    let ports = fixture.ports();
    let (mut shell, id, _) = lost(&ports, true, Before::Nothing);
    let ran = shell.run(listed(&id, vec![exact("the-one", "accepted")]), &ports);
    sends_nothing(&ran, "an unasked listing");
    unknown(&shell, &id, "never asked");
}

/// A dedicated no-query shell with `AGENT`'s opening of 10 AAPL at 150 sent and unanswered.
fn sent(ports: &Ports<'_>) -> (Shell, ClientOrderId) {
    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(ports);
    shell.state = shell.state.clone().with_profile(profile(false));
    let sent = shell.run(handoff(INTENT, AGENT, opening(AAPL, "10", "150")), ports);
    (shell, sent.submissions()[0].client_order_id.clone())
}

#[test]
#[ignore = "pending E7-23"]
fn a_duplicate_client_order_id_lists_on_a_no_query_profile() {
    let fixture = Fixture::new();
    let ports = fixture.ports();
    let (mut shell, id) = sent(&ports);
    let client_order_id = id.as_str().to_owned();
    let ran = shell.run(
        Input::Broker(Ok(BrokerOutcome::DuplicateClientOrderId {
            client_order_id,
        })),
        &ports,
    );
    one_listing(&ran, &id, "duplicate");
}

#[test]
#[ignore = "pending E7-23"]
fn a_reconciliation_missing_the_order_lists_on_a_no_query_profile() {
    let fixture = Fixture::new();
    let ports = fixture.ports();
    let (mut shell, id) = sent(&ports);
    let mut answer = exact("b-1", "accepted").order;
    answer.client_order_id = Some(id.as_str().to_owned());
    shell.run(Input::Broker(Ok(BrokerOutcome::Submitted(answer))), &ports);
    let missing = snapshot(shell.head().0, ReconcileReason::Scheduled);
    let ran = shell.run(Input::BrokerSnapshot(missing), &ports);
    one_listing(&ran, &id, "reconciliation");
}

/// Rule 5's bound: a risk exit waits on the agent's unanswered opening past the window, which
/// marks it overdue and `Unknown`.
#[test]
#[ignore = "pending E7-23"]
fn an_overdue_opening_lists_on_a_no_query_profile() {
    let fixture = Fixture::new();
    let ports = fixture.ports();
    let (mut shell, held) = sent(&ports);
    let mut answer = exact("b-1", "filled").order;
    answer.client_order_id = Some(held.as_str().to_owned());
    answer.filled_qty = qty("10");
    let fill = broker_fill("f-1", Some(held.as_str()), "10", "150");
    shell.run(Input::BrokerUpdate(BrokerUpdate::Fill(fill)), &ports);
    shell.run(Input::BrokerUpdate(BrokerUpdate::Order(answer)), &ports);
    let second = shell.run(handoff(AGAIN, AGENT, opening(AAPL, "10", "150")), &ports);
    let id = second.submissions()[0].client_order_id.clone();
    shell.run(
        handoff(OTHER_INTENT, AGENT, risk_exit(AAPL, "10", "140")),
        &ports,
    );
    let window = config().unknown_absent_window_s;
    let at = submitted_at(&shell, &id).secs() + window + 1;
    let ran = shell.run(Input::Tick(RiskClock::from_secs(at)), &ports);
    one_listing(&ran, &id, "overdue");
}

/// Restart re-queries every `Unknown` order: the new process is given the profile before it
/// folds the journal, then `Started`.
#[test]
#[ignore = "pending E7-23"]
fn a_restart_lists_an_unknown_order_on_a_no_query_profile() {
    let fixture = Fixture::new();
    let ports = fixture.ports();
    let (shell, id, _) = lost(&ports, false, Before::Nothing);
    let mut next = Shell::new(2);
    next.state = next.state.clone().with_profile(profile(false));
    for event in &shell.account_journal {
        next.fold_one(event).expect("the journal folds");
    }
    let epoch = next.epoch;
    let ran = next.run(Input::Started(epoch), &ports);
    one_listing(&ran, &id, "restart");
}
