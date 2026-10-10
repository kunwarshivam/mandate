//! A filled bracket's legs at reconciliation (E7-4, the slice that reconciles protective legs;
//! the first paper trade's FT-11; [DEC-878](../../../docs/project/decisions/DEC-878.md)).
//!
//! Once a bracket entry fills completely, its legs are recorded `placed` under the platform's
//! name for them, `{entry}-p{record}` (trading-domain spec §2.3, §5.4). Alpaca never carries that
//! name: a bracket's parent is the entry itself, and the broker names each leg with its own
//! `client_order_id` (the recorded `submit_bracket_accepted`). The open-orders read nests the legs
//! under their parent (`nested=true`), so reconciliation (§11 step 1) finds them there, through
//! the entry's `client_order_id`, by side and status, and never by the leg's own id (DEC-878
//! items 2 to 4). A placement whose legs the broker does not list live is still adopted
//! `Unknown`, with its `CompensatingEvent`, as any missing order is (rule 3, DEC-878 item 5).

mod common;

use common::{
    FixedInstruments, FixedMandate, Shell, TestIds, broker_account, broker_fill, broker_order,
    broker_position, config, handoff, named_orders, ports, price, protected_opening, qty,
    risk_exit, snapshot, stream_opened,
};
use mandate_accounting::Side;
use mandate_canon::Value;
use mandate_executor::{
    BrokerOrder, BrokerRequest, BrokerUpdate, Input, OrderState, Ports, ReconcileReason,
};

const AAPL: &str = FixedInstruments::LIQUID_EQUITY;
const INTENT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ0";
const EXIT_INTENT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ2";

/// One leg of the bracket as Alpaca nests it under the entry: the broker's own order id, and the
/// broker's own `client_order_id`, a UUID that is never one of ours.
fn leg(broker_id: &str, side: Side, status: &str, take_profit: bool) -> BrokerOrder {
    BrokerOrder {
        broker_order_id: broker_id.to_owned(),
        client_order_id: Some(format!("{broker_id}-0000-4000-8000-000000000000")),
        side,
        limit_price: take_profit.then(|| price("170")),
        stop_price: (!take_profit).then(|| price("140")),
        status: status.to_owned(),
        created_on: None,
        ..broker_order(broker_id, None, AAPL, side, "10", "0", status)
    }
}

/// The two legs of the filled bracket, live as Alpaca reports them once the entry fills: the
/// take-profit `new`, the stop `held` (§5.7 maps both to `Accepted`).
fn live_legs() -> Vec<BrokerOrder> {
    vec![
        leg("a1111111", Side::Sell, "new", true),
        leg("b2222222", Side::Sell, "held", false),
    ]
}

/// The entry as the open-orders read lists it once filled, `client_order_id` its own, with `legs`
/// nested under it.
fn listed_entry(client_order_id: &str, legs: Vec<BrokerOrder>) -> BrokerOrder {
    BrokerOrder {
        legs,
        ..broker_order(
            "e0000000",
            Some(client_order_id),
            AAPL,
            Side::Buy,
            "10",
            "10",
            "filled",
        )
    }
}

/// A ready executor whose bracket entry of 10 `AAPL` at 150 (stop 140, take-profit 170) has filled
/// completely, its fill ingested and its legs recorded `placed`; answers the entry's id and the
/// placement's.
fn filled_bracket(ports: &Ports<'_>) -> (Shell, String, String) {
    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("the stream opens");
    let mut shell = shell.restart_ready(ports);
    let sent = shell.run(
        handoff(
            INTENT,
            common::AGENT,
            protected_opening(AAPL, "10", "150", "140", Some("170")),
        ),
        ports,
    );
    let entry = sent
        .submissions()
        .first()
        .filter(|order| order.bracket.is_some())
        .map(|order| order.client_order_id.as_str().to_owned())
        .expect("the entry goes as one bracket (§5.4)");
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-1",
            Some(&entry),
            "10",
            "150",
        ))),
        ports,
    );
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "e0000000",
            Some(&entry),
            AAPL,
            Side::Buy,
            "10",
            "10",
            "filled",
        ))),
        ports,
    );
    let placed: Vec<String> = shell
        .account_journal
        .iter()
        .filter(|event| event.event_type == "ProtectionChanged")
        .filter(|event| {
            matches!(event.payload.get("action"), Some(Value::Str(action)) if action == "placed")
        })
        .flat_map(|event| named_orders(&event.payload, "orders"))
        .collect();
    let [protection] = placed.as_slice() else {
        panic!("the complete fill records the legs placed, once, under one name: {placed:?}");
    };
    (shell, entry, protection.clone())
}

/// The snapshot of the account right after the fill: the 10 held, and `open_orders`.
fn after_the_fill(shell: &Shell, open_orders: Vec<BrokerOrder>) -> Input {
    let mut taken = snapshot(shell.head().0, ReconcileReason::Scheduled);
    taken.open_orders = open_orders;
    taken.positions = vec![broker_position(AAPL, "10")];
    taken.account = shell_account();
    Input::BrokerSnapshot(taken)
}

/// The broker's account once the 10 at 150 are bought: 1500 out of the 20000 cash.
fn shell_account() -> mandate_executor::BrokerAccount {
    let mut account = broker_account();
    account.cash = common::usd("18500");
    account
}

/// Whether `ran` adopted a state for `id`: an `OrderStateChanged` naming it, and its
/// `CompensatingEvent` (§11).
fn adopted(ran: &common::Ran, id: &str) -> (bool, bool) {
    let names = |kind: &str, member: &str| {
        ran.drafts.iter().any(|draft| {
            draft.event_type == kind
                && matches!(draft.payload.get(member), Some(Value::Str(named)) if named == id)
        })
    };
    (
        names("OrderStateChanged", "client_order_id"),
        names("CompensatingEvent", "subject"),
    )
}

/// Whether `ran` asked the broker for `id` by client order id.
fn queried(ran: &common::Ran, id: &str) -> bool {
    ran.requests.iter().any(|request| {
        matches!(request, BrokerRequest::GetOrderByClientId(asked) if asked.as_str() == id)
    })
}

/// DEC-878 items 2 and 3, FT-11: right after a complete fill, a reconciliation that lists the
/// entry with its two live sell legs nested under it finds the placement present. Nothing is
/// adopted for it, no `CompensatingEvent` names it, it is never asked for by the name Alpaca does
/// not hold, it stays `Accepted` and still covers the 10, and the run is clean. The legs' own
/// `client_order_id`s, the broker's, are not external activity (item 4).
#[test]
#[ignore = "pending E7-4"]
fn a_filled_brackets_legs_listed_under_its_entry_keep_their_placement() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, entry, protection) = filled_bracket(&ports);

    let ran = shell.run(
        after_the_fill(&shell, vec![listed_entry(&entry, live_legs())]),
        &ports,
    );

    assert_eq!(
        adopted(&ran, &protection),
        (false, false),
        "the legs are listed live under their entry, so their placement is present, not adopted \
         unknown and compensated (DEC-878 item 2): {:?}",
        ran.draft_types()
    );
    assert!(
        !queried(&ran, &protection),
        "no lookup by a name Alpaca never gives a leg (DEC-878 item 1)"
    );
    assert!(
        !ran.draft_types().contains(&"ExternalActivityIngested"),
        "the broker's leg ids, nested under our entry, are not external activity (DEC-878 item 4)"
    );
    let order = shell
        .state
        .orders()
        .values()
        .find(|order| order.client_order_id.as_str() == protection)
        .map(|order| (order.state, order.qty));
    assert_eq!(
        order,
        Some((OrderState::Accepted, qty("10"))),
        "the placement stays live, covering the whole 10 (§5.4, rule 13)"
    );
    let result = ran
        .draft("ReconciliationRun")
        .and_then(|run| run.payload.get("result").cloned());
    assert_eq!(
        result,
        Some(Value::Str("clean".to_owned())),
        "nothing differs: {:?}",
        ran.draft_types()
    );
}

/// How many listings [`listing`] answers.
const LISTINGS: usize = 7;

/// Listing `case` of the open orders right after the fill, its name, and whether it keeps the
/// placement: only a live sell leg nested under `entry` does.
fn listing(case: usize, entry: &str) -> (&'static str, bool, Vec<BrokerOrder>) {
    let done = vec![
        leg("a1111111", Side::Sell, "canceled", true),
        leg("b2222222", Side::Sell, "expired", false),
    ];
    let buying = vec![
        leg("a1111111", Side::Buy, "new", true),
        leg("b2222222", Side::Buy, "held", false),
    ];
    let one_live = vec![
        leg("a1111111", Side::Sell, "canceled", true),
        leg("b2222222", Side::Sell, "held", false),
    ];
    let another = "md-01JABCDEFGHJKMNPQRSTVWXYZ9";
    match case {
        0 => (
            "both legs live under the entry",
            true,
            vec![listed_entry(entry, live_legs())],
        ),
        1 => (
            "one leg live under the entry",
            true,
            vec![listed_entry(entry, one_live)],
        ),
        2 => ("both legs done", false, vec![listed_entry(entry, done)]),
        3 => ("no legs", false, vec![listed_entry(entry, Vec::new())]),
        4 => ("legs that buy", false, vec![listed_entry(entry, buying)]),
        5 => (
            "live legs under another order",
            false,
            vec![listed_entry(another, live_legs())],
        ),
        _ => ("nothing listed", false, Vec::new()),
    }
}

/// DEC-878 items 2 and 5: only a live sell leg nested under **this** entry keeps the placement.
/// Listed live under the entry, it is kept; every other listing adopts it `Unknown` with its
/// `CompensatingEvent` and asks after it, as §11 adopts any order the broker does not list (rule
/// 3: what cannot be shown live is in doubt, never assumed). The others: the entry listed with
/// both legs done, with no legs, with legs that buy, the same live legs under another order's id,
/// and nothing listed at all.
#[test]
#[ignore = "pending E7-4"]
fn only_a_live_sell_leg_under_the_entry_keeps_the_placement() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = ports(&ids, &mandates, &instruments, &config);
    for case in 0..LISTINGS {
        let (mut shell, entry, protection) = filled_bracket(&ports);
        let (case, kept, open) = listing(case, &entry);
        let ran = shell.run(after_the_fill(&shell, open), &ports);
        let state = shell
            .state
            .orders()
            .values()
            .find(|order| order.client_order_id.as_str() == protection)
            .map(|order| order.state);
        if kept {
            assert_eq!(
                (
                    adopted(&ran, &protection),
                    queried(&ran, &protection),
                    state
                ),
                ((false, false), false, Some(OrderState::Accepted)),
                "{case}: a live sell leg under the entry keeps the placement (DEC-878 item 2)"
            );
        } else {
            assert_eq!(
                (
                    adopted(&ran, &protection),
                    queried(&ran, &protection),
                    state
                ),
                ((true, true), true, Some(OrderState::Unknown)),
                "{case}: no live sell leg under the entry, so the placement is adopted unknown, \
                 compensated and asked after (DEC-878 item 5, rule 3)"
            );
        }
    }
}

/// `AGENTS.md` rule 13, DEC-878 item 6: the reconciliation that finds the legs under their entry
/// never holds an exit. A risk exit right after it passes the gate's `unknown_order_in_flight`,
/// and its sequence's first step cancels the placement (§5.4's marketable exit sequence), rather
/// than waiting on a placement the run made `Unknown`.
#[test]
#[ignore = "pending E7-4"]
fn a_risk_exit_after_the_reconciliation_is_never_held_by_the_legs() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, entry, protection) = filled_bracket(&ports);
    shell.run(
        after_the_fill(&shell, vec![listed_entry(&entry, live_legs())]),
        &ports,
    );

    let exit = shell.run(
        handoff(EXIT_INTENT, common::AGENT, risk_exit(AAPL, "10", "150")),
        &ports,
    );

    let decided = exit
        .draft("GateDecided")
        .and_then(|draft| draft.payload.get("verdict").cloned());
    assert_eq!(
        decided,
        Some(Value::Str("allow".to_owned())),
        "no order of the instrument is in doubt, so the gate holds nothing, never \
         `unknown_order_in_flight` (rule 13): {:?}",
        exit.draft("GateDecided").map(|draft| &draft.payload)
    );
    assert!(
        exit.requests.iter().any(|request| matches!(
            request,
            BrokerRequest::Cancel { client_order_id } if client_order_id.as_str() == protection
        )),
        "the exit sequence cancels the placement first (§5.4): {:?}",
        exit.requests
    );
}
