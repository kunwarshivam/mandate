//! A kill switch's cancel lost to a crash (DEC-2302; trading-domain spec §5.5, §5.7; `AGENTS.md`
//! rules 3 and 13).
//!
//! The switch's batch journals the order `PendingCancel` before the `Cancel` reaches the broker,
//! so a crash between the two leaves a cancel the journal asked for and the broker never saw.
//! Recovery sends that cancel again. A broker that still reports the order working (`new`,
//! `accepted`, `partially_filled`, ...) has not cancelled it yet, and a broker that refuses a kill
//! switch's cancel while the order works has not ended it either: the order stays `PendingCancel`
//! with its cancel unconfirmed, so every later start sends it again, until the broker reports the
//! order in a terminal state, which ends it and every cancel of it. A cancel that is not a kill
//! switch's keeps §5.7's "cancel rejected" edge back to the prior state.

mod common;

use common::{
    CrashPoint, FixedInstruments, FixedMandate, Ran, Shell, TestIds, agent, broker_fill,
    broker_order, broker_reject, config, handoff, instrument, opening, ports, qty, signed_qty,
    stream_opened,
};
use mandate_accounting::Side;
use mandate_executor::{
    BrokerOutcome, BrokerRequest, BrokerUnknown, BrokerUpdate, ClientOrderId, Command, Initiator,
    Input, KillScope, OrderState, Ports,
};
use proptest::prelude::*;

const AAPL: &str = FixedInstruments::LIQUID_EQUITY;
const INTENT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ0";

/// A stream opened and a process started on it, ready to open.
fn fresh(ports: &Ports<'_>) -> Shell {
    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("folds");
    shell.restart_ready(ports)
}

/// An opening of the agent's for ten, submitted and acknowledged `new`, answering its client
/// order id.
fn working(shell: &mut Shell, ports: &Ports<'_>) -> String {
    let ran = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        ports,
    );
    let id = ran
        .submissions()
        .first()
        .map(|order| order.client_order_id.as_str().to_owned())
        .expect("the opening is sent");
    shell.run(reported(&id, "new", "0"), ports);
    id
}

/// The broker's description of the order, as a lookup or a cancel's read-back answers it.
fn reported(id: &str, status: &str, filled: &str) -> Input {
    Input::Broker(Ok(BrokerOutcome::Order(broker_order(
        "b-1",
        Some(id),
        AAPL,
        Side::Buy,
        "10",
        filled,
        status,
    ))))
}

fn refused(id: &str) -> Input {
    Input::Broker(Ok(BrokerOutcome::Rejected(broker_reject(
        Some(id),
        422,
        "order is not cancelable",
    ))))
}

fn filled(fill: &str, id: &str, quantity: &str) -> Input {
    Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
        fill,
        Some(id),
        quantity,
        "150",
    )))
}

fn switch() -> Input {
    Input::Command(Command::KillSwitch {
        scope: KillScope::Agent(agent(common::AGENT)),
        initiator: Initiator::RiskLimit,
        confirmation: None,
    })
}

/// The process that ran the switch crashed after its batch committed and before its `Cancel`
/// reached the broker.
fn crashed_after_the_switch(ports: &Ports<'_>) -> (Shell, String) {
    let mut shell = fresh(ports);
    let id = working(&mut shell, ports);
    shell
        .step_crashing(switch(), ports, CrashPoint::CancelBeforeConfirmation)
        .unwrap_or_else(|e| panic!("the switch refused with {}: {e}", e.code()));
    assert_eq!(
        shell.crashed_at,
        Some(CrashPoint::CancelBeforeConfirmation),
        "the switch's list holds the cancel the crash stops before"
    );
    assert!(
        !shell.connector.requests.contains(&format!("cancel {id}")),
        "the broker never saw the cancel"
    );
    assert_eq!(state_of(&shell, &id), Some(OrderState::PendingCancel));
    (shell, id)
}

/// The same crash, restarted once: the process the broker's answers then go to.
fn restarted_after_the_switch(ports: &Ports<'_>) -> (Shell, String) {
    let (mut crashed, id) = crashed_after_the_switch(ports);
    let (restarted, started) = crashed.restart_keeping_broker(ports);
    assert_eq!(cancels(&started, &id), 1, "{:?}", started.requests);
    (restarted, id)
}

fn state_of(shell: &Shell, id: &str) -> Option<OrderState> {
    shell.state.order(&key(id)).map(|order| order.state)
}

fn unconfirmed(shell: &Shell, id: &str) -> bool {
    shell
        .state
        .order(&key(id))
        .is_some_and(|order| order.cancel_unconfirmed)
}

fn filled_qty(shell: &Shell, id: &str) -> Option<mandate_num::Qty> {
    shell.state.order(&key(id)).map(|order| order.filled_qty)
}

fn held(shell: &Shell) -> Option<mandate_num::SignedQty> {
    shell.state.positions().get(&instrument(AAPL)).copied()
}

fn key(id: &str) -> ClientOrderId {
    ClientOrderId::parse(id).expect("an id this platform derived")
}

/// The cancels of `id` one effect list asks, counted from the list itself.
fn cancels(ran: &Ran, id: &str) -> usize {
    ran.requests
        .iter()
        .filter(|request| {
            matches!(request, BrokerRequest::Cancel { client_order_id } if client_order_id.as_str() == id)
        })
        .count()
}

/// The lookups of `id` one effect list asks.
fn queries(ran: &Ran, id: &str) -> usize {
    ran.requests
        .iter()
        .filter(|request| {
            matches!(request, BrokerRequest::GetOrderByClientId(asked) if asked.as_str() == id)
        })
        .count()
}

/// The reproduction: the restart sends the cancel the crash lost, once, in place of a lookup.
#[test]
fn a_cancel_lost_to_a_crash_is_sent_again_on_restart() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);
    let (mut crashed, id) = crashed_after_the_switch(&ports);
    let (_, started) = crashed.restart_keeping_broker(&ports);
    assert_eq!(
        cancels(&started, &id),
        1,
        "the start sends the lost cancel once: {:?}",
        started.requests
    );
    assert_eq!(queries(&started, &id), 0, "{:?}", started.requests);
}

/// A broker that still reports the order working has not cancelled it yet: the order stays
/// `PendingCancel`, the next start sends the cancel again, and only the broker's confirmation
/// ends it, releasing its reservation.
#[test]
fn an_order_the_broker_reports_working_stays_pending_cancel_until_confirmed() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);
    let (mut restarted, id) = restarted_after_the_switch(&ports);
    let answered = restarted.run(reported(&id, "new", "0"), &ports);
    assert_eq!(cancels(&answered, &id), 0);
    assert_eq!(state_of(&restarted, &id), Some(OrderState::PendingCancel));
    assert!(unconfirmed(&restarted, &id));

    let (mut again, started) = restarted.restart_keeping_broker(&ports);
    assert_eq!(cancels(&started, &id), 1, "{:?}", started.requests);
    again.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: id.clone(),
        })),
        &ports,
    );
    assert_eq!(state_of(&again, &id), Some(OrderState::Canceled));
    assert!(!again.state.reservations().contains_key(&key(&id)));
    let (_, after) = again.restart_keeping_broker(&ports);
    assert_eq!(
        cancels(&after, &id),
        0,
        "a confirmed cancel is never sent again"
    );
}

/// A fill reported while the cancel is outstanding is booked and leaves the order
/// `PendingCancel` with its cancel unconfirmed (§5.7), and the cancel of the rest is still sent
/// at the next start.
#[test]
fn a_partial_fill_during_the_lost_cancel_is_booked_and_keeps_it_pending() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);
    let (mut restarted, id) = restarted_after_the_switch(&ports);
    restarted.run(filled("f-1", &id, "4"), &ports);
    let answered = restarted.run(reported(&id, "partially_filled", "4"), &ports);
    assert_eq!(cancels(&answered, &id), 0);
    assert_eq!(state_of(&restarted, &id), Some(OrderState::PendingCancel));
    assert!(unconfirmed(&restarted, &id));
    assert_eq!(filled_qty(&restarted, &id), Some(qty("4")));
    assert_eq!(held(&restarted), Some(signed_qty("4")));
    let (_, started) = restarted.restart_keeping_broker(&ports);
    assert_eq!(cancels(&started, &id), 1, "{:?}", started.requests);
}

/// A kill switch's cancel the broker refuses while the order still works is not given up: one
/// refusal asks one lookup and no cancel, a working answer to it keeps the order `PendingCancel`
/// and asks nothing, and the next start sends the cancel again (rule 13).
#[test]
fn a_refused_kill_switch_cancel_stays_pending_and_is_sent_again_at_the_next_start() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);
    let (mut restarted, id) = restarted_after_the_switch(&ports);
    let refusal = restarted.run(refused(&id), &ports);
    assert_eq!(queries(&refusal, &id), 1, "{:?}", refusal.requests);
    assert_eq!(cancels(&refusal, &id), 0, "{:?}", refusal.requests);
    let answered = restarted.run(reported(&id, "new", "0"), &ports);
    assert_eq!(cancels(&answered, &id), 0, "{:?}", answered.requests);
    assert_eq!(queries(&answered, &id), 0, "{:?}", answered.requests);
    assert_eq!(state_of(&restarted, &id), Some(OrderState::PendingCancel));
    assert!(unconfirmed(&restarted, &id));
    let (_, started) = restarted.restart_keeping_broker(&ports);
    assert_eq!(cancels(&started, &id), 1, "{:?}", started.requests);
}

/// A cancel that is not a kill switch's keeps §5.7's "cancel rejected" edge: refused, then
/// reported working, the order returns to its prior state, and no start sends that cancel again,
/// which the sequence that asked it decides (§5.4).
#[test]
fn a_refused_cancel_that_is_not_a_kill_switchs_returns_to_the_prior_state() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);
    let mut shell = fresh(&ports);
    let id = working(&mut shell, &ports);
    let asked = shell.run(
        Input::Command(Command::CancelOpenings {
            agent: agent(common::AGENT),
            instrument: instrument(AAPL),
        }),
        &ports,
    );
    assert_eq!(cancels(&asked, &id), 1, "{:?}", asked.requests);
    assert_eq!(state_of(&shell, &id), Some(OrderState::PendingCancel));
    shell.run(refused(&id), &ports);
    shell.run(reported(&id, "new", "0"), &ports);
    assert_eq!(state_of(&shell, &id), Some(OrderState::Accepted));
    assert!(unconfirmed(&shell, &id));
    let (_, started) = shell.restart_keeping_broker(&ports);
    assert_eq!(
        cancels(&started, &id),
        0,
        "a start sends again only a cancel still pending: {:?}",
        started.requests
    );
}

/// Each terminal state the broker may report for the order instead of confirming the cancel
/// ends it: the reservation is released, and no start sends a cancel of an ended order.
#[test]
fn a_terminal_report_ends_the_lost_cancel_and_nothing_is_cancelled_after() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);
    for (status, ended) in [
        ("canceled", OrderState::Canceled),
        ("expired", OrderState::Expired),
        ("rejected", OrderState::Rejected),
        ("filled", OrderState::Filled),
    ] {
        let (mut restarted, id) = restarted_after_the_switch(&ports);
        let done = if status == "filled" {
            restarted.run(filled("f-all", &id, "10"), &ports);
            "10"
        } else {
            "0"
        };
        restarted.run(reported(&id, status, done), &ports);
        assert_eq!(state_of(&restarted, &id), Some(ended), "{status}");
        assert!(!unconfirmed(&restarted, &id), "{status}");
        assert!(
            !restarted.state.reservations().contains_key(&key(&id)),
            "{status}: the reservation is released"
        );
        if status == "filled" {
            assert_eq!(filled_qty(&restarted, &id), Some(qty("10")));
            assert_eq!(held(&restarted), Some(signed_qty("10")));
        }
        let (_, after) = restarted.restart_keeping_broker(&ports);
        assert_eq!(cancels(&after, &id), 0, "{status}: {:?}", after.requests);
    }
}

/// What the broker does between two starts, in the property below.
#[derive(Debug, Clone)]
enum Between {
    /// It reports the order working with this status.
    Reports(&'static str),
    /// It fills this many more, and reports the order partly filled.
    Fills(u8),
    /// Its answer is lost.
    Lost,
    /// A lookup finds no such order (a 404).
    Absent,
    /// It refuses the cancel.
    Refuses,
    /// The process crashes and starts again.
    Restart,
}

/// How the broker finally ends the order.
#[derive(Debug, Clone, Copy)]
enum End {
    Confirms,
    Reports(&'static str, OrderState),
    FillsTheRest,
}

fn between() -> impl Strategy<Value = Between> {
    prop_oneof![
        prop::sample::select(vec![
            "new",
            "accepted",
            "pending_new",
            "held",
            "pending_cancel"
        ])
        .prop_map(Between::Reports),
        (1u8..4).prop_map(Between::Fills),
        Just(Between::Lost),
        Just(Between::Absent),
        Just(Between::Refuses),
        Just(Between::Restart),
    ]
}

fn end() -> impl Strategy<Value = End> {
    prop_oneof![
        Just(End::Confirms),
        Just(End::Reports("canceled", OrderState::Canceled)),
        Just(End::Reports("expired", OrderState::Expired)),
        Just(End::Reports("rejected", OrderState::Rejected)),
        Just(End::FillsTheRest),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    /// A crash between the switch's batch and its cancel always leads, at every later start, to
    /// the cancel sent again once, whatever the broker answered in between short of a terminal
    /// state: working reports, partial fills, lost answers, 404s, and refusals all leave the order
    /// `PendingCancel` with its cancel unconfirmed, every fill booked, and a refusal asks one
    /// lookup and no cancel. Once the broker ends the order, however it ends it, the reservation is
    /// released and no start cancels it again. The oracle is the test's own record of what the
    /// broker said and filled, never the core's state.
    #[test]
    fn every_start_sends_the_lost_cancel_until_the_broker_ends_the_order(
        steps in prop::collection::vec(between(), 0..12),
        ending in end(),
    ) {
        let ids = TestIds;
        let mandates = FixedMandate::covering(&[AAPL]);
        let instruments = FixedInstruments;
        let configuration = config();
        let ports = ports(&ids, &mandates, &instruments, &configuration);
        let (mut shell, id) = restarted_after_the_switch(&ports);
        let mut done: u8 = 0;
        for (n, step) in steps.into_iter().enumerate() {
            match step {
                Between::Reports(status) => {
                    let ran = shell.run(reported(&id, status, &done.to_string()), &ports);
                    prop_assert_eq!(cancels(&ran, &id), 0);
                }
                Between::Fills(more) => {
                    if done + more < 10 {
                        done += more;
                        shell.run(filled(&format!("f-{n}"), &id, &more.to_string()), &ports);
                        let ran = shell.run(
                            reported(&id, "partially_filled", &done.to_string()),
                            &ports,
                        );
                        prop_assert_eq!(cancels(&ran, &id), 0);
                    }
                }
                Between::Lost => {
                    let ran = shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);
                    prop_assert_eq!(cancels(&ran, &id), 0);
                }
                Between::Absent => {
                    let ran = shell.run(
                        Input::Broker(Ok(BrokerOutcome::Absent { client_order_id: id.clone() })),
                        &ports,
                    );
                    prop_assert_eq!(cancels(&ran, &id), 0);
                }
                Between::Refuses => {
                    let ran = shell.run(refused(&id), &ports);
                    prop_assert_eq!(queries(&ran, &id), 1, "{:?}", ran.requests);
                    prop_assert_eq!(cancels(&ran, &id), 0, "{:?}", ran.requests);
                }
                Between::Restart => {
                    let (next, started) = shell.restart_keeping_broker(&ports);
                    prop_assert_eq!(cancels(&started, &id), 1, "{:?}", started.requests);
                    shell = next;
                }
            }
            prop_assert_eq!(state_of(&shell, &id), Some(OrderState::PendingCancel));
            prop_assert!(unconfirmed(&shell, &id));
            prop_assert_eq!(filled_qty(&shell, &id), Some(qty(&done.to_string())));
        }
        let (mut last, started) = shell.restart_keeping_broker(&ports);
        prop_assert_eq!(cancels(&started, &id), 1, "{:?}", started.requests);
        let ended = match ending {
            End::Confirms => {
                last.run(
                    Input::Broker(Ok(BrokerOutcome::CancelAccepted { client_order_id: id.clone() })),
                    &ports,
                );
                OrderState::Canceled
            }
            End::Reports(status, ended) => {
                last.run(reported(&id, status, &done.to_string()), &ports);
                ended
            }
            End::FillsTheRest => {
                let rest = 10 - done;
                last.run(filled("f-rest", &id, &rest.to_string()), &ports);
                last.run(reported(&id, "filled", "10"), &ports);
                done = 10;
                OrderState::Filled
            }
        };
        prop_assert_eq!(state_of(&last, &id), Some(ended));
        prop_assert!(!unconfirmed(&last, &id));
        prop_assert!(!last.state.reservations().contains_key(&key(&id)));
        prop_assert_eq!(filled_qty(&last, &id), Some(qty(&done.to_string())));
        let (_, after) = last.restart_keeping_broker(&ports);
        prop_assert_eq!(cancels(&after, &id), 0);
    }
}
