//! A filled bracket's legs at reconciliation (E7-4, the slice that reconciles protective legs;
//! the first paper trade's FT-11; [DEC-878](../../../docs/project/decisions/DEC-878.md)).
//!
//! Once a bracket entry fills completely, its legs are recorded `placed` under the platform's
//! name for them, `{entry}-p{record}` (trading-domain spec §2.3, §5.4). Alpaca never carries that
//! name: a bracket's parent is the entry itself, and the broker names each leg with its own
//! `client_order_id` (the recorded `submit_bracket_accepted`). The open-orders read nests the legs
//! under their parent (`nested=true`), so reconciliation (§11 step 1) finds them there, through
//! the entry's `client_order_id`, and never by the leg's own id (DEC-878 items 2 to 4). The
//! placement counts as present only when the whole bracket rests as recorded: the entry listed
//! `filled`, and exactly its stop and its take-profit nested under it, both resting, each for the
//! placement's quantity at the recorded price. Anything else, a half-legged bracket included, is
//! still adopted `Unknown` with its `CompensatingEvent`, as any missing order is (rule 3, DEC-878
//! item 5).

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

/// The stop leg as Alpaca nests it under the entry: a sell stop at 140 for `quantity`, `status`,
/// under the broker's own order id and the broker's own `client_order_id`, never one of ours.
fn stop_leg(status: &str, quantity: &str) -> BrokerOrder {
    BrokerOrder {
        client_order_id: Some("94969c96-b018-47e9-a6fc-c3faa85b65af".to_owned()),
        limit_price: None,
        stop_price: Some(price("140")),
        created_on: None,
        ..broker_order("b2222222", None, AAPL, Side::Sell, quantity, "0", status)
    }
}

/// The take-profit leg: a sell limit at 170 for `quantity`, `status`.
fn take_profit_leg(status: &str, quantity: &str) -> BrokerOrder {
    BrokerOrder {
        client_order_id: Some("86942a5e-a11d-4464-872b-05f671c148ac".to_owned()),
        limit_price: Some(price("170")),
        stop_price: None,
        created_on: None,
        ..broker_order("a1111111", None, AAPL, Side::Sell, quantity, "0", status)
    }
}

/// The two legs of the filled bracket of 10, resting as Alpaca reports them once the entry fills:
/// the take-profit `new`, the stop `held` (§5.7 maps both to `Accepted`).
fn live_legs() -> Vec<BrokerOrder> {
    vec![take_profit_leg("new", "10"), stop_leg("held", "10")]
}

/// The entry as the open-orders read lists it once filled, `client_order_id` its own, with `legs`
/// nested under it.
fn listed_entry(client_order_id: &str, legs: Vec<BrokerOrder>) -> BrokerOrder {
    listed_entry_as(client_order_id, "filled", legs)
}

/// [`listed_entry`] with the entry's own status `status`.
fn listed_entry_as(client_order_id: &str, status: &str, legs: Vec<BrokerOrder>) -> BrokerOrder {
    BrokerOrder {
        legs,
        ..broker_order(
            "e0000000",
            Some(client_order_id),
            AAPL,
            Side::Buy,
            "10",
            "10",
            status,
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
    after_a_fill_of(shell, "10", "18500", open_orders)
}

/// The snapshot after a fill of `held` at 150, with `cash` left of the 20000, and `open_orders`.
fn after_a_fill_of(shell: &Shell, held: &str, cash: &str, open_orders: Vec<BrokerOrder>) -> Input {
    let mut taken = snapshot(shell.head().0, ReconcileReason::Scheduled);
    taken.open_orders = open_orders;
    taken.positions = vec![broker_position(AAPL, held)];
    let mut account = broker_account();
    account.cash = common::usd(cash);
    taken.account = account;
    Input::BrokerSnapshot(taken)
}

/// A ready executor whose bracket entry of 10 filled 4 and was then cancelled, so its 4 are
/// protected by one GTC OCO the executor **sent** (an `OrderSubmitted` of its own), named
/// `{entry}-p{record}` like a bracket's placement and acknowledged by the broker (§5.4); answers
/// the entry's id and the OCO's.
fn partly_filled_bracket(ports: &Ports<'_>) -> (Shell, String, String) {
    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("the stream opens");
    let mut shell = shell.restart_ready(ports);
    let entry = shell
        .run(
            handoff(
                INTENT,
                common::AGENT,
                protected_opening(AAPL, "10", "150", "140", Some("170")),
            ),
            ports,
        )
        .submissions()
        .first()
        .map(|order| order.client_order_id.as_str().to_owned())
        .expect("the entry goes as one bracket (§5.4)");
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-1",
            Some(&entry),
            "4",
            "150",
        ))),
        ports,
    );
    let ended = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "e0000000",
            Some(&entry),
            AAPL,
            Side::Buy,
            "10",
            "4",
            "canceled",
        ))),
        ports,
    );
    let oco = ended
        .submissions()
        .first()
        .filter(|order| order.oco.is_some())
        .map(|order| order.client_order_id.as_str().to_owned())
        .expect("ended partly filled, the entry gets one GTC OCO for its 4 (§5.4)");
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "o0000000",
            Some(&oco),
            AAPL,
            Side::Sell,
            "4",
            "0",
            "accepted",
        ))),
        ports,
    );
    (shell, entry, oco)
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
/// entry `filled` with its stop and take-profit resting under it, each for the 10 at the recorded
/// price, finds the placement present. Nothing is
/// adopted for it, no `CompensatingEvent` names it, it is never asked for by the name Alpaca does
/// not hold, it stays `Accepted` and still covers the 10, and the run is clean. The legs' own
/// `client_order_id`s, the broker's, are not external activity (item 4).
#[test]
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
        "the whole bracket rests under its filled entry as recorded, so its placement is \
         present, not adopted unknown and compensated (DEC-878 item 2): {:?}",
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

/// What a listing is shown to: the filled bracket's placement, or the OCO a partly filled
/// bracket was sent.
#[derive(Clone, Copy)]
enum Placement {
    Bracket,
    SentOco,
}

/// How many listings [`listing`] answers.
const LISTINGS: usize = 22;

/// Listing `case` of the open orders right after the fill: its name, the placement it is shown
/// to, and whether it keeps that placement. Only the whole filled bracket resting as recorded does
/// (DEC-878 item 2): the entry `filled`, exactly one resting sell stop at 140 and one resting sell
/// limit at 170 with no stop price nested under it, each for the placement's 10. A leg rests when
/// §5.7 maps its status to `Accepted` and none of it has filled.
fn listing(case: usize, entry: &str) -> (&'static str, Placement, bool, Vec<BrokerOrder>) {
    let under = |legs: Vec<BrokerOrder>| vec![listed_entry(entry, legs)];
    let another = "md-01JABCDEFGHJKMNPQRSTVWXYZ9";
    let buying = |mut leg: BrokerOrder| {
        leg.side = Side::Buy;
        leg
    };
    let priced = |mut leg: BrokerOrder, stop: Option<&str>, limit: Option<&str>| {
        leg.stop_price = stop.map(price);
        leg.limit_price = limit.map(price);
        leg
    };
    let bracket = Placement::Bracket;
    match case {
        0 => (
            "both legs resting under the entry",
            bracket,
            true,
            under(live_legs()),
        ),
        1 => (
            "the take-profit resting, the stop cancelled",
            bracket,
            false,
            under(vec![
                take_profit_leg("new", "10"),
                stop_leg("canceled", "10"),
            ]),
        ),
        2 => (
            "the stop resting, the take-profit cancelled",
            bracket,
            false,
            under(vec![
                take_profit_leg("canceled", "10"),
                stop_leg("held", "10"),
            ]),
        ),
        3 => (
            "the take-profit alone, no stop nested",
            bracket,
            false,
            under(vec![take_profit_leg("new", "10")]),
        ),
        4 => (
            "both legs done",
            bracket,
            false,
            under(vec![
                take_profit_leg("canceled", "10"),
                stop_leg("expired", "10"),
            ]),
        ),
        5 => ("no legs", bracket, false, under(Vec::new())),
        6 => (
            "legs that buy",
            bracket,
            false,
            under(vec![
                buying(take_profit_leg("new", "10")),
                buying(stop_leg("held", "10")),
            ]),
        ),
        7 => (
            "resting legs under another order",
            bracket,
            false,
            vec![listed_entry(another, live_legs())],
        ),
        8 => ("nothing listed", bracket, false, Vec::new()),
        9 => (
            "the stop for 9 of the 10",
            bracket,
            false,
            under(vec![take_profit_leg("new", "10"), stop_leg("held", "9")]),
        ),
        10 => (
            "the take-profit for 9 of the 10",
            bracket,
            false,
            under(vec![take_profit_leg("new", "9"), stop_leg("held", "10")]),
        ),
        11 => (
            "the stop at 139, not the recorded 140",
            bracket,
            false,
            under(vec![
                take_profit_leg("new", "10"),
                priced(stop_leg("held", "10"), Some("139"), None),
            ]),
        ),
        12 => (
            "the take-profit at 171, not the recorded 170",
            bracket,
            false,
            under(vec![
                priced(take_profit_leg("new", "10"), None, Some("171")),
                stop_leg("held", "10"),
            ]),
        ),
        13 => (
            "the entry canceled, its legs resting",
            bracket,
            false,
            vec![listed_entry_as(entry, "canceled", live_legs())],
        ),
        14 => (
            "the entry rejected, its legs resting",
            bracket,
            false,
            vec![listed_entry_as(entry, "rejected", live_legs())],
        ),
        15 => (
            "the stop suspended, a restricted status",
            bracket,
            false,
            under(vec![
                take_profit_leg("new", "10"),
                stop_leg("suspended", "10"),
            ]),
        ),
        16 => (
            "the stop in a status outside §5.7's table",
            bracket,
            false,
            under(vec![take_profit_leg("new", "10"), stop_leg("frozen", "10")]),
        ),
        17 => (
            "two stops and no take-profit",
            bracket,
            false,
            under(vec![
                priced(take_profit_leg("new", "10"), Some("140"), None),
                stop_leg("held", "10"),
            ]),
        ),
        18 => (
            "a third leg nested, done, beside the two resting",
            bracket,
            false,
            under(vec![
                take_profit_leg("new", "10"),
                stop_leg("held", "10"),
                stop_leg("canceled", "10"),
            ]),
        ),
        19 => (
            "a stop carrying a limit at the take-profit's 170, the take-profit cancelled",
            bracket,
            false,
            under(vec![
                priced(stop_leg("held", "10"), Some("140"), Some("170")),
                take_profit_leg("canceled", "10"),
            ]),
        ),
        20 => (
            "the take-profit `new` with 3 of its 10 already filled",
            bracket,
            false,
            under(vec![
                BrokerOrder {
                    filled_qty: qty("3"),
                    ..take_profit_leg("new", "10")
                },
                stop_leg("held", "10"),
            ]),
        ),
        _ => (
            "a sent OCO absent, the entry listed filled with its legs resting for the OCO's 4",
            Placement::SentOco,
            false,
            under(vec![take_profit_leg("new", "4"), stop_leg("held", "4")]),
        ),
    }
}

/// DEC-878 items 2 and 5: only the whole bracket, resting as recorded under its filled entry,
/// keeps the placement. Every other listing adopts it `Unknown` with its `CompensatingEvent` and
/// asks after it, as §11 adopts any order the broker does not list (rule 3: what cannot be shown
/// resting is in doubt, never assumed). A half-legged bracket is one of them: a placement whose
/// stop is gone must never read as covering (rule 13's protection, DEC-878 item 9). So is a sent
/// OCO that is itself absent, whatever its entry lists: it has its own `OrderSubmitted` and is
/// found by its own id or not at all.
#[test]
fn only_the_whole_bracket_resting_under_its_filled_entry_keeps_the_placement() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = ports(&ids, &mandates, &instruments, &config);
    for case in 0..LISTINGS {
        let (_, shown, _, _) = listing(case, "md-unused");
        let (mut shell, entry, protection) = match shown {
            Placement::Bracket => filled_bracket(&ports),
            Placement::SentOco => partly_filled_bracket(&ports),
        };
        let (case, _, kept, open) = listing(case, &entry);
        let taken = match shown {
            Placement::Bracket => after_the_fill(&shell, open),
            Placement::SentOco => after_a_fill_of(&shell, "4", "19400", open),
        };
        let ran = shell.run(taken, &ports);
        let state = shell
            .state
            .orders()
            .values()
            .find(|order| order.client_order_id.as_str() == protection)
            .map(|order| order.state);
        let seen = (
            adopted(&ran, &protection),
            queried(&ran, &protection),
            state,
        );
        if kept {
            assert_eq!(
                seen,
                ((false, false), false, Some(OrderState::Accepted)),
                "{case}: the whole bracket rests under its filled entry, so the placement is kept \
                 (DEC-878 item 2)"
            );
        } else {
            assert_eq!(
                seen,
                ((true, true), true, Some(OrderState::Unknown)),
                "{case}: not the whole bracket resting as recorded, so the placement is adopted \
                 unknown, compensated and asked after (DEC-878 item 5, rule 3)"
            );
        }
    }
}

/// `AGENTS.md` rule 13, DEC-878 item 6: the reconciliation that finds the legs under their entry
/// never holds an exit. A risk exit right after it passes the gate's `unknown_order_in_flight`,
/// and its sequence's first step cancels the placement (§5.4's marketable exit sequence), rather
/// than waiting on a placement the run made `Unknown`.
#[test]
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

/// DEC-878 item 1, the "Not decided here" it parks for this slice and #1270's "Not done", rule 3:
/// the in-doubt lookup of a filled bracket's placement never asks the broker for the handle
/// `{entry}-p{record}`, which no Alpaca order carries and whose every answer is the broker's
/// 404. The doubt is settled through the entry — the legs Alpaca nests under it — so the
/// placement is asked after by the entry's own id, and until that settles it the placement
/// stays `Unknown`: in doubt, holding exits, never confirmed absent by a name the broker does
/// not hold.
#[ignore = "pending E7-4"]
#[test]
fn the_doubted_bracket_placement_is_asked_after_by_its_entry_never_by_its_handle() {
    let (ids, mandates, instruments, config) = (
        TestIds,
        FixedMandate::covering(&[AAPL]),
        FixedInstruments,
        config(),
    );
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, entry, protection) = filled_bracket(&ports);
    let half_bracket = listed_entry(
        &entry,
        vec![take_profit_leg("new", "10"), stop_leg("canceled", "10")],
    );
    let ran = shell.run(after_the_fill(&shell, vec![half_bracket]), &ports);
    assert_eq!(
        adopted(&ran, &protection),
        (true, true),
        "a half-legged bracket keeps today's fail-safe adoption, its `CompensatingEvent` \
         included (DEC-878 item 5): {:?}",
        ran.draft_types()
    );
    assert!(
        !queried(&ran, &protection),
        "no lookup asks the broker for the handle `{protection}`, which no Alpaca order \
         carries (DEC-878 item 1): {:?}",
        ran.requests
    );
    assert!(
        queried(&ran, &entry),
        "the doubt is settled through the entry whose nested legs Alpaca lists, so the \
         lookup names the entry's own id (DEC-878 item 2, #1270's Not done): {:?}",
        ran.requests
    );
    let state = shell
        .state
        .orders()
        .values()
        .find(|order| order.client_order_id.as_str() == protection)
        .map(|order| order.state);
    assert_eq!(
        state,
        Some(OrderState::Unknown),
        "the placement stays in doubt, never assumed gone whatever the broker answered by a \
         name it does not hold (rule 3, DEC-878 item 1)"
    );
}
