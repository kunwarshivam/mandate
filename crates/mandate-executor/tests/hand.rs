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
    FixedMandate, MISSING_BINDING_GATE, OTHER_AGENT, OTHER_AGENT_STREAM, Shell, TestIds, agent,
    broker_account, broker_fill, broker_order, broker_position, broker_reject, clock, config,
    copied, derived_id, discretionary_exit, event, handoff, instrument, int, object, opening,
    ports, price, protected_opening, qty, quote, risk_exit, scope, snapshot, stale_quote,
    stream_opened, text, usd, with_clock,
};
use mandate_accounting::Side;
use mandate_executor::{
    AccountState, BrokerOutcome, BrokerRequest, BrokerUnknown, BrokerUpdate, Command, Effect,
    EventId, ExecutorError, ExecutorState, FOLD_VERSION, Initiator, Input, KillScope, Mode,
    OrderState, OwnerConfirmation, Purpose, ReconcileReason, WriterEpoch, fold, handle, reconcile,
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

/// Journal §2's gapless `seq` runs both ways: an event already folded is not folded again.
#[test]
fn a_repeated_seq_fails_the_fold() {
    let mut state = ExecutorState::new(scope());
    fold(&mut state, &stream_opened()).expect("seq 1 folds");
    let error = fold(&mut state, &stream_opened()).expect_err("a repeat must fail");
    assert_eq!(error.code(), "sequence_out_of_order", "{error}");
    assert!(
        matches!(
            error,
            ExecutorError::SequenceOutOfOrder {
                expected: 2,
                found: 1,
                ..
            }
        ),
        "the refusal names the seq it wanted and the repeated one it got: {error}"
    );
}

/// Journal §2's gapless `seq` runs both ways: a followed stream cannot be rewound either.
#[test]
fn a_rewound_seq_fails_the_fold() {
    let mut state = ExecutorState::new(scope());
    fold(
        &mut state,
        &event(CLOCK_STREAM, 1, "ClockAdvanced", object(&[])),
    )
    .expect("seq 1 folds");
    fold(
        &mut state,
        &event(CLOCK_STREAM, 2, "ClockAdvanced", object(&[])),
    )
    .expect("seq 2 folds");
    let rewound = event(CLOCK_STREAM, 1, "ClockAdvanced", object(&[]));
    let error = fold(&mut state, &rewound).expect_err("a rewind must fail");
    assert_eq!(error.code(), "sequence_out_of_order", "{error}");
    assert!(
        matches!(
            error,
            ExecutorError::SequenceOutOfOrder {
                expected: 3,
                found: 1,
                ..
            }
        ),
        "the refusal names the seq it wanted and the earlier one it got: {error}"
    );
}

#[test]
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
fn an_event_on_a_stream_the_executor_does_not_follow_is_refused() {
    let mut state = ExecutorState::new(scope());
    fold(&mut state, &stream_opened()).expect("seq 1 folds");
    let foreign = event("acct:ws9:acct-9", 1, "StreamOpened", object(&[]));
    let error = fold(&mut state, &foreign).expect_err("another account's stream must be refused");
    assert_eq!(error.code(), "foreign_stream", "{error}");
}

#[test]
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
fn a_submission_journals_before_the_request_leaves() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);

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
fn an_opening_without_protective_prices_is_gated_and_sent_as_a_plain_order() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);

    let ran = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );

    let decided = ran
        .draft("GateDecided")
        .and_then(|d| d.payload.get("verdict"))
        .and_then(mandate_canon::Value::as_str);
    assert_eq!(decided, Some("allow"), "it is gated like any other intent");
    let submissions = ran.submissions();
    assert_eq!(submissions.len(), 1, "and sent once");
    let order = submissions.first().copied().expect("one submission");
    assert_eq!(order.order_type, mandate_executor::OrderType::Limit);
    assert_eq!(order.limit_price, Some(price("150")));
    assert!(
        order.bracket.is_none() && order.oco.is_none(),
        "an intent without protective prices is a plain limit order: the executor places the \
         prices the order builder computed and never invents a bracket (whether protection is \
         required is the mandate's `protection_required`, which the gate reads, DEC-133)"
    );
}

#[test]
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
        &MISSING_BINDING_GATE,
    );
    let error = effects.expect_err("nothing may be handled before Started");
    assert_eq!(
        error.code(),
        "not_started",
        "an intent is an input like any other and takes the same discipline: {error}"
    );
}

#[test]
fn a_re_handed_intent_produces_no_effect_at_all() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);

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
fn two_processes_derive_one_client_order_id_for_one_intent() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, CPHC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);

    let mut first = Shell::new(1);
    first.fold_one(&stream_opened()).expect("folds");
    let mut first = first.restart_ready(&ports);
    let one = first.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );

    let mut second = Shell::new(9);
    second.fold_one(&stream_opened()).expect("folds");
    let mut second = second.restart_ready(&ports);
    let earlier = second.run(
        handoff(OTHER_INTENT, common::AGENT, opening(CPHC, "5", "20")),
        &ports,
    );
    let two = second.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );

    let id_of = |ran: &common::Ran| {
        ran.submissions()
            .first()
            .map(|o| o.client_order_id.as_str().to_owned())
            .expect("each intent is submitted once")
    };
    assert_ne!(
        id_of(&earlier),
        id_of(&two),
        "two intents in one process never share an id"
    );
    assert_eq!(
        id_of(&one),
        id_of(&two),
        "the id is a pure function of the intent id: another epoch, and another intent handled \
         first, change nothing — which a per-process counter or a fresh ULID could not satisfy \
         (E7-2, planted bug 8)"
    );
}

#[test]
fn a_new_intent_after_a_restart_derives_a_fresh_client_order_id() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
    let first = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );

    let mut shell = shell.restart_ready(&ports);
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
fn a_resubmission_after_a_crash_reuses_the_same_client_order_id() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
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
fn an_unacknowledged_submission_queries_before_it_resubmits() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
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
fn an_order_the_broker_confirms_present_is_adopted_not_resent() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
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
fn one_absent_lookup_does_not_resubmit() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
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
fn an_absence_confirmed_over_the_window_resubmits_the_same_id() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
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
    let attempt = |ran: &common::Ran| {
        ran.draft("OrderSubmitted")
            .and_then(|draft| draft.payload.get("attempt"))
            .and_then(mandate_canon::Value::as_int)
    };
    let prior = attempt(&submitted).expect("the first submission names its attempt");
    assert_eq!(
        attempt(&third),
        Some(prior.saturating_add(1)),
        "under the next attempt number (§5.7)"
    );
    assert_eq!(
        shell.connector.accepted_for(&id),
        1,
        "and the broker still counts one distinct submission for it"
    );
}

#[test]
fn a_stale_intent_is_abandoned_rather_than_resubmitted() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
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
fn a_stale_intent_is_abandoned_at_its_first_submission() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
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
        .fold_one(&copied(
            ACCOUNT_STREAM,
            shell.head().0.saturating_add(1),
            "ClockAdvanced",
            with_clock(&[], 500),
            &EventId(format!("{CLOCK_STREAM}-1")),
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
fn an_abandoned_intent_is_never_re_sent() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
    shell.run(Input::Tick(clock(100)), &ports);
    shell
        .step_crashing(
            handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
            &ports,
            common::CrashPoint::IntentReceivedBeforeGate,
        )
        .expect("the step itself does not refuse");
    shell
        .fold_one(&copied(
            ACCOUNT_STREAM,
            shell.head().0.saturating_add(1),
            "ClockAdvanced",
            with_clock(&[], 500),
            &EventId(format!("{CLOCK_STREAM}-1")),
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
fn a_gate_denial_on_re_check_abandons_the_intent() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
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
fn a_timeout_is_not_a_rejection() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
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
fn a_duplicate_client_order_id_is_folded_as_already_submitted() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
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
fn a_retried_append_derives_the_same_event_id() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
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
fn a_new_input_is_refused_at_an_unresolved_head() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
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
fn a_fenced_append_stops_the_executor() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
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
fn a_step_under_another_epoch_is_refused() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);

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
        let mut shell = shell.restart_ready(&ports);
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
fn a_described_rejection_records_the_broker_status_and_reject_code() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
    let id = accepted_order(&mut shell, &ports, INTENT);
    let mut rejected = broker_order("b-1", Some(&id), AAPL, Side::Buy, "10", "0", "rejected");
    rejected.reject_code = Some("insufficient_buying_power".to_owned());

    let ran = shell.run(Input::BrokerUpdate(BrokerUpdate::Order(rejected)), &ports);
    let changed = ran
        .draft("OrderStateChanged")
        .unwrap_or_else(|| panic!("rejected state change"));
    assert_eq!(
        changed
            .payload
            .get("broker_status")
            .and_then(mandate_canon::Value::as_str),
        Some("rejected")
    );
    assert_eq!(
        changed
            .payload
            .get("reject_code")
            .and_then(mandate_canon::Value::as_str),
        Some("insufficient_buying_power")
    );
}

#[test]
fn the_unchanged_statuses_leave_the_state_alone() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    for status in ["done_for_day", "stopped", "calculated"] {
        let mut shell = started();
        shell.fold_one(&stream_opened()).expect("folds");
        let mut shell = shell.restart_ready(&ports);
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
fn an_unknown_broker_status_pauses_the_agent_and_alerts() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
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
fn a_suspended_status_flags_restricted_and_triggers_a_reconciliation() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
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
fn an_illegal_transition_is_journaled_and_ignored() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
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
fn a_fill_inside_an_illegal_transition_is_still_applied() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
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
fn a_fill_after_a_terminal_state_is_applied_as_a_late_fill() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
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
fn a_late_fill_triggers_a_reconciliation() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
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
fn a_repeated_fill_id_changes_nothing() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
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
fn an_unknown_order_reserves_its_maximum_cost() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
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
fn an_unknown_order_blocks_new_orders_in_the_instrument() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, CPHC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
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
fn an_abandoned_order_releases_its_reservation() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
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
fn a_replaced_orders_reservation_passes_to_the_new_order() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
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
    let ran = shell.run(Input::BrokerUpdate(BrokerUpdate::Order(replaced)), &ports);

    assert_eq!(
        shell.state.order(&key).map(|o| o.state),
        Some(OrderState::Replaced),
        "the old order is Replaced"
    );
    assert_eq!(
        ran.draft("OrderStateChanged")
            .and_then(|draft| draft.payload.get("replaced_by_broker_order_id"))
            .and_then(mandate_canon::Value::as_str),
        Some("b-2"),
        "the old order preserves the broker's replacement link"
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
fn a_reject_releases_the_reservation_and_is_journaled_with_its_code() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
    let submitted = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );
    let id = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.clone())
        .expect("the attempt is sent");

    let mut reject = broker_reject(Some(id.as_str()), 422, "insufficient buying power");
    reject.code = Some("insufficient_buying_power".to_owned());
    let ran = shell.run(Input::Broker(Ok(BrokerOutcome::Rejected(reject))), &ports);

    assert!(
        ran.draft_types().contains(&"RejectObserved"),
        "the reject is journaled with its code (§5.7, §7.3): {:?}",
        ran.draft_types()
    );
    assert!(
        !shell.state.reservations().contains_key(&id),
        "Rejected is terminal and releases the reservation"
    );
    assert_eq!(
        ran.draft("OrderStateChanged")
            .and_then(|draft| draft.payload.get("reject_code"))
            .and_then(mandate_canon::Value::as_str),
        Some("insufficient_buying_power"),
        "the state transition preserves the broker reject code"
    );
}

/// A shell that has started and folded the stream, for reconciliation cases.
fn reconciling(ports: &mandate_executor::Ports<'_>) -> Shell {
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    shell.restart_ready(ports)
}

#[test]
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
        run.differences
            .iter()
            .any(|d| d.subject == id && d.adopted()),
        "the broker wins on the order set: our own state is never kept because it is ours \
         (planted bug 3): {:?}",
        run.differences
    );
}

#[test]
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

    let types = journaled(&run.effects);
    assert!(
        !types.contains(&"CompensatingEvent"),
        "§11's on-mismatch column never adopts a position: nothing in the order set differs, so \
         nothing may be compensated, and a compensating event here would make the ledger agree \
         with the broker while destroying the evidence that they disagreed (interpretation 13, \
         planted bug 18): {types:?}"
    );
    let observed = run
        .effects
        .iter()
        .find_map(|e| match e {
            Effect::Journal(d) if d.event_type == "BrokerPositionObserved" => Some(d),
            _ => None,
        })
        .expect("the difference is recorded as observed, not written away");
    assert_eq!(
        observed
            .payload
            .get("instrument")
            .and_then(mandate_canon::Value::as_str),
        Some(AAPL)
    );
    assert!(
        format!("{:?}", observed.payload).contains("Str(\"7\")"),
        "with the broker's own seven shares on the record: {:?}",
        observed.payload
    );
    assert!(
        paused_anyone(&run.effects),
        "and the difference pauses rather than resolves: {types:?}"
    );
    assert_eq!(
        run.verdict,
        mandate_executor::ReconciliationVerdict::Mismatch,
        "it is a mismatch, not an adoption"
    );
}

#[test]
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
    taken.account.cash = usd("18500");
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
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let bought = event(
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
            0,
        ),
    );
    shell.fold_one(&bought).expect("the position folds");
    let mut shell = shell.restart_ready(&ports);
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
    let applied = event(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "CorporateActionApplied",
        with_clock(
            &[
                ("instrument", text(AAPL)),
                ("action", text("split")),
                ("ratio", text("4")),
            ],
            51,
        ),
    );
    shell.fold_one(&applied).expect("the applied action folds");
    let mut taken = snapshot(shell.head().0, ReconcileReason::Startup);
    taken.positions = vec![broker_position(AAPL, "10")];

    let run = reconcile(&shell.state, &taken, &ports).expect("the reconciliation runs");

    let types = journaled(&run.effects);
    assert!(
        types.contains(&"BrokerPositionObserved") && types.last() == Some(&"ReconciliationRun"),
        "the position is still compared and recorded, and the run closes the batch: {types:?}"
    );
    assert!(
        run.differences
            .iter()
            .all(|d| d.kind != mandate_executor::DifferenceKind::Position),
        "§11's tolerance column excepts an instrument under `pending_corporate_action` (§8.5): \
         {:?}",
        run.differences
    );
    assert_ne!(
        run.verdict,
        mandate_executor::ReconciliationVerdict::Mismatch
    );
    assert!(
        !paused_anyone(&run.effects),
        "and nobody is paused for a difference the split explains"
    );
}

/// The event types a list of effects journals, in order.
fn journaled(effects: &[Effect]) -> Vec<&str> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Journal(d) => Some(d.event_type.as_str()),
            _ => None,
        })
        .collect()
}

/// Whether a list of effects pauses any agent.
fn paused_anyone(effects: &[Effect]) -> bool {
    effects.iter().any(|e| {
        matches!(
            e,
            Effect::Journal(d) if d.event_type == "AgentModeApplied"
                && d.payload.get("to").and_then(mandate_canon::Value::as_str) == Some("paused")
        )
    })
}

#[test]
fn unposted_crypto_asset_fees_explain_the_crypto_difference() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    let bought = event(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "FillApplied",
        with_clock(
            &[
                ("fill_id", text("f-btc")),
                ("instrument", text(BTC)),
                ("side", text("buy")),
                ("qty_gross", text("0.5")),
                ("price", text("60000")),
            ],
            40,
        ),
    );
    shell.fold_one(&bought).expect("the crypto buy folds");
    let charged = event(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "FeesCharged",
        with_clock(
            &[
                ("family", text("crypto_asset")),
                ("accrued", text("0.00125")),
                ("charged", text("0")),
                ("instrument", text(BTC)),
            ],
            50,
        ),
    );
    shell.fold_one(&charged).expect("the accrual folds");

    let mut taken = snapshot(shell.head().0, ReconcileReason::Startup);
    taken.positions = vec![broker_position(BTC, "0.5")];
    taken.account.cash = usd("-10000");
    let run = reconcile(&shell.state, &taken, &ports).expect("the reconciliation runs");
    let observed = run
        .effects
        .iter()
        .find_map(|e| match e {
            Effect::Journal(d) if d.event_type == "BrokerPositionObserved" => Some(d),
            _ => None,
        })
        .expect("the crypto position is compared and recorded");
    assert_eq!(
        observed
            .payload
            .get("instrument")
            .and_then(mandate_canon::Value::as_str),
        Some(BTC)
    );
    assert_ne!(
        run.verdict,
        mandate_executor::ReconciliationVerdict::Mismatch,
        "0.5 bought gross at 25 bps taker is a model net of 0.49875 and an asset fee of 0.00125 \
         that Alpaca has not posted; until it does, the broker shows net + unposted = 0.5, which \
         is RC-07's own `broker_qty_before_fee_posting` (§6.3, §11)"
    );

    let mut off = snapshot(shell.head().0, ReconcileReason::Startup);
    off.positions = vec![broker_position(BTC, "0.49")];
    let run = reconcile(&shell.state, &off, &ports).expect("the reconciliation runs");
    assert_eq!(
        run.verdict,
        mandate_executor::ReconciliationVerdict::Mismatch,
        "and the unposted fee explains exactly its own amount, not any difference"
    );
}

#[test]
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
        !fee.adopted(),
        "§11 says a fee difference is alerted and never silently adjusted"
    );
    assert!(
        run.effects.iter().any(|e| matches!(e, Effect::Notify(_))),
        "and the owner is alerted"
    );
}

#[test]
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
fn external_activity_switches_every_agent_to_exits_only() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    for (who, stream) in [
        (common::AGENT, AGENT_STREAM),
        (OTHER_AGENT, OTHER_AGENT_STREAM),
    ] {
        let mode = copied(
            ACCOUNT_STREAM,
            shell.head().0.saturating_add(1),
            "AgentModeApplied",
            with_clock(&[("agent", text(who)), ("to", text("normal"))], 10),
            &EventId(format!("{stream}-1")),
        );
        shell.fold_one(&mode).expect("each agent's mode folds");
        assert_eq!(shell.state.effective_mode(&agent(who)), Mode::Normal);
    }
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

    assert!(
        ran.draft_types().contains(&"ExternalActivityIngested"),
        "the foreign order is ingested: {:?}",
        ran.draft_types()
    );
    for who in [common::AGENT, OTHER_AGENT] {
        assert_eq!(
            shell.state.effective_mode(&agent(who)),
            Mode::ExitsOnly,
            "**every** agent on the account goes exits_only until the owner acknowledges (§7.1): \
             {who}"
        );
    }
    assert_eq!(
        shell.state.effective_mode(&agent("agent-deployed-later")),
        Mode::ExitsOnly,
        "and so does an agent the executor has never seen: the restriction is the account's, so \
         an agent deployed inside the unacknowledged window is covered too"
    );
}

#[test]
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
    assert!(
        shell.state.mismatched().contains(&instrument(AAPL)),
        "the seven unexplained shares are recorded as a mismatch before anything else is tried"
    );

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
    let stale = reconcile(&shell.state, &taken, &ports).expect("the reconciliation runs");
    assert_eq!(
        stale.expected_head,
        stale_head,
        "the run is appended at the head its snapshot was taken at, not the current head {:?}, \
         so the append answers HeadMismatch rather than positioning the run after a submission \
         it never saw (interpretation 15, planted bug 17)",
        shell.head()
    );
    assert!(
        journaled(&stale.effects).contains(&"ReconciliationRun"),
        "the run is drafted at the stale head"
    );

    let runs = |shell: &Shell| {
        shell
            .account_journal
            .iter()
            .filter(|e| e.event_type == "ReconciliationRun")
            .count()
    };
    let runs_before = runs(&shell);
    shell.next_append = AppendOutcome::HeadMismatch;
    let refused = shell.run(Input::BrokerSnapshot(taken), &ports);
    let stale_drafts: Vec<_> = stale
        .effects
        .iter()
        .filter_map(|e| match e {
            Effect::Journal(d) => Some(d.clone()),
            _ => None,
        })
        .collect();
    assert!(
        !refused.drafts.is_empty() && stale_drafts.starts_with(&refused.drafts),
        "the stale batch is offered to the journal, draft for draft, every field equal: {:?}",
        refused.draft_types()
    );
    assert_eq!(runs(&shell), runs_before, "and the journal refused it");
    shell.next_append = AppendOutcome::Committed;
    let fresh_head = shell.head();
    let again = shell.run(
        Input::BrokerSnapshot(snapshot(fresh_head.0, ReconcileReason::Startup)),
        &ports,
    );
    assert!(
        again.draft_types().contains(&"ReconciliationRun"),
        "and it is recomputed against a fresh snapshot rather than published as covering \
         something it never saw: {:?}",
        again.draft_types()
    );
    assert!(
        shell
            .state
            .reconciled_through()
            .is_some_and(|at| at.0 > fresh_head.0),
        "and only the fresh run is positioned in the stream"
    );
}

#[test]
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

/// The events of `RC-14`'s initial state from `first_seq` on: ten `AAPL` bought at 150 by
/// `common::AGENT`'s own order, filled, and one resting GTC OCO for them at 170 and 140, created on
/// 2026-09-22 (so it expires on 2026-12-21, trading-domain spec §5.2). The buy is attributed, so
/// the position has a single holder the OCO belongs to (DEC-160's leg-agent rule).
fn protected_position_events(first_seq: u64) -> Vec<mandate_executor::FoldedEvent> {
    vec![
        event(
            ACCOUNT_STREAM,
            first_seq,
            "OrderSubmitted",
            with_clock(
                &[
                    ("client_order_id", text("md-held-1")),
                    ("agent", text(common::AGENT)),
                    ("instrument", text(AAPL)),
                    ("side", text("buy")),
                    ("qty", text("10")),
                    ("limit", text("150")),
                ],
                10,
            ),
        ),
        event(
            ACCOUNT_STREAM,
            first_seq.saturating_add(1),
            "FillApplied",
            with_clock(
                &[
                    ("fill_id", text("f-0")),
                    ("client_order_id", text("md-held-1")),
                    ("instrument", text(AAPL)),
                    ("side", text("buy")),
                    ("qty_gross", text("10")),
                    ("price", text("150")),
                ],
                10,
            ),
        ),
        event(
            ACCOUNT_STREAM,
            first_seq.saturating_add(2),
            "OrderStateChanged",
            with_clock(
                &[
                    ("client_order_id", text("md-held-1")),
                    ("state", text("filled")),
                ],
                10,
            ),
        ),
        event(
            ACCOUNT_STREAM,
            first_seq.saturating_add(3),
            "ProtectionChanged",
            with_clock(
                &[
                    ("instrument", text(AAPL)),
                    ("action", text("placed")),
                    ("orders", text("md-oco-1")),
                    ("qty", text("10")),
                    ("take_profit", text("170")),
                    ("stop", text("140")),
                    ("created_on", text("2026-09-22")),
                ],
                11,
            ),
        ),
    ]
}

/// The same protected position with the agent's own opening buy of ten resting beside it, as
/// `accepted_order` would leave it: the buy was submitted and accepted **before** the protection
/// was placed, so it is folded as those journaled facts. It cannot be sent through the step once
/// protection rests, because §5.4 denies a plain risk-increasing order there
/// (`add_blocked_by_protective_order`). Answers the shell and the buy's `client_order_id`.
fn protected_position_with_a_resting_buy(ports: &mandate_executor::Ports<'_>) -> (Shell, String) {
    let buy = format!("md-{INTENT}");
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let held = protected_position_events(2);
    let resting = [
        event(
            ACCOUNT_STREAM,
            5,
            "OrderSubmitted",
            with_clock(
                &[
                    ("client_order_id", text(&buy)),
                    ("intent_id", text(INTENT)),
                    ("agent", text(common::AGENT)),
                    ("instrument", text(AAPL)),
                    ("side", text("buy")),
                    ("qty", text("10")),
                    ("limit", text("150")),
                    ("purpose", text("open")),
                ],
                10,
            ),
        ),
        event(
            ACCOUNT_STREAM,
            6,
            "OrderStateChanged",
            with_clock(
                &[("client_order_id", text(&buy)), ("state", text("accepted"))],
                10,
            ),
        ),
    ];
    let protection = protected_position_events(4);
    for event in held
        .iter()
        .take(3)
        .chain(resting.iter())
        .chain(protection.iter().skip(3))
    {
        shell
            .fold_one(event)
            .expect("the protected position and its resting buy fold");
    }
    (shell.restart_ready(ports), buy)
}

/// A position of ten `AAPL` protected by one resting GTC OCO, which is `RC-14`'s initial state.
fn protected_position(ports: &mandate_executor::Ports<'_>) -> Shell {
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    for event in protected_position_events(2) {
        shell
            .fold_one(&event)
            .expect("the protected position folds");
    }
    shell.restart_ready(ports)
}

#[test]
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
fn a_passive_exit_never_leaves_the_position_unprotected() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), &ports);

    let asked = shell.run(
        handoff(INTENT, common::AGENT, discretionary_exit(AAPL, "10", "160")),
        &ports,
    );
    assert!(
        asked.requests.iter().any(|r| matches!(
            r,
            BrokerRequest::Cancel { client_order_id } if client_order_id.as_str() == "md-oco-1"
        )),
        "the resting OCO is cancelled to make room for the new one: {:?}",
        asked.requests
    );
    let after = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );
    let oco = after
        .submissions()
        .first()
        .and_then(|o| o.oco.clone())
        .expect("the passive exit rests as the take-profit leg of a new OCO");
    assert_eq!(
        (oco.stop, oco.qty),
        (price("140"), qty("10")),
        "the new OCO keeps the stop for the whole position"
    );

    let starts = shell
        .account_journal
        .iter()
        .filter(|e| {
            e.event_type == "ProtectionChanged"
                && e.payload
                    .get("action")
                    .and_then(mandate_canon::Value::as_str)
                    == Some("unprotected_start")
        })
        .count();
    assert_eq!(
        starts, 0,
        "the stop is carried into the new OCO, so no unprotected interval is opened at all"
    );
    assert!(
        shell
            .state
            .unprotected_intervals()
            .iter()
            .all(|i| i.ended_at.is_some()),
        "and the fold holds no open one"
    );
}

#[test]
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
            common::protected_add(AAPL, "5", "151", "140", Some("170")),
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
fn a_partly_filled_bracket_becomes_an_oco_for_the_filled_quantity() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
    let submitted = shell.run(
        handoff(
            INTENT,
            common::AGENT,
            protected_opening(AAPL, "100", "150", "140", Some("170")),
        ),
        &ports,
    );
    let entry = submitted
        .submissions()
        .first()
        .copied()
        .cloned()
        .expect("the bracket is sent");
    assert_eq!(
        entry.bracket,
        Some(mandate_executor::BracketLegs {
            take_profit: price("170"),
            stop: price("140"),
        }),
        "an entry that carries its protective prices goes as one GTC bracket at those prices"
    );
    let id = entry.client_order_id.as_str().to_owned();
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
    assert_eq!(
        (oco.take_profit, oco.stop),
        (price("170"), price("140")),
        "at the bracket's own prices, which the intent carried: the executor never invents one \
         (RC-21)"
    );
}

#[test]
fn an_entry_unfinished_at_the_timeout_is_cancelled_then_oco_d() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
    let submitted = shell.run(
        handoff(
            INTENT,
            common::AGENT,
            protected_opening(AAPL, "100", "150", "140", Some("170")),
        ),
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
fn a_terminal_partly_filled_entry_is_oco_d_at_once() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
    let submitted = shell.run(
        handoff(
            INTENT,
            common::AGENT,
            protected_opening(AAPL, "100", "150", "140", Some("170")),
        ),
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
    assert_eq!((oco.take_profit, oco.stop), (price("170"), price("140")));
}

/// DEC-346 item 6 sizes the OCO for a terminal partly filled entry to that entry's own filled
/// quantity, capped by what the position leaves after every live protective order and exit. The
/// cap only lowers it: with another bracket's 6 shares unprotected beside it, the room is 11 and the
/// OCO is still the 5 that filled, which is what keeps the first bracket's legs, if it completes,
/// within the position (DEC-521 item 2).
#[test]
fn a_terminal_entry_oco_covers_its_own_fill_not_the_room() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
    let first = shell
        .run(
            handoff(
                INTENT,
                common::AGENT,
                protected_opening(AAPL, "10", "150", "140", Some("170")),
            ),
            &ports,
        )
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the first bracket is sent");
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-1",
            Some(&first),
            "6",
            "150",
        ))),
        &ports,
    );
    let second = shell
        .run(
            handoff(
                OTHER_INTENT,
                common::AGENT,
                protected_opening(AAPL, "10", "150", "140", Some("170")),
            ),
            &ports,
        )
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("a second bracket is sent beside the first");

    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-2",
            Some(&second),
            AAPL,
            Side::Buy,
            "10",
            "5",
            "canceled",
        ))),
        &ports,
    );

    let oco = ran
        .submissions()
        .first()
        .copied()
        .and_then(|o| o.oco.clone())
        .expect("the second bracket ends partly filled, so its filled quantity is OCO'd (§5.4)");
    assert_eq!(
        oco.qty,
        qty("5"),
        "the OCO covers the 5 shares the second bracket filled, not the 11 the position leaves \
         uncovered, 6 of which are the first bracket's, whose legs are held (DEC-346 item 6)"
    );
}

/// A bracket entry of 10 AAPL at 150 for `who`, with 4 filled and its legs held until it completes
/// (§5.4). Its legs, once active, sell its own 10, and its OCO, if it ends partly filled, its
/// filled 4 (DEC-346 item 6), so no other protection may cover those 4 (DEC-532 item 1).
fn a_held_bracket(
    shell: &mut Shell,
    ports: &mandate_executor::Ports<'_>,
    who: &str,
    intent: &str,
) -> String {
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), ports);
    let entry = shell
        .run(
            handoff(
                intent,
                who,
                protected_opening(AAPL, "10", "150", "140", Some("170")),
            ),
            ports,
        )
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the bracket is sent: a new tranche is a new bracket (§5.4)");
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-held",
            Some(&entry),
            "4",
            "150",
        ))),
        ports,
    );
    entry
}

/// The quantity of the one protective OCO a step submits that is not `except`.
fn re_placed(ran: &common::Ran, except: Option<&str>) -> mandate_num::Qty {
    ran.submissions()
        .into_iter()
        .find(|o| o.oco.is_some() && Some(o.client_order_id.as_str()) != except)
        .map(|o| o.qty)
        .expect("protection is re-placed in this step")
}

/// E1 (DEC-532 item 2, `replace` after an exit sequence). The protected 10 and another agent's
/// held bracket (4 of 10 filled) make a position of 14; a risk exit sells 2. Once it fills, the
/// protection re-placed covers 8: the 10 the exit left outside the bracket. Sized on the whole
/// 12, it and the bracket's legs would sell 12 + 10 = 22 once the bracket completes, against a
/// position of 8 + 10 + 2 = 20 (§5.4's tranche model, rule 12).
#[test]
#[ignore = "pending E7-4"]
fn an_exits_re_placement_leaves_a_held_brackets_shares_to_its_legs() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    a_held_bracket(&mut shell, &ports, OTHER_AGENT, OTHER_INTENT);
    shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "2", "155")),
        &ports,
    );
    shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );
    let exit = format!("md-{INTENT}");
    let ended = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-exit",
            Some(&exit),
            AAPL,
            Side::Sell,
            "2",
            "2",
            "filled",
        ))),
        &ports,
    );

    assert_eq!(
        re_placed(&ended, None),
        qty("8"),
        "10 held, less the 2 sold; the bracket's 4 are its held legs' (§5.4, rule 12)"
    );
}

/// E1 (DEC-532 item 2, `new_day`'s re-placement before expiry, through `replacements`). The
/// protected 10 is re-placed at the buffer day while the agent's own bracket holds 4 of 10. Once
/// the expiring OCO's cancel is confirmed, its replacement covers the 10 it covered, not 14:
/// with the bracket's 10 legs that would be 24 against 20 once it completes.
#[test]
#[ignore = "pending E7-4"]
fn a_re_placement_before_expiry_leaves_a_held_brackets_shares_to_its_legs() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    a_held_bracket(&mut shell, &ports, common::AGENT, OTHER_INTENT);
    let day = copied(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "TradingDayStarted",
        with_clock(&[("date", text("2026-12-14"))], 7_000_000),
        &EventId(format!("{CLOCK_STREAM}-9")),
    );
    shell.fold_one(&day).expect("the trading day folds");
    shell.run(Input::Journal(day), &ports);
    let confirmed = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );

    assert_eq!(
        re_placed(&confirmed, None),
        qty("10"),
        "the 10 the expiring OCO covered; the bracket's 4 are its held legs' (§5.4, rule 12)"
    );
}

/// E1 (DEC-532 item 2, `passive_exit`'s rest). The protected 10 and another agent's held bracket
/// (4 of 10) make 14; this agent's passive exit of 3 becomes the take-profit of a new OCO, and
/// the rest of the position keeps its stop. The rest is 7, not 11: 3 + 11 and the bracket's 10
/// legs would sell 24 against 20 once it completes.
#[test]
#[ignore = "pending E7-4"]
fn a_passive_exits_rest_leaves_a_held_brackets_shares_to_its_legs() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    a_held_bracket(&mut shell, &ports, OTHER_AGENT, OTHER_INTENT);
    shell.run(
        handoff(INTENT, common::AGENT, discretionary_exit(AAPL, "3", "160")),
        &ports,
    );
    let placed = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );

    let exit = format!("md-{INTENT}");
    assert_eq!(
        re_placed(&placed, Some(&exit)),
        qty("7"),
        "the 10 outside the bracket, less the passive exit's 3 (§5.4, rule 12)"
    );
}

/// E1 (DEC-532 item 2, `re_cover`). A completed bracket of 2 rests its legs beside the protected
/// 10; then another bracket holds 4 of 10. When the broker cancels the first bracket's legs, the
/// shares they covered are re-covered: 2, not the 6 that the whole position of 16 less the 10
/// still resting leaves, since the held bracket's legs cover its 4.
#[test]
#[ignore = "pending E7-4"]
fn a_re_cover_leaves_a_held_brackets_shares_to_its_legs() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), &ports);
    let first = shell
        .run(
            handoff(
                INTENT,
                common::AGENT,
                protected_opening(AAPL, "2", "150", "140", Some("170")),
            ),
            &ports,
        )
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the first bracket is sent");
    let completed = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-first",
            Some(&first),
            "2",
            "150",
        ))),
        &ports,
    );
    let legs = completed
        .drafts
        .iter()
        .find(|d| {
            d.event_type == "ProtectionChanged"
                && d.payload.get("action").and_then(|v| v.as_str()) == Some("placed")
        })
        .and_then(|d| d.payload.get("orders").and_then(|v| v.as_str()))
        .map(str::to_owned)
        .expect("the completed bracket's legs are recorded placed (§5.4)");
    a_held_bracket(&mut shell, &ports, common::AGENT, OTHER_INTENT);
    let lost = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-legs",
            Some(&legs),
            AAPL,
            Side::Sell,
            "2",
            "0",
            "canceled",
        ))),
        &ports,
    );

    assert_eq!(
        re_placed(&lost, None),
        qty("2"),
        "the 2 the cancelled legs covered; the held bracket's 4 are its legs' (§5.4, rule 12)"
    );
}

/// E1's boundary (DEC-532 item 1): only a bracket whose legs are still held is subtracted. Another
/// agent's bracket that ended partly filled (4 of 10) has its OCO for those 4 already placed
/// (DEC-346 item 6), so nothing of it is held. A risk exit of 2 cancels both resting OCOs, and
/// once it fills, protection is re-placed for all 12 left: the OCO it cancelled covered the 4, and
/// subtracting them again would leave 4 shares unprotected.
#[test]
fn a_terminal_brackets_placed_oco_is_not_subtracted_again() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    let entry = a_held_bracket(&mut shell, &ports, OTHER_AGENT, OTHER_INTENT);
    let ended = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-held",
            Some(&entry),
            AAPL,
            Side::Buy,
            "10",
            "4",
            "canceled",
        ))),
        &ports,
    );
    let oco = ended
        .submissions()
        .first()
        .filter(|o| o.oco.is_some())
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the bracket ended partly filled, so its 4 get an OCO (DEC-346 item 6)");
    let begun = shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "2", "155")),
        &ports,
    );
    let mut cancelled: Vec<String> = begun
        .requests
        .iter()
        .filter_map(|r| match r {
            BrokerRequest::Cancel { client_order_id } => Some(client_order_id.as_str().to_owned()),
            _ => None,
        })
        .collect();
    cancelled.sort();
    let mut expected = vec!["md-oco-1".to_owned(), oco];
    expected.sort();
    assert_eq!(
        cancelled, expected,
        "the exit cancels every resting protective order first (§5.4)"
    );
    for id in expected {
        shell.run(
            Input::Broker(Ok(BrokerOutcome::CancelAccepted {
                client_order_id: id,
            })),
            &ports,
        );
    }
    let exit = format!("md-{INTENT}");
    let done = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-exit",
            Some(&exit),
            AAPL,
            Side::Sell,
            "2",
            "2",
            "filled",
        ))),
        &ports,
    );

    assert_eq!(
        re_placed(&done, None),
        qty("12"),
        "the 14 less the exit's 2; the ended bracket holds no legs, so nothing is subtracted"
    );
}

/// §5.4's bound is per interval: two partly filled brackets in one instrument keep two intervals,
/// and the second's legs, once placed, end the second's interval, not the first's. The first
/// bracket's shares are still unprotected, so the owner is alerted at the first tick
/// `max_unprotected_s` after its partial fill (backlog: "E7-4 (stream K), E4 from E7-4 slice 7's
/// second tests correction", DEC-521 item 3).
#[test]
fn a_second_brackets_end_leaves_the_first_brackets_interval_bounded() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
    shell.run(Input::Tick(clock(10)), &ports);
    let first = shell
        .run(
            handoff(
                INTENT,
                common::AGENT,
                protected_opening(AAPL, "10", "150", "140", Some("170")),
            ),
            &ports,
        )
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the first bracket is sent");
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&first),
            AAPL,
            Side::Buy,
            "10",
            "6",
            "partially_filled",
        ))),
        &ports,
    );
    shell.run(Input::Tick(clock(20)), &ports);
    let second = shell
        .run(
            handoff(
                OTHER_INTENT,
                common::AGENT,
                protected_opening(AAPL, "10", "150", "140", Some("170")),
            ),
            &ports,
        )
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("a second bracket is sent beside the first: it only adds a tranche (§5.4)");
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-2",
            Some(&second),
            AAPL,
            Side::Buy,
            "10",
            "5",
            "partially_filled",
        ))),
        &ports,
    );
    shell.run(Input::Tick(clock(30)), &ports);
    let completed = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-2",
            Some(&second),
            AAPL,
            Side::Buy,
            "10",
            "10",
            "filled",
        ))),
        &ports,
    );
    assert!(
        completed
            .drafts
            .iter()
            .any(|d| d.event_type == "ProtectionChanged"
                && d.payload.get("action").and_then(|v| v.as_str()) == Some("unprotected_end")
                && d.payload.get("bracket").and_then(|v| v.as_str()) == Some(second.as_str())),
        "the second bracket's legs are placed and its own interval ends: {:?}",
        completed.draft_types()
    );

    let early = shell.run(Input::Tick(clock(69)), &ports);
    assert!(
        !early.notifications.contains(&"unprotected_interval_limit"),
        "59 seconds after the first partial fill is inside the bound: {:?}",
        early.notifications
    );
    let bound = shell.run(Input::Tick(clock(70)), &ports);
    assert!(
        bound.notifications.contains(&"unprotected_interval_limit"),
        "the first bracket's 6 shares have been unprotected since 10, so at 70 the owner is \
         alerted; the second bracket's end at 30 must not have closed that interval (§5.4): {:?}",
        bound.notifications
    );
    let after = shell.run(Input::Tick(clock(85)), &ports);
    assert!(
        !after.notifications.contains(&"unprotected_interval_limit"),
        "the first interval is alerted once, and the second ended at 30, so 65 seconds after the \
         second partial fill nothing alerts again: {:?}",
        after.notifications
    );
}

/// The same bound when the second bracket ends partly filled: its OCO ends the second's interval
/// only once the broker acknowledges it (DEC-348 item 2), and that acknowledgment, which names no
/// bracket, must end the interval of the bracket the OCO protects, not the first bracket's
/// (backlog: "E7-4 (stream K), E4 from E7-4 slice 7's second tests correction", DEC-521 item 3).
#[test]
fn an_acknowledged_oco_for_a_second_bracket_leaves_the_first_brackets_interval_bounded() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
    shell.run(Input::Tick(clock(10)), &ports);
    let first = shell
        .run(
            handoff(
                INTENT,
                common::AGENT,
                protected_opening(AAPL, "10", "150", "140", Some("170")),
            ),
            &ports,
        )
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the first bracket is sent");
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&first),
            AAPL,
            Side::Buy,
            "10",
            "6",
            "partially_filled",
        ))),
        &ports,
    );
    shell.run(Input::Tick(clock(20)), &ports);
    let second = shell
        .run(
            handoff(
                OTHER_INTENT,
                common::AGENT,
                protected_opening(AAPL, "10", "150", "140", Some("170")),
            ),
            &ports,
        )
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("a second bracket is sent beside the first: it only adds a tranche (§5.4)");
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-2",
            Some(&second),
            AAPL,
            Side::Buy,
            "10",
            "5",
            "partially_filled",
        ))),
        &ports,
    );
    shell.run(Input::Tick(clock(30)), &ports);
    let oco = shell
        .run(
            Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
                "b-2",
                Some(&second),
                AAPL,
                Side::Buy,
                "10",
                "5",
                "canceled",
            ))),
            &ports,
        )
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect(
            "the second bracket ends partly filled, so its 5 shares get an OCO (DEC-346 item 6)",
        );
    shell.run(Input::Tick(clock(35)), &ports);
    let acknowledged = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-3",
            Some(&oco),
            AAPL,
            Side::Sell,
            "5",
            "0",
            "accepted",
        ))),
        &ports,
    );
    assert!(
        acknowledged
            .drafts
            .iter()
            .any(|d| d.event_type == "ProtectionChanged"
                && d.payload.get("action").and_then(|v| v.as_str()) == Some("unprotected_end")),
        "the OCO's acknowledgment ends an interval (DEC-348 item 2): {:?}",
        acknowledged.draft_types()
    );

    let early = shell.run(Input::Tick(clock(69)), &ports);
    assert!(
        !early.notifications.contains(&"unprotected_interval_limit"),
        "59 seconds after the first partial fill is inside the bound: {:?}",
        early.notifications
    );
    let bound = shell.run(Input::Tick(clock(70)), &ports);
    assert!(
        bound.notifications.contains(&"unprotected_interval_limit"),
        "the first bracket's 6 shares have been unprotected since 10, so at 70 the owner is \
         alerted; the acknowledgment of the second bracket's OCO at 35 must not have closed that \
         interval (§5.4): {:?}",
        bound.notifications
    );
    let after = shell.run(Input::Tick(clock(85)), &ports);
    assert!(
        !after.notifications.contains(&"unprotected_interval_limit"),
        "the first interval is alerted once, and the second ended at its acknowledgment, so 65 \
         seconds after the second partial fill nothing alerts again: {:?}",
        after.notifications
    );
}

/// E4b (backlog: "E7-4 (stream K), E4b from E4's fix"; DEC-521 item 3): while the OCO for a
/// second bracket awaits the broker's acknowledgment (DEC-348 item 2), a third bracket's partial
/// fill starts a new interval and ends the awaited one, the second bracket's, never the first's.
/// The first bracket's 6 shares stay unprotected from 10, so the owner is alerted at 70, once.
#[test]
fn a_new_brackets_start_ends_the_awaited_interval_not_the_first_brackets() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
    let partly_filled = |shell: &mut Shell, at: i64, intent: &str, broker: &str, filled: &str| {
        shell.run(Input::Tick(clock(at)), &ports);
        let entry = shell
            .run(
                handoff(
                    intent,
                    common::AGENT,
                    protected_opening(AAPL, "10", "150", "140", Some("170")),
                ),
                &ports,
            )
            .submissions()
            .first()
            .map(|o| o.client_order_id.as_str().to_owned())
            .expect("each bracket is sent: a new tranche is a new bracket (§5.4)");
        shell.run(
            Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
                broker,
                Some(&entry),
                AAPL,
                Side::Buy,
                "10",
                filled,
                "partially_filled",
            ))),
            &ports,
        );
        entry
    };
    partly_filled(&mut shell, 10, INTENT, "b-1", "6");
    let second = partly_filled(&mut shell, 20, OTHER_INTENT, "b-2", "5");
    shell.run(Input::Tick(clock(30)), &ports);
    let awaited = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-2",
            Some(&second),
            AAPL,
            Side::Buy,
            "10",
            "5",
            "canceled",
        ))),
        &ports,
    );
    assert!(
        awaited.submissions().iter().any(|o| o.oco.is_some()),
        "the second bracket ends partly filled, so its 5 shares get an OCO, whose acknowledgment \
         is awaited (DEC-346 item 6, DEC-348 item 2)"
    );
    partly_filled(&mut shell, 32, "01JABCDEFGHJKMNPQRSTVWXYZ2", "b-3", "3");

    let early = shell.run(Input::Tick(clock(69)), &ports);
    assert!(
        !early.notifications.contains(&"unprotected_interval_limit"),
        "59 seconds after the first partial fill is inside the bound: {:?}",
        early.notifications
    );
    let bound = shell.run(Input::Tick(clock(70)), &ports);
    assert!(
        bound.notifications.contains(&"unprotected_interval_limit"),
        "the first bracket's 6 shares have been unprotected since 10, so at 70 the owner is \
         alerted; the third bracket's start at 32 ended the awaited second interval, not the first \
         (§5.4): {:?}",
        bound.notifications
    );
    let after = shell.run(Input::Tick(clock(85)), &ports);
    assert!(
        !after.notifications.contains(&"unprotected_interval_limit"),
        "the first interval is alerted once, the second ended at 32 and the third is 53 seconds \
         old, so nothing alerts again: {:?}",
        after.notifications
    );
}

/// E4b, the case that tells "the awaited interval" from both "the first" and "the latest" (#771
/// review): three brackets open intervals at 10, 20 and 25; the middle one is cancelled partly
/// filled at 30, so its OCO's acknowledgment is awaited; a fourth bracket starts at 32 and must end
/// the middle one only. The first (from 10) is then alerted at 70, the middle one never, and the
/// third (from 25) at 85; nothing alerts at 80, which an awaited interval left open would.
#[test]
fn a_new_start_ends_the_awaited_middle_interval_only() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
    let partly_filled = |shell: &mut Shell, at: i64, intent: &str, broker: &str| {
        shell.run(Input::Tick(clock(at)), &ports);
        let entry = shell
            .run(
                handoff(
                    intent,
                    common::AGENT,
                    protected_opening(AAPL, "10", "150", "140", Some("170")),
                ),
                &ports,
            )
            .submissions()
            .first()
            .map(|o| o.client_order_id.as_str().to_owned())
            .expect("each bracket is sent: a new tranche is a new bracket (§5.4)");
        shell.run(
            Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
                broker,
                Some(&entry),
                AAPL,
                Side::Buy,
                "10",
                "4",
                "partially_filled",
            ))),
            &ports,
        );
        entry
    };
    partly_filled(&mut shell, 10, INTENT, "b-1");
    let middle = partly_filled(&mut shell, 20, OTHER_INTENT, "b-2");
    partly_filled(&mut shell, 25, "01JABCDEFGHJKMNPQRSTVWXYZ2", "b-3");
    shell.run(Input::Tick(clock(30)), &ports);
    let awaited = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-2",
            Some(&middle),
            AAPL,
            Side::Buy,
            "10",
            "4",
            "canceled",
        ))),
        &ports,
    );
    assert!(
        awaited.submissions().iter().any(|o| o.oco.is_some()),
        "the middle bracket ends partly filled, so its 4 shares get an OCO, whose acknowledgment \
         is awaited (DEC-346 item 6, DEC-348 item 2)"
    );
    partly_filled(&mut shell, 32, "01JABCDEFGHJKMNPQRSTVWXYZ3", "b-4");

    let alerted = |shell: &mut Shell, at: i64| {
        shell
            .run(Input::Tick(clock(at)), &ports)
            .notifications
            .contains(&"unprotected_interval_limit")
    };
    assert!(!alerted(&mut shell, 69), "nothing is 60 seconds old at 69");
    assert!(
        alerted(&mut shell, 70),
        "the first bracket's interval, from 10, is alerted at 70: the start at 32 did not end it"
    );
    assert!(
        !alerted(&mut shell, 80),
        "the middle bracket's interval, from 20, was the awaited one the start at 32 ended, so \
         nothing alerts at 80"
    );
    assert!(
        alerted(&mut shell, 85),
        "the third bracket's interval, from 25, is alerted at 85: the start at 32 did not end it"
    );
}

#[test]
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
fn protection_is_not_re_placed_early() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);

    let early = copied(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "TradingDayStarted",
        with_clock(&[("date", text("2026-12-11"))], 6_000_000),
        &EventId(format!("{CLOCK_STREAM}-8")),
    );
    shell.fold_one(&early).expect("the trading day folds");
    let ran = shell.run(Input::Journal(early), &ports);

    assert!(
        !ran.requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::Cancel { .. })),
        "2026-12-11 is six trading days before the 2026-12-21 expiry, one short of the buffer: \
         {:?}",
        ran.requests
    );
    assert!(
        shell
            .state
            .protection(&instrument(AAPL))
            .expect("the protection accessor answers")
            .is_some_and(|p| p.resting.iter().any(|id| id.as_str() == "md-oco-1")),
        "and the resting OCO stays where it is"
    );

    let buffer = copied(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "TradingDayStarted",
        with_clock(&[("date", text("2026-12-14"))], 6_100_000),
        &EventId(format!("{CLOCK_STREAM}-9")),
    );
    shell.fold_one(&buffer).expect("the next trading day folds");
    let ran = shell.run(Input::Journal(buffer), &ports);
    assert!(
        ran.requests.iter().any(|r| matches!(
            r,
            BrokerRequest::Cancel { client_order_id } if client_order_id.as_str() == "md-oco-1"
        )),
        "the next trading day is the buffer day, so the same case does see a re-placement: {:?}",
        ran.requests
    );
}

#[test]
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
        Some(price("138.31")),
        "the liquid-equity tier's 0.5% off a 139 reference bid, 138.305, rounded up to the cent \
         (§2.1: a sell limit rounds up to the tick; DEC-260 (6))"
    );
}

/// The broker's fill of the whole crypto entry `id`: `bought` `BTC` at 60000.
fn btc_fill(id: &str, bought: &str) -> mandate_executor::BrokerFill {
    mandate_executor::BrokerFill {
        instrument: instrument(BTC),
        ..broker_fill("f-btc-1", Some(id), bought, "60000")
    }
}

/// What a crypto buy of `bought` holds once its taker fee is withheld in the asset, recomputed in
/// whole nano-units from trading-domain spec §6.3: `fee_qty = round(gross × 25 ÷ 10000, 9,
/// half_up)`, `received = gross − fee_qty`. Independent of the crate's own decimal arithmetic.
fn net_of_taker_fee(bought: &str) -> String {
    let (whole, fraction) = bought.split_once('.').unwrap_or((bought, ""));
    let digits = format!("{whole}{fraction:0<9}");
    let gross: u128 = digits.parse().unwrap_or_else(|e| panic!("{bought}: {e}"));
    let scaled = gross * 25;
    let fee = scaled / 10_000 + u128::from(scaled % 10_000 >= 5_000);
    let net = gross - fee;
    let text = format!("{}.{:09}", net / 1_000_000_000, net % 1_000_000_000);
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
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
    let mut shell = shell.restart_ready(&ports);
    shell.run(Input::Market(quote(BTC, "60000", "60010", 20)), &ports);
    let submitted = shell.run(
        handoff(
            INTENT,
            common::AGENT,
            protected_opening(BTC, "0.5", "60000", "54000", None),
        ),
        &ports,
    );
    let id = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the crypto entry is sent");

    shell.run(
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
    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(btc_fill(&id, "0.5"))),
        &ports,
    );

    let protection = ran
        .submissions()
        .into_iter()
        .find(|o| o.order_type == mandate_executor::OrderType::StopLimit)
        .expect("one GTC stop-limit for the whole position (§5.4, DEC-36)");
    assert_eq!(
        protection.qty,
        qty("0.49875"),
        "for the whole position, less the 0.00125 its buy withholds as the taker fee, the larger \
         rate: a stop-limit never outlasts the position once that fee posts (RC-07, RC-20)"
    );
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
    for held in [
        event(
            ACCOUNT_STREAM,
            2,
            "OrderSubmitted",
            with_clock(
                &[
                    ("client_order_id", text("md-held-btc")),
                    ("agent", text(common::AGENT)),
                    ("instrument", text(BTC)),
                    ("side", text("buy")),
                    ("qty", text("0.5")),
                    ("limit", text("60000")),
                ],
                10,
            ),
        ),
        event(
            ACCOUNT_STREAM,
            3,
            "FillApplied",
            with_clock(
                &[
                    ("fill_id", text("f-0")),
                    ("client_order_id", text("md-held-btc")),
                    ("instrument", text(BTC)),
                    ("side", text("buy")),
                    ("qty_gross", text("0.5")),
                    ("price", text("60000")),
                ],
                10,
            ),
        ),
        event(
            ACCOUNT_STREAM,
            4,
            "OrderStateChanged",
            with_clock(
                &[
                    ("client_order_id", text("md-held-btc")),
                    ("state", text("filled")),
                ],
                10,
            ),
        ),
    ] {
        shell
            .fold_one(&held)
            .expect("the agent's own crypto position folds, so its stop-limit has a holder");
    }
    let protection = event(
        ACCOUNT_STREAM,
        5,
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
    let mut shell = shell.restart_ready(&ports);
    shell.run(Input::Market(quote(BTC, "60000", "60010", 20)), &ports);

    let ran = shell.run(
        handoff(
            INTENT,
            common::AGENT,
            common::protected_add(BTC, "0.1", "60000", "54000", None),
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
    const BOUGHT: &str = "0.123456789";
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
    shell.run(Input::Market(quote(BTC, "60000", "60010", 20)), &ports);
    let submitted = shell.run(
        handoff(
            INTENT,
            common::AGENT,
            protected_opening(BTC, BOUGHT, "60000", "54000", None),
        ),
        &ports,
    );
    let id = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the entry is sent");
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&id),
            BTC,
            Side::Buy,
            BOUGHT,
            BOUGHT,
            "filled",
        ))),
        &ports,
    );
    let first = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(btc_fill(&id, BOUGHT))),
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
    assert_eq!(
        stop.qty,
        qty(&net_of_taker_fee(BOUGHT)),
        "the whole position net of a fee that rounds at the ninth place, half up: 0.123456789 \
         less round(0.000308641972, 9, half_up) = 0.000308642 (§6.3, DEC-349 item 2)"
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
    let mut shell = shell.restart_ready(&ports);
    shell.run(Input::Market(quote(FRAC, "20", "20.1", 20)), &ports);
    let submitted = shell.run(
        handoff(
            INTENT,
            common::AGENT,
            protected_opening(FRAC, "10.4", "20", "18", Some("24")),
        ),
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

/// Drives `RC-14`'s protected position into a risk exit at a 150 bid: the resting OCO is
/// cancelled and confirmed, and the first rung is submitted. The clock after the restart is the
/// last folded `risk_clock`, 11, and a quote's `observed_at` never moves it (§5.6's observation is
/// "never for time"), so the first rung goes at second 11.
fn first_rung(shell: &mut Shell, ports: &mandate_executor::Ports<'_>) -> String {
    shell.run(Input::Market(quote(AAPL, "150", "150.2", 20)), ports);
    shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "10", "150")),
        ports,
    );
    let sent = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        ports,
    );
    let rung = sent
        .submissions()
        .first()
        .copied()
        .cloned()
        .expect("the first rung is sent once the OCO's cancel is confirmed");
    assert_eq!(
        rung.limit_price,
        Some(price("149.25")),
        "150 x (1 - 0.005), the liquid tier's first offset"
    );
    rung.client_order_id.as_str().to_owned()
}

/// One step of the ladder at `at`: the resting rung is cancelled, and only once the broker
/// confirms that cancel is the next rung submitted (§5.6 step 2 is cancel, confirm, resubmit, and
/// nothing is submitted while a cancel is unconfirmed).
fn step_rung(
    shell: &mut Shell,
    ports: &mandate_executor::Ports<'_>,
    at: i64,
    resting: &str,
) -> (common::Ran, common::Ran) {
    let ticked = shell.run(Input::Tick(clock(at)), ports);
    let confirmed = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: resting.to_owned(),
        })),
        ports,
    );
    (ticked, confirmed)
}

#[test]
fn the_ladder_steps_only_after_the_interval() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    let first = first_rung(&mut shell, &ports);

    let early = shell.run(Input::Tick(clock(14)), &ports);
    assert!(
        early.requests.is_empty(),
        "three seconds after the rung is inside exit_step_s of five, so nothing is repriced: {:?}",
        early.requests
    );
    let (stepped, confirmed) = step_rung(&mut shell, &ports, 16, &first);
    assert!(
        stepped.requests.iter().any(|r| matches!(
            r,
            BrokerRequest::Cancel { client_order_id } if client_order_id.as_str() == first
        )),
        "five seconds after the rung it is cancelled to step: {:?}",
        stepped.requests
    );
    assert!(
        stepped.submissions().is_empty(),
        "and the next rung waits for the cancel's confirmation"
    );
    let repriced = confirmed
        .submissions()
        .first()
        .copied()
        .cloned()
        .expect("the confirmation releases the next rung");
    assert_eq!(
        repriced.limit_price,
        Some(price("148.5")),
        "the offset rises by exit_offset_step to 1%: 150 x 0.99 (§5.6, RC-24)"
    );
    assert_ne!(
        repriced.client_order_id.as_str(),
        first,
        "a new rung is a new order under a new derived id"
    );
}

#[test]
fn the_ladder_never_prices_below_the_floor() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    let mut resting = first_rung(&mut shell, &ports);

    let mut prices = Vec::new();
    let mut alerted = false;
    for at in [16_i64, 21, 26, 31, 36] {
        let (ticked, confirmed) = step_rung(&mut shell, &ports, at, &resting);
        alerted =
            alerted || !ticked.notifications.is_empty() || !confirmed.notifications.is_empty();
        let rung = confirmed
            .submissions()
            .first()
            .copied()
            .cloned()
            .unwrap_or_else(|| panic!("a rung follows the confirmation at {at}"));
        prices.push(rung.limit_price);
        resting = rung.client_order_id.as_str().to_owned();
    }
    assert_eq!(
        prices,
        ["148.5", "147.75", "147", "146.25", "145.5"]
            .map(|p| Some(price(p)))
            .to_vec(),
        "150 x (1 - offset) for offsets 1% to 3% in steps of 0.5% (§5.6's liquid tier)"
    );
    for at in [41_i64, 46] {
        let rested = shell.run(Input::Tick(clock(at)), &ports);
        alerted = alerted || !rested.notifications.is_empty();
        assert!(
            rested.requests.is_empty(),
            "at the floor of max_exit_offset, 3%, the order rests: 145.5 is never undercut ({at}): \
             {:?}",
            rested.requests
        );
    }
    assert!(alerted, "and the owner is alerted at the floor");
}

#[test]
fn an_agent_kill_switch_cancels_only_that_agents_orders() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, mine) = protected_position_with_a_resting_buy(&ports);
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
        !ran.requests.iter().any(common::is_account_wide),
        "and never the account-wide endpoints"
    );
}

#[test]
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
    let mandates = FixedMandate::covering(&[AAPL, CPHC]);
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

/// 2026-09-22 08:00 ET (12:00 UTC), a Tuesday on the committed calendar: pre-market. The suite's
/// clock-0 instants are read as the regular session (DEC-260 (13)), so a session test that must
/// see the session matter moves its clock onto the calendar (DEC-506).
const PRE_MARKET: i64 = 1_790_078_400;

/// 2026-09-22 11:00 ET (15:00 UTC): the regular session of the same day, the control that shows
/// the session, not something else, decides.
const REGULAR_SESSION: i64 = 1_790_089_200;

/// Whether a run asks the cancel of the protected position's resting OCO.
fn cancels_the_protection(ran: &common::Ran) -> bool {
    ran.effects.iter().any(|e| {
        matches!(e, Effect::Broker(BrokerRequest::Cancel { client_order_id })
            if client_order_id.as_str() == "md-oco-1")
    })
}

/// The instruments a run's `KillSwitchActivated` defers to the regular session.
fn deferred(ran: &common::Ran) -> Vec<String> {
    ran.draft("KillSwitchActivated")
        .and_then(|d| d.payload.get("deferred"))
        .and_then(mandate_canon::Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(mandate_canon::Value::as_str)
        .map(str::to_owned)
        .collect()
}

/// §5.5: an automated kill switch sells equities only in the regular session, leaving protection
/// in place until then (DEC-485 items 5 and 10). Pre-market it asks no cancel of the resting OCO,
/// sells nothing, and journals the instrument as `deferred`; the same switch in the regular
/// session cancels the protection for its close and defers nothing.
#[test]
fn an_automated_flatten_defers_equity_sells_to_the_session() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    for (at, pre_market) in [(PRE_MARKET, true), (REGULAR_SESSION, false)] {
        let mut shell = protected_position(&ports);
        shell.run(Input::Tick(clock(at)), &ports);
        shell.run(Input::Market(quote(AAPL, "155", "155.1", at)), &ports);

        let ran = shell.run(
            Input::Command(Command::KillSwitch {
                scope: KillScope::Agent(agent(common::AGENT)),
                initiator: Initiator::RiskLimit,
                confirmation: None,
            }),
            &ports,
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
        if pre_market {
            assert!(
                ran.submissions().iter().all(|o| o.side != Side::Sell),
                "an automated kill switch sells equities only in the regular session (§5.5)"
            );
            assert!(
                !cancels_the_protection(&ran),
                "and leaves protection in place until then (§5.5)"
            );
            assert_eq!(
                deferred(&ran),
                vec![AAPL.to_owned()],
                "and the deferral is journaled rather than forgotten"
            );
        } else {
            assert!(
                cancels_the_protection(&ran),
                "in the regular session the close cancels the protection first (§5.4, §5.5)"
            );
            assert!(
                deferred(&ran).is_empty(),
                "and nothing is deferred in the session"
            );
        }
    }
}

#[test]
fn an_automated_flatten_sells_crypto_at_once() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let bought = event(
        ACCOUNT_STREAM,
        2,
        "OrderSubmitted",
        with_clock(
            &[
                ("client_order_id", text("md-held-btc")),
                ("agent", text(common::AGENT)),
                ("instrument", text(BTC)),
                ("side", text("buy")),
                ("qty", text("0.5")),
                ("limit", text("60000")),
            ],
            10,
        ),
    );
    shell
        .fold_one(&bought)
        .expect("the agent's own buy folds, so the sub-ledger is its (§5.5)");
    let held = event(
        ACCOUNT_STREAM,
        3,
        "FillApplied",
        with_clock(
            &[
                ("fill_id", text("f-0")),
                ("client_order_id", text("md-held-btc")),
                ("instrument", text(BTC)),
                ("side", text("buy")),
                ("qty_gross", text("0.5")),
                ("price", text("60000")),
            ],
            10,
        ),
    );
    shell.fold_one(&held).expect("the crypto position folds");
    let done = event(
        ACCOUNT_STREAM,
        4,
        "OrderStateChanged",
        with_clock(
            &[
                ("client_order_id", text("md-held-btc")),
                ("state", text("filled")),
            ],
            10,
        ),
    );
    shell.fold_one(&done).expect("the buy's fill completes it");
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
        price("154.23"),
        "155 x (1 - 0.005) = 154.225 from the confirmed bid, not from a quote nobody saw, rounded \
         up to the equity tick (§2.1, §5.5, §5.6; DEC-260 (6))"
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

/// §5.5: without the owner's confirmed bid, bid size and floor, an owner kill switch's equity sells
/// wait for the session. DEC-260 (13) reads that wait for an owner exit in pre-market as a
/// regular-session limit the broker queues to the session, never an extended-hours order. So
/// pre-market, once the protection's cancel is confirmed, no sell goes with `extended_hours`; in
/// the regular session, the control, the same switch's close is submitted.
#[test]
fn an_unconfirmed_owner_exit_waits_for_the_session() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    for (at, pre_market) in [(PRE_MARKET, true), (REGULAR_SESSION, false)] {
        let mut shell = protected_position(&ports);
        shell.run(Input::Tick(clock(at)), &ports);
        shell.run(Input::Market(quote(AAPL, "155", "155.1", at)), &ports);

        let ran = shell.run(
            Input::Command(Command::KillSwitch {
                scope: KillScope::Agent(agent(common::AGENT)),
                initiator: Initiator::Owner,
                confirmation: None,
            }),
            &ports,
        );
        assert!(
            ran.draft_types().contains(&"KillSwitchActivated"),
            "the switch itself is journaled and the mode applied: {:?}",
            ran.draft_types()
        );
        assert!(
            ran.submissions().iter().all(|o| o.side != Side::Sell),
            "no sell goes before the protection's cancel is confirmed (§5.4)"
        );
        let after = shell.run(
            Input::Broker(Ok(BrokerOutcome::CancelAccepted {
                client_order_id: "md-oco-1".to_owned(),
            })),
            &ports,
        );
        let sells: Vec<_> = after
            .submissions()
            .into_iter()
            .filter(|o| o.side == Side::Sell)
            .collect();
        if pre_market {
            assert!(
                sells.iter().all(|o| !o.extended_hours),
                "without the confirmed bid, bid size, and floor, equity sells wait for the \
                 session (§5.5): no extended-hours sell pre-market, {sells:?}"
            );
        } else {
            assert_eq!(
                sells.len(),
                1,
                "in the regular session the owner's close is submitted once its protection's \
                 cancel is confirmed"
            );
        }
    }
}

/// §5.5 exempts a kill switch's sells from the agent's mode, and DEC-260 (3) binds the slice that
/// owns the kill switch to narrow the ladder's pause stop and the gate's `mode_failure` together:
/// a mandate-limit flatten pauses the agent first, and its own sells are still gated `allow`,
/// laddered, and stepped while the agent is paused, each step's cancel confirmed before the next
/// rung, and no step ends the sequence (#373 round 1, major 1).
#[test]
fn a_paused_flattens_ladder_steps_and_its_step_cancel_does_not_end_the_sequence() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), &ports);

    let switched = shell.run(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(agent(common::AGENT)),
            initiator: Initiator::RiskLimit,
            confirmation: None,
        }),
        &ports,
    );
    let sent = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );

    let to = switched
        .draft("AgentModeApplied")
        .and_then(|d| d.payload.get("to"))
        .and_then(mandate_canon::Value::as_str);
    assert_eq!(to, Some("paused"), "a mandate limit pauses the agent first");
    let verdicts: Vec<(&str, &str)> = switched
        .drafts
        .iter()
        .chain(sent.drafts.iter())
        .filter(|d| d.event_type == "GateDecided")
        .map(|d| {
            let field = |name: &str| {
                d.payload
                    .get(name)
                    .and_then(mandate_canon::Value::as_str)
                    .unwrap_or_default()
            };
            (field("verdict"), field("reason_code"))
        })
        .collect();
    assert!(
        verdicts.iter().any(|(verdict, _)| *verdict == "allow"),
        "the flatten's sell is allowed under `paused`, which holds every other exit of the agent \
         (§5.5, DEC-260 (3)): {verdicts:?}"
    );
    assert!(
        !verdicts.iter().any(|(_, reason)| *reason == "agent_paused"),
        "and `mode_failure` never holds it: {verdicts:?}"
    );
    let first =
        sent.submissions().first().copied().cloned().expect(
            "the flatten's first rung goes once the OCO's cancel is confirmed, paused or not",
        );
    assert_eq!(first.qty, qty("10"), "for the whole sub-ledger");
    assert_eq!(
        first.limit_price,
        Some(price("154.23")),
        "155 x (1 - 0.005) = 154.225, rounded up to the equity tick (§5.6, DEC-260 (6))"
    );
    let resting = first.client_order_id.as_str().to_owned();

    let (stepped, confirmed) = step_rung(&mut shell, &ports, 16, &resting);

    assert!(
        stepped.requests.iter().any(|r| matches!(
            r,
            BrokerRequest::Cancel { client_order_id } if client_order_id.as_str() == resting
        )),
        "exit_step_s after the rung it is cancelled to step, though the agent is paused: {:?}",
        stepped.requests
    );
    assert!(
        stepped.submissions().is_empty(),
        "and nothing is submitted while that cancel is unconfirmed"
    );
    let next = confirmed.submissions().first().copied().cloned().expect(
        "the step's confirmation submits the next rung: the pause does not end the sequence",
    );
    assert_eq!(
        next.limit_price,
        Some(price("153.45")),
        "155 x (1 - 0.01), the offset raised by exit_offset_step"
    );
    assert_eq!(next.qty, qty("10"));
    assert!(
        !confirmed
            .drafts
            .iter()
            .any(|d| d.event_type == "ProtectionChanged"
                && d.payload
                    .get("action")
                    .and_then(mandate_canon::Value::as_str)
                    == Some("placed")),
        "no protection is re-placed for a flatten whose ladder still climbs: {:?}",
        confirmed.draft_types()
    );
}

/// §5.5, §5.6: an owner's kill switch sells through the ladder, never below the floor the owner
/// confirmed; the rung the floor clamps is `at_floor` and rests rather than being cancelled and
/// resubmitted at the same price every `exit_step_s` (#373 round 1, minor 1). The case runs in
/// the regular session (the suite's clock, DEC-260 (13)), so the ladder's reference is the
/// observed quote's bid, as for every exit there; the confirmation carries a floor above the
/// tier's own, so the clamp is what binds. The confirmed bid as the reference outside the session
/// is slice 7's session tests'. §5.6 (3)'s alert is unconditional, so it is asserted here too,
/// on the rung the floor clamps rather than anywhere in the run: clamping and resting without it
/// would otherwise pass while the owner is never told, and an alert raised on the 153.45 rung,
/// which is above the floor, would not be the one §5.6 (3) requires.
/// [`the_ladder_never_prices_below_the_floor`] pins the alert only for `max_exit_offset`'s own
/// floor.
#[test]
fn an_owner_flattens_rung_rests_at_the_confirmed_floor() {
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
            confirmation: Some(OwnerConfirmation {
                floor: price("153"),
                ..owner_confirmation()
            }),
        }),
        &ports,
    );
    let sent = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );
    let first = sent
        .submissions()
        .first()
        .copied()
        .cloned()
        .expect("the owner's flatten goes once the OCO's cancel is confirmed");
    assert_eq!(
        first.limit_price,
        Some(price("154.23")),
        "155 x (1 - 0.005) from the quote's bid, rounded up to the tick"
    );
    let mut resting = first.client_order_id.as_str().to_owned();

    let mut limits = Vec::new();
    let mut floored = Vec::new();
    let mut alerts = Vec::new();
    for at in [16_i64, 21] {
        let (ticked, confirmed) = step_rung(&mut shell, &ports, at, &resting);
        alerts.push(!ticked.notifications.is_empty() || !confirmed.notifications.is_empty());
        let rung = confirmed
            .submissions()
            .first()
            .copied()
            .cloned()
            .unwrap_or_else(|| panic!("a rung follows the confirmation at {at}"));
        limits.push(rung.limit_price);
        floored.push(
            confirmed
                .draft("OrderRequestRecorded")
                .and_then(|d| d.payload.get("at_floor"))
                .cloned(),
        );
        resting = rung.client_order_id.as_str().to_owned();
    }
    assert_eq!(
        limits,
        vec![Some(price("153.45")), Some(price("153"))],
        "155 x 0.99 = 153.45 is above the floor; 155 x 0.985 = 152.675 is clamped to 153 (§5.5, \
         §5.6: the ladder never prices below the floor)"
    );
    assert_eq!(
        floored,
        vec![
            Some(mandate_canon::Value::Bool(false)),
            Some(mandate_canon::Value::Bool(true))
        ],
        "the clamped rung is journaled at the floor"
    );
    assert_eq!(
        alerts.get(1).copied(),
        Some(true),
        "§5.6 (3): the owner is alerted on the rung the confirmed floor clamps to 153, not on \
         153.45, which is above it"
    );
    for at in [26_i64, 31] {
        let rested = shell.run(Input::Tick(clock(at)), &ports);
        assert!(
            !rested
                .requests
                .iter()
                .any(|r| matches!(r, BrokerRequest::Cancel { .. })),
            "at the floor the remainder rests: it is not cancelled to step at {at}: {:?}",
            rested.requests
        );
    }
}

/// §2.3 and §5.5 (DEC-160 (11), (24)): a watchdog exit in a position no single agent holds
/// belongs to no agent (`*`). An agent-scoped kill switch in an instrument it closes cancels it by
/// its own `client_order_id`, never by cancel-all, and never treats it as the agent's own: the
/// switch sells exactly the agent's attributed lots, not the position the exit was sized to.
#[test]
fn an_agent_kill_switch_cancels_a_watchdog_exit_of_no_agent_and_sells_only_its_own_lots() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let unattributed = event(
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
    shell
        .fold_one(&unattributed)
        .expect("ten shares no order of ours accounts for fold");
    let mine = [
        event(
            ACCOUNT_STREAM,
            3,
            "OrderSubmitted",
            with_clock(
                &[
                    ("client_order_id", text("md-held-1")),
                    ("agent", text(common::AGENT)),
                    ("instrument", text(AAPL)),
                    ("side", text("buy")),
                    ("qty", text("5")),
                    ("limit", text("150")),
                ],
                10,
            ),
        ),
        event(
            ACCOUNT_STREAM,
            4,
            "FillApplied",
            with_clock(
                &[
                    ("fill_id", text("f-1")),
                    ("client_order_id", text("md-held-1")),
                    ("instrument", text(AAPL)),
                    ("side", text("buy")),
                    ("qty_gross", text("5")),
                    ("price", text("150")),
                ],
                10,
            ),
        ),
        event(
            ACCOUNT_STREAM,
            5,
            "OrderStateChanged",
            with_clock(
                &[
                    ("client_order_id", text("md-held-1")),
                    ("state", text("filled")),
                ],
                10,
            ),
        ),
        event(
            ACCOUNT_STREAM,
            6,
            "ProtectionChanged",
            with_clock(
                &[
                    ("instrument", text(AAPL)),
                    ("action", text("placed")),
                    ("orders", text("md-oco-1")),
                    ("qty", text("15")),
                    ("take_profit", text("170")),
                    ("stop", text("140")),
                    ("created_on", text("2026-09-22")),
                ],
                11,
            ),
        ),
    ];
    for event in &mine {
        shell
            .fold_one(event)
            .expect("the agent's five and the OCO over all fifteen fold");
    }
    let mut shell = shell.restart_ready(&ports);
    shell.run(Input::Market(quote(AAPL, "139", "139.1", 30)), &ports);
    let fired = shell.run(Input::Tick(clock(95)), &ports);
    let watchdog = fired
        .drafts
        .iter()
        .find(|d| {
            d.event_type == "ProtectionChanged"
                && d.payload
                    .get("action")
                    .and_then(mandate_canon::Value::as_str)
                    == Some("watchdog")
        })
        .expect("the stop at 140 under a 139 mark for stop_watchdog_s fires the watchdog");
    assert_eq!(
        watchdog
            .payload
            .get("agent")
            .and_then(mandate_canon::Value::as_str),
        Some("*"),
        "five of fifteen is no single holder, so the exit belongs to no agent (§2.3)"
    );
    let exited = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );
    let exit = exited
        .submissions()
        .first()
        .copied()
        .cloned()
        .expect("the watchdog's exit goes once the OCO's cancel is confirmed");
    assert_eq!(exit.qty, qty("15"), "for the whole covered position");
    let watchdog_id = exit.client_order_id.as_str().to_owned();
    assert!(
        watchdog_id.starts_with("md-w-"),
        "named as the watchdog's own (§2.3): {watchdog_id}"
    );
    shell.run(
        Input::Broker(Ok(BrokerOutcome::Submitted(broker_order(
            "b-w",
            Some(&watchdog_id),
            AAPL,
            Side::Sell,
            "15",
            "0",
            "new",
        )))),
        &ports,
    );

    let switched = shell.run(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(agent(common::AGENT)),
            initiator: Initiator::RiskLimit,
            confirmation: None,
        }),
        &ports,
    );

    assert!(
        switched.requests.iter().any(|r| matches!(
            r,
            BrokerRequest::Cancel { client_order_id } if client_order_id.as_str() == watchdog_id
        )),
        "the switch closes AAPL, so it cancels the `*` exit there by its own id (§5.5): {:?}",
        switched.requests
    );
    assert!(
        !switched.requests.iter().any(common::is_account_wide),
        "and never through the account-wide endpoints"
    );
    assert!(
        switched.submissions().is_empty(),
        "nothing is sold while that cancel is unconfirmed"
    );
    let after = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: watchdog_id.clone(),
        })),
        &ports,
    );
    let sell = after
        .submissions()
        .into_iter()
        .find(|o| o.side == Side::Sell)
        .expect("the agent's own lots are sold once the exit's cancel is confirmed");
    assert_eq!(
        sell.qty,
        qty("5"),
        "exactly the agent's attributed lots: neither the fifteen the watchdog exit was sized to \
         nor the ten nobody attributed (§5.5)"
    );
    assert_ne!(
        sell.client_order_id.as_str(),
        watchdog_id,
        "under the switch's own id, never the watchdog's"
    );
    let sold = sell.client_order_id.as_str().to_owned();
    let filled = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(mandate_executor::BrokerFill {
            side: Side::Sell,
            ..broker_fill("f-2", Some(&sold), "5", "139")
        })),
        &ports,
    );
    let re_placed = filled
        .drafts
        .iter()
        .find(|d| {
            d.event_type == "ProtectionChanged"
                && d.payload.get("action").and_then(mandate_canon::Value::as_str) == Some("placed")
        })
        .expect("the ten shares nobody attributed are re-protected once the switch's sell is done (§5.4)");
    assert_eq!(
        re_placed
            .payload
            .get("qty")
            .and_then(mandate_canon::Value::as_str),
        Some("10"),
        "for exactly what is left, neither the fifteen the watchdog covered nor the five sold"
    );
    assert!(
        filled
            .submissions()
            .iter()
            .any(|o| o.purpose == Purpose::Protective && o.qty == qty("10")),
        "and the OCO for them goes to the broker: {:?}",
        filled.submissions()
    );
}

#[test]
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
                ("non_marginable_buying_power", text("0")),
                ("accrued_fees", text("0")),
                ("equity", text("0")),
                ("cash", text("0")),
            ],
            10,
        ),
    );
    shell
        .fold_one(&broke)
        .expect("a zero-buying-power account folds");
    for event in protected_position_events(3) {
        shell
            .fold_one(&event)
            .expect("the protected position folds");
    }
    let (mut shell, _) = shell.restart(&ports);
    assert!(
        shell
            .state
            .buying_power()
            .is_some_and(|power| power <= usd("0")),
        "the account has no buying power at all: {:?}",
        shell.state.buying_power()
    );

    let day = copied(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "TradingDayStarted",
        with_clock(&[("date", text("2026-12-14"))], 7_000_000),
        &EventId(format!("{CLOCK_STREAM}-9")),
    );
    shell.fold_one(&day).expect("the buffer day folds");
    shell.run(Input::Journal(day), &ports);
    let ran = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );

    let protective = ran
        .submissions()
        .into_iter()
        .find(|o| o.purpose == Purpose::Protective)
        .expect(
            "the re-placement before expiry is submitted with no buying power: a protective order \
             is never denied for it (AGENTS.md rule 13)",
        );
    let oco = protective.oco.clone().expect("as a GTC OCO");
    assert_eq!(
        (oco.qty, oco.take_profit, oco.stop),
        (qty("10"), price("170"), price("140")),
        "for the held ten shares at the journaled prices of the protection it replaces, which the \
         executor re-places and never invents"
    );
}

#[test]
fn an_unknown_order_holds_an_exit_in_that_instrument_alone() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, CPHC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    for event in protected_position_events(2).iter().take(3) {
        shell
            .fold_one(event)
            .expect("the held position folds unprotected, so the opening is one the gate allows");
    }
    let mut shell = shell.restart_ready(&ports);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), &ports);
    shell.run(Input::Market(quote(CPHC, "20", "20.1", 20)), &ports);
    let opening_id = format!("md-{OTHER_INTENT}");
    let cphc_id = "md-cphc-1";
    let facts = [
        (
            "OrderSubmitted",
            vec![
                ("client_order_id", text(&opening_id)),
                ("intent_id", text(OTHER_INTENT)),
                ("agent", text(common::AGENT)),
                ("instrument", text(AAPL)),
                ("side", text("buy")),
                ("qty", text("1")),
                ("limit", text("150")),
                ("purpose", text("open")),
            ],
        ),
        (
            "OrderSubmitted",
            vec![
                ("client_order_id", text(cphc_id)),
                ("agent", text(common::AGENT)),
                ("instrument", text(CPHC)),
                ("side", text("buy")),
                ("qty", text("1")),
                ("limit", text("20")),
                ("purpose", text("open")),
            ],
        ),
        (
            "FillApplied",
            vec![
                ("fill_id", text("f-cphc")),
                ("client_order_id", text(cphc_id)),
                ("instrument", text(CPHC)),
                ("side", text("buy")),
                ("qty_gross", text("1")),
                ("price", text("20")),
            ],
        ),
        (
            "OrderStateChanged",
            vec![
                ("client_order_id", text(cphc_id)),
                ("state", text("filled")),
            ],
        ),
    ];
    for (event_type, pairs) in facts {
        let fact = event(
            ACCOUNT_STREAM,
            shell.head().0.saturating_add(1),
            event_type,
            with_clock(&pairs, 20),
        );
        shell
            .fold_one(&fact)
            .expect("the opening in flight and the CPHC share fold");
    }
    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);

    let held = shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "10", "155")),
        &ports,
    );
    assert!(
        held.submissions().is_empty(),
        "an Unknown order in the same instrument is one of the four holds rule 13 names"
    );
    let gate = held.draft("GateDecided").map(|draft| {
        (
            draft
                .payload
                .get("verdict")
                .and_then(mandate_canon::Value::as_str),
            draft
                .payload
                .get("reason_code")
                .and_then(mandate_canon::Value::as_str),
        )
    });
    assert_eq!(
        gate,
        Some((Some("hold"), Some("unknown_order_in_flight"))),
        "held for the Unknown, and named so"
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
fn entering_exits_only_cancels_the_working_opening_orders() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, opening_id) = protected_position_with_a_resting_buy(&ports);

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
fn entering_exits_only_leaves_protective_orders_resting() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, _) = protected_position_with_a_resting_buy(&ports);

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
            .expect("the protection accessor answers")
            .is_some_and(|p| !p.resting.is_empty()),
        "and the fold still carries them"
    );
}

#[test]
fn a_reducing_sell_cancels_the_resting_opening_buys_first() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, buy) = protected_position_with_a_resting_buy(&ports);
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
fn the_reducing_sell_waits_for_the_cancel_confirmation() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, buy) = protected_position_with_a_resting_buy(&ports);
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
fn a_reservation_lowers_buying_power_by_its_amount() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    let startup = snapshot(shell.head().0, ReconcileReason::Startup);
    shell.run(Input::BrokerSnapshot(startup), &ports);
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
fn a_paper_fill_books_a_simulated_fee_in_the_shadow_ledger() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    let held = event(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
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
    shell.fold_one(&held).expect("the ten shares fold");
    shell.run(Input::Market(quote(AAPL, "150", "150.1", 10)), &ports);
    let sent = shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "10", "150")),
        &ports,
    );
    let id = sent
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the unprotected position's exit is sent at once");

    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(mandate_executor::BrokerFill {
            side: Side::Sell,
            ..broker_fill("f-1", Some(&id), "10", "150")
        })),
        &ports,
    );

    let charged = ran
        .drafts
        .iter()
        .find(|d| d.event_type == "FeesCharged")
        .expect("Alpaca paper charges no regulatory fee, so ours is booked in the shadow ledger");
    assert_eq!(
        charged.payload.get("simulated"),
        Some(&mandate_canon::Value::Bool(true)),
        "and marked `simulated = true`, so it stays out of the cash comparison (§10)"
    );
    assert_eq!(
        charged
            .payload
            .get("accrued")
            .and_then(mandate_canon::Value::as_str),
        Some("0.0471"),
        "a sell of 10 at 150 under the `test_default` fee configuration, accrued at full precision \
         (§6.2): SEC 1500 x 0.00003 = 0.045, TAF 10 x 0.0002 = 0.002 (under the 9.79 cap), CAT \
         10 x 0.00001 = 0.0001"
    );
}

#[test]
fn a_simulated_fee_is_excluded_from_cash_reconciliation_and_included_in_buying_power() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Account(broker_account())),
        &ports,
    );
    let simulated = event(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "FeesCharged",
        with_clock(
            &[
                ("family", text("equities")),
                ("day", text("2026-09-22")),
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

    assert_eq!(
        journaled(&run.effects).last(),
        Some(&"ReconciliationRun"),
        "the cash row was compared and the run recorded"
    );
    assert!(
        run.differences
            .iter()
            .all(|d| d.kind != mandate_executor::DifferenceKind::Cash),
        "the broker's 20000 against the model's 20000: a simulated record that reached the cash \
         comparison would fail every paper reconciliation (§10, interpretation 25): {:?}",
        run.differences
    );
    assert_eq!(
        shell.state.buying_power(),
        Some(usd("19999.98")),
        "while the 0.02 still counts against buying power, or paper is flatter than live (R-22)"
    );
}

#[test]
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
        ExecutorError::BindingGateInputMissing,
        ExecutorError::BindingGateFailed {
            code: "unimplemented",
        },
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
        24,
        "the set is closed: a new variant adds a row here and a match arm in `code()`"
    );
}

/// 2026-09-22, a Tuesday, at 19:59:57 ET: three seconds before the after-hours session ends.
const THREE_SECONDS_BEFORE_THE_NIGHT: i64 = 1_790_121_597;
/// The Wednesday's 04:00 ET pre-market open, which ends that night's park.
const NEXT_OPEN: i64 = 1_790_150_400;

/// A risk exit for ten AAPL held with no protection resting, at a 150 bid three seconds before
/// the after-hours session ends: §5.6 ladders it alone, and its first rung goes after-hours at
/// 149.25; its step is asked at 20:00:03 and confirmed in the overnight session, where no rung
/// may go, so the ladder parks (DEC-260 (14), (18)). Answers the shell, the rung's client order
/// id, and the step's confirmation.
fn parked_in_the_night(ports: &mandate_executor::Ports<'_>) -> (Shell, String, common::Ran) {
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    for event in protected_position_events(2).into_iter().take(3) {
        shell.fold_one(&event).expect("the position folds");
    }
    let mut shell = shell.restart_ready(ports);
    shell.run(Input::Tick(clock(THREE_SECONDS_BEFORE_THE_NIGHT)), ports);
    shell.run(
        Input::Market(quote(AAPL, "150", "150.2", THREE_SECONDS_BEFORE_THE_NIGHT)),
        ports,
    );
    let sent = shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "10", "150")),
        ports,
    );
    let first = sent
        .submissions()
        .first()
        .copied()
        .cloned()
        .expect("the exit's first rung goes after-hours");
    assert_eq!(first.limit_price, Some(price("149.25")), "150 x (1 - 0.5%)");
    assert!(
        first.extended_hours,
        "after-hours, so marked extended (§5.5)"
    );
    let rung = first.client_order_id.as_str().to_owned();
    let (stepped, parked) = step_rung(&mut shell, ports, THREE_SECONDS_BEFORE_THE_NIGHT + 6, &rung);
    assert!(
        stepped.requests.iter().any(|request| matches!(
            request,
            BrokerRequest::Cancel { client_order_id } if client_order_id.as_str() == rung
        )),
        "the rung is cancelled to step at 20:00:03: {:?}",
        stepped.requests
    );
    assert!(
        parked.submissions().is_empty(),
        "no rung goes in the overnight session: {:?}",
        parked.submissions()
    );
    (shell, rung, parked)
}

/// Every notification a step raised.
fn notes_of(ran: &common::Ran) -> Vec<mandate_executor::NotificationRef> {
    ran.effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Notify(note) => Some(note.clone()),
            _ => None,
        })
        .collect()
}

/// The night after the park, swept with a quote and a tick every half hour from `from` to the
/// second before the next open: every notification raised, and anything sent or abandoned.
fn sweep_the_night(
    shell: &mut Shell,
    ports: &mandate_executor::Ports<'_>,
    from: i64,
) -> (Vec<mandate_executor::NotificationRef>, Vec<String>) {
    let mut notes = Vec::new();
    let mut wrong = Vec::new();
    let mut instants: Vec<i64> = (0..)
        .map(|n| from + n * 1_800)
        .take_while(|at| *at < NEXT_OPEN)
        .collect();
    instants.push(NEXT_OPEN - 1);
    for at in instants {
        for input in [
            Input::Market(quote(AAPL, "150", "150.2", at)),
            Input::Tick(clock(at)),
        ] {
            let ran = shell.run(input, ports);
            notes.extend(notes_of(&ran));
            wrong.extend(
                ran.submissions()
                    .iter()
                    .map(|order| format!("{} sent at {at}", order.client_order_id.as_str())),
            );
            wrong.extend(
                ran.draft_types()
                    .into_iter()
                    .filter(|kind| *kind == "OrderAbandoned")
                    .map(|kind| format!("{kind} at {at}")),
            );
        }
    }
    (notes, wrong)
}

/// At the next open, priced from a 151 bid then: the rungs sent.
fn at_the_open(shell: &mut Shell, ports: &mandate_executor::Ports<'_>) -> Vec<(String, bool)> {
    shell.run(Input::Market(quote(AAPL, "151", "151.2", NEXT_OPEN)), ports);
    let opened = shell.run(Input::Tick(clock(NEXT_OPEN)), ports);
    opened
        .submissions()
        .iter()
        .map(|order| {
            assert_eq!(
                order.limit_price,
                Some(price("149.49")),
                "151 x (1 - 1%), the next rung"
            );
            (
                order.client_order_id.as_str().to_owned(),
                order.extended_hours,
            )
        })
        .collect()
}

/// The coordinator's ruling on #400 round 2 (5930410998, backlog E7-4 slice 6): a ladder that
/// parks at the close alerts the owner **once**, when it parks, for `session_closed` as for
/// `session_unknown`, so an exit never waits out an 8-hour night unannounced; and the park counts
/// in what `exit_held_long` reports, so no second alert follows at `max_intent_age_s`. The alert
/// names the park's own `GateDecided` and a generic key, nothing about the order (`AGENTS.md`
/// rule 6). The exit is never abandoned: at the next open it goes, from its next rung.
#[test]
fn a_parked_night_alerts_the_owner_exactly_once() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, rung, parked) = parked_in_the_night(&ports);
    let park = parked
        .draft("GateDecided")
        .cloned()
        .expect("the park is journaled");
    assert_eq!(park.payload.get("verdict"), Some(&text("hold")));
    assert_eq!(
        park.payload.get("reason_code"),
        Some(&text("session_closed"))
    );
    assert_eq!(
        park.payload.get("parked"),
        Some(&mandate_canon::Value::Bool(true))
    );
    assert_eq!(
        park.payload.get("held_long"),
        Some(&mandate_canon::Value::Bool(true)),
        "the park counts as a long hold from the start: nothing opens before 04:00"
    );
    let mut notes = notes_of(&parked);
    let (night, wrong) = sweep_the_night(&mut shell, &ports, THREE_SECONDS_BEFORE_THE_NIGHT + 60);
    notes.extend(night);
    assert!(wrong.is_empty(), "{wrong:?}");
    assert_eq!(
        notes.len(),
        1,
        "exactly one notification across the parked night: {notes:?}"
    );
    assert_eq!(
        notes[0].subject_event, park.event_id,
        "the alert names the park's own record"
    );
    assert_eq!(notes[0].message_key, "session_closed");
    assert_eq!(
        at_the_open(&mut shell, &ports),
        vec![(format!("{rung}-l1"), true)],
        "the exit goes at the 04:00 open from its next rung, extended, never abandoned"
    );
}

/// The same park across a restart in the night: the journal carries the park and its alert, so
/// the restarted process raises nothing more before the open, and still sends the exit's next
/// rung at the open (journal spec §8; `AGENTS.md` rules 3 and 13).
#[test]
fn a_restart_in_a_parked_night_alerts_nothing_more() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (shell, rung, parked) = parked_in_the_night(&ports);
    assert_eq!(
        parked.notifications,
        vec!["session_closed"],
        "the park alerts once, when it parks"
    );
    let (mut restarted, started) = shell.restart(&ports);
    assert!(
        started.notifications.is_empty(),
        "a restart re-alerts nothing: {:?}",
        started.notifications
    );
    let (notes, wrong) = sweep_the_night(
        &mut restarted,
        &ports,
        THREE_SECONDS_BEFORE_THE_NIGHT + 3_600,
    );
    assert!(wrong.is_empty(), "{wrong:?}");
    assert!(
        notes.is_empty(),
        "nothing more is raised after the restart: {notes:?}"
    );
    assert_eq!(
        at_the_open(&mut restarted, &ports),
        vec![(format!("{rung}-l1"), true)],
        "the exit still goes at the open"
    );
}
