//! An `Unknown` order on a profile with no query by client order id (E7-23 B2b; DEC-529 item 4,
//! DEC-862; trading-domain spec §5.3 rule 9, §5.7; connections spec §6.2; first-live-trade brief
//! LT-2, LT-6; `AGENTS.md` rules 3 and 13).
//!
//! The oracle is written here: a record matches only when its instrument, side, quantity, limit
//! price, order type and time in force are the order's as sent, and it carries no client order id
//! or ours. Exactly one match is adopted; zero or several leave the order `Unknown`, its
//! instrument blocked, with nothing sent, cancelled or filled. Part 1 pins the listing and the
//! cases that adopt nothing; part 2 pins the adoptions.

mod common;

use common::{
    AGENT, FixedInstruments, FixedMandate, OTHER_AGENT, Ran, Shell, TestIds, broker_order, config,
    handoff, opening, ports, price, qty, stream_opened,
};
use mandate_accounting::Side;
use mandate_canon::Value;
use mandate_domain::{
    AssetClass, CapabilityProfile, Cell, Idempotency, MarketSession, OrderType as Kind,
    ProtectionForm, QuantityForm, Retry, Row, TimeInForce as Tif,
};
use mandate_executor::{
    BrokerOutcome, BrokerRequest, BrokerUnknown, ClientOrderId, ExecutorConfig, Input, ListedOrder,
    OrderListing, OrderOrigin, OrderState, OrderType, Ports, RiskClock, TimeInForce,
};
use mandate_num::{Price, Qty};
use mandate_time::UtcNanos;

const AAPL: &str = "AAPL";
const OTHER: &str = "FRAC";
/// DEC-862 item 2: the listing starts this long before the order's `OrderSubmitted`.
const MARGIN_S: i64 = 300;
const INTENT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ0";
const AGAIN: &str = "01JABCDEFGHJKMNPQRSTVWXYZ2";
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

/// A started shell on `profile`, with `OTHER_AGENT` holding one accepted order first when
/// `shared`, and `AGENT`'s opening of 10 AAPL at 150 sent and its answer lost.
fn lost(ports: &Ports<'_>, query: bool, shared: bool) -> (Shell, ClientOrderId, Ran) {
    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(ports);
    shell.state = shell.state.clone().with_profile(profile(query));
    if shared {
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

/// Nothing in `ran` sends, cancels or re-queries an order (rule 13, LT-6).
fn sends_nothing(ran: &Ran, what: &str) {
    let only = ran
        .requests
        .iter()
        .all(|r| matches!(r, BrokerRequest::ListOrders(_)));
    assert!(
        only,
        "{what}: nothing sent, cancelled or queried: {:?}",
        ran.requests
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
#[ignore = "pending E7-23"]
fn a_lost_answer_lists_by_instrument_origin_and_creation_time() {
    let fixture = Fixture::new();
    let ports = fixture.ports();
    let (shell, id, lost) = lost(&ports, false, false);
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
#[ignore = "pending E7-23"]
fn no_matching_record_leaves_the_order_unknown_and_its_instrument_blocked() {
    let fixture = Fixture::new();
    let ports = fixture.ports();
    let (mut shell, id, _) = lost(&ports, false, false);
    let empty = shell.run(listed(&id, Vec::new()), &ports);
    sends_nothing(&empty, "an empty listing");
    unknown(&shell, &id, "zero records");
    stays_unknown_and_blocks(&mut shell, &id, &ports, near_misses());
}

#[test]
#[ignore = "pending E7-23"]
fn an_account_another_agent_trades_on_is_never_listed() {
    let fixture = Fixture::new();
    let ports = fixture.ports();
    let (_, _, alone) = lost(&ports, false, false);
    let listings = alone
        .requests
        .iter()
        .filter(|r| matches!(r, BrokerRequest::ListOrders(_)));
    assert_eq!(
        listings.count(),
        1,
        "the control: a dedicated account lists"
    );
    let (mut shell, id, lost) = lost(&ports, false, true);
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
    let (shell, id, lost) = lost(&ports, true, false);
    assert_eq!(
        lost.requests,
        vec![BrokerRequest::GetOrderByClientId(id.clone())],
        "§5.2: reconciliation queries by client order id when the profile can"
    );
    unknown(&shell, &id, "§5.7");
}
