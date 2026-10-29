//! DEC-2857 (the founder's ruling; trading spec v0.27 §5.3 rule 9, §5.5): an agent-scoped kill
//! switch's flatten never waits on an `Unknown` order in the instrument. Its certain part,
//! `k-<switch>-<n>`, sells at once what no `Unknown` order could still sell; its remainder,
//! `k-<switch>-<n>r`, is held by §5.3 rule 9 and sells what is left once nothing there is
//! `Unknown`, with no new approval.
//!
//! Each case computes its quantities by hand from the DEC's items. The property at the end drives
//! the executor through random resolutions and judges the run with its own accumulator, which is
//! shown to fail on the seeded bugs the DEC names.

mod common;

use common::{
    ACCOUNT_STREAM, FixedInstruments, FixedMandate, Shell, TestIds, agent, broker_fill,
    broker_order, clock, config, discretionary_exit, event, handoff, ports, price, qty, quote,
    stream_opened, text, with_clock,
};
use mandate_accounting::Side;
use mandate_canon::Value;
use mandate_executor::{
    BrokerOutcome, BrokerRequest, BrokerUnknown, BrokerUpdate, Command, Initiator, Input,
    KillScope, OwnerConfirmation, Ports, Purpose,
};
use mandate_num::Qty;
use proptest::prelude::*;
use std::collections::VecDeque;

const AAPL: &str = FixedInstruments::LIQUID_EQUITY;
const OCO: &str = "md-oco-1";

/// The agent's ten `AAPL`, bought at 150 and protected by one resting OCO, made ready, with a
/// sane quote at 155.
fn ten_held(ports: &Ports<'_>) -> Shell {
    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("folds");
    let held = [
        (
            "OrderSubmitted",
            vec![
                ("client_order_id", text("md-held-1")),
                ("agent", text(common::AGENT)),
                ("instrument", text(AAPL)),
                ("side", text("buy")),
                ("qty", text("10")),
                ("limit", text("150")),
            ],
        ),
        (
            "FillApplied",
            vec![
                ("fill_id", text("f-0")),
                ("client_order_id", text("md-held-1")),
                ("instrument", text(AAPL)),
                ("side", text("buy")),
                ("qty_gross", text("10")),
                ("price", text("150")),
            ],
        ),
        (
            "OrderStateChanged",
            vec![
                ("client_order_id", text("md-held-1")),
                ("state", text("filled")),
            ],
        ),
        (
            "ProtectionChanged",
            vec![
                ("instrument", text(AAPL)),
                ("action", text("placed")),
                ("orders", text(OCO)),
                ("qty", text("10")),
                ("take_profit", text("170")),
                ("stop", text("140")),
                ("created_on", text("2026-09-22")),
            ],
        ),
    ];
    for (seq, (event_type, pairs)) in (2..).zip(held) {
        shell
            .fold_one(&event(
                ACCOUNT_STREAM,
                seq,
                event_type,
                with_clock(&pairs, 10),
            ))
            .expect("the held position folds");
    }
    let mut shell = shell.restart_ready(ports);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 10)), ports);
    shell
}

/// One order the broker may hold as the agent's, its answer lost: `side` `qty` of `AAPL`, an exit
/// for a sell and an opening for a buy, journaled submitted and then `Unknown`.
fn unknown(shell: &mut Shell, id: &str, side: Side, quantity: &str) {
    let (side, purpose) = match side {
        Side::Sell => ("sell", "discretionary_exit"),
        Side::Buy => ("buy", "open"),
    };
    let submitted = [
        ("client_order_id", text(&format!("md-{id}"))),
        ("intent_id", text(id)),
        ("agent", text(common::AGENT)),
        ("instrument", text(AAPL)),
        ("side", text(side)),
        ("qty", text(quantity)),
        ("limit", text("155")),
        ("purpose", text(purpose)),
    ];
    let lost = [
        ("client_order_id", text(&format!("md-{id}"))),
        ("state", text("unknown")),
    ];
    for (event_type, pairs) in [
        ("OrderSubmitted", &submitted[..]),
        ("OrderStateChanged", &lost),
    ] {
        let at = shell.head().0.saturating_add(1);
        shell
            .fold_one(&event(
                ACCOUNT_STREAM,
                at,
                event_type,
                with_clock(pairs, 10),
            ))
            .expect("the order in doubt folds");
    }
}

/// The OCO's own placement answer lost: the protective order in doubt is the resting one.
fn oco_unknown(shell: &mut Shell) {
    let at = shell.head().0.saturating_add(1);
    let lost = [("client_order_id", text(OCO)), ("state", text("unknown"))];
    shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            at,
            "OrderStateChanged",
            with_clock(&lost, 10),
        ))
        .expect("the protective order in doubt folds");
}

/// The owner's agent kill switch over `AAPL`, confirmed, then the OCO's cancel confirmed if the
/// switch asked it: every record and request of both steps, in order.
fn switched(shell: &mut Shell, ports: &Ports<'_>) -> Vec<common::Ran> {
    let switch = shell.run(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(agent(common::AGENT)),
            initiator: Initiator::Owner,
            confirmation: Some(OwnerConfirmation {
                bid: price("155"),
                bid_size: qty("100"),
                floor: price("150.35"),
                user: "user-1".to_owned(),
                step_up: "assertion-1".to_owned(),
            }),
            cause: None,
        }),
        ports,
    );
    let asked = switch.requests.iter().any(|request| {
        matches!(request, BrokerRequest::Cancel { client_order_id } if client_order_id.as_str() == OCO)
    });
    let mut ran = vec![switch];
    if asked {
        ran.push(shell.run(
            Input::Broker(Ok(BrokerOutcome::CancelAccepted {
                client_order_id: OCO.to_owned(),
            })),
            ports,
        ));
    }
    ran
}

fn member<'a>(payload: &'a Value, name: &str) -> Option<&'a str> {
    payload.get(name).and_then(Value::as_str)
}

/// Each sell the runs sent: its client order id and quantity.
fn sells(runs: &[common::Ran]) -> Vec<(String, Qty)> {
    runs.iter()
        .flat_map(common::Ran::submissions)
        .filter(|order| order.side == Side::Sell && order.purpose != Purpose::Protective)
        .map(|order| (order.client_order_id.as_str().to_owned(), order.qty))
        .collect()
}

/// Each switch flatten the runs received (`k-…`): its intent id and quantity.
fn flattens(runs: &[common::Ran]) -> Vec<(String, String)> {
    runs.iter()
        .flat_map(|ran| ran.drafts.iter())
        .filter(|draft| draft.event_type == "IntentReceived")
        .filter_map(|draft| {
            let intent = member(&draft.payload, "intent_id")?;
            intent.starts_with("k-").then(|| {
                let quantity = member(&draft.payload, "qty").unwrap_or_default();
                (intent.to_owned(), quantity.to_owned())
            })
        })
        .collect()
}

/// Each gate verdict the runs journaled for `intent`, with its reason code.
fn verdicts(runs: &[common::Ran], intent: &str) -> Vec<(String, Option<String>)> {
    runs.iter()
        .flat_map(|ran| ran.drafts.iter())
        .filter(|draft| {
            draft.event_type == "GateDecided" && member(&draft.payload, "intent_id") == Some(intent)
        })
        .map(|draft| {
            (
                member(&draft.payload, "verdict")
                    .unwrap_or_default()
                    .to_owned(),
                member(&draft.payload, "reason_code").map(str::to_owned),
            )
        })
        .collect()
}

/// The reasons of the `OrderAbandoned` records the runs journaled.
fn abandoned(runs: &[common::Ran]) -> Vec<String> {
    runs.iter()
        .flat_map(|ran| ran.drafts.iter())
        .filter(|draft| draft.event_type == "OrderAbandoned")
        .filter_map(|draft| member(&draft.payload, "reason").map(str::to_owned))
        .collect()
}

/// The broker's answer to the lookup of `md-{id}`: found `status` with `filled` of `qty`, its
/// fill reported first as the broker streams it, then the tick at `at`, at which the executor
/// re-gates what it holds (§5.7).
fn resolved(
    shell: &mut Shell,
    ports: &Ports<'_>,
    at: i64,
    (id, side, quantity): (&str, Side, &str),
    (status, filled): (&str, &str),
) -> Vec<common::Ran> {
    let client = format!("md-{id}");
    let mut ran = Vec::new();
    if qty(filled) > Qty::ZERO {
        let mut fill = broker_fill(&format!("f-{id}"), Some(&client), filled, "155");
        fill.side = side;
        ran.push(shell.run(Input::BrokerUpdate(BrokerUpdate::Fill(fill)), ports));
    }
    let order = broker_order(
        &format!("b-{id}"),
        Some(&client),
        AAPL,
        side,
        quantity,
        filled,
        status,
    );
    ran.push(shell.run(Input::Broker(Ok(BrokerOutcome::Order(order))), ports));
    ran.push(shell.run(Input::Tick(clock(at)), ports));
    ran
}

/// A sell the executor sent, accepted and then filled whole at 155.
fn filled(shell: &mut Shell, ports: &Ports<'_>, client: &str, quantity: Qty) -> Vec<common::Ran> {
    let text_qty = quantity.to_string();
    let accepted = broker_order(
        &format!("b-{client}"),
        Some(client),
        AAPL,
        Side::Sell,
        &text_qty,
        "0",
        "new",
    );
    let mut fill = broker_fill(&format!("f-{client}"), Some(client), &text_qty, "155");
    fill.side = Side::Sell;
    let done = broker_order(
        &format!("b-{client}"),
        Some(client),
        AAPL,
        Side::Sell,
        &text_qty,
        &text_qty,
        "filled",
    );
    vec![
        shell.run(Input::Broker(Ok(BrokerOutcome::Submitted(accepted))), ports),
        shell.run(Input::BrokerUpdate(BrokerUpdate::Fill(fill)), ports),
        shell.run(Input::BrokerUpdate(BrokerUpdate::Order(done)), ports),
    ]
}

const SELL_IN_DOUBT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ0";
const BUY_IN_DOUBT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ1";

/// The agent's switch over its ten with a sell of four in doubt: the runs and the two parts' ids.
fn switched_beside_a_sell_of_four(shell: &mut Shell, ports: &Ports<'_>) -> Vec<common::Ran> {
    unknown(shell, SELL_IN_DOUBT, Side::Sell, "4");
    switched(shell, ports)
}

/// DEC-2857 items 1, 3 and 4: with a sell of four in doubt, the switch sells the six no `Unknown`
/// order could still sell at once, its certain part `k-…-0` of the sub-ledger's ten journaled
/// `rung_short` with six sent, and raises its remainder `k-…-0r` of ten, which the gate holds
/// `unknown_order_in_flight`.
#[test]
fn a_sell_in_doubt_leaves_the_certain_six_sold_at_once_and_the_remainder_held() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = ten_held(&ports);
    let runs = switched_beside_a_sell_of_four(&mut shell, &ports);
    let parts = flattens(&runs);
    let [(certain, certain_qty), (remainder, remainder_qty)] = parts.as_slice() else {
        panic!("two parts: {parts:?}");
    };
    assert!(!certain.ends_with('r') && remainder == &format!("{certain}r"));
    assert_eq!((certain_qty.as_str(), remainder_qty.as_str()), ("10", "10"));
    assert_eq!(
        sells(&runs),
        [(format!("md-{certain}"), qty("6"))],
        "ten held less the four the sell in doubt could still sell"
    );
    assert_eq!(
        verdicts(&runs, remainder),
        [(
            "hold".to_owned(),
            Some("unknown_order_in_flight".to_owned())
        )]
    );
    assert!(
        verdicts(&runs, certain)
            .iter()
            .all(|(verdict, _)| verdict == "allow"),
        "the certain part is never held by an Unknown order"
    );
    let short: Vec<(Option<&str>, Option<&str>)> = runs
        .iter()
        .flat_map(|ran| ran.drafts.iter())
        .filter(|draft| member(&draft.payload, "action") == Some("rung_short"))
        .map(|draft| {
            (
                member(&draft.payload, "intent_id"),
                member(&draft.payload, "sent"),
            )
        })
        .collect();
    assert_eq!(short, [(Some(certain.as_str()), Some("6"))]);
}

/// DEC-2857 items 4 and 3's cap: the sell in doubt is found cancelled with nothing filled, so the
/// remainder is released with no new approval and sells the four it left, beside the certain six
/// still resting: ten in all, never more.
#[test]
fn the_remainder_sells_what_a_cancelled_sell_in_doubt_left() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = ten_held(&ports);
    let runs = switched_beside_a_sell_of_four(&mut shell, &ports);
    let parts = flattens(&runs);
    let remainder = &parts.last().expect("the remainder").0;
    let after = resolved(
        &mut shell,
        &ports,
        20,
        (SELL_IN_DOUBT, Side::Sell, "4"),
        ("canceled", "0"),
    );
    assert_eq!(sells(&after), [(format!("md-{remainder}"), qty("4"))]);
    assert!(
        verdicts(&after, remainder)
            .iter()
            .any(|(verdict, _)| verdict == "allow"),
        "released by the re-gate, with no new approval"
    );
}

/// DEC-2857 item 5: the sell in doubt is found filled while the certain six still rest. The
/// remainder's cap is zero and nothing in the instrument is `Unknown` any more, so it does not
/// wait for the six: it ends at once, abandoned `flatten_covered`, never sent, and the owner is not
/// alerted about it. The six then fill, and the position is flat.
#[test]
fn a_remainder_left_nothing_is_abandoned_flatten_covered_without_an_alert() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = ten_held(&ports);
    let runs = switched_beside_a_sell_of_four(&mut shell, &ports);
    let (certain, six) = sells(&runs).first().cloned().expect("the certain part");
    let mut after = resolved(
        &mut shell,
        &ports,
        20,
        (SELL_IN_DOUBT, Side::Sell, "4"),
        ("filled", "4"),
    );
    assert_eq!(
        abandoned(&after),
        ["flatten_covered"],
        "a zero cap waits on nothing but an Unknown order"
    );
    after.extend(filled(&mut shell, &ports, &certain, six));
    assert!(sells(&after).is_empty(), "nothing more is sold");
    assert_eq!(abandoned(&after), ["flatten_covered"]);
    assert!(
        after
            .iter()
            .all(|ran| !ran.notifications.contains(&"exit_stalled")),
        "a remainder the flatten covered alerts nobody"
    );
    assert_eq!(
        shell
            .state
            .positions()
            .get(&common::instrument(AAPL))
            .copied(),
        Some(common::signed_qty("0"))
    );
}

/// DEC-2857 items 2 and 4: a buy of five in doubt could sell nothing, so the switch sells all ten
/// held at once. The remainder, ten plus the five the buy could fill, is held until the buy is
/// found filled, and then sells those five.
#[test]
fn a_buy_in_doubt_sells_the_ten_held_at_once_then_the_five_it_bought() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = ten_held(&ports);
    unknown(&mut shell, BUY_IN_DOUBT, Side::Buy, "5");
    let runs = switched(&mut shell, &ports);
    let parts = flattens(&runs);
    let quantities: Vec<&str> = parts
        .iter()
        .map(|(_, quantity)| quantity.as_str())
        .collect();
    assert_eq!(quantities, ["10", "15"]);
    let (certain, ten) = sells(&runs).first().cloned().expect("the certain part");
    assert_eq!(ten, qty("10"));
    let mut after = filled(&mut shell, &ports, &certain, ten);
    after.extend(resolved(
        &mut shell,
        &ports,
        20,
        (BUY_IN_DOUBT, Side::Buy, "5"),
        ("filled", "5"),
    ));
    let remainder = &parts.last().expect("the remainder").0;
    assert_eq!(sells(&after), [(format!("md-{remainder}"), qty("5"))]);
}

/// DEC-2857 items 2 and 3: the OCO's own answer was lost, and a stop that may have fired could
/// sell all ten, so nothing is certainly held. Only the remainder is raised, and the gate holds it.
#[test]
fn a_protective_order_in_doubt_over_the_whole_position_raises_only_the_remainder() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = ten_held(&ports);
    oco_unknown(&mut shell);
    let runs = switched(&mut shell, &ports);
    let parts = flattens(&runs);
    let [(remainder, ten)] = parts.as_slice() else {
        panic!("the remainder alone: {parts:?}");
    };
    assert!(remainder.ends_with('r'));
    assert_eq!(ten, "10");
    assert!(sells(&runs).is_empty());
    assert_eq!(
        verdicts(&runs, remainder),
        [(
            "hold".to_owned(),
            Some("unknown_order_in_flight".to_owned())
        )]
    );
}

/// DEC-2857 item 6: the remainder still held past `max_intent_age_s` (120 s) is journaled
/// `held_long` once and alerted once, `exit_stalled`, the alert carrying no quantity or price.
#[test]
fn a_remainder_held_past_the_intent_age_is_alerted_once() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = ten_held(&ports);
    let runs = switched_beside_a_sell_of_four(&mut shell, &ports);
    let remainder = flattens(&runs).last().cloned().expect("the remainder").0;
    let ticks: Vec<common::Ran> = [60, 131, 140, 200]
        .into_iter()
        .map(|at| shell.run(Input::Tick(clock(at)), &ports))
        .collect();
    let long: Vec<&str> = ticks
        .iter()
        .flat_map(|ran| ran.drafts.iter())
        .filter(|draft| {
            draft.event_type == "GateDecided"
                && draft.payload.get("held_long") == Some(&Value::Bool(true))
        })
        .filter_map(|draft| member(&draft.payload, "intent_id"))
        .collect();
    assert_eq!(long, [remainder.as_str()]);
    let alerts: usize = ticks
        .iter()
        .map(|ran| {
            ran.notifications
                .iter()
                .filter(|kind| **kind == "exit_stalled")
                .count()
        })
        .sum();
    assert_eq!(alerts, 1);
}

/// DEC-2857 item 8: any exit but the switch's certain part is still held by an `Unknown` order in
/// its instrument, as rule 13 says.
#[test]
fn an_exit_other_than_the_flatten_is_still_held_by_an_order_in_doubt() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = ten_held(&ports);
    unknown(&mut shell, BUY_IN_DOUBT, Side::Buy, "5");
    let exit = "01JABCDEFGHJKMNPQRSTVWXYZ2";
    let held = shell.run(
        handoff(exit, common::AGENT, discretionary_exit(AAPL, "3", "155")),
        &ports,
    );
    assert_eq!(
        verdicts(&[held], exit),
        [(
            "hold".to_owned(),
            Some("unknown_order_in_flight".to_owned())
        )]
    );
}

/// How the broker turns out to have handled one order in doubt, once its lookup is answered.
#[derive(Debug, Clone, Copy)]
enum Outcome {
    Filled,
    PartlyThenCancelled(u32),
    Cancelled,
    Rejected,
}

/// One order in doubt beside the agent's ten: its side, quantity, outcome, and the round of
/// lookups whose answer first finds it.
#[derive(Debug, Clone, Copy)]
struct Doubt {
    side: Side,
    qty: u32,
    outcome: Outcome,
    round: u32,
}

impl Doubt {
    fn filled(&self) -> u32 {
        match self.outcome {
            Outcome::Filled => self.qty,
            Outcome::PartlyThenCancelled(part) => part.min(self.qty.saturating_sub(1)),
            Outcome::Cancelled | Outcome::Rejected => 0,
        }
    }

    fn status(&self) -> &'static str {
        match self.outcome {
            Outcome::Filled => "filled",
            Outcome::PartlyThenCancelled(_) | Outcome::Cancelled => "canceled",
            Outcome::Rejected => "rejected",
        }
    }
}

/// What the run showed, in order: the oracle's only input besides the scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Seen {
    /// A flatten sell of this many was sent.
    Sent(u32),
    /// A flatten sell of this many filled.
    Sold(u32),
    /// Order in doubt `n` filled this many.
    DoubtFilled(usize, u32),
    /// Order in doubt `n` was found terminal.
    Resolved(usize),
}

const HELD: u32 = 10;

/// The independent oracle of DEC-2857 invariants 1 to 3, over the scenario and what the run
/// showed, with its own accumulators: the position, the flatten's sells outstanding, and the most
/// each order in doubt could still sell. It never reads the executor's state.
fn judge(doubts: &[(Side, u32)], seen: &[Seen]) -> Result<(), String> {
    let mut position = i64::from(HELD);
    let mut outstanding = 0_i64;
    let mut could_sell: Vec<i64> = doubts
        .iter()
        .map(|(side, quantity)| {
            if *side == Side::Sell {
                i64::from(*quantity)
            } else {
                0
            }
        })
        .collect();
    let certain = (i64::from(HELD) - could_sell.iter().sum::<i64>()).max(0);
    let mut before_any_resolved: Option<i64> = None;
    let mut sent_so_far = 0_i64;
    for step in seen {
        match *step {
            Seen::Sent(quantity) => {
                let quantity = i64::from(quantity);
                if outstanding + quantity + could_sell.iter().sum::<i64>() > position {
                    return Err(format!(
                        "a sell of {quantity} beside {outstanding} outstanding and {} in doubt \
                         exceeds the {position} held: it could go short",
                        could_sell.iter().sum::<i64>()
                    ));
                }
                outstanding += quantity;
                sent_so_far += quantity;
            }
            Seen::Sold(quantity) => {
                position -= i64::from(quantity);
                outstanding -= i64::from(quantity);
            }
            Seen::DoubtFilled(at, quantity) => {
                let quantity = i64::from(quantity);
                match doubts[at].0 {
                    Side::Sell => {
                        position -= quantity;
                        could_sell[at] -= quantity;
                    }
                    Side::Buy => position += quantity,
                }
            }
            Seen::Resolved(at) => {
                before_any_resolved.get_or_insert(sent_so_far);
                could_sell[at] = 0;
            }
        }
        if position < 0 {
            return Err(format!("the position went short: {position}"));
        }
    }
    let waited_on = before_any_resolved.unwrap_or(sent_so_far);
    if waited_on != certain {
        return Err(format!(
            "{waited_on} sold before any order in doubt resolved; {certain} was certainly held"
        ));
    }
    if position != 0 || outstanding != 0 {
        return Err(format!(
            "did not end flat: {position} held, {outstanding} outstanding"
        ));
    }
    Ok(())
}

/// A quantity the cases trade in whole shares, as a count; anything else reads as `u32::MAX`, which
/// the oracle can never balance, so a fractional send fails the case rather than vanishing.
fn shares(quantity: Qty) -> u32 {
    quantity.to_string().parse().unwrap_or(u32::MAX)
}

/// Runs the agent's switch over its ten beside `doubts` against a broker that fills every flatten
/// sell whole at once, answers each lookup of an order in doubt from its round on, and ticks eight
/// seconds a round: what the run showed.
fn drive(doubts: &[Doubt]) -> Vec<Seen> {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = ten_held(&ports);
    let named: Vec<String> = (0..doubts.len())
        .map(|at| format!("01JABCDEFGHJKMNPQRSTVWXY{at:02}"))
        .collect();
    for (doubt, id) in doubts.iter().zip(&named) {
        unknown(&mut shell, id, doubt.side, &doubt.qty.to_string());
    }
    let mut seen = Vec::new();
    let mut queue: VecDeque<BrokerRequest> = switched(&mut shell, &ports)
        .into_iter()
        .flat_map(|ran| ran.requests)
        .collect();
    let mut found = vec![false; doubts.len()];
    let mut round = 0_u32;
    let mut budget = 2_000_u32;
    while round < 40 && budget > 0 {
        let Some(request) = queue.pop_front() else {
            round += 1;
            let at = 10 + i64::from(round) * 8;
            queue.extend(shell.run(Input::Tick(clock(at)), &ports).requests);
            continue;
        };
        budget -= 1;
        let answered: Vec<common::Ran> = match &request {
            BrokerRequest::Submit(order)
                if order.side == Side::Sell && order.purpose != Purpose::Protective =>
            {
                seen.push(Seen::Sent(shares(order.qty)));
                let ran = filled(
                    &mut shell,
                    &ports,
                    order.client_order_id.as_str(),
                    order.qty,
                );
                seen.push(Seen::Sold(shares(order.qty)));
                ran
            }
            BrokerRequest::Submit(order) => {
                let text_qty = order.qty.to_string();
                let resting = broker_order(
                    "b-resting",
                    Some(order.client_order_id.as_str()),
                    AAPL,
                    order.side,
                    &text_qty,
                    "0",
                    "new",
                );
                vec![shell.run(Input::Broker(Ok(BrokerOutcome::Submitted(resting))), &ports)]
            }
            BrokerRequest::Cancel { client_order_id } => vec![shell.run(
                Input::Broker(Ok(BrokerOutcome::CancelAccepted {
                    client_order_id: client_order_id.as_str().to_owned(),
                })),
                &ports,
            )],
            BrokerRequest::GetOrderByClientId(client) => {
                let at = named
                    .iter()
                    .position(|id| format!("md-{id}") == client.as_str());
                match at {
                    Some(at) if !found[at] && doubts[at].round <= round => {
                        found[at] = true;
                        let doubt = doubts[at];
                        let part = doubt.filled();
                        if part > 0 {
                            seen.push(Seen::DoubtFilled(at, part));
                        }
                        seen.push(Seen::Resolved(at));
                        resolved(
                            &mut shell,
                            &ports,
                            10 + i64::from(round) * 8,
                            (&named[at], doubt.side, &doubt.qty.to_string()),
                            (doubt.status(), &part.to_string()),
                        )
                    }
                    _ => vec![shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports)],
                }
            }
            _ => vec![shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports)],
        };
        queue.extend(answered.into_iter().flat_map(|ran| ran.requests));
    }
    seen
}

fn doubt() -> impl Strategy<Value = Doubt> {
    (
        any::<bool>(),
        1_u32..=4,
        prop_oneof![
            Just(Outcome::Filled),
            (1_u32..=3).prop_map(Outcome::PartlyThenCancelled),
            Just(Outcome::Cancelled),
            Just(Outcome::Rejected),
        ],
        0_u32..=6,
    )
        .prop_map(|(sell, quantity, outcome, round)| Doubt {
            side: if sell { Side::Sell } else { Side::Buy },
            qty: quantity,
            outcome,
            round,
        })
}

/// DEC-2857 item 9, invariants 1 to 3: across random orders in doubt beside the agent's ten (buys
/// and sells, each found filled, partly filled then cancelled, cancelled or rejected, in any
/// order), the flatten never leaves the position able to go short, sells exactly what was
/// certainly held before anything resolved, and ends flat.
#[test]
fn across_random_resolutions_the_flatten_never_goes_short_and_ends_flat() {
    let mut runner = proptest::test_runner::TestRunner::new(proptest::test_runner::Config {
        cases: 48,
        failure_persistence: None,
        ..proptest::test_runner::Config::default()
    });
    let result = runner.run(&proptest::collection::vec(doubt(), 1..=3), |doubts| {
        let sells: u32 = doubts
            .iter()
            .filter(|d| d.side == Side::Sell)
            .map(|d| d.qty)
            .sum();
        prop_assume!(sells <= HELD);
        let scenario: Vec<(Side, u32)> = doubts.iter().map(|d| (d.side, d.qty)).collect();
        let seen = drive(&doubts);
        judge(&scenario, &seen).map_err(|why| TestCaseError::fail(format!("{why}: {seen:?}")))
    });
    assert!(result.is_ok(), "{result:?}");
}

/// The oracle accepts the run DEC-2857 describes for a sell of four in doubt found cancelled: six
/// at once, then the four left.
#[test]
fn the_oracle_accepts_the_certain_six_then_the_four_left() {
    let seen = [
        Seen::Sent(6),
        Seen::Sold(6),
        Seen::Resolved(0),
        Seen::Sent(4),
        Seen::Sold(4),
    ];
    assert_eq!(judge(&[(Side::Sell, 4)], &seen), Ok(()));
}

/// Seeded bug 1: a flatten that waits on the sell in doubt anyway sells nothing before it
/// resolves.
#[test]
fn the_oracle_catches_a_flatten_that_waits_anyway() {
    let seen = [Seen::Resolved(0), Seen::Sent(10), Seen::Sold(10)];
    assert!(judge(&[(Side::Sell, 4)], &seen).is_err());
}

/// Seeded bug 2: a flatten that sells the whole sub-ledger while a sell is in doubt, which then
/// turns out filled, leaves the account short.
#[test]
fn the_oracle_catches_the_whole_sub_ledger_sold_beside_a_sell_in_doubt() {
    let seen = [
        Seen::Sent(10),
        Seen::Sold(10),
        Seen::DoubtFilled(0, 4),
        Seen::Resolved(0),
    ];
    assert!(judge(&[(Side::Sell, 4)], &seen).is_err());
}

/// Seeded bug 4: a remainder sent uncapped, the whole ten, beside the certain six still resting,
/// could sell sixteen of ten: the two parts together must never exceed the position.
#[test]
fn the_oracle_catches_a_remainder_sent_uncapped_beside_the_certain_part() {
    let seen = [
        Seen::Sent(6),
        Seen::Resolved(0),
        Seen::Sent(10),
        Seen::Sold(6),
        Seen::Sold(4),
    ];
    assert!(judge(&[(Side::Sell, 4)], &seen).is_err());
}

/// Seeded bug 3: a flatten that raises no remainder leaves the five a buy in doubt bought unsold.
#[test]
fn the_oracle_catches_no_remainder_after_a_buy_resolves() {
    let seen = [
        Seen::Sent(10),
        Seen::Sold(10),
        Seen::DoubtFilled(0, 5),
        Seen::Resolved(0),
    ];
    assert!(judge(&[(Side::Buy, 5)], &seen).is_err());
}
