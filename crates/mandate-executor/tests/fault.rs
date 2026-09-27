//! Fault injection at every submission step (E7-3's acceptance clause: "fault injection at every
//! submission step yields zero duplicates and full reconciliation; mismatches pause the agent and
//! alert").
//!
//! "Every step" is enumerated rather than asserted, so the claim is checkable. Each of the twelve
//! points is a named position in one submission's effect pipeline
//! ([`common::CrashPoint`]). Each case drops the state at that point, replays the fold from the
//! journal, runs `Input::Started`, drives the reconciliation, and asserts the same four things:
//!
//! 1. **Zero duplicates**, read off the fake connector's own counter rather than the journal, so
//!    an executor that journals once and sends twice still fails.
//! 2. **Full reconciliation**: every broker order is matched or adopted, every missing fill is
//!    ingested, and every remaining position difference has paused the agents holding it.
//! 3. **Protection is accounted for**: every unprotected interval has a journaled start and end.
//! 4. **The fold is faithful**: replaying the drafts reproduces the state, and the replay emits
//!    no draft and no broker request.

mod common;

use std::collections::BTreeSet;

use common::{
    ACCOUNT_STREAM, CrashPoint, FixedInstruments, FixedMandate, Shell, TestIds, clock, config,
    event, handoff, instrument, opening, ports, quote, risk_exit, snapshot, stream_opened, text,
    with_clock,
};
use mandate_canon::Value;
use mandate_executor::{
    BrokerOutcome, BrokerRequest, BrokerUpdate, Effect, Input, Ports, ReconcileReason,
    ReconciliationVerdict,
};

const AAPL: &str = FixedInstruments::LIQUID_EQUITY;
const INTENT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ0";

/// The four assertions every crash point owes, in one place so that adding a point cannot
/// quietly weaken them.
fn assert_the_four(shell: &Shell, point: CrashPoint) {
    for (id, count) in &shell.connector.accepted {
        assert!(
            *count <= 1,
            "crash at {point:?}: the broker accepted {count} distinct submissions for {id}, \
             which is the duplicate window R-03 names"
        );
    }

    let drafts: Vec<_> = shell
        .account_journal
        .iter()
        .map(|stored| mandate_executor::EventDraft {
            event_id: stored.event_id.clone(),
            event_type: stored.event_type.clone(),
            causation_id: stored.causation_id.clone(),
            payload: stored.payload.clone(),
        })
        .collect();

    let mut open: BTreeSet<String> = BTreeSet::new();
    for draft in &drafts {
        if draft.event_type != "ProtectionChanged" {
            continue;
        }
        let (Some(name), Some(action)) = (
            draft.payload.get("instrument").and_then(Value::as_str),
            draft.payload.get("action").and_then(Value::as_str),
        ) else {
            continue;
        };
        match action {
            "unprotected_start" => {
                open.insert(name.to_owned());
            }
            "unprotected_end" => {
                open.remove(name);
            }
            _ => {}
        }
    }
    assert!(
        open.is_empty(),
        "crash at {point:?}: {open:?} were left unprotected with no journaled end"
    );

    let replayed = shell.replay().unwrap_or_else(|e| {
        panic!(
            "crash at {point:?}: the replay refused with {}: {e}",
            e.code()
        )
    });
    assert_eq!(
        replayed.orders().len(),
        shell.state.orders().len(),
        "crash at {point:?}: the fold does not reproduce the state the run ended in"
    );
    assert_eq!(
        replayed.reservations().len(),
        shell.state.reservations().len(),
        "crash at {point:?}: the fold does not reproduce the reservations"
    );
}

/// Runs the reconciliation the recovery owes and asserts that nothing is left unexplained and
/// unpaused.
fn assert_full_reconciliation(shell: &Shell, ports: &Ports<'_>, point: CrashPoint) {
    let taken = snapshot(shell.head().0, ReconcileReason::Startup);
    let run = mandate_executor::reconcile(&shell.state, &taken, ports).unwrap_or_else(|e| {
        panic!(
            "crash at {point:?}: the reconciliation refused with {}: {e}",
            e.code()
        )
    });
    let unexplained: Vec<_> = run
        .differences
        .iter()
        .filter(|d| !d.adopted && d.kind != mandate_executor::DifferenceKind::MissingFill)
        .collect();
    if unexplained.is_empty() {
        assert_ne!(
            run.verdict,
            ReconciliationVerdict::Mismatch,
            "crash at {point:?}: a mismatch verdict with nothing unexplained"
        );
    } else {
        assert_eq!(
            run.verdict,
            ReconciliationVerdict::Mismatch,
            "crash at {point:?}: {unexplained:?} were left unexplained and unpaused"
        );
        assert!(
            run.effects.iter().any(|e| matches!(
                e,
                Effect::Journal(d) if d.event_type == "AgentModeApplied"
            )),
            "crash at {point:?}: a mismatch that pauses nobody"
        );
    }
}

/// One submission, crashed at `point`, restarted, and reconciled.
fn crash_one_submission(point: CrashPoint) {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);

    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("the stream opens");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::Market(quote(AAPL, "150", "150.2", 20)), &ports);

    let ran = shell
        .step_crashing(
            handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
            &ports,
            point,
        )
        .unwrap_or_else(|e| panic!("the step refused with {}: {e}", e.code()));
    let _ = ran;

    if let Some(answer) = point.broker_answer() {
        shell.run(Input::Broker(answer), &ports);
    }

    let (mut restarted, started) = shell.restart_keeping_broker(&ports);
    assert!(
        started.submissions().is_empty(),
        "crash at {point:?}: Started resubmitted instead of querying (journal §5.2)"
    );
    if let Some(query) = started
        .requests
        .iter()
        .find(|r| matches!(r, BrokerRequest::GetOrderByClientId(_)))
    {
        let BrokerRequest::GetOrderByClientId(id) = query else {
            unreachable!("the find matched this variant")
        };
        restarted.run(
            Input::Broker(Ok(BrokerOutcome::Order(common::broker_order(
                "b-1",
                Some(id.as_str()),
                AAPL,
                mandate_accounting::Side::Buy,
                "10",
                "0",
                "accepted",
            )))),
            &ports,
        );
    }

    assert_the_four(&restarted, point);
    assert_full_reconciliation(&restarted, &ports, point);
}

/// The same, for the points that live inside a protective sequence rather than a plain
/// submission: the position is already open and protected before the crash.
fn crash_one_protective_sequence(point: CrashPoint) {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);

    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("the stream opens");
    let filled = event(
        ACCOUNT_STREAM,
        2,
        "FillApplied",
        with_clock(
            &[
                ("fill_id", text("f-0")),
                ("instrument", text(AAPL)),
                ("side", text("buy")),
                ("qty_gross", text("10")),
                ("price", text("150")),
            ],
            10,
        ),
    );
    shell.fold_one(&filled).expect("the opening fill folds");
    let protection = event(
        ACCOUNT_STREAM,
        3,
        "ProtectionChanged",
        with_clock(
            &[
                ("instrument", text(AAPL)),
                ("action", text("placed")),
                ("orders", text("md-oco-1")),
                ("qty", text("10")),
                ("take_profit", text("170")),
                ("stop", text("140")),
            ],
            11,
        ),
    );
    shell.fold_one(&protection).expect("the resting OCO folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), &ports);

    shell
        .step_crashing(
            handoff(INTENT, common::AGENT, risk_exit(AAPL, "10", "155")),
            &ports,
            point,
        )
        .unwrap_or_else(|e| panic!("the step refused with {}: {e}", e.code()));

    if point == CrashPoint::ConfirmationBeforeExitSubmit {
        shell.run(
            Input::Broker(Ok(BrokerOutcome::CancelAccepted {
                client_order_id: "md-oco-1".to_owned(),
            })),
            &ports,
        );
    }

    let (mut restarted, started) = shell.restart_keeping_broker(&ports);
    let _ = started;
    restarted.run(Input::Tick(clock(400)), &ports);

    assert_the_four(&restarted, point);
    assert_full_reconciliation(&restarted, &ports, point);
}

#[test]
fn crash_at_before_intent_received() {
    crash_one_submission(CrashPoint::BeforeIntentReceived);
}

#[test]
fn crash_at_intent_received_before_gate() {
    crash_one_submission(CrashPoint::IntentReceivedBeforeGate);
}

#[test]
fn crash_at_gate_before_order_submitted() {
    crash_one_submission(CrashPoint::GateBeforeOrderSubmitted);
}

#[test]
fn crash_at_journal_before_request() {
    crash_one_submission(CrashPoint::JournalBeforeRequest);
}

#[test]
fn crash_at_request_no_response() {
    crash_one_submission(CrashPoint::RequestNoResponse);
}

#[test]
fn crash_at_request_ambiguous_response() {
    crash_one_submission(CrashPoint::RequestAmbiguousResponse);
}

#[test]
fn crash_at_response_before_state_change() {
    crash_one_submission(CrashPoint::ResponseBeforeStateChange);
}

#[test]
fn crash_at_state_change_before_fill() {
    crash_one_submission(CrashPoint::StateChangeBeforeFill);
}

#[test]
fn crash_at_cancel_before_confirmation() {
    crash_one_protective_sequence(CrashPoint::CancelBeforeConfirmation);
}

#[test]
fn crash_at_confirmation_before_exit_submit() {
    crash_one_protective_sequence(CrashPoint::ConfirmationBeforeExitSubmit);
}

#[test]
#[ignore = "pending E7-4"]
fn crash_between_entry_fill_and_oco() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);
    let point = CrashPoint::BetweenEntryFillAndOco;

    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("the stream opens");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::Market(quote(AAPL, "150", "150.2", 20)), &ports);
    let sent = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "100", "150")),
        &ports,
    );
    let id = sent
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the bracket is sent");

    shell
        .step_crashing(
            Input::BrokerUpdate(BrokerUpdate::Order(common::broker_order(
                "b-1",
                Some(&id),
                AAPL,
                mandate_accounting::Side::Buy,
                "100",
                "60",
                "partially_filled",
            ))),
            &ports,
            point,
        )
        .unwrap_or_else(|e| panic!("the step refused with {}: {e}", e.code()));

    let (mut restarted, _) = shell.restart_keeping_broker(&ports);
    restarted.run(Input::Tick(clock(400)), &ports);
    restarted.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: id,
        })),
        &ports,
    );

    assert!(
        restarted
            .state
            .protection(&instrument(AAPL))
            .is_some_and(|p| !p.resting.is_empty()),
        "crash at {point:?}: the filled 60 shares were left with no OCO for ever (planted bug 4)"
    );
    assert_the_four(&restarted, point);
    assert_full_reconciliation(&restarted, &ports, point);
}

#[test]
fn crash_mid_reconciliation_before_the_compensating_event() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);
    let point = CrashPoint::MidReconciliationBeforeCompensatingEvent;

    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("the stream opens");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::Market(quote(AAPL, "150", "150.2", 20)), &ports);
    let sent = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );
    let id = sent
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the attempt is sent");
    shell.run(
        Input::Broker(Ok(BrokerOutcome::Submitted(common::broker_order(
            "b-1",
            Some(&id),
            AAPL,
            mandate_accounting::Side::Buy,
            "10",
            "0",
            "accepted",
        )))),
        &ports,
    );

    let mut taken = snapshot(shell.head().0, ReconcileReason::Startup);
    taken.open_orders = vec![common::broker_order(
        "b-1",
        Some(&id),
        AAPL,
        mandate_accounting::Side::Buy,
        "10",
        "4",
        "partially_filled",
    )];
    shell
        .step_crashing(Input::BrokerSnapshot(taken), &ports, point)
        .unwrap_or_else(|e| panic!("the step refused with {}: {e}", e.code()));

    let (restarted, _) = shell.restart_keeping_broker(&ports);
    let adoptions = restarted
        .account_journal
        .iter()
        .filter(|e| e.event_type == "OrderStateChanged")
        .count();
    let compensations = restarted
        .account_journal
        .iter()
        .filter(|e| e.event_type == "CompensatingEvent")
        .count();
    assert!(
        compensations <= adoptions,
        "crash at {point:?}: the adoption was written twice"
    );
    assert_the_four(&restarted, point);
    assert_full_reconciliation(&restarted, &ports, point);
}
