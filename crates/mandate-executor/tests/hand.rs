//! Hand-calculated cases for the idempotent executor and the broker connector abstraction
//! ([task brief](../../../docs/project/tasks/M6-K-executor-and-connector.md), backlog E7-2, E7-3,
//! E7-4).
//!
//! Every case states one clause of the trading-domain, journal, or mandate spec and computes its
//! expectation by hand, so the case is evidence about the spec rather than a restatement of the
//! code. Cases whose subject is the idempotency chain or the order state machine are
//! `pending E7-2`; reconciliation and recovery are `pending E7-3`; protective sequences, exits,
//! and kill switches are `pending E7-4`.

mod common;

use common::{
    ACCOUNT_STREAM, AGENT_STREAM, AppendOutcome, CLOCK_STREAM, CONTROL_STREAM, FixedInstruments,
    FixedMandate, OTHER_AGENT, OTHER_AGENT_STREAM, Shell, TestIds, agent, broker_account,
    broker_fill, broker_order, broker_position, broker_reject, clock, config, copied, derived_id,
    discretionary_exit, event, handoff, instrument, int, object, opening, ports, price, qty, quote,
    risk_exit, scope, snapshot, stale_quote, stream_opened, text, usd, with_clock,
};
use mandate_accounting::Side;
use mandate_executor::{
    AccountState, BrokerOutcome, BrokerRequest, BrokerUnknown, BrokerUpdate, Command, Effect,
    EventId, ExecutorError, ExecutorState, FOLD_VERSION, Initiator, Input, IntentBody, KillScope,
    Mode, OrderState, OwnerConfirmation, Purpose, ReconcileReason, WriterEpoch, fold, handle,
    reconcile,
};

const AAPL: &str = FixedInstruments::LIQUID_EQUITY;
const CPHC: &str = FixedInstruments::THIN_EQUITY;
const BTC: &str = FixedInstruments::CRYPTO;
const FRAC: &str = FixedInstruments::FRACTIONABLE;

/// A started shell with the stream open and one clean reconciliation folded, which is the state
/// almost every case begins from.
fn started() -> Shell {
    Shell::new(1)
}

fn reconciliation_run(seq: u64, at: i64) -> mandate_executor::FoldedEvent {
    event(
        ACCOUNT_STREAM,
        seq,
        "ReconciliationRun",
        with_clock(&[("result", text("clean"))], at),
    )
}

fn owner_confirmation() -> OwnerConfirmation {
    OwnerConfirmation {
        bid: price("155"),
        bid_size: qty("100"),
        floor: price("150.35"),
        user: "user-1".to_owned(),
        step_up: "assertion-1".to_owned(),
    }
}

/// The one id every single-intent case uses, so the reader can follow it across cases.
const INTENT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ0";
const OTHER_INTENT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ1";

#[test]
#[ignore = "pending E7-2"]
fn a_gap_in_seq_fails_the_fold() {
    let mut state = ExecutorState::new(scope());
    fold(&mut state, &stream_opened()).expect("seq 1 folds");
    let gap = reconciliation_run(3, 10);
    let error = fold(&mut state, &gap).expect_err("a gap must fail");
    assert_eq!(error.code(), "sequence_out_of_order", "{error}");
    assert!(
        matches!(
            error,
            ExecutorError::SequenceOutOfOrder {
                expected: 2,
                found: 3,
                ..
            }
        ),
        "the refusal names the seq it wanted and the one it got: {error}"
    );
}

#[test]
#[ignore = "pending E7-2"]
fn an_unknown_event_type_fails_the_fold() {
    let mut state = ExecutorState::new(scope());
    let unknown = event(ACCOUNT_STREAM, 1, "SomethingNobodyWrote", object(&[]));
    let error = fold(&mut state, &unknown).expect_err("an uninterpreted event must fail loudly");
    assert_eq!(error.code(), "not_interpreted", "{error}");
    assert!(
        format!("{error}").contains("SomethingNobodyWrote"),
        "the refusal names what it could not interpret (DEC-85): {error}"
    );
}

#[test]
#[ignore = "pending E7-2"]
fn an_uninterpreted_broker_field_names_its_story() {
    let mut state = ExecutorState::new(scope());
    fold(&mut state, &stream_opened()).expect("seq 1 folds");
    let risk_state = event(
        ACCOUNT_STREAM,
        2,
        "RiskLimitTriggered",
        with_clock(&[("limit", text("daily_loss"))], 10),
    );
    let error = fold(&mut state, &risk_state)
        .expect_err("a risk-state record stream F owns is not interpreted here yet");
    assert_eq!(error.code(), "not_interpreted", "{error}");
    let message = format!("{error}");
    assert!(
        message.contains("E1") || message.contains("E6") || message.contains("E17"),
        "the refusal names the story that owns the value, not only the field: {message}"
    );
}

#[test]
#[ignore = "pending E7-2"]
fn an_event_on_a_stream_the_executor_does_not_follow_is_refused() {
    let mut state = ExecutorState::new(scope());
    fold(&mut state, &stream_opened()).expect("seq 1 folds");
    let foreign = event("acct:ws9:acct-9", 1, "StreamOpened", object(&[]));
    let error = fold(&mut state, &foreign).expect_err("another account's stream must be refused");
    assert_eq!(error.code(), "foreign_stream", "{error}");
}

#[test]
#[ignore = "pending E7-2"]
fn a_copied_agent_mode_points_at_the_originating_event() {
    let mut state = ExecutorState::new(scope());
    fold(&mut state, &stream_opened()).expect("seq 1 folds");
    let origin = EventId(format!("{AGENT_STREAM}-7"));
    let copy = copied(
        ACCOUNT_STREAM,
        2,
        "AgentModeApplied",
        with_clock(
            &[("agent", text(common::AGENT)), ("to", text("exits_only"))],
            10,
        ),
        &origin,
    );
    fold(&mut state, &copy).expect("a copy with its causation folds");
    assert_eq!(
        state.effective_mode(&agent(common::AGENT)),
        Mode::ExitsOnly,
        "the copied mode is what the account stream says it is"
    );
}

#[test]
#[ignore = "pending E7-2"]
fn a_copied_fact_without_a_causation_id_is_refused() {
    let mut state = ExecutorState::new(scope());
    fold(&mut state, &stream_opened()).expect("seq 1 folds");
    let orphan = event(
        ACCOUNT_STREAM,
        2,
        "AgentModeApplied",
        with_clock(
            &[("agent", text(common::AGENT)), ("to", text("paused"))],
            10,
        ),
    );
    let error = fold(&mut state, &orphan).expect_err("a copy must cite its origin (journal §2)");
    assert_eq!(error.code(), "copy_without_causation", "{error}");
}

#[test]
#[ignore = "pending E7-2"]
fn a_risk_input_without_a_risk_clock_is_refused() {
    let mut state = ExecutorState::new(scope());
    fold(&mut state, &stream_opened()).expect("seq 1 folds");
    let bare = event(
        ACCOUNT_STREAM,
        2,
        "MarkUpdated",
        object(&[("instrument", text(AAPL)), ("price", text("150"))]),
    );
    let error = fold(&mut state, &bare)
        .expect_err("every account-stream risk input carries risk_clock (journal §2)");
    assert_eq!(error.code(), "risk_clock_missing", "{error}");
}

#[test]
#[ignore = "pending E7-2"]
fn a_risk_clock_that_goes_backwards_is_refused() {
    let mut state = ExecutorState::new(scope());
    fold(&mut state, &stream_opened()).expect("seq 1 folds");
    let forward = event(
        ACCOUNT_STREAM,
        2,
        "MarkUpdated",
        with_clock(&[("instrument", text(AAPL)), ("price", text("150"))], 200),
    );
    fold(&mut state, &forward).expect("a mark at 200 folds");
    let backward = event(
        ACCOUNT_STREAM,
        3,
        "MarkUpdated",
        with_clock(&[("instrument", text(AAPL)), ("price", text("151"))], 199),
    );
    let error = fold(&mut state, &backward).expect_err("the risk clock never decreases");
    assert_eq!(error.code(), "risk_clock_went_backwards", "{error}");
}

#[test]
#[ignore = "pending E7-2"]
fn an_append_with_another_environment_is_rejected() {
    let mut state = ExecutorState::new(scope());
    fold(&mut state, &stream_opened()).expect("seq 1 folds");
    assert_eq!(
        state.environment(),
        Some("paper"),
        "StreamOpened fixes the environment for good (ES-23)"
    );
    let live = event(
        ACCOUNT_STREAM,
        2,
        "StreamOpened",
        object(&[
            ("environment", text("live")),
            ("stream_type", text("account")),
        ]),
    );
    let error =
        fold(&mut state, &live).expect_err("nothing in this stream may address a live host");
    assert!(
        matches!(
            error.code(),
            "environment_mismatch" | "sequence_out_of_order"
        ),
        "the refusal is about the environment or the repeated StreamOpened, never silence: {error}"
    );
}

#[test]
#[ignore = "pending E7-2"]
fn the_golden_journal_folds_to_the_committed_state() {
    let text = include_str!("golden-journal.json");
    let golden = common::golden::parse_golden(text);
    assert_eq!(
        golden.fold_version, FOLD_VERSION,
        "a change to what the fold derives bumps FOLD_VERSION and regenerates this file (ES-21)"
    );
    let mut state = ExecutorState::new(scope());
    for stored in &golden.events {
        fold(&mut state, stored).unwrap_or_else(|e| {
            panic!(
                "the golden journal must fold: {} refused with {e}",
                stored.event_type
            )
        });
    }
    golden.expected.assert_against(&state);
}

#[test]
#[ignore = "pending E7-2"]
fn a_submission_journals_before_the_request_leaves() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);

    let ran = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );

    let submitted = ran
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Journal(d) if d.event_type == "OrderSubmitted"))
        .expect("the submission is journaled");
    let sent = ran
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Broker(BrokerRequest::Submit(_))))
        .expect("the request is described");
    assert!(
        submitted < sent,
        "OrderSubmitted precedes the request that it names (journal §5.2, AGENTS.md rule 5): {:?}",
        ran.draft_types()
    );
    let draft = ran.draft("OrderSubmitted").expect("the draft is there");
    let named = draft
        .payload
        .get("client_order_id")
        .and_then(mandate_canon::Value::as_str)
        .expect("the draft carries the client order id");
    let submission = ran.submissions().first().copied().expect("one submission");
    assert_eq!(
        named,
        submission.client_order_id.as_str(),
        "the draft names the very id the request carries, not merely some id"
    );
}

#[test]
#[ignore = "pending E7-2"]
fn an_intent_enters_only_as_an_input() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut state = ExecutorState::new(scope());
    let effects = handle(
        &mut state,
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );
    let error = effects.expect_err("nothing may be handled before Started");
    assert_eq!(
        error.code(),
        "not_started",
        "an intent is an input like any other and takes the same discipline: {error}"
    );
}

#[test]
#[ignore = "pending E7-2"]
fn a_re_handed_intent_produces_no_effect_at_all() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);

    let first = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );
    assert!(!first.is_empty(), "the first handoff does something");
    let again = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );
    assert!(
        again.is_empty(),
        "a re-hand of an intent the fold carries costs nothing (interpretation 8): {:?}",
        again.draft_types()
    );
    assert_eq!(
        shell.connector.total_accepted(),
        1,
        "and the broker saw exactly one submission"
    );
}

#[test]
#[ignore = "pending E7-2"]
fn two_processes_derive_one_client_order_id_for_one_intent() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);

    let mut first = Shell::new(1);
    first.fold_one(&stream_opened()).expect("folds");
    let (mut first, _) = first.restart(&ports);
    let one = first.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );

    let mut second = Shell::new(9);
    second.fold_one(&stream_opened()).expect("folds");
    let (mut second, _) = second.restart(&ports);
    let two = second.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );

    let id_of = |ran: &common::Ran| {
        ran.submissions()
            .first()
            .map(|o| o.client_order_id.as_str().to_owned())
            .expect("each process submits once")
    };
    assert_eq!(
        id_of(&one),
        id_of(&two),
        "the id is a pure function of the intent id, so the epoch cannot change it (E7-2)"
    );
}

#[test]
#[ignore = "pending E7-2"]
fn a_new_intent_after_a_restart_derives_a_fresh_client_order_id() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let first = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );

    let (mut shell, _) = shell.restart(&ports);
    let second = shell.run(
        handoff(OTHER_INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );

    let first_id = first
        .submissions()
        .first()
        .map(|o| o.client_order_id.clone());
    let second_id = second
        .submissions()
        .first()
        .map(|o| o.client_order_id.clone());
    assert!(
        first_id.is_some() && second_id.is_some(),
        "both intents reach the broker"
    );
    assert_ne!(
        first_id, second_id,
        "a per-process counter would repeat here; the derivation cannot (planted bug 8)"
    );
}

#[test]
#[ignore = "pending E7-2"]
fn a_resubmission_after_a_crash_reuses_the_same_client_order_id() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let first = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );
    let original = first
        .submissions()
        .first()
        .map(|o| o.client_order_id.clone())
        .expect("the first attempt reaches the broker");

    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);
    let absent = || {
        Input::Broker(Ok(BrokerOutcome::Absent {
            client_order_id: original.as_str().to_owned(),
        }))
    };
    shell.run(absent(), &ports);
    shell.run(Input::Tick(clock(20)), &ports);
    shell.run(absent(), &ports);
    shell.run(Input::Tick(clock(40)), &ports);
    let third = shell.run(absent(), &ports);

    let resubmitted = third
        .submissions()
        .first()
        .map(|o| o.client_order_id.clone())
        .expect("three absences over the window resubmit");
    assert_eq!(
        resubmitted, original,
        "§5.7 requires the resubmission to carry the same id (interpretation 5)"
    );
}

#[test]
#[ignore = "pending E7-2"]
fn an_unacknowledged_submission_queries_before_it_resubmits() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.next_append = AppendOutcome::Committed;
    let submitted = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );
    assert_eq!(
        submitted.submissions().len(),
        1,
        "the first attempt is sent"
    );

    let (restarted, started_effects) = shell.restart_keeping_broker(&ports);

    assert!(
        started_effects
            .requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::GetOrderByClientId(_))),
        "Started queries by client order id (journal §5.2): {:?}",
        started_effects.requests
    );
    assert!(
        started_effects.submissions().is_empty(),
        "and never resubmits blindly, which is the duplicate window R-03 names (planted bug 1)"
    );
    assert_eq!(
        restarted.connector.total_accepted(),
        1,
        "the broker has seen exactly one order for this intent"
    );
}

#[test]
#[ignore = "pending E7-2"]
fn an_order_the_broker_confirms_present_is_adopted_not_resent() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let submitted = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );
    let id = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the attempt is sent");

    let (mut shell, _) = shell.restart_keeping_broker(&ports);
    let found = shell.run(
        Input::Broker(Ok(BrokerOutcome::Order(broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Buy,
            "10",
            "0",
            "accepted",
        )))),
        &ports,
    );

    assert!(
        found.submissions().is_empty(),
        "an order the broker has is adopted, never resent"
    );
    assert!(
        found.draft_types().contains(&"OrderStateChanged"),
        "and the adoption is journaled: {:?}",
        found.draft_types()
    );
    assert_eq!(shell.connector.total_accepted(), 1, "still one order");
}

#[test]
#[ignore = "pending E7-2"]
fn one_absent_lookup_does_not_resubmit() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let submitted = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );
    let id = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the attempt is sent");

    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);
    let once = shell.run(
        Input::Broker(Ok(BrokerOutcome::Absent {
            client_order_id: id,
        })),
        &ports,
    );

    assert!(
        once.submissions().is_empty(),
        "a lagging read replica would otherwise be enough to double an order (interpretation 9)"
    );
    assert_eq!(shell.connector.total_accepted(), 1);
}

#[test]
#[ignore = "pending E7-2"]
fn an_absence_confirmed_over_the_window_resubmits_the_same_id() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let submitted = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );
    let id = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the attempt is sent");
    let absent = || {
        Input::Broker(Ok(BrokerOutcome::Absent {
            client_order_id: id.clone(),
        }))
    };
    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);

    shell.run(absent(), &ports);
    shell.run(Input::Tick(clock(8)), &ports);
    let second = shell.run(absent(), &ports);
    assert!(
        second.submissions().is_empty(),
        "two absences inside the 15-second window are not confirmation"
    );
    shell.run(Input::Tick(clock(16)), &ports);
    let third = shell.run(absent(), &ports);

    assert_eq!(
        third.submissions().len(),
        1,
        "three absences spanning the window confirm it and the order returns to Intent"
    );
    assert_eq!(
        third
            .submissions()
            .first()
            .map(|o| o.client_order_id.as_str()),
        Some(id.as_str()),
        "with the same client order id"
    );
    assert_eq!(
        shell.connector.accepted_for(&id),
        1,
        "and the broker still counts one distinct submission for it"
    );
}

#[test]
#[ignore = "pending E7-2"]
fn a_stale_intent_is_abandoned_rather_than_resubmitted() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let submitted = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );
    let id = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the attempt is sent");
    let absent = || {
        Input::Broker(Ok(BrokerOutcome::Absent {
            client_order_id: id.clone(),
        }))
    };
    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);
    shell.run(absent(), &ports);
    shell.run(Input::Tick(clock(60)), &ports);
    shell.run(absent(), &ports);
    shell.run(Input::Tick(clock(200)), &ports);
    let third = shell.run(absent(), &ports);

    assert!(
        third.submissions().is_empty(),
        "200 seconds is past max_intent_age of 120, so nothing is sent"
    );
    assert!(
        third.draft_types().contains(&"OrderAbandoned"),
        "and the abandonment is journaled: {:?}",
        third.draft_types()
    );
}

#[test]
#[ignore = "pending E7-2"]
fn a_stale_intent_is_abandoned_at_its_first_submission() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::Tick(clock(100)), &ports);

    shell
        .step_crashing(
            handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
            &ports,
            common::CrashPoint::IntentReceivedBeforeGate,
        )
        .expect("the step itself does not refuse");
    assert!(
        shell
            .account_journal
            .iter()
            .any(|e| e.event_type == "IntentReceived"),
        "crash point 2 leaves the intent journaled with no submission behind it"
    );

    shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            shell.head().0.saturating_add(1),
            "ClockAdvanced",
            with_clock(&[], 500),
        ))
        .expect("the outage's clock folds");
    let (shell, resumed) = shell.restart(&ports);

    assert!(
        resumed.submissions().is_empty(),
        "the age check guards **every** Intent to Submitting transition, not only a \
         resubmission (§5.7, interpretation 11, planted bug 19)"
    );
    assert!(
        resumed.draft_types().contains(&"OrderAbandoned"),
        "400 seconds is past max_intent_age of 120, so recovery abandons it: {:?}",
        resumed.draft_types()
    );
    assert_eq!(
        shell.connector.total_accepted(),
        0,
        "and nothing reached the broker"
    );
}

#[test]
#[ignore = "pending E7-2"]
fn an_abandoned_intent_is_never_re_sent() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::Tick(clock(100)), &ports);
    shell
        .step_crashing(
            handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
            &ports,
            common::CrashPoint::IntentReceivedBeforeGate,
        )
        .expect("the step itself does not refuse");
    shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            shell.head().0.saturating_add(1),
            "ClockAdvanced",
            with_clock(&[], 500),
        ))
        .expect("the outage's clock folds");
    let (mut shell, resumed) = shell.restart(&ports);
    assert!(
        resumed.draft_types().contains(&"OrderAbandoned"),
        "recovery abandoned the stale intent: {:?}",
        resumed.draft_types()
    );

    let again = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );

    assert!(
        again.is_empty(),
        "a later handoff of an abandoned intent hits the fold lookup and produces nothing \
         (interpretation 11, planted bug 6): {:?}",
        again.draft_types()
    );
    assert_eq!(shell.connector.total_accepted(), 0);
}

#[test]
#[ignore = "pending E7-2"]
fn a_gate_denial_on_re_check_abandons_the_intent() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let submitted = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );
    let id = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the attempt is sent");
    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);

    let blocked = event(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "AccountRestrictionChanged",
        with_clock(&[("restriction", text("blocked"))], 5),
    );
    shell.fold_one(&blocked).expect("the restriction folds");

    let absent = || {
        Input::Broker(Ok(BrokerOutcome::Absent {
            client_order_id: id.clone(),
        }))
    };
    shell.run(absent(), &ports);
    shell.run(Input::Tick(clock(10)), &ports);
    shell.run(absent(), &ports);
    shell.run(Input::Tick(clock(30)), &ports);
    let third = shell.run(absent(), &ports);

    assert!(
        third.submissions().is_empty(),
        "the gate re-check denies on a blocked account, so nothing is sent"
    );
    assert!(
        third.draft_types().contains(&"OrderAbandoned"),
        "and §5.7's gate-re-check denial abandons it: {:?}",
        third.draft_types()
    );
}

#[test]
#[ignore = "pending E7-2"]
fn a_first_pass_gate_denial_produces_no_order_to_abandon() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let blocked = event(
        ACCOUNT_STREAM,
        2,
        "AccountRestrictionChanged",
        with_clock(&[("restriction", text("blocked"))], 5),
    );
    shell.fold_one(&blocked).expect("the restriction folds");
    let (mut shell, _) = shell.restart(&ports);

    let denied = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );

    assert!(
        denied.draft_types().contains(&"GateDecided"),
        "a deny is journaled with its verdict: {:?}",
        denied.draft_types()
    );
    assert!(
        !denied.draft_types().contains(&"OrderAbandoned"),
        "a first-pass deny never produces an order in the Intent state, so there is nothing to \
         abandon (interpretation 11, review round 2 nit 4)"
    );
    assert!(denied.submissions().is_empty(), "and nothing is sent");
}

#[test]
#[ignore = "pending E7-2"]
fn a_timeout_is_not_a_rejection() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let submitted = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );
    let id = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.clone())
        .expect("the attempt is sent");

    let after = shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);

    let order = shell.state.order(&id).expect("the order is folded");
    assert_eq!(
        order.state,
        OrderState::Unknown,
        "silence is Unknown, never Rejected (interpretation 10, planted bug 11)"
    );
    assert!(
        shell.state.reservations().contains_key(&id),
        "and its reservation is held, not released"
    );
    assert!(
        after.submissions().is_empty(),
        "and nothing is resubmitted while the first order may be live"
    );
}

#[test]
#[ignore = "pending E7-2"]
fn a_duplicate_client_order_id_is_folded_as_already_submitted() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let submitted = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );
    let id = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.clone())
        .expect("the attempt is sent");

    shell.run(
        Input::Broker(Ok(BrokerOutcome::DuplicateClientOrderId {
            client_order_id: id.as_str().to_owned(),
        })),
        &ports,
    );

    let order = shell.state.order(&id).expect("the order is folded");
    assert_ne!(
        order.state,
        OrderState::Rejected,
        "the broker refusing our own id means the order is already there (E7-2 step 6)"
    );
    assert!(
        matches!(order.state, OrderState::Accepted | OrderState::Unknown),
        "so it is adopted or queried, never failed: {:?}",
        order.state
    );
}

#[test]
#[ignore = "pending E7-2"]
fn a_retried_append_derives_the_same_event_id() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let head = shell.head();
    shell.next_append = AppendOutcome::Unresolved;

    let first = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );
    let retried = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );

    let first_ids: Vec<_> = first.drafts.iter().map(|d| d.event_id.clone()).collect();
    let retried_ids: Vec<_> = retried.drafts.iter().map(|d| d.event_id.clone()).collect();
    assert!(!first_ids.is_empty(), "the batch drafted something");
    assert_eq!(
        first_ids, retried_ids,
        "the retry re-derives the same ids, so the append answers AlreadyCommitted (journal §5.1)"
    );
    assert_eq!(
        first_ids.first(),
        Some(&derived_id(shell.epoch, head, 0)),
        "and the derivation is (epoch, head, ordinal), not a fresh ULID"
    );
}

#[test]
#[ignore = "pending E7-2"]
fn a_new_input_is_refused_at_an_unresolved_head() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.next_append = AppendOutcome::Unresolved;
    shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );

    let error = shell
        .step(Input::Tick(clock(50)), &ports)
        .expect_err("no new input may be handled at an unresolved head");

    assert_eq!(error.code(), "append_unresolved", "{error}");
}

#[test]
#[ignore = "pending E7-2"]
fn a_fenced_append_stops_the_executor() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.next_append = AppendOutcome::Fenced;
    shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );
    assert!(shell.stopped, "the shell stops on Fenced");

    let error = shell
        .step(Input::Tick(clock(50)), &ports)
        .expect_err("a fenced writer is a ghost and handles nothing more");

    assert!(
        matches!(error.code(), "fenced" | "append_unresolved"),
        "a newer epoch owns the stream (journal §5.1): {error}"
    );
    assert_eq!(
        shell.connector.total_accepted(),
        0,
        "and nothing reached the broker after the fence"
    );
}

#[test]
#[ignore = "pending E7-2"]
fn a_step_under_another_epoch_is_refused() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);

    let error = shell
        .step(Input::Started(WriterEpoch(99)), &ports)
        .expect_err("a second Started under a different epoch is a bug, not a restart");

    assert!(
        matches!(error.code(), "already_started" | "epoch_mismatch"),
        "{error}"
    );
}

/// Drives one intent to an accepted order and answers with its client order id.
fn accepted_order(shell: &mut Shell, ports: &mandate_executor::Ports<'_>, intent: &str) -> String {
    let submitted = shell.run(
        handoff(intent, common::AGENT, opening(AAPL, "10", "150")),
        ports,
    );
    let id = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the attempt is sent");
    shell.run(
        Input::Broker(Ok(BrokerOutcome::Submitted(broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Buy,
            "10",
            "0",
            "new",
        )))),
        ports,
    );
    id
}

#[test]
#[ignore = "pending E7-2"]
fn every_broker_status_maps_to_the_table_row() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let table = [
        ("new", OrderState::Accepted),
        ("accepted", OrderState::Accepted),
        ("pending_new", OrderState::Accepted),
        ("accepted_for_bidding", OrderState::Accepted),
        ("held", OrderState::Accepted),
        ("pending_cancel", OrderState::PendingCancel),
        ("canceled", OrderState::Canceled),
        ("expired", OrderState::Expired),
        ("rejected", OrderState::Rejected),
        ("suspended", OrderState::Accepted),
        ("pending_replace", OrderState::PendingReplace),
    ];
    for (status, expected) in table {
        let mut shell = started();
        shell.fold_one(&stream_opened()).expect("folds");
        let (mut shell, _) = shell.restart(&ports);
        let id = accepted_order(&mut shell, &ports, INTENT);
        shell.run(
            Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
                "b-1",
                Some(&id),
                AAPL,
                Side::Buy,
                "10",
                "0",
                status,
            ))),
            &ports,
        );
        let parsed = shell
            .state
            .orders()
            .keys()
            .find(|k| k.as_str() == id)
            .cloned()
            .expect("the order is folded");
        let order = shell.state.order(&parsed).expect("the order is folded");
        assert_eq!(
            order.state, expected,
            "§5.7 maps `{status}` to {expected:?}, and the mapping is total"
        );
    }
}

#[test]
#[ignore = "pending E7-2"]
fn the_unchanged_statuses_leave_the_state_alone() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    for status in ["done_for_day", "stopped", "calculated"] {
        let mut shell = started();
        shell.fold_one(&stream_opened()).expect("folds");
        let (mut shell, _) = shell.restart(&ports);
        let id = accepted_order(&mut shell, &ports, INTENT);
        let key = shell
            .state
            .orders()
            .keys()
            .find(|k| k.as_str() == id)
            .cloned()
            .expect("the order is folded");
        let before = shell.state.order(&key).map(|o| o.state);
        shell.run(
            Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
                "b-1",
                Some(&id),
                AAPL,
                Side::Buy,
                "10",
                "0",
                status,
            ))),
            &ports,
        );
        let after = shell.state.order(&key).map(|o| o.state);
        assert_eq!(
            before, after,
            "§5.7's table says `{status}` leaves the internal state unchanged"
        );
    }
}

#[test]
#[ignore = "pending E7-2"]
fn an_unknown_broker_status_pauses_the_agent_and_alerts() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let id = accepted_order(&mut shell, &ports, INTENT);

    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Buy,
            "10",
            "0",
            "quantum_superposition",
        ))),
        &ports,
    );

    let paused = ran
        .drafts
        .iter()
        .filter(|d| d.event_type == "AgentModeApplied")
        .filter_map(|d| d.payload.get("to").and_then(mandate_canon::Value::as_str))
        .any(|to| to == "paused");
    assert!(
        paused,
        "§5.7's last row pauses the agent rather than folding the status as a no-op \
         (planted bug 15): {:?}",
        ran.draft_types()
    );
    assert!(
        !ran.notifications.is_empty(),
        "and alerts the owner: {:?}",
        ran.notifications
    );
}

#[test]
#[ignore = "pending E7-2"]
fn a_suspended_status_flags_restricted_and_triggers_a_reconciliation() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let id = accepted_order(&mut shell, &ports, INTENT);

    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Buy,
            "10",
            "0",
            "suspended",
        ))),
        &ports,
    );

    assert!(
        ran.requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::ListOpenOrders)),
        "§5.7's `suspended` row triggers a reconciliation: {:?}",
        ran.requests
    );
}

#[test]
#[ignore = "pending E7-2"]
fn an_illegal_transition_is_journaled_and_ignored() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let id = accepted_order(&mut shell, &ports, INTENT);
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Buy,
            "10",
            "10",
            "filled",
        ))),
        &ports,
    );
    let key = shell
        .state
        .orders()
        .keys()
        .find(|k| k.as_str() == id)
        .cloned()
        .expect("the order is folded");

    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Buy,
            "10",
            "10",
            "accepted",
        ))),
        &ports,
    );

    assert!(
        !ran.drafts.is_empty(),
        "the illegal transition is journaled rather than dropped"
    );
    assert_eq!(
        shell.state.order(&key).map(|o| o.state),
        Some(OrderState::Filled),
        "terminal states are final (§5.7)"
    );
}

#[test]
#[ignore = "pending E7-2"]
fn a_fill_inside_an_illegal_transition_is_still_applied() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let id = accepted_order(&mut shell, &ports, INTENT);
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Buy,
            "10",
            "10",
            "canceled",
        ))),
        &ports,
    );

    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-1",
            Some(&id),
            "4",
            "150",
        ))),
        &ports,
    );

    assert!(
        ran.draft_types()
            .iter()
            .any(|t| *t == "FillApplied" || *t == "LateFillApplied"),
        "fills are always applied to accounting (§5.7): {:?}",
        ran.draft_types()
    );
}

#[test]
#[ignore = "pending E7-2"]
fn a_fill_after_a_terminal_state_is_applied_as_a_late_fill() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let id = accepted_order(&mut shell, &ports, INTENT);
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Buy,
            "10",
            "10",
            "filled",
        ))),
        &ports,
    );

    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-late",
            Some(&id),
            "1",
            "150",
        ))),
        &ports,
    );

    assert!(
        ran.draft_types().contains(&"LateFillApplied"),
        "a fill for a terminal order is a late fill (§5.7): {:?}",
        ran.draft_types()
    );
}

#[test]
#[ignore = "pending E7-3"]
fn a_late_fill_triggers_a_reconciliation() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let id = accepted_order(&mut shell, &ports, INTENT);
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Buy,
            "10",
            "10",
            "filled",
        ))),
        &ports,
    );

    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-late",
            Some(&id),
            "1",
            "150",
        ))),
        &ports,
    );

    assert!(
        ran.requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::ListOpenOrders)),
        "a late fill schedules another reconciliation (§5.7, interpretation 12): {:?}",
        ran.requests
    );
}

#[test]
#[ignore = "pending E7-2"]
fn a_repeated_fill_id_changes_nothing() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let id = accepted_order(&mut shell, &ports, INTENT);
    let fill = || {
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-1",
            Some(&id),
            "4",
            "150",
        )))
    };
    let first = shell.run(fill(), &ports);
    assert!(!first.is_empty(), "the first fill is applied");
    let position = shell.state.positions().get(&instrument(AAPL)).copied();

    let repeat = shell.run(fill(), &ports);

    assert!(
        repeat.is_empty(),
        "a fill is applied by fill id, not by arrival (planted bug 16): {:?}",
        repeat.draft_types()
    );
    assert_eq!(
        shell.state.positions().get(&instrument(AAPL)).copied(),
        position,
        "and the position is unchanged"
    );
}

#[test]
#[ignore = "pending E7-2"]
fn an_unknown_order_reserves_its_maximum_cost() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let submitted = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );
    let id = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.clone())
        .expect("the attempt is sent");

    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);

    assert_eq!(
        shell.state.reservations().get(&id).copied(),
        Some(usd("1500")),
        "10 shares at a 150 limit is a maximum cost of 1500 (§5.3 rule 9)"
    );
}

#[test]
#[ignore = "pending E7-2"]
fn an_unknown_order_blocks_new_orders_in_the_instrument() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, CPHC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );
    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);

    let blocked = shell.run(
        handoff(OTHER_INTENT, common::AGENT, opening(AAPL, "5", "150")),
        &ports,
    );
    assert!(
        blocked.submissions().is_empty(),
        "§5.3 rule 9 blocks new orders in that instrument until the Unknown resolves"
    );

    let elsewhere = shell.run(
        handoff(
            "01JABCDEFGHJKMNPQRSTVWXYZ2",
            common::AGENT,
            opening(CPHC, "5", "20"),
        ),
        &ports,
    );
    assert_eq!(
        elsewhere.submissions().len(),
        1,
        "and only in that instrument: another one is unaffected"
    );
}

#[test]
#[ignore = "pending E7-2"]
fn an_abandoned_order_releases_its_reservation() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let submitted = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );
    let id = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.clone())
        .expect("the attempt is sent");
    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);
    assert!(
        shell.state.reservations().contains_key(&id),
        "an Unknown order holds its reservation"
    );
    let absent = || {
        Input::Broker(Ok(BrokerOutcome::Absent {
            client_order_id: id.as_str().to_owned(),
        }))
    };
    shell.run(absent(), &ports);
    shell.run(Input::Tick(clock(60)), &ports);
    shell.run(absent(), &ports);
    shell.run(Input::Tick(clock(300)), &ports);
    shell.run(absent(), &ports);

    assert_eq!(
        shell.state.order(&id).map(|o| o.state),
        Some(OrderState::Abandoned),
        "past max_intent_age the order is abandoned"
    );
    assert!(
        !shell.state.reservations().contains_key(&id),
        "and Abandoned releases the reservation, or the account's buying power drains for ever \
         (interpretation 26, planted bug 20)"
    );
}

#[test]
#[ignore = "pending E7-2"]
fn a_replaced_orders_reservation_passes_to_the_new_order() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let id = accepted_order(&mut shell, &ports, INTENT);
    let key = shell
        .state
        .orders()
        .keys()
        .find(|k| k.as_str() == id)
        .cloned()
        .expect("the order is folded");
    let held = shell.state.reservations().get(&key).copied();
    assert!(held.is_some(), "an accepted buy reserves its cost");

    let mut replaced = broker_order("b-1", Some(&id), AAPL, Side::Buy, "10", "0", "replaced");
    replaced.replaced_by_broker_order_id = Some("b-2".to_owned());
    shell.run(Input::BrokerUpdate(BrokerUpdate::Order(replaced)), &ports);

    assert_eq!(
        shell.state.order(&key).map(|o| o.state),
        Some(OrderState::Replaced),
        "the old order is Replaced"
    );
    let total: Vec<_> = shell.state.reservations().values().copied().collect();
    assert_eq!(
        total,
        held.into_iter().collect::<Vec<_>>(),
        "the reservation passes to the linked new order: the pair never double-reserves and never \
         under-reserves (interpretation 26)"
    );
}

#[test]
#[ignore = "pending E7-2"]
fn a_reject_releases_the_reservation_and_is_journaled_with_its_code() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let submitted = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );
    let id = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.clone())
        .expect("the attempt is sent");

    let ran = shell.run(
        Input::Broker(Ok(BrokerOutcome::Rejected(broker_reject(
            Some(id.as_str()),
            422,
            "insufficient buying power",
        )))),
        &ports,
    );

    assert!(
        ran.draft_types().contains(&"RejectObserved"),
        "the reject is journaled with its code (§5.7, §7.3): {:?}",
        ran.draft_types()
    );
    assert!(
        !shell.state.reservations().contains_key(&id),
        "Rejected is terminal and releases the reservation"
    );
}

/// A shell that has started and folded the stream, for reconciliation cases.
fn reconciling(ports: &mandate_executor::Ports<'_>) -> Shell {
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (shell, _) = shell.restart(ports);
    shell
}

#[test]
#[ignore = "pending E7-3"]
fn a_journal_only_order_is_reconciled_away_not_kept() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    let id = accepted_order(&mut shell, &ports, INTENT);

    let mut taken = snapshot(shell.head().0, ReconcileReason::Startup);
    taken.open_orders = Vec::new();
    let run = reconcile(&shell.state, &taken, &ports).expect("the reconciliation runs");

    assert!(
        run.differences.iter().any(|d| d.subject == id && d.adopted),
        "the broker wins on the order set: our own state is never kept because it is ours \
         (planted bug 3): {:?}",
        run.differences
    );
}

#[test]
#[ignore = "pending E7-3"]
fn every_adoption_journals_a_compensating_event_with_the_difference() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    let id = accepted_order(&mut shell, &ports, INTENT);

    let mut taken = snapshot(shell.head().0, ReconcileReason::Startup);
    taken.open_orders = vec![broker_order(
        "b-1",
        Some(&id),
        AAPL,
        Side::Buy,
        "10",
        "4",
        "partially_filled",
    )];
    let run = reconcile(&shell.state, &taken, &ports).expect("the reconciliation runs");

    let types: Vec<&str> = run
        .effects
        .iter()
        .filter_map(|e| match e {
            Effect::Journal(d) => Some(d.event_type.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        types.contains(&"OrderStateChanged") && types.contains(&"CompensatingEvent"),
        "an adoption is `OrderStateChanged` plus a `CompensatingEvent` naming what changed \
         (§11): {types:?}"
    );
    let compensating = run
        .effects
        .iter()
        .find_map(|e| match e {
            Effect::Journal(d) if d.event_type == "CompensatingEvent" => Some(d),
            _ => None,
        })
        .expect("the compensating event is drafted");
    assert!(
        compensating.payload.get("corrected_event_ids").is_some(),
        "carrying the corrected event ids, so a replay reproduces the adoption"
    );
}

#[test]
#[ignore = "pending E7-3"]
fn a_position_difference_is_not_written_away_as_a_compensating_event() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let shell = reconciling(&ports);

    let mut taken = snapshot(shell.head().0, ReconcileReason::Startup);
    taken.positions = vec![broker_position(AAPL, "7")];
    let run = reconcile(&shell.state, &taken, &ports).expect("the reconciliation runs");

    let position_difference = run
        .differences
        .iter()
        .find(|d| d.kind == mandate_executor::DifferenceKind::Position)
        .expect("seven shares the ledger does not have is a difference");
    assert!(
        !position_difference.adopted,
        "§11's on-mismatch column never adopts a position: writing it away would destroy the \
         evidence that they disagreed (interpretation 13, planted bug 18)"
    );
    assert_eq!(
        run.verdict,
        mandate_executor::ReconciliationVerdict::Mismatch,
        "it is a mismatch, not an adoption"
    );
}

#[test]
#[ignore = "pending E7-3"]
fn an_unexplained_position_pauses_the_agent_and_alerts() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    let mut taken = snapshot(shell.head().0, ReconcileReason::Startup);
    taken.positions = vec![broker_position(AAPL, "7")];

    let ran = shell.run(Input::BrokerSnapshot(taken), &ports);

    let paused = ran
        .drafts
        .iter()
        .filter(|d| d.event_type == "AgentModeApplied")
        .filter_map(|d| d.payload.get("to").and_then(mandate_canon::Value::as_str))
        .any(|to| to == "paused");
    assert!(
        paused,
        "an agent must never trade against a position it does not have (planted bug 9): {:?}",
        ran.draft_types()
    );
    assert!(!ran.notifications.is_empty(), "and the owner is alerted");
}

#[test]
#[ignore = "pending E7-3"]
fn a_missing_fill_explains_the_position_and_pauses_nothing() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    let id = accepted_order(&mut shell, &ports, INTENT);

    let mut taken = snapshot(shell.head().0, ReconcileReason::Startup);
    taken.open_orders = vec![broker_order(
        "b-1",
        Some(&id),
        AAPL,
        Side::Buy,
        "10",
        "10",
        "filled",
    )];
    taken.fills = vec![broker_fill("f-1", Some(&id), "10", "150")];
    taken.positions = vec![broker_position(AAPL, "10")];
    let run = reconcile(&shell.state, &taken, &ports).expect("the reconciliation runs");

    assert!(
        run.differences
            .iter()
            .all(|d| d.kind != mandate_executor::DifferenceKind::Position),
        "positions are compared after the fills are ingested, so a difference a fill explains is \
         not a mismatch (interpretation 12, planted bug 10): {:?}",
        run.differences
    );
    assert!(
        run.differences
            .iter()
            .any(|d| d.kind == mandate_executor::DifferenceKind::MissingFill),
        "the fill itself is ingested"
    );
    assert_ne!(
        run.verdict,
        mandate_executor::ReconciliationVerdict::Mismatch,
        "and no healthy agent is paused at a restart"
    );
}

#[test]
#[ignore = "pending E7-3"]
fn an_equity_difference_of_one_share_is_a_mismatch() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    let id = accepted_order(&mut shell, &ports, INTENT);
    let mut taken = snapshot(shell.head().0, ReconcileReason::Startup);
    taken.open_orders = vec![broker_order(
        "b-1",
        Some(&id),
        AAPL,
        Side::Buy,
        "10",
        "10",
        "filled",
    )];
    taken.fills = vec![broker_fill("f-1", Some(&id), "10", "150")];
    taken.positions = vec![broker_position(AAPL, "11")];

    let run = reconcile(&shell.state, &taken, &ports).expect("the reconciliation runs");

    assert_eq!(
        run.verdict,
        mandate_executor::ReconciliationVerdict::Mismatch,
        "an equity position is exact except under a pending corporate action (§11)"
    );
}

#[test]
#[ignore = "pending E7-3"]
fn a_pending_corporate_action_difference_is_not_a_mismatch() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    let prepared = event(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "CorporateActionPrepared",
        with_clock(
            &[
                ("instrument", text(AAPL)),
                ("action", text("split")),
                ("ratio", text("4")),
            ],
            50,
        ),
    );
    shell
        .fold_one(&prepared)
        .expect("the prepared action folds");
    let mut taken = snapshot(shell.head().0, ReconcileReason::Startup);
    taken.positions = vec![broker_position(AAPL, "40")];

    let run = reconcile(&shell.state, &taken, &ports).expect("the reconciliation runs");

    assert_ne!(
        run.verdict,
        mandate_executor::ReconciliationVerdict::Mismatch,
        "§11's tolerance column excepts an instrument under `pending_corporate_action` (§8.5)"
    );
}

#[test]
#[ignore = "pending E7-3"]
fn unposted_crypto_asset_fees_explain_the_crypto_difference() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    let charged = event(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "FeesCharged",
        with_clock(
            &[
                ("family", text("crypto_asset")),
                ("accrued", text("0.001")),
                ("charged", text("0")),
                ("instrument", text(BTC)),
            ],
            50,
        ),
    );
    shell.fold_one(&charged).expect("the accrual folds");
    let mut taken = snapshot(shell.head().0, ReconcileReason::Startup);
    taken.positions = vec![broker_position(BTC, "-0.001")];

    let run = reconcile(&shell.state, &taken, &ports).expect("the reconciliation runs");

    assert_ne!(
        run.verdict,
        mandate_executor::ReconciliationVerdict::Mismatch,
        "crypto is model net plus unposted asset fees until posting (§11)"
    );
}

#[test]
#[ignore = "pending E7-3"]
fn a_fee_difference_is_alerted_and_never_adjusted() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let shell = reconciling(&ports);
    let mut taken = snapshot(shell.head().0, ReconcileReason::FeePosting);
    taken.account = mandate_executor::BrokerAccount {
        accrued_fees: usd("3.5"),
        ..broker_account()
    };

    let run = reconcile(&shell.state, &taken, &ports).expect("the reconciliation runs");

    let fee = run
        .differences
        .iter()
        .find(|d| d.kind == mandate_executor::DifferenceKind::Fee)
        .expect("3.5 of accrued fees the ledger does not have is a difference");
    assert!(
        !fee.adopted,
        "§11 says a fee difference is alerted and never silently adjusted"
    );
    assert!(
        run.effects.iter().any(|e| matches!(e, Effect::Notify(_))),
        "and the owner is alerted"
    );
}

#[test]
#[ignore = "pending E7-3"]
fn an_unknown_broker_order_becomes_external_activity() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let shell = reconciling(&ports);
    let mut taken = snapshot(shell.head().0, ReconcileReason::Startup);
    taken.open_orders = vec![broker_order(
        "b-9",
        Some("somebody-elses-order"),
        AAPL,
        Side::Buy,
        "5",
        "0",
        "new",
    )];

    let run = reconcile(&shell.state, &taken, &ports).expect("the reconciliation runs");

    let types: Vec<&str> = run
        .effects
        .iter()
        .filter_map(|e| match e {
            Effect::Journal(d) => Some(d.event_type.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        types.contains(&"ExternalActivityIngested"),
        "an id we could not have derived is external (§7.1, interpretation 16): {types:?}"
    );
}

#[test]
#[ignore = "pending E7-3"]
fn external_activity_switches_every_agent_to_exits_only() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    let other_mode = copied(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "AgentModeApplied",
        with_clock(&[("agent", text(OTHER_AGENT)), ("to", text("normal"))], 10),
        &EventId(format!("{OTHER_AGENT_STREAM}-1")),
    );
    shell
        .fold_one(&other_mode)
        .expect("the sibling's mode folds");
    let mut taken = snapshot(shell.head().0, ReconcileReason::Startup);
    taken.open_orders = vec![broker_order(
        "b-9",
        Some("somebody-elses-order"),
        AAPL,
        Side::Buy,
        "5",
        "0",
        "new",
    )];

    let ran = shell.run(Input::BrokerSnapshot(taken), &ports);

    let switched: Vec<&str> = ran
        .drafts
        .iter()
        .filter(|d| d.event_type == "AgentModeApplied")
        .filter_map(|d| {
            d.payload
                .get("agent")
                .and_then(mandate_canon::Value::as_str)
        })
        .collect();
    assert!(
        switched.contains(&common::AGENT) && switched.contains(&OTHER_AGENT),
        "**every** agent on the account goes exits_only until the owner acknowledges (§7.1): \
         {switched:?}"
    );
}

#[test]
#[ignore = "pending E7-3"]
fn a_reject_for_an_unknown_client_order_id_is_not_external_activity_without_a_fill() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);

    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Reject(broker_reject(
            Some("not-one-of-ours"),
            403,
            "forbidden",
        ))),
        &ports,
    );

    assert!(
        !ran.draft_types().contains(&"ExternalActivityIngested"),
        "§7.3's own sentence: such a reject counts toward the 403 threshold only \
         (interpretation 16): {:?}",
        ran.draft_types()
    );
    assert_eq!(
        shell.state.consecutive_403s(),
        1,
        "but it does count toward the threshold"
    );
}

#[test]
#[ignore = "pending E7-3"]
fn the_executor_never_lifts_a_reconciliation_pause_itself() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    let mut bad = snapshot(shell.head().0, ReconcileReason::Startup);
    bad.positions = vec![broker_position(AAPL, "7")];
    shell.run(Input::BrokerSnapshot(bad), &ports);
    assert!(
        shell.state.mismatched().contains(&instrument(AAPL)),
        "the mismatch is recorded"
    );

    shell.run(Input::Tick(clock(10_000)), &ports);
    let agreeing = snapshot(shell.head().0, ReconcileReason::Scheduled);
    shell.run(Input::BrokerSnapshot(agreeing), &ports);

    assert!(
        shell.state.mismatched().contains(&instrument(AAPL)),
        "no tick and no later agreeing reconciliation clears it (§11, interpretation 14)"
    );
    assert_eq!(
        shell.state.effective_mode(&agent(common::AGENT)),
        Mode::Paused,
        "and the agent stays paused"
    );
}

#[test]
#[ignore = "pending E7-3"]
fn only_an_acknowledged_owner_ack_clears_a_mismatch_pause() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    let mut bad = snapshot(shell.head().0, ReconcileReason::Startup);
    bad.positions = vec![broker_position(AAPL, "7")];
    shell.run(Input::BrokerSnapshot(bad), &ports);

    let ack = copied(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "OwnerAcknowledged",
        with_clock(
            &[
                ("subject", text(AAPL)),
                ("user", text("user-1")),
                ("step_up", text("assertion-1")),
            ],
            20_000,
        ),
        &EventId(format!("{CONTROL_STREAM}-4")),
    );
    shell
        .fold_one(&ack)
        .expect("the copied acknowledgment folds");

    assert!(
        !shell.state.mismatched().contains(&instrument(AAPL)),
        "resuming needs an owner acknowledgment with step-up evidence, and only that (§11)"
    );
}

#[test]
#[ignore = "pending E7-3"]
fn a_startup_reconciliation_covers_every_submission_it_reports_on() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    let id = accepted_order(&mut shell, &ports, INTENT);
    let head = shell.head();

    let mut taken = snapshot(head.0, ReconcileReason::Startup);
    taken.open_orders = vec![broker_order(
        "b-1",
        Some(&id),
        AAPL,
        Side::Buy,
        "10",
        "0",
        "accepted",
    )];
    let run = reconcile(&shell.state, &taken, &ports).expect("the reconciliation runs");

    assert_eq!(
        run.expected_head, head,
        "the run is appended at the head its snapshot was taken at (interpretation 15)"
    );
}

#[test]
#[ignore = "pending E7-3"]
fn a_submission_between_the_snapshot_and_the_run_recomputes_the_run() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    let stale_head = shell.head();
    accepted_order(&mut shell, &ports, INTENT);
    assert!(
        shell.head().0 > stale_head.0,
        "a submission landed after the snapshot was taken"
    );

    let taken = snapshot(stale_head.0, ReconcileReason::Startup);
    shell.next_append = AppendOutcome::HeadMismatch;
    let ran = shell.run(Input::BrokerSnapshot(taken), &ports);

    assert!(
        ran.draft_types().contains(&"ReconciliationRun"),
        "the run is drafted at the stale head: {:?}",
        ran.draft_types()
    );
    shell.next_append = AppendOutcome::Committed;
    let again = shell.run(
        Input::BrokerSnapshot(snapshot(shell.head().0, ReconcileReason::Startup)),
        &ports,
    );
    assert!(
        again.draft_types().contains(&"ReconciliationRun"),
        "and is recomputed against a fresh snapshot rather than published as covering something \
         it never saw (planted bug 17): {:?}",
        again.draft_types()
    );
}

#[test]
#[ignore = "pending E7-3"]
fn a_reconciliation_ingests_a_missing_fill_before_it_compares_positions() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    let id = accepted_order(&mut shell, &ports, INTENT);
    let mut taken = snapshot(shell.head().0, ReconcileReason::Startup);
    taken.open_orders = vec![broker_order(
        "b-1",
        Some(&id),
        AAPL,
        Side::Buy,
        "10",
        "10",
        "filled",
    )];
    taken.fills = vec![broker_fill("f-1", Some(&id), "10", "150")];
    taken.positions = vec![broker_position(AAPL, "10")];

    let run = reconcile(&shell.state, &taken, &ports).expect("the reconciliation runs");

    let order_of = |wanted: &str| {
        run.effects.iter().position(|e| match e {
            Effect::Journal(d) => d.event_type == wanted,
            _ => false,
        })
    };
    let fill = order_of("FillApplied").expect("the missing fill is ingested");
    let observed = order_of("BrokerPositionObserved").expect("the position is compared");
    let finished = order_of("ReconciliationRun").expect("the run is last");
    assert!(
        fill < observed && observed < finished,
        "orders, then fills, then positions, then the run (interpretation 12)"
    );
}

/// A position of ten `AAPL` protected by one resting GTC OCO, which is `RC-14`'s initial state.
fn protected_position(ports: &mandate_executor::Ports<'_>) -> Shell {
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
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
    let (shell, _) = shell.restart(ports);
    shell
}

#[test]
#[ignore = "pending E7-4"]
fn an_exit_follows_cancel_confirm_regate_submit_replace() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), &ports);

    let ran = shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "10", "155")),
        &ports,
    );

    let cancel = ran
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Broker(BrokerRequest::Cancel { .. })))
        .expect("the protective order is cancelled first (§5.4)");
    let submit = ran
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Broker(BrokerRequest::Submit(_))));
    assert!(
        submit.is_none_or(|at| at > cancel),
        "and nothing is submitted before the cancel"
    );
    let gate = ran
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Journal(d) if d.event_type == "GateDecided"))
        .expect("the gate is re-run on fresh state");
    assert!(gate > cancel, "the gate re-run follows the cancel");
    assert!(
        ran.draft_types().contains(&"ProtectionChanged"),
        "and the unprotected interval opens with the cancel: {:?}",
        ran.draft_types()
    );
}

#[test]
#[ignore = "pending E7-4"]
fn an_exit_never_submits_before_the_cancel_is_confirmed() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), &ports);
    let ran = shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "10", "155")),
        &ports,
    );
    assert!(
        ran.submissions().is_empty(),
        "an accepted cancel request is not a confirmation (planted bug 5)"
    );

    let after = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );

    assert_eq!(
        after.submissions().len(),
        1,
        "the exit goes only once the broker confirms the cancel"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn an_order_submitted_without_protection_is_marketable() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), &ports);
    shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "10", "155")),
        &ports,
    );

    let after = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );

    let submission = after
        .submissions()
        .first()
        .copied()
        .expect("the exit is submitted");
    let limit = submission.limit_price.expect("a limit is set");
    assert!(
        limit <= price("155"),
        "an order submitted while protection is cancelled is marketable at submission (§5.4): \
         priced at {limit} against a 155 bid"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn a_passive_exit_becomes_a_new_oco_keeping_the_stop() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), &ports);

    shell.run(
        handoff(INTENT, common::AGENT, discretionary_exit(AAPL, "10", "160")),
        &ports,
    );
    let after = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );

    let submission = after
        .submissions()
        .first()
        .copied()
        .expect("a new OCO is submitted");
    let legs = submission
        .oco
        .as_ref()
        .expect("a passive exit above the bid is the take-profit leg of a new OCO (§5.4)");
    assert_eq!(legs.take_profit, price("160"), "at the exit's own price");
    assert_eq!(
        legs.stop,
        price("140"),
        "keeping the existing stop: protection is never removed for a passive exit"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn a_passive_exit_never_leaves_the_position_unprotected() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), &ports);

    shell.run(
        handoff(INTENT, common::AGENT, discretionary_exit(AAPL, "10", "160")),
        &ports,
    );
    shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );

    let open: Vec<_> = shell
        .state
        .unprotected_intervals()
        .iter()
        .filter(|i| i.ended_at.is_none())
        .collect();
    assert!(
        open.is_empty(),
        "the stop is carried into the new OCO, so no interval is opened at all: {open:?}"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn an_add_is_a_new_bracket_not_a_replacement() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "151", "151.1", 20)), &ports);

    let ran = shell.run(
        handoff(
            INTENT,
            common::AGENT,
            IntentBody::Order {
                instrument: instrument(AAPL),
                side: Side::Buy,
                qty: qty("5"),
                limit: price("151"),
                purpose: Purpose::Increase,
            },
        ),
        &ports,
    );

    let submission = ran
        .submissions()
        .first()
        .copied()
        .expect("an add to a protected position goes as a bracket (§5.4)");
    assert!(
        submission.bracket.is_some(),
        "each protected entry is one GTC bracket, and an add is a new bracket"
    );
    assert!(
        !ran.requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::Cancel { .. })),
        "never a replacement of the resting protection"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn a_partly_filled_bracket_becomes_an_oco_for_the_filled_quantity() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let submitted = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "100", "150")),
        &ports,
    );
    let id = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the bracket is sent");
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Buy,
            "100",
            "60",
            "partially_filled",
        ))),
        &ports,
    );

    let timeout = shell.run(Input::Tick(clock(70)), &ports);

    assert!(
        timeout
            .requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::Cancel { .. })),
        "at bracket_partial_fill_timeout the entry remainder is cancelled (§5.4)"
    );
    let after = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: id.clone(),
        })),
        &ports,
    );
    let oco = after
        .submissions()
        .first()
        .copied()
        .and_then(|o| o.oco.clone())
        .expect("a GTC OCO for the filled quantity follows the confirmation (planted bug 4)");
    assert_eq!(oco.qty, qty("60"), "for exactly the 60 shares that filled");
}

#[test]
#[ignore = "pending E7-4"]
fn an_entry_unfinished_at_the_timeout_is_cancelled_then_oco_d() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let submitted = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "100", "150")),
        &ports,
    );
    let id = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the bracket is sent");
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Buy,
            "100",
            "60",
            "partially_filled",
        ))),
        &ports,
    );

    let early = shell.run(Input::Tick(clock(40)), &ports);
    assert!(
        !early
            .requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::Cancel { .. })),
        "40 seconds is inside the 60-second timeout, so the legs are still held"
    );
    let at_timeout = shell.run(Input::Tick(clock(61)), &ports);
    assert!(
        at_timeout
            .requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::Cancel { .. })),
        "and at 61 seconds the remainder is cancelled"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn a_terminal_partly_filled_entry_is_oco_d_at_once() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let submitted = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "100", "150")),
        &ports,
    );
    let id = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the bracket is sent");

    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Buy,
            "100",
            "60",
            "canceled",
        ))),
        &ports,
    );

    let oco = ran
        .submissions()
        .first()
        .copied()
        .and_then(|o| o.oco.clone())
        .expect("a terminal partly filled entry is OCO'd without waiting for the timeout (§5.4)");
    assert_eq!(oco.qty, qty("60"));
}

#[test]
#[ignore = "pending E7-4"]
fn an_unprotected_interval_at_the_limit_cancels_re_places_and_alerts() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), &ports);
    shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "10", "155")),
        &ports,
    );
    shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );

    let bound = shell.run(Input::Tick(clock(100)), &ports);

    assert!(
        bound
            .requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::Cancel { .. })),
        "at max_unprotected_s the unfilled exit is cancelled (§5.4, planted bug 14)"
    );
    assert!(
        !bound.notifications.is_empty(),
        "and the owner is alerted: {:?}",
        bound.notifications
    );
}

#[test]
#[ignore = "pending E7-4"]
fn protection_is_re_placed_at_the_buffer_day() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);

    let day = copied(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "TradingDayStarted",
        with_clock(&[("date", text("2026-12-14"))], 7_000_000),
        &EventId(format!("{CLOCK_STREAM}-9")),
    );
    shell.fold_one(&day).expect("the trading day folds");
    let ran = shell.run(Input::Journal(day), &ports);

    assert!(
        ran.requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::Cancel { .. })),
        "a GTC created 2026-09-22 expires 2026-12-21; five trading days before that is 2026-12-14 \
         (§5.4): {:?}",
        ran.requests
    );
}

#[test]
#[ignore = "pending E7-4"]
fn protection_is_not_re_placed_early() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);

    let day = copied(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "TradingDayStarted",
        with_clock(&[("date", text("2026-11-02"))], 6_000_000),
        &EventId(format!("{CLOCK_STREAM}-8")),
    );
    shell.fold_one(&day).expect("the trading day folds");
    let ran = shell.run(Input::Journal(day), &ports);

    assert!(
        !ran.requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::Cancel { .. })),
        "seven weeks before expiry is not the buffer day: {:?}",
        ran.requests
    );
}

#[test]
#[ignore = "pending E7-4"]
fn a_stop_at_its_trigger_price_without_a_fill_is_watchdogged() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "139", "139.1", 30)), &ports);

    let ran = shell.run(Input::Tick(clock(95)), &ports);

    assert!(
        ran.requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::Cancel { .. })),
        "a sane mark at or below the 140 stop for stop_watchdog_s with no fill cancels it (§5.4)"
    );
    assert!(
        !ran.notifications.is_empty(),
        "and alerts the owner: {:?}",
        ran.notifications
    );
}

#[test]
#[ignore = "pending E7-4"]
fn the_watchdog_exit_is_a_risk_exit_through_the_ladder() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "139", "139.1", 30)), &ports);
    shell.run(Input::Tick(clock(95)), &ports);

    let after = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );

    let submission = after
        .submissions()
        .first()
        .copied()
        .expect("the held quantity exits through the ladder");
    assert_eq!(
        submission.purpose,
        Purpose::RiskExit,
        "as a risk_exit, which no pacing control may deny (§5.4, AGENTS.md rule 13)"
    );
    assert_eq!(
        submission.limit_price,
        Some(price("138.305")),
        "the liquid-equity tier's 0.5% off a 139 reference bid, rounded per §2.1"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn a_crypto_position_carries_one_stop_limit_for_the_whole_position() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::Market(quote(BTC, "60000", "60010", 20)), &ports);
    let submitted = shell.run(
        handoff(INTENT, common::AGENT, opening(BTC, "0.5", "60000")),
        &ports,
    );
    let id = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the crypto entry is sent");

    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&id),
            BTC,
            Side::Buy,
            "0.5",
            "0.5",
            "filled",
        ))),
        &ports,
    );

    let protection = ran
        .submissions()
        .into_iter()
        .find(|o| o.order_type == mandate_executor::OrderType::StopLimit)
        .expect("one GTC stop-limit for the whole position (§5.4, DEC-36)");
    assert_eq!(protection.qty, qty("0.5"), "for the whole position");
    assert_eq!(
        protection.tif,
        mandate_executor::TimeInForce::Gtc,
        "as a GTC simple order: crypto takes simple orders only"
    );
    assert!(
        protection.oco.is_none() && protection.bracket.is_none(),
        "never an OCO or a bracket"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn a_crypto_add_is_a_limit_ioc_inside_the_sequence() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let held = event(
        ACCOUNT_STREAM,
        2,
        "FillApplied",
        with_clock(
            &[
                ("fill_id", text("f-0")),
                ("instrument", text(BTC)),
                ("side", text("buy")),
                ("qty_gross", text("0.5")),
                ("price", text("60000")),
            ],
            10,
        ),
    );
    shell.fold_one(&held).expect("the crypto position folds");
    let protection = event(
        ACCOUNT_STREAM,
        3,
        "ProtectionChanged",
        with_clock(
            &[
                ("instrument", text(BTC)),
                ("action", text("placed")),
                ("orders", text("md-stop-1")),
                ("qty", text("0.5")),
                ("stop", text("54000")),
            ],
            11,
        ),
    );
    shell.fold_one(&protection).expect("the stop-limit folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::Market(quote(BTC, "60000", "60010", 20)), &ports);

    let ran = shell.run(
        handoff(
            INTENT,
            common::AGENT,
            IntentBody::Order {
                instrument: instrument(BTC),
                side: Side::Buy,
                qty: qty("0.1"),
                limit: price("60000"),
                purpose: Purpose::Increase,
            },
        ),
        &ports,
    );
    assert!(
        ran.requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::Cancel { .. })),
        "the add begins by cancelling the stop-limit (§5.4's crypto sequence)"
    );
    let after = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-stop-1".to_owned(),
        })),
        &ports,
    );
    let add = after
        .submissions()
        .first()
        .copied()
        .expect("then submits the add");
    assert_eq!(
        add.tif,
        mandate_executor::TimeInForce::Ioc,
        "adds to a protected crypto position are limit IOC (§5.4)"
    );
    assert_eq!(add.order_type, mandate_executor::OrderType::Limit);
}

#[test]
#[ignore = "pending E7-4"]
fn a_crypto_stop_limit_is_re_placed_for_the_new_net_quantity() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::Market(quote(BTC, "60000", "60010", 20)), &ports);
    let submitted = shell.run(
        handoff(INTENT, common::AGENT, opening(BTC, "0.5", "60000")),
        &ports,
    );
    let id = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the entry is sent");
    let first = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&id),
            BTC,
            Side::Buy,
            "0.5",
            "0.5",
            "filled",
        ))),
        &ports,
    );
    let stop = first
        .submissions()
        .into_iter()
        .find(|o| o.order_type == mandate_executor::OrderType::StopLimit)
        .expect("the first stop-limit");
    assert_eq!(
        stop.limit_price,
        Some(price("53730")),
        "limit = stop x (1 - crypto_stop_limit_offset): 54000 x 0.995 (DEC-36)"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn a_fractional_position_protects_the_whole_shares_and_discloses_the_fraction() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[FRAC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::Market(quote(FRAC, "20", "20.1", 20)), &ports);
    let submitted = shell.run(
        handoff(INTENT, common::AGENT, opening(FRAC, "10.4", "20")),
        &ports,
    );
    let id = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the fractional entry is sent");

    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&id),
            FRAC,
            Side::Buy,
            "10.4",
            "10.4",
            "filled",
        ))),
        &ports,
    );

    let protective = ran
        .submissions()
        .into_iter()
        .find(|o| o.purpose == Purpose::Protective)
        .expect("the whole-share part is protected (§5.4)");
    assert_eq!(
        protective.qty,
        qty("10"),
        "only the whole shares: a fractional leg is not allowed in an OCO or a bracket (§5.2)"
    );
    let disclosed = ran
        .draft("ProtectionChanged")
        .and_then(|d| d.payload.get("unprotected_fraction"))
        .and_then(mandate_canon::Value::as_str);
    assert_eq!(
        disclosed,
        Some("0.4"),
        "and the unprotected fraction is disclosed rather than hidden"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn the_ladder_prices_from_a_fresh_sane_quote_first() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "150", "150.2", 20)), &ports);
    shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "10", "150")),
        &ports,
    );

    let after = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );

    let submission = after
        .submissions()
        .first()
        .copied()
        .expect("the exit is sent");
    assert_eq!(
        submission.limit_price,
        Some(price("149.25")),
        "150 x (1 - 0.005) for the liquid tier, from the fresh sane quote's bid (§5.6, RC-24)"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn the_ladder_falls_back_to_the_last_sane_bid_then_the_last_trade() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "150", "150.2", 20)), &ports);
    shell.run(Input::Market(stale_quote(AAPL, "148", 200)), &ports);
    shell.run(Input::Tick(clock(200)), &ports);
    shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "10", "150")),
        &ports,
    );

    let after = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );

    let submission = after
        .submissions()
        .first()
        .copied()
        .expect("the exit is sent");
    assert_eq!(
        submission.limit_price,
        Some(price("149.25")),
        "the newest quote is not sane, so the last sane bid within five minutes is used (§5.6)"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn the_ladder_steps_only_after_the_interval() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "150", "150.2", 20)), &ports);
    shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "10", "150")),
        &ports,
    );
    shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );

    let early = shell.run(Input::Tick(clock(23)), &ports);
    assert!(
        early.submissions().is_empty(),
        "three seconds is inside exit_step_s of five, so nothing is repriced"
    );
    let stepped = shell.run(Input::Tick(clock(26)), &ports);
    let repriced = stepped
        .submissions()
        .first()
        .copied()
        .expect("after five seconds the ladder steps");
    assert_eq!(
        repriced.limit_price,
        Some(price("148.5")),
        "the offset rises by exit_offset_step to 1%: 150 x 0.99 (§5.6, RC-24)"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn the_ladder_never_prices_below_the_floor() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "150", "150.2", 20)), &ports);
    shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "10", "150")),
        &ports,
    );
    shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );

    let mut last = None;
    let mut alerted = false;
    for at in [26_i64, 32, 38, 44, 50, 56, 62, 68] {
        let stepped = shell.run(Input::Tick(clock(at)), &ports);
        alerted = alerted || !stepped.notifications.is_empty();
        if let Some(order) = stepped.submissions().first() {
            last = order.limit_price;
        }
    }

    assert_eq!(
        last,
        Some(price("145.5")),
        "the offset never exceeds max_exit_offset of 3%: 150 x 0.97 (§5.6)"
    );
    assert!(
        alerted,
        "and at the floor the order rests and the owner is alerted"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn an_agent_kill_switch_cancels_only_that_agents_orders() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    let mine = accepted_order(&mut shell, &ports, INTENT);
    let theirs = event(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "OrderSubmitted",
        with_clock(
            &[
                ("client_order_id", text("md-other-1")),
                ("agent", text(OTHER_AGENT)),
                ("instrument", text(AAPL)),
                ("attempt", int(1)),
            ],
            30,
        ),
    );
    shell.fold_one(&theirs).expect("the sibling's order folds");

    let ran = shell.run(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(agent(common::AGENT)),
            initiator: Initiator::RiskLimit,
            confirmation: None,
        }),
        &ports,
    );

    let cancelled: Vec<String> = ran
        .requests
        .iter()
        .filter_map(|r| match r {
            BrokerRequest::Cancel { client_order_id } => Some(client_order_id.as_str().to_owned()),
            _ => None,
        })
        .collect();
    assert!(
        !cancelled.is_empty(),
        "the switch cancels this agent's working orders by client order id (§5.5)"
    );
    assert!(
        cancelled.contains(&mine),
        "including the one it holds: {cancelled:?}"
    );
    assert!(
        !cancelled.iter().any(|id| id == "md-other-1"),
        "and never another agent's (planted bug 12): {cancelled:?}"
    );
    assert!(
        !ran.requests.iter().any(BrokerRequest::is_account_wide),
        "and never the account-wide endpoints"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn an_agent_kill_switch_sells_exactly_the_sub_ledger_quantity() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), &ports);

    shell.run(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(agent(common::AGENT)),
            initiator: Initiator::Owner,
            confirmation: Some(owner_confirmation()),
        }),
        &ports,
    );
    let after = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );

    let sell = after
        .submissions()
        .into_iter()
        .find(|o| o.side == Side::Sell)
        .expect("the agent's position is sold");
    assert_eq!(
        sell.qty,
        qty("10"),
        "exactly the agent's sub-ledger quantity, never close-position (§5.5)"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn an_account_kill_switch_uses_cancel_all_and_close_position() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), &ports);

    let ran = shell.run(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Account(mandate_executor::AccountRef(common::ACCOUNT.to_owned())),
            initiator: Initiator::Owner,
            confirmation: Some(owner_confirmation()),
        }),
        &ports,
    );

    assert!(
        ran.requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::CancelAll(_))),
        "the account scope is one of the only two that may use cancel-all (§5.5): {:?}",
        ran.requests
    );
}

#[test]
#[ignore = "pending E7-4"]
fn an_account_cancel_all_covers_unknown_orders() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    let submitted = shell.run(
        handoff(OTHER_INTENT, common::AGENT, opening(CPHC, "5", "20")),
        &ports,
    );
    let unknown = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.clone());
    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);
    assert!(
        unknown
            .as_ref()
            .and_then(|id| shell.state.order(id))
            .is_some_and(|o| o.state == OrderState::Unknown),
        "there is an Unknown order on the account"
    );

    let ran = shell.run(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Account(mandate_executor::AccountRef(common::ACCOUNT.to_owned())),
            initiator: Initiator::Owner,
            confirmation: Some(owner_confirmation()),
        }),
        &ports,
    );

    assert!(
        ran.requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::CancelAll(_))),
        "the broker's cancel-all includes Unknown orders, which is why the account scope uses it"
    );
    assert!(
        !ran.requests.iter().any(|r| matches!(
            r,
            BrokerRequest::Cancel { client_order_id } if Some(client_order_id) == unknown.as_ref()
        )),
        "and the executor does not also cancel the Unknown one by id"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn a_kill_switch_applies_the_mode_before_it_cancels() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), &ports);

    let ran = shell.run(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(agent(common::AGENT)),
            initiator: Initiator::Owner,
            confirmation: Some(owner_confirmation()),
        }),
        &ports,
    );

    let mode = ran
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Journal(d) if d.event_type == "AgentModeApplied"))
        .expect("the final mode is journaled");
    let cancel = ran
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Broker(BrokerRequest::Cancel { .. })))
        .expect("then the cancels go");
    assert!(
        mode < cancel,
        "the mode first, so an intent handled in the same batch is gated under the new mode \
         (§5.5, planted bug 13)"
    );
    let to = ran
        .draft("AgentModeApplied")
        .and_then(|d| d.payload.get("to"))
        .and_then(mandate_canon::Value::as_str);
    assert_eq!(to, Some("stopped"), "an owner kill switch is `stopped`");
}

#[test]
#[ignore = "pending E7-4"]
fn an_automated_flatten_defers_equity_sells_to_the_session() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), &ports);

    let ran = shell.run(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(agent(common::AGENT)),
            initiator: Initiator::RiskLimit,
            confirmation: None,
        }),
        &ports,
    );

    assert!(
        ran.submissions().iter().all(|o| o.side != Side::Sell),
        "an automated kill switch sells equities only in the regular session (§5.5)"
    );
    let to = ran
        .draft("AgentModeApplied")
        .and_then(|d| d.payload.get("to"))
        .and_then(mandate_canon::Value::as_str);
    assert_eq!(
        to,
        Some("paused"),
        "a mandate limit is `paused`, not `stopped`"
    );
    assert!(
        ran.draft("KillSwitchActivated")
            .and_then(|d| d.payload.get("deferred"))
            .is_some(),
        "and the deferral is journaled rather than forgotten"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn an_automated_flatten_sells_crypto_at_once() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let held = event(
        ACCOUNT_STREAM,
        2,
        "FillApplied",
        with_clock(
            &[
                ("fill_id", text("f-0")),
                ("instrument", text(BTC)),
                ("side", text("buy")),
                ("qty_gross", text("0.5")),
                ("price", text("60000")),
            ],
            10,
        ),
    );
    shell.fold_one(&held).expect("the crypto position folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::Market(quote(BTC, "60000", "60010", 20)), &ports);

    let ran = shell.run(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(agent(common::AGENT)),
            initiator: Initiator::RiskLimit,
            confirmation: None,
        }),
        &ports,
    );

    let sell = ran
        .submissions()
        .into_iter()
        .find(|o| o.side == Side::Sell)
        .expect("crypto sells go immediately (§5.5)");
    assert_eq!(sell.qty, qty("0.5"), "for the whole sub-ledger quantity");
}

#[test]
#[ignore = "pending E7-4"]
fn an_owner_exit_outside_the_session_prices_from_the_confirmed_bid() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);

    shell.run(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(agent(common::AGENT)),
            initiator: Initiator::Owner,
            confirmation: Some(owner_confirmation()),
        }),
        &ports,
    );
    let after = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );

    let sell = after
        .submissions()
        .into_iter()
        .find(|o| o.side == Side::Sell)
        .expect("the owner's exit goes outside the session on their confirmation");
    let limit = sell.limit_price.expect("priced through the ladder");
    assert_eq!(
        limit,
        price("154.225"),
        "155 x (1 - 0.005) from the confirmed bid, not from a quote nobody saw (§5.5, §5.6)"
    );
    assert!(
        limit >= price("150.35"),
        "and never below the floor `OwnerExitRequested` carried"
    );
    assert!(
        sell.extended_hours,
        "outside the session an equity limit is extended-hours"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn an_unconfirmed_owner_exit_waits_for_the_session() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);

    let ran = shell.run(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(agent(common::AGENT)),
            initiator: Initiator::Owner,
            confirmation: None,
        }),
        &ports,
    );

    assert!(
        ran.submissions().iter().all(|o| o.side != Side::Sell),
        "without the confirmed bid, bid size, and floor, equity sells wait for the session (§5.5)"
    );
    assert!(
        ran.draft_types().contains(&"KillSwitchActivated"),
        "the switch itself is still journaled and the mode still applied: {:?}",
        ran.draft_types()
    );
}

#[test]
#[ignore = "pending E7-4"]
fn a_risk_exit_submits_inside_the_close_window() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    let breach = event(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "ConductBreachDetected",
        with_clock(
            &[
                ("control", text("closing_auction_window")),
                ("agent", text(common::AGENT)),
            ],
            25,
        ),
    );
    shell.fold_one(&breach).expect("the conduct control folds");
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 26)), &ports);

    shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "10", "155")),
        &ports,
    );
    let after = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );

    assert_eq!(
        after.submissions().len(),
        1,
        "risk reduction is never denied by a conduct control (AGENTS.md rule 13)"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn a_protective_order_submits_with_no_buying_power() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let broke = event(
        ACCOUNT_STREAM,
        2,
        "AccountStateObserved",
        with_clock(
            &[
                ("status", text("ACTIVE")),
                ("buying_power", text("0")),
                ("equity", text("0")),
                ("cash", text("0")),
            ],
            10,
        ),
    );
    shell
        .fold_one(&broke)
        .expect("a zero-buying-power account folds");
    let filled = event(
        ACCOUNT_STREAM,
        3,
        "FillApplied",
        with_clock(
            &[
                ("fill_id", text("f-0")),
                ("instrument", text(AAPL)),
                ("side", text("buy")),
                ("qty_gross", text("10")),
                ("price", text("150")),
            ],
            11,
        ),
    );
    shell.fold_one(&filled).expect("the position folds");
    let (mut shell, _) = shell.restart(&ports);

    let ran = shell.run(Input::Tick(clock(20)), &ports);

    assert!(
        ran.submissions()
            .iter()
            .any(|o| o.purpose == Purpose::Protective),
        "a protective order is never denied for buying power (AGENTS.md rule 13): {:?}",
        ran.requests
    );
}

#[test]
#[ignore = "pending E7-4"]
fn an_unknown_order_holds_an_exit_in_that_instrument_alone() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, CPHC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), &ports);
    shell.run(Input::Market(quote(CPHC, "20", "20.1", 20)), &ports);
    shell.run(
        handoff(OTHER_INTENT, common::AGENT, opening(AAPL, "1", "150")),
        &ports,
    );
    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);

    let held = shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "10", "155")),
        &ports,
    );
    assert!(
        held.submissions().is_empty(),
        "an Unknown order in the same instrument is one of the four holds rule 13 names"
    );

    let elsewhere = shell.run(
        handoff(
            "01JABCDEFGHJKMNPQRSTVWXYZ3",
            common::AGENT,
            risk_exit(CPHC, "1", "20"),
        ),
        &ports,
    );
    assert_eq!(
        elsewhere.submissions().len(),
        1,
        "and it holds nothing in another instrument"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn entering_exits_only_cancels_the_working_opening_orders() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    let opening_id = accepted_order(&mut shell, &ports, INTENT);

    let tightening = copied(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "AgentModeApplied",
        with_clock(
            &[("agent", text(common::AGENT)), ("to", text("exits_only"))],
            40,
        ),
        &EventId(format!("{AGENT_STREAM}-11")),
    );
    shell.fold_one(&tightening).expect("the mode folds");
    let ran = shell.run(Input::Journal(tightening), &ports);

    let cancelled: Vec<String> = ran
        .requests
        .iter()
        .filter_map(|r| match r {
            BrokerRequest::Cancel { client_order_id } => Some(client_order_id.as_str().to_owned()),
            _ => None,
        })
        .collect();
    assert!(
        cancelled.contains(&opening_id),
        "mandate spec §5.9's executor half: the working opening orders are cancelled \
         (interpretation 19): {cancelled:?}"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn entering_exits_only_leaves_protective_orders_resting() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    accepted_order(&mut shell, &ports, INTENT);

    let tightening = copied(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "AgentModeApplied",
        with_clock(
            &[("agent", text(common::AGENT)), ("to", text("exits_only"))],
            40,
        ),
        &EventId(format!("{AGENT_STREAM}-11")),
    );
    shell.fold_one(&tightening).expect("the mode folds");
    let ran = shell.run(Input::Journal(tightening), &ports);

    assert!(
        !ran.requests.iter().any(|r| matches!(
            r,
            BrokerRequest::Cancel { client_order_id } if client_order_id.as_str() == "md-oco-1"
        )),
        "resting protective orders stay (§7.4, mandate §5.9): {:?}",
        ran.requests
    );
    assert!(
        shell
            .state
            .protection(&instrument(AAPL))
            .is_some_and(|p| !p.resting.is_empty()),
        "and the fold still carries them"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn a_reducing_sell_cancels_the_resting_opening_buys_first() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    let buy = accepted_order(&mut shell, &ports, INTENT);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 30)), &ports);

    let ran = shell.run(
        handoff(OTHER_INTENT, common::AGENT, risk_exit(AAPL, "10", "155")),
        &ports,
    );

    let cancelled: Vec<String> = ran
        .requests
        .iter()
        .filter_map(|r| match r {
            BrokerRequest::Cancel { client_order_id } => Some(client_order_id.as_str().to_owned()),
            _ => None,
        })
        .collect();
    assert!(
        cancelled.contains(&buy),
        "§5.3 rule 5: the agent's own resting opening buys go first: {cancelled:?}"
    );
    assert!(
        ran.submissions().is_empty(),
        "and the sell waits for the confirmation"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn the_reducing_sell_waits_for_the_cancel_confirmation() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    let buy = accepted_order(&mut shell, &ports, INTENT);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 30)), &ports);
    shell.run(
        handoff(OTHER_INTENT, common::AGENT, risk_exit(AAPL, "10", "155")),
        &ports,
    );
    shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: buy,
        })),
        &ports,
    );

    let after = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );

    assert_eq!(
        after.submissions().len(),
        1,
        "the sell goes once every cancel it waited on is confirmed (§5.3 rule 5, §5.4)"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn a_kill_switch_jumps_a_full_queue() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), &ports);

    let switch = shell.run(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(agent(common::AGENT)),
            initiator: Initiator::Owner,
            confirmation: Some(owner_confirmation()),
        }),
        &ports,
    );
    let queued = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );

    assert!(
        switch.draft_types().contains(&"KillSwitchActivated"),
        "the shell's priority channel hands the switch first (ES-06)"
    );
    assert!(
        queued.submissions().is_empty(),
        "so the ordinary intent behind it is gated under `stopped` (interpretation 30)"
    );
}

#[test]
#[ignore = "pending E7-4"]
fn the_journaled_order_is_the_handling_order() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), &ports);
    shell.run(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(agent(common::AGENT)),
            initiator: Initiator::Owner,
            confirmation: Some(owner_confirmation()),
        }),
        &ports,
    );
    shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );

    let types: Vec<&str> = shell
        .account_journal
        .iter()
        .map(|e| e.event_type.as_str())
        .collect();
    let switch = types
        .iter()
        .position(|t| *t == "KillSwitchActivated")
        .expect("the switch is journaled");
    let received = types
        .iter()
        .position(|t| *t == "IntentReceived")
        .expect("the intent behind it is journaled too");
    assert!(
        switch < received,
        "what gets journaled is the order in which handle saw them, so a replay reproduces the \
         priority (ES-06, interpretation 30): {types:?}"
    );
}

#[test]
#[ignore = "pending E7-3"]
fn a_closing_only_reject_sets_the_account_restricted() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);

    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Reject(broker_reject(
            None,
            403,
            "account is restricted to closing-only transactions",
        ))),
        &ports,
    );

    assert_eq!(
        shell.state.account_state(),
        AccountState::ClosingOnly,
        "§7.3's reject row stores the restriction as account state (interpretation 17)"
    );
    let switched = ran
        .drafts
        .iter()
        .filter(|d| d.event_type == "AgentModeApplied")
        .filter_map(|d| d.payload.get("to").and_then(mandate_canon::Value::as_str))
        .any(|to| to == "exits_only");
    assert!(
        switched,
        "and every agent goes exits_only: {:?}",
        ran.draft_types()
    );
}

#[test]
#[ignore = "pending E7-3"]
fn n_consecutive_403s_without_a_known_cause_set_closing_only() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    let reject =
        || Input::BrokerUpdate(BrokerUpdate::Reject(broker_reject(None, 403, "forbidden")));

    shell.run(reject(), &ports);
    shell.run(reject(), &ports);
    assert_eq!(
        shell.state.account_state(),
        AccountState::Active,
        "two is below the configured threshold of three"
    );
    shell.run(reject(), &ports);

    assert_eq!(
        shell.state.account_state(),
        AccountState::ClosingOnly,
        "three consecutive 403s with no known order-level cause set closing_only (§7.3)"
    );
}

#[test]
#[ignore = "pending E7-3"]
fn an_unknown_client_order_id_reject_counts_only_toward_the_threshold() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);

    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Reject(broker_reject(
            Some("not-one-of-ours"),
            403,
            "forbidden",
        ))),
        &ports,
    );

    assert_eq!(shell.state.consecutive_403s(), 1, "it counts (§7.3)");
    assert!(
        !ran.draft_types().contains(&"ExternalActivityIngested"),
        "and does nothing else without a fill: {:?}",
        ran.draft_types()
    );
}

#[test]
#[ignore = "pending E7-3"]
fn a_blocked_account_status_pauses_every_agent() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);

    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Account(mandate_executor::BrokerAccount {
            trading_blocked: true,
            ..broker_account()
        })),
        &ports,
    );

    assert_eq!(
        shell.state.account_state(),
        AccountState::Blocked,
        "§7.3's first row: `trading_blocked` is the `blocked` account state"
    );
    let paused = ran
        .drafts
        .iter()
        .filter(|d| d.event_type == "AgentModeApplied")
        .filter_map(|d| d.payload.get("to").and_then(mandate_canon::Value::as_str))
        .any(|to| to == "paused");
    assert!(paused, "and all agents are paused: {:?}", ran.draft_types());
}

#[test]
#[ignore = "pending E7-3"]
fn buying_power_is_the_lower_of_model_and_broker() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);

    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Account(mandate_executor::BrokerAccount {
            cash: usd("20000"),
            buying_power: usd("12000"),
            ..broker_account()
        })),
        &ports,
    );

    assert_eq!(
        shell.state.buying_power(),
        Some(usd("12000")),
        "the gate uses the lower of the two (§7.2, DEC-34, DEC-104)"
    );
}

#[test]
#[ignore = "pending E7-3"]
fn a_reservation_lowers_buying_power_by_its_amount() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Account(mandate_executor::BrokerAccount {
            cash: usd("20000"),
            buying_power: usd("20000"),
            ..broker_account()
        })),
        &ports,
    );
    let before = shell.state.buying_power();

    shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );
    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);

    assert_eq!(before, Some(usd("20000")), "the account starts whole");
    assert_eq!(
        shell.state.buying_power(),
        Some(usd("18500")),
        "the 1500 reservation of an Unknown order lowers it by exactly its amount (DEC-104)"
    );
}

#[test]
#[ignore = "pending E7-3"]
fn a_paper_fill_books_a_simulated_fee_in_the_shadow_ledger() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    let id = accepted_order(&mut shell, &ports, INTENT);

    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(mandate_executor::BrokerFill {
            side: Side::Sell,
            ..broker_fill("f-1", Some(&id), "10", "150")
        })),
        &ports,
    );

    let simulated = ran
        .drafts
        .iter()
        .find(|d| d.event_type == "FeesCharged")
        .and_then(|d| d.payload.get("simulated"))
        .cloned();
    assert_eq!(
        simulated,
        Some(mandate_canon::Value::Bool(true)),
        "Alpaca paper simulates no regulatory fee, so ours is booked `simulated = true` (§10)"
    );
}

#[test]
#[ignore = "pending E7-3"]
fn a_simulated_fee_is_excluded_from_cash_reconciliation_and_included_in_buying_power() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    let simulated = event(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "FeesCharged",
        with_clock(
            &[
                ("family", text("sec_31")),
                ("accrued", text("0.02")),
                ("charged", text("0")),
                ("simulated", mandate_canon::Value::Bool(true)),
            ],
            30,
        ),
    );
    shell.fold_one(&simulated).expect("the simulated fee folds");

    let taken = snapshot(shell.head().0, ReconcileReason::Scheduled);
    let run = reconcile(&shell.state, &taken, &ports).expect("the reconciliation runs");

    assert!(
        run.differences
            .iter()
            .all(|d| d.kind != mandate_executor::DifferenceKind::Cash),
        "a simulated record that reached a cash comparison would fail every paper reconciliation \
         (§10, interpretation 25): {:?}",
        run.differences
    );
    assert!(
        shell
            .state
            .buying_power()
            .is_none_or(|power| power <= usd("20000")),
        "while still counting against buying power, or paper is flatter than live (R-22)"
    );
}

#[test]
#[ignore = "pending E7-2"]
fn an_alert_carries_only_opaque_ids() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    let id = accepted_order(&mut shell, &ports, INTENT);

    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Buy,
            "10",
            "0",
            "quantum_superposition",
        ))),
        &ports,
    );

    let alerts: Vec<_> = ran
        .effects
        .iter()
        .filter_map(|e| match e {
            Effect::Notify(reference) => Some(reference),
            _ => None,
        })
        .collect();
    assert!(!alerts.is_empty(), "an unmapped status alerts the owner");
    for alert in alerts {
        assert!(
            !alert.message_key.contains(AAPL) && !alert.message_key.contains("150"),
            "a notification carries an opaque id and a message key only (AGENTS.md rule 6): {}",
            alert.message_key
        );
    }
}

#[test]
#[ignore = "pending E7-3"]
fn the_account_ref_is_an_opaque_id_not_an_account_number() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);

    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Account(broker_account())),
        &ports,
    );

    let observed = ran
        .draft("AccountStateObserved")
        .expect("the account snapshot is journaled");
    let rendered = format!("{:?}", observed.payload);
    assert!(
        !rendered.contains("account_number") && !rendered.contains("\"id\""),
        "journal §6.4 keeps the broker's account number and id in the vault: {rendered}"
    );
    assert_eq!(
        shell.state.scope().account.0,
        common::ACCOUNT,
        "the stream's subject is the opaque account_ref"
    );
}

#[test]
fn every_error_code_is_stable_and_unique() {
    let codes = ExecutorError::CODES;
    let mut seen = std::collections::BTreeSet::new();
    for code in codes {
        assert!(seen.insert(code), "`{code}` appears twice in the code set");
    }
    let samples = [
        ExecutorError::Unimplemented { story: "E7-2" },
        ExecutorError::NotInterpreted {
            what: "x".to_owned(),
            story: "E7-2",
        },
        ExecutorError::SequenceOutOfOrder {
            stream: "s".to_owned(),
            expected: 1,
            found: 2,
        },
        ExecutorError::ForeignStream {
            stream: "s".to_owned(),
        },
        ExecutorError::EnvironmentMismatch {
            opened: "paper".to_owned(),
            found: "live".to_owned(),
        },
        ExecutorError::CopyWithoutCausation {
            event_type: "t".to_owned(),
        },
        ExecutorError::RiskClockMissing {
            event_type: "t".to_owned(),
        },
        ExecutorError::RiskClockWentBackwards { last: 2, found: 1 },
        ExecutorError::AppendUnresolved { head: 1 },
        ExecutorError::Fenced { owner: 2, found: 1 },
        ExecutorError::EpochMismatch {
            folded: 1,
            found: 2,
        },
        ExecutorError::AlreadyStarted,
        ExecutorError::NotStarted,
        ExecutorError::NonCanonicalPayload {
            field: "f".to_owned(),
        },
        ExecutorError::MalformedClientOrderId {
            raw: "x".to_owned(),
        },
        ExecutorError::UnmappedBrokerStatus {
            status: "x".to_owned(),
        },
        ExecutorError::UnknownOrder {
            client_order_id: "x".to_owned(),
        },
        ExecutorError::SnapshotOvertaken {
            snapshot: 1,
            head: 2,
        },
    ];
    for sample in &samples {
        assert!(
            codes.contains(&sample.code()),
            "`{}` is not in the declared code set (ES-09)",
            sample.code()
        );
        assert!(
            !format!("{sample}").is_empty(),
            "every variant has a message a log can carry"
        );
    }
    assert_eq!(
        codes.len(),
        22,
        "the set is closed: a new variant adds a row here and a match arm in `code()`"
    );
}
