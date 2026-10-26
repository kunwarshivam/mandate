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
        .and_then(|d| common::named_orders(&d.payload, "orders").pop())
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

/// An OCO the executor could not place again is never the one it cancels (DEC-2312 item 2): at
/// its buffer day, protection resting on a profile that offers no protective form at all is kept,
/// not cancelled, and the shortfall is journaled and the owner alerted, as for protection with no
/// recorded prices (`expiry_unreplaceable`, `protection_expiring`; DEC-367 item 4).
#[test]
fn an_oco_that_cannot_be_re_placed_is_kept_at_its_buffer_day() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    for event in protected_position_events(2) {
        shell
            .fold_one(&event)
            .expect("the protected position folds");
    }
    shell.profile = Some(unprotectable_profile());
    let mut shell = shell.restart_ready(&ports);

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
        !ran.requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::Cancel { .. })),
        "the OCO is kept: nothing could replace it (§5.4): {:?}",
        ran.requests
    );
    assert!(
        ran.drafts
            .iter()
            .any(|d| d.event_type == "ProtectionChanged"
                && d.payload
                    .get("action")
                    .and_then(mandate_canon::Value::as_str)
                    == Some("expiry_unreplaceable")),
        "the shortfall is journaled: {:?}",
        ran.draft_types()
    );
    assert!(
        ran.notifications.contains(&"protection_expiring"),
        "and the owner alerted: {:?}",
        ran.notifications
    );
}

/// The kept OCO of `an_oco_that_cannot_be_re_placed_is_kept_at_its_buffer_day` at its buffer
/// day, 2026-12-14, journaled and alerted there.
fn kept_at_the_buffer_day(ports: &mandate_executor::Ports<'_>) -> Shell {
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    for event in protected_position_events(2) {
        shell
            .fold_one(&event)
            .expect("the protected position folds");
    }
    shell.profile = Some(unprotectable_profile());
    let mut shell = shell.restart_ready(ports);
    trading_day(&mut shell, ports, "2026-12-14", 9);
    shell
}

/// Copies a `TradingDayStarted` for `date` onto the account stream and runs it.
fn trading_day(
    shell: &mut Shell,
    ports: &mandate_executor::Ports<'_>,
    date: &str,
    ordinal: u32,
) -> common::Ran {
    let day = copied(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "TradingDayStarted",
        with_clock(
            &[("date", text(date))],
            7_000_000 + i64::from(ordinal) * 86_400,
        ),
        &EventId(format!("{CLOCK_STREAM}-{ordinal}")),
    );
    shell.fold_one(&day).expect("the trading day folds");
    shell.run(Input::Journal(day), ports)
}

/// Whether `ran` journals `expiry_unreplaceable` or alerts `protection_expiring`.
fn alerts_the_kept_oco(ran: &common::Ran) -> bool {
    ran.notifications.contains(&"protection_expiring")
        || ran.drafts.iter().any(|d| {
            d.event_type == "ProtectionChanged"
                && d.payload
                    .get("action")
                    .and_then(mandate_canon::Value::as_str)
                    == Some("expiry_unreplaceable")
        })
}

/// DEC-2355 item 2 (accepted by the founder, 2026-10-25): a kept OCO is journaled and alerted
/// once for its resting set. The next trading day, with the same orders resting, journals and
/// alerts nothing, and neither does a restarted executor, since the fold keeps what was alerted.
#[test]
fn a_kept_oco_is_alerted_once_for_its_resting_set() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = kept_at_the_buffer_day(&ports);

    let next = trading_day(&mut shell, &ports, "2026-12-15", 10);
    assert!(!alerts_the_kept_oco(&next), "{:?}", next.draft_types());

    let (mut restarted, _) = shell.restart(&ports);
    let after = trading_day(&mut restarted, &ports, "2026-12-16", 11);
    assert!(!alerts_the_kept_oco(&after), "{:?}", after.draft_types());
}

/// DEC-2355 item 2: a change to what rests alerts again. A second tranche placed beside the kept
/// OCO, itself due, makes a new resting set, which the next trading day journals and alerts.
#[test]
fn a_changed_resting_set_is_alerted_again() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = kept_at_the_buffer_day(&ports);
    let tranche = event(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "ProtectionChanged",
        with_clock(
            &[
                ("instrument", text(AAPL)),
                ("action", text("placed")),
                ("orders", text("md-oco-2")),
                ("qty", text("5")),
                ("take_profit", text("170")),
                ("stop", text("140")),
                ("created_on", text("2026-09-22")),
            ],
            7_000_000 + 9 * 86_400 + 100,
        ),
    );
    shell.fold_one(&tranche).expect("the second tranche folds");

    let next = trading_day(&mut shell, &ports, "2026-12-15", 10);
    assert!(alerts_the_kept_oco(&next), "{:?}", next.draft_types());
}

/// The kept OCO of [`kept_at_the_buffer_day`] with a second tranche, `md-oco-2`, placed beside
/// it, the new resting set alerted on the next trading day, 2026-12-15.
fn two_tranches_alerted(ports: &mandate_executor::Ports<'_>) -> Shell {
    let mut shell = kept_at_the_buffer_day(ports);
    let tranche = event(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "ProtectionChanged",
        with_clock(
            &[
                ("instrument", text(AAPL)),
                ("action", text("placed")),
                ("orders", text("md-oco-2")),
                ("qty", text("5")),
                ("take_profit", text("170")),
                ("stop", text("140")),
                ("created_on", text("2026-09-22")),
            ],
            7_000_000 + 9 * 86_400 + 100,
        ),
    );
    shell.fold_one(&tranche).expect("the second tranche folds");
    let next = trading_day(&mut shell, ports, "2026-12-15", 10);
    assert!(alerts_the_kept_oco(&next), "{:?}", next.draft_types());
    shell
}

/// DEC-2355 item 1: a confirmed cancel of one of the kept orders changes the resting set, so the
/// kept OCO left resting is alerted again on the next trading day.
#[test]
fn a_cancel_of_a_kept_order_alerts_the_rest_again() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = two_tranches_alerted(&ports);
    let cancelled = event(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "ProtectionChanged",
        with_clock(
            &[
                ("instrument", text(AAPL)),
                ("action", text("cancelled")),
                ("orders", text("md-oco-2")),
                ("qty", text("5")),
            ],
            7_000_000 + 10 * 86_400 + 100,
        ),
    );
    shell.fold_one(&cancelled).expect("the cancel folds");

    let next = trading_day(&mut shell, &ports, "2026-12-16", 11);
    assert!(alerts_the_kept_oco(&next), "{:?}", next.draft_types());
}

/// DEC-2355 item 1: a kept order the broker reports ended, here expired, leaves the resting set,
/// so the kept OCO left resting is alerted again on the next trading day.
#[test]
fn a_kept_order_ended_at_the_broker_alerts_the_rest_again() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = two_tranches_alerted(&ports);
    let expired = event(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "OrderStateChanged",
        with_clock(
            &[
                ("client_order_id", text("md-oco-2")),
                ("state", text("expired")),
            ],
            7_000_000 + 10 * 86_400 + 100,
        ),
    );
    shell.fold_one(&expired).expect("the expiry folds");

    let next = trading_day(&mut shell, &ports, "2026-12-16", 11);
    assert!(alerts_the_kept_oco(&next), "{:?}", next.draft_types());
}

/// DEC-2355 item 1 (the independent review's probe): `re_cover`'s and `re_place`'s shortfall
/// record, `expiry_unreplaceable` naming no order, written before the buffer day, is not the
/// due-day alert, so the kept OCO is still journaled and alerted at its buffer day.
#[test]
fn a_shortfall_record_before_the_buffer_day_leaves_the_due_day_alert() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    for event in protected_position_events(2) {
        shell
            .fold_one(&event)
            .expect("the protected position folds");
    }
    shell.profile = Some(unprotectable_profile());
    let mut shell = shell.restart_ready(&ports);
    let shortfall = event(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "ProtectionChanged",
        with_clock(
            &[
                ("instrument", text(AAPL)),
                ("action", text("expiry_unreplaceable")),
            ],
            7_000_000,
        ),
    );
    shell.fold_one(&shortfall).expect("the shortfall folds");

    let due = trading_day(&mut shell, &ports, "2026-12-14", 9);
    assert!(alerts_the_kept_oco(&due), "{:?}", due.draft_types());
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
         rate: a stop-limit never outlasts the position once that fee posts (RC-07)"
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

/// What two or more crypto buy fills hold once each fill's taker fee is withheld, each rounded on
/// its own as §6.3 rounds a fill's fee, summed in whole nano-units by [`net_of_taker_fee`]'s own
/// arithmetic, never the crate's.
fn net_of_taker_fees(fills: &[&str]) -> String {
    let nanos = |text: &str| -> u128 {
        let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
        format!("{whole}{fraction:0<9}")
            .parse()
            .unwrap_or_else(|e| panic!("{text}: {e}"))
    };
    let net: u128 = fills
        .iter()
        .map(|fill| nanos(&net_of_taker_fee(fill)))
        .sum();
    let text = format!("{}.{:09}", net / 1_000_000_000, net % 1_000_000_000);
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// §6.3 rounds each crypto fill's fee on its own, so the stop-limit after an entry filled in parts
/// is the sum of each fill net of its own fee, never the sum net of one fee on the total: two fills
/// of 0.1000002 at 25 bps hold 0.099750199 each, 0.199500398 together, where one rounding over
/// 0.2000004 would cover 0.199500399, a nano-unit more than is held, which the broker refuses
/// (`AGENTS.md` rule 12; DEC-349 item 2).
#[test]
fn a_crypto_stop_limit_nets_each_fills_own_fee() {
    const FILL: &str = "0.1000002";
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
            protected_opening(BTC, "0.2000004", "60000", "54000", None),
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
            "0.2000004",
            "0.2000004",
            "filled",
        ))),
        &ports,
    );
    let mut stops = Vec::new();
    for fill in ["f-btc-1", "f-btc-2"] {
        let ran = shell.run(
            Input::BrokerUpdate(BrokerUpdate::Fill(mandate_executor::BrokerFill {
                instrument: instrument(BTC),
                ..broker_fill(fill, Some(&id), FILL, "60000")
            })),
            &ports,
        );
        stops.extend(
            ran.submissions()
                .into_iter()
                .filter(|o| o.order_type == mandate_executor::OrderType::StopLimit)
                .map(|o| o.qty),
        );
    }

    assert_eq!(
        net_of_taker_fees(&[FILL, FILL]),
        "0.199500398",
        "the oracle: each fill holds 0.1000002 less round(0.00025000050, 9, half_up) = 0.000250001"
    );
    assert_eq!(
        stops,
        vec![qty(&net_of_taker_fees(&[FILL, FILL]))],
        "one stop-limit, once both fills are applied, for the sum of each fill net of its own fee"
    );
}

/// A profile that lists an OCO for crypto, beside the one stop-limit, in every quantity form: the
/// shape crypto protection must never take whatever a profile offers (§5.4: simple orders only).
fn crypto_oco_profile() -> mandate_domain::CapabilityProfile {
    use mandate_domain::{
        AssetClass, Cell, Idempotency, MarketSession, OrderType as Kind, ProtectionForm,
        QuantityForm, Retry, Row, TimeInForce as Tif,
    };
    let cell = |order_type, quantity_form, forms: &[ProtectionForm]| Cell {
        order_type,
        quantity_form,
        times_in_force: [Tif::Gtc, Tif::Ioc].into_iter().collect(),
        protection_forms: forms.iter().copied().collect(),
    };
    let mut cells = Vec::new();
    for form in [QuantityForm::Whole, QuantityForm::Fractional] {
        cells.push(cell(Kind::Limit, form, &[ProtectionForm::Oco]));
        cells.push(cell(Kind::StopLimit, form, &[ProtectionForm::StopLimit]));
    }
    let row = Row {
        asset_class: AssetClass::Crypto,
        session: MarketSession::Crypto,
        cells,
    };
    let idempotency = Idempotency {
        client_order_id: true,
        retry: Retry::Unknown,
        query_by_client_order_id: true,
    };
    mandate_domain::CapabilityProfile::new(1, vec![row], idempotency).expect("a valid profile")
}

/// §5.4 gives crypto simple orders only: its protection is one GTC stop-limit even under a profile
/// that lists an OCO for crypto, and even for an entry that carries a take-profit (DEC-36).
#[test]
fn a_crypto_position_never_takes_an_oco_even_where_the_profile_lists_one() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    shell.profile = Some(crypto_oco_profile());
    let mut shell = shell.restart_ready(&ports);
    shell.run(Input::Market(quote(BTC, "60000", "60010", 20)), &ports);
    let submitted = shell.run(
        handoff(
            INTENT,
            common::AGENT,
            protected_opening(BTC, "0.5", "60000", "54000", Some("70000")),
        ),
        &ports,
    );
    let id = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the crypto entry is sent");
    assert!(
        submitted
            .submissions()
            .iter()
            .all(|o| o.bracket.is_none() && o.oco.is_none()),
        "the entry is never a bracket"
    );
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

    let protective: Vec<_> = ran
        .submissions()
        .into_iter()
        .filter(|o| o.purpose == Purpose::Protective)
        .collect();
    assert_eq!(protective.len(), 1, "one protective order: {protective:?}");
    assert!(
        protective.iter().all(|o| o.oco.is_none()
            && o.bracket.is_none()
            && o.order_type == mandate_executor::OrderType::StopLimit),
        "a stop-limit, never an OCO: {protective:?}"
    );
}

#[test]
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
    assert_eq!(
        protective.oco.as_ref().map(|legs| legs.qty),
        Some(qty("10")),
        "on Alpaca one OCO at the entry's prices, both legs for the whole ten, never the 0.4 \
         (DEC-2312 item 2)"
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

/// A fractionable position of `held` shares of `FRAC`, its whole ten protected by one resting GTC
/// OCO created on 2026-09-22, as the fractional slice leaves it after its entry fills (DEC-2312
/// item 2).
fn fractional_position(ports: &mandate_executor::Ports<'_>, held: &str) -> Shell {
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    for held in [
        event(
            ACCOUNT_STREAM,
            2,
            "OrderSubmitted",
            with_clock(
                &[
                    ("client_order_id", text("md-held-frac")),
                    ("agent", text(common::AGENT)),
                    ("instrument", text(FRAC)),
                    ("side", text("buy")),
                    ("qty", text(held)),
                    ("limit", text("20")),
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
                    ("client_order_id", text("md-held-frac")),
                    ("instrument", text(FRAC)),
                    ("side", text("buy")),
                    ("qty_gross", text(held)),
                    ("price", text("20")),
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
                    ("client_order_id", text("md-held-frac")),
                    ("state", text("filled")),
                ],
                10,
            ),
        ),
        event(
            ACCOUNT_STREAM,
            5,
            "ProtectionChanged",
            with_clock(
                &[
                    ("instrument", text(FRAC)),
                    ("action", text("placed")),
                    ("orders", text("md-oco-1")),
                    ("qty", text("10")),
                    ("take_profit", text("24")),
                    ("stop", text("18")),
                    ("created_on", text("2026-09-22")),
                ],
                11,
            ),
        ),
    ] {
        shell
            .fold_one(&held)
            .expect("the fractional position and its whole shares' protection fold");
    }
    shell.restart_ready(ports)
}

/// §5.2's Alpaca table keeps a fractional equity order out of OCO and bracket orders and makes it
/// TIF day, and §5.3 rule 7 says the same: a fractional entry goes as a plain DAY limit whatever
/// protective prices it carries, on a profile that offers brackets, and only that form (DEC-2312
/// item 1). A whole-share entry in the same instrument still goes as its bracket.
#[test]
fn a_fractional_entry_goes_as_a_plain_day_limit_whatever_its_prices() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[FRAC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    for (quantity, take_profit) in [("10.4", Some("24")), ("10.4", None), ("0.4", Some("24"))] {
        let mut shell = started();
        shell.fold_one(&stream_opened()).expect("folds");
        let mut shell = shell.restart_ready(&ports);
        shell.run(Input::Market(quote(FRAC, "20", "20.1", 20)), &ports);
        let ran = shell.run(
            handoff(
                INTENT,
                common::AGENT,
                protected_opening(FRAC, quantity, "20", "18", take_profit),
            ),
            &ports,
        );
        let entry = ran
            .submissions()
            .first()
            .copied()
            .unwrap_or_else(|| panic!("the fractional entry of {quantity} is sent"));
        assert_eq!(
            (
                entry.qty,
                entry.bracket.is_some(),
                entry.tif,
                entry.order_type
            ),
            (
                qty(quantity),
                false,
                mandate_executor::TimeInForce::Day,
                mandate_executor::OrderType::Limit
            ),
            "{quantity} with take-profit {take_profit:?}: never a bracket, always a DAY limit (§5.2)"
        );
        let intended = ran
            .drafts
            .iter()
            .find(|d| {
                d.event_type == "ProtectionChanged"
                    && d.payload
                        .get("action")
                        .and_then(mandate_canon::Value::as_str)
                        == Some("intended")
            })
            .unwrap_or_else(|| {
                panic!("{quantity}: the protective prices are journaled as intended")
            });
        assert_eq!(
            (
                intended
                    .payload
                    .get("stop")
                    .and_then(mandate_canon::Value::as_str),
                intended
                    .payload
                    .get("take_profit")
                    .and_then(mandate_canon::Value::as_str)
            ),
            (Some("18"), take_profit),
            "{quantity}: going plain keeps the prices its protection follows its fill at (DEC-446 \
             item 4)"
        );
    }

    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
    shell.run(Input::Market(quote(FRAC, "20", "20.1", 20)), &ports);
    let whole = shell.run(
        handoff(
            INTENT,
            common::AGENT,
            protected_opening(FRAC, "10", "20", "18", Some("24")),
        ),
        &ports,
    );
    assert!(
        whole
            .submissions()
            .first()
            .is_some_and(|entry| entry.bracket.is_some()),
        "a whole-share entry in a fractionable instrument is still its bracket (§5.4)"
    );
}

/// A fractional add is a plain order, not a bracket, so where protection already rests §5.3 rule 8
/// denies it as `add_blocked_by_protective_order`, as it denies any plain equity add: the
/// conservative reading (DEC-2312 item 1). Nothing is sent and the resting OCO is not touched.
#[test]
fn a_fractional_add_where_protection_rests_is_denied_as_a_plain_add() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[FRAC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fractional_position(&ports, "10");
    shell.run(Input::Market(quote(FRAC, "20", "20.1", 20)), &ports);

    let ran = shell.run(
        handoff(
            INTENT,
            common::AGENT,
            common::protected_add(FRAC, "0.5", "20", "18", Some("24")),
        ),
        &ports,
    );

    let decided = ran
        .draft("GateDecided")
        .expect("the add is gated and the decision journaled");
    assert_eq!(
        (
            decided
                .payload
                .get("verdict")
                .and_then(mandate_canon::Value::as_str),
            decided
                .payload
                .get("reason_code")
                .and_then(mandate_canon::Value::as_str)
        ),
        (Some("deny"), Some("add_blocked_by_protective_order")),
        "a plain add beside resting protection (§5.3 rule 8)"
    );
    assert!(
        ran.requests.is_empty(),
        "nothing is sent and the OCO keeps resting: {:?}",
        ran.requests
    );
}

/// A position under one share has no whole share to protect (§5.4): once its entry fills, no
/// protective order is sent, the interval it started ends, and both the start and the end
/// disclose the whole position as the fraction left unprotected (DEC-2312 items 2 and 3, DEC-2315).
#[test]
fn a_position_under_one_share_places_nothing_and_discloses_its_fraction() {
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
            protected_opening(FRAC, "0.4", "20", "18", Some("24")),
        ),
        &ports,
    );
    let id = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the entry under one share is sent");

    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&id),
            FRAC,
            Side::Buy,
            "0.4",
            "0.4",
            "filled",
        ))),
        &ports,
    );

    assert!(
        ran.submissions().is_empty(),
        "no whole share, so no protective order: {:?}",
        ran.requests
    );
    let changes: Vec<(Option<&str>, Option<&str>)> = ran
        .drafts
        .iter()
        .filter(|d| d.event_type == "ProtectionChanged")
        .map(|d| {
            (
                d.payload
                    .get("action")
                    .and_then(mandate_canon::Value::as_str),
                d.payload
                    .get("unprotected_fraction")
                    .and_then(mandate_canon::Value::as_str),
            )
        })
        .collect();
    assert_eq!(
        changes,
        vec![
            (Some("unprotected_start"), Some("0.4")),
            (Some("unprotected_end"), Some("0.4"))
        ],
        "the start discloses the fraction, and the interval ends with nothing to cover and the \
         position's final fraction"
    );
}

/// §5.4's re-placement before expiry re-places the whole shares only: a held 10.4 whose OCO for ten
/// nears its GTC expiry is cancelled, confirmed, and re-placed for ten, never 10.4, and the
/// placement discloses the 0.4 it leaves unprotected (DEC-2312 items 2 and 3).
#[test]
fn a_re_placement_before_expiry_covers_only_the_whole_shares() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[FRAC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fractional_position(&ports, "10.4");
    let day = copied(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "TradingDayStarted",
        with_clock(&[("date", text("2026-12-14"))], 7_000_000),
        &EventId(format!("{CLOCK_STREAM}-9")),
    );
    shell.fold_one(&day).expect("the trading day folds");
    let cancelled = shell.run(Input::Journal(day), &ports);
    assert!(
        cancelled
            .requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::Cancel { .. })),
        "the OCO created 2026-09-22 is cancelled at its buffer day (§5.4): {:?}",
        cancelled.requests
    );

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
        .expect("the whole shares are protected again");
    assert_eq!(
        protective.qty,
        qty("10"),
        "the whole shares only: a fraction is not allowed in an OCO (§5.2)"
    );
    let placed = ran
        .drafts
        .iter()
        .find(|d| {
            d.event_type == "ProtectionChanged"
                && d.payload
                    .get("action")
                    .and_then(mandate_canon::Value::as_str)
                    == Some("placed")
        })
        .expect("the re-placement is journaled");
    assert_eq!(
        placed
            .payload
            .get("unprotected_fraction")
            .and_then(mandate_canon::Value::as_str),
        Some("0.4"),
        "and discloses the fraction it leaves"
    );
}

/// A profile that offers a US equity a plain limit and no form at all to protect a position with:
/// an entry on it goes plain, and its fill leaves nothing to place, which is journaled and alerted
/// (`expiry_unreplaceable`, DEC-838 item 1).
fn unprotectable_profile() -> mandate_domain::CapabilityProfile {
    use mandate_domain::{
        AssetClass, Cell, Idempotency, MarketSession, OrderType as Kind, QuantityForm, Retry, Row,
        TimeInForce as Tif,
    };
    let row = Row {
        asset_class: AssetClass::UsEquity,
        session: MarketSession::Regular,
        cells: vec![Cell {
            order_type: Kind::Limit,
            quantity_form: QuantityForm::Whole,
            times_in_force: [Tif::Day, Tif::Gtc].into_iter().collect(),
            protection_forms: Default::default(),
        }],
    };
    let idempotency = Idempotency {
        client_order_id: true,
        retry: Retry::Unknown,
        query_by_client_order_id: true,
    };
    mandate_domain::CapabilityProfile::new(1, vec![row], idempotency).expect("a valid profile")
}

/// Every `ProtectionChanged` the executor writes is version 2, with `unprotected_fraction` present
/// on every record and null where nothing is disclosed (journal spec §9.5, rule 44; DEC-2312 item
/// 3). Three runs between them write `intended`, `unprotected_start`, `cancelled`, `placed`,
/// `unprotected_end` and `expiry_unreplaceable`: a whole-share entry, a fractional position's
/// re-placement before expiry, and an entry on a profile with nothing to protect it with.
#[test]
fn every_protection_changed_is_written_at_version_2() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, FRAC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut drafts = Vec::new();

    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
    shell.run(Input::Market(quote(AAPL, "150", "150.1", 20)), &ports);
    let intended = shell.run(
        handoff(
            INTENT,
            common::AGENT,
            protected_opening(AAPL, "10", "150", "140", Some("170")),
        ),
        &ports,
    );
    drafts.extend(intended.drafts);

    let mut shell = fractional_position(&ports, "10.4");
    let day = copied(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "TradingDayStarted",
        with_clock(&[("date", text("2026-12-14"))], 7_000_000),
        &EventId(format!("{CLOCK_STREAM}-9")),
    );
    shell.fold_one(&day).expect("the trading day folds");
    drafts.extend(shell.run(Input::Journal(day), &ports).drafts);
    let replaced = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );
    drafts.extend(replaced.drafts);

    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    shell.profile = Some(unprotectable_profile());
    let mut shell = shell.restart_ready(&ports);
    shell.run(Input::Market(quote(AAPL, "150", "150.1", 20)), &ports);
    let submitted = shell.run(
        handoff(
            INTENT,
            common::AGENT,
            protected_opening(AAPL, "10", "150", "140", Some("170")),
        ),
        &ports,
    );
    let id = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the plain entry is sent");
    drafts.extend(submitted.drafts);
    let filled = shell.run(
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
    drafts.extend(filled.drafts);

    let records: Vec<_> = drafts
        .iter()
        .filter(|d| d.event_type == "ProtectionChanged")
        .collect();
    let off: Vec<_> = records
        .iter()
        .filter(|d| d.schema_version != 2 || d.payload.get("unprotected_fraction").is_none())
        .map(|d| (d.schema_version, d.payload.get("action")))
        .collect();
    assert!(
        off.is_empty(),
        "every record is version 2 and carries the fraction, null or not: {off:?}"
    );
    let actions: std::collections::BTreeSet<&str> = records
        .iter()
        .filter_map(|d| {
            d.payload
                .get("action")
                .and_then(mandate_canon::Value::as_str)
        })
        .collect();
    for action in [
        "intended",
        "unprotected_start",
        "cancelled",
        "placed",
        "unprotected_end",
        "expiry_unreplaceable",
    ] {
        assert!(
            actions.contains(action),
            "the runs write `{action}`: {actions:?}"
        );
    }
}

/// The fractional rule is an equity rule (§5.2's equities row; DEC-2312 item 1): a crypto quantity
/// is fractional by nature and still goes as crypto's GTC simple limit, allowed by the gate, never
/// held to the equity's DAY form.
#[test]
fn a_crypto_entry_is_never_held_to_the_fractional_equity_rule() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
    shell.run(Input::Market(quote(BTC, "60000", "60010", 20)), &ports);

    let ran = shell.run(
        handoff(
            INTENT,
            common::AGENT,
            protected_opening(BTC, "0.5", "60000", "54000", None),
        ),
        &ports,
    );

    let entry = ran
        .submissions()
        .first()
        .copied()
        .expect("the crypto entry is sent");
    assert_eq!(
        (
            entry.qty,
            entry.bracket.is_some(),
            entry.tif,
            entry.order_type
        ),
        (
            qty("0.5"),
            false,
            mandate_executor::TimeInForce::Gtc,
            mandate_executor::OrderType::Limit
        ),
        "crypto's own GTC simple limit (§5.2, §5.4)"
    );
}

/// A fractional hold at its protection's buffer day keeps its whole shares covered: the OCO for ten
/// is cancelled only to be re-placed for ten, and once the broker acknowledges the new one the
/// position is protected again, its interval ending with the 0.4 it leaves (§5.4; DEC-2312 item 2,
/// DEC-2315). An OCO the executor cannot re-place is never the one it cancels.
#[test]
fn a_fractional_hold_keeps_its_whole_shares_covered_through_the_buffer_day() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[FRAC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fractional_position(&ports, "10.4");
    let frac = instrument(FRAC);
    assert_eq!(
        mandate_executor::is_protected(&shell.state, &frac, &ports),
        Ok(true),
        "the whole ten are covered before the buffer day"
    );
    let day = copied(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "TradingDayStarted",
        with_clock(&[("date", text("2026-12-14"))], 7_000_000),
        &EventId(format!("{CLOCK_STREAM}-9")),
    );
    shell.fold_one(&day).expect("the trading day folds");
    shell.run(Input::Journal(day), &ports);
    let replaced = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );
    let new = replaced
        .submissions()
        .into_iter()
        .find(|o| o.purpose == Purpose::Protective)
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the whole ten are protected again");
    let ended = replaced
        .drafts
        .iter()
        .find(|d| {
            d.event_type == "ProtectionChanged"
                && d.payload
                    .get("action")
                    .and_then(mandate_canon::Value::as_str)
                    == Some("unprotected_end")
        })
        .expect("the re-placement ends its interval");
    assert_eq!(
        ended
            .payload
            .get("unprotected_fraction")
            .and_then(mandate_canon::Value::as_str),
        Some("0.4"),
        "and discloses the fraction it leaves (DEC-2315)"
    );

    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-2",
            Some(&new),
            FRAC,
            Side::Sell,
            "10",
            "0",
            "accepted",
        ))),
        &ports,
    );
    assert_eq!(
        mandate_executor::is_protected(&shell.state, &frac, &ports),
        Ok(true),
        "once the broker acknowledges the new OCO, the whole ten are covered again"
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

/// DEC-2358 (its review's pin): a discretionary exit with a limit of its own, laddered from a
/// fresh quote inside its sequence, whose price vanishes mid-sequence (the quote is more than five
/// minutes old at its next rung, which a `max_unprotected_s` above five minutes allows) goes on at
/// its own limit, journaled and alerted `exit_unpriced`, and the sequence goes on: nothing is
/// re-placed, as it would be for an exit with no limit that ends.
#[test]
fn a_discretionary_exits_rung_falls_back_to_its_own_limit_when_its_price_vanishes() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let mut config = config();
    config.max_unprotected_s = 900;
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), &ports);
    shell.run(
        handoff(INTENT, common::AGENT, discretionary_exit(AAPL, "10", "150")),
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
        .expect("the exit's first rung goes once the OCO's cancel is confirmed");
    assert_ne!(
        first.limit_price,
        Some(price("150")),
        "the first rung is laddered from the fresh quote"
    );

    let (_, confirmed) = step_rung(&mut shell, &ports, 20 + 400, first.client_order_id.as_str());

    let next = confirmed
        .submissions()
        .first()
        .copied()
        .cloned()
        .expect("the next rung goes, at the exit's own limit");
    assert_eq!(
        (next.limit_price, next.qty, next.purpose),
        (Some(price("150")), qty("10"), Purpose::DiscretionaryExit),
        "{:?}",
        confirmed.draft_types()
    );
    assert!(
        confirmed.notifications.contains(&"exit_unpriced"),
        "{:?}",
        confirmed.notifications
    );
    assert!(
        !confirmed
            .drafts
            .iter()
            .any(|d| d.event_type == "ProtectionChanged"
                && d.payload
                    .get("action")
                    .and_then(mandate_canon::Value::as_str)
                    == Some("placed")),
        "the sequence goes on, so nothing is re-placed: {:?}",
        confirmed.draft_types()
    );
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
            cause: None,
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

/// DEC-2347 item 3.5: the agent whose own buy filled holds lots, the quantity its kill switch would
/// sell, and another agent, with none of its own, holds none in the same position.
#[test]
fn only_the_agent_whose_buy_filled_holds_lots() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let shell = protected_position(&ports);
    assert_eq!(shell.state.holds_lots(&agent(common::AGENT)), Ok(true));
    assert_eq!(shell.state.holds_lots(&agent(OTHER_AGENT)), Ok(false));
}

/// A reconciliation whose broker reports one fill of 10 AAPL at 150 naming only the broker's order
/// id `broker`, as Alpaca's `FILL` activity does, lists no open order (a filled order is not open),
/// and holds `held` AAPL.
fn fill_by_broker_id(
    shell: &mut Shell,
    ports: &mandate_executor::Ports<'_>,
    broker: &str,
    held: &str,
) -> common::Ran {
    let mut taken = snapshot(shell.head().0, ReconcileReason::Startup);
    let mut fill = broker_fill("f-1", None, "10", "150");
    fill.broker_order_id = Some(broker.to_owned());
    taken.fills = vec![fill];
    taken.positions = vec![broker_position(AAPL, held)];
    shell.run(Input::BrokerSnapshot(taken), ports)
}

/// The agent's switch after a quote, and the quantities of the sells it sends.
fn switch_sells(shell: &mut Shell, ports: &mandate_executor::Ports<'_>) -> Vec<String> {
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), ports);
    let ran = shell.run(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(agent(common::AGENT)),
            initiator: Initiator::Owner,
            confirmation: Some(owner_confirmation()),
            cause: None,
        }),
        ports,
    );
    ran.submissions()
        .into_iter()
        .filter(|order| order.side == Side::Sell)
        .map(|order| order.qty.to_string())
        .collect()
}

/// DEC-2353 item 2: a fill naming only the broker's order id the order's acknowledgment journaled
/// is that order's, so its quantity is applied to it and the agent holds the lots.
#[test]
fn a_fill_naming_only_the_broker_id_is_its_orders() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    let id = accepted_order(&mut shell, &ports, INTENT);
    let ran = fill_by_broker_id(&mut shell, &ports, "b-1", "10");
    let applied = ran.draft("FillApplied").expect("the fill is applied");
    assert_eq!(
        applied.payload.get("client_order_id"),
        Some(&text(&id)),
        "{:?}",
        ran.draft_types()
    );
    assert!(!ran.draft_types().contains(&"ExternalActivityIngested"));
    let order = shell.state.orders().values().next().expect("the order");
    assert_eq!(order.filled_qty, qty("10"));
    assert_eq!(shell.state.holds_lots(&agent(common::AGENT)), Ok(true));
}

/// DEC-2353, rule 13: once the broker-id fill is the agent's, its kill switch sells exactly it.
#[test]
fn the_agents_switch_sells_the_lots_a_broker_id_fill_bought() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    accepted_order(&mut shell, &ports, INTENT);
    fill_by_broker_id(&mut shell, &ports, "b-1", "10");
    assert_eq!(switch_sells(&mut shell, &ports), ["10"]);
}

/// DEC-2353, the review of #2270: a buy that filled 5 of its 10 and then ended canceled or expired,
/// its fill naming only the broker's order id, is the agent's, so its switch sells exactly the 5.
#[test]
fn the_agents_switch_sells_what_an_ended_buys_broker_id_fill_bought() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    for status in ["canceled", "expired"] {
        let mut shell = reconciling(&ports);
        let id = accepted_order(&mut shell, &ports, INTENT);
        let mut taken = snapshot(shell.head().0, ReconcileReason::Startup);
        taken.open_orders = vec![broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Buy,
            "10",
            "5",
            status,
        )];
        let mut fill = broker_fill("f-1", None, "5", "150");
        fill.broker_order_id = Some("b-1".to_owned());
        taken.fills = vec![fill];
        taken.positions = vec![broker_position(AAPL, "5")];
        shell.run(Input::BrokerSnapshot(taken), &ports);
        assert!(
            shell
                .state
                .orders()
                .values()
                .all(|order| order.state.is_terminal() && order.state != OrderState::Filled),
            "{status}: the order ended"
        );
        assert_eq!(switch_sells(&mut shell, &ports), ["5"], "{status}");
    }
}

/// DEC-2353 item 3 (the review of #2299): a fill naming only the broker's order id is no order's
/// when that order's records carry two ids, and it is not taken past the order's own checks: more
/// than the order has left, the other side, or another instrument. Each stays external activity,
/// every agent goes `exits_only`, and nothing is applied to the order; the account holds what the
/// fill bought, so no position difference is in play.
#[test]
fn a_broker_id_fill_failing_its_orders_checks_stays_external() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    for case in ["two ids", "overfill", "side", "instrument"] {
        let mut shell = reconciling(&ports);
        let id = accepted_order(&mut shell, &ports, INTENT);
        let mut fill = broker_fill("f-1", None, "10", "150");
        fill.broker_order_id = Some("b-1".to_owned());
        let mut held = (AAPL, "10");
        match case {
            "two ids" => {
                shell.run(
                    Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
                        "b-2",
                        Some(&id),
                        AAPL,
                        Side::Buy,
                        "10",
                        "0",
                        "accepted",
                    ))),
                    &ports,
                );
            }
            "overfill" => {
                fill.qty = qty("11");
                held = (AAPL, "11");
            }
            "side" => {
                fill.side = Side::Sell;
                held = (AAPL, "-10");
            }
            _ => {
                fill.instrument = instrument(FixedInstruments::THIN_EQUITY);
                held = (FixedInstruments::THIN_EQUITY, "10");
            }
        }
        let mut taken = snapshot(shell.head().0, ReconcileReason::Startup);
        taken.fills = vec![fill];
        taken.positions = vec![broker_position(held.0, held.1)];
        let ran = shell.run(Input::BrokerSnapshot(taken), &ports);
        assert!(
            ran.draft_types().contains(&"ExternalActivityIngested"),
            "{case}: {:?}",
            ran.draft_types()
        );
        assert_eq!(
            ran.draft("FillApplied")
                .and_then(|applied| applied.payload.get("client_order_id")),
            None,
            "{case}"
        );
        assert!(
            shell
                .state
                .orders()
                .values()
                .all(|order| order.filled_qty == qty("0")),
            "{case}"
        );
        assert_eq!(
            shell.state.effective_mode(&agent(common::AGENT)),
            Mode::ExitsOnly,
            "{case}"
        );
    }
}

/// DEC-2353 invariant 4: the broker id is folded from the journaled acknowledgment, so after a
/// restart the fill is still the order's.
#[test]
fn a_broker_id_fill_is_the_orders_after_a_restart() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = reconciling(&ports);
    let id = accepted_order(&mut shell, &ports, INTENT);
    let (mut restarted, _) = shell.restart(&ports);
    let ran = fill_by_broker_id(&mut restarted, &ports, "b-1", "10");
    let applied = ran.draft("FillApplied").expect("the fill is applied");
    assert_eq!(applied.payload.get("client_order_id"), Some(&text(&id)));
}

/// DEC-2353 item 3, invariant 3: a fill whose broker id no order's records carry, or two orders'
/// records carry, is no agent's. It is external activity, every agent goes `exits_only`, and the
/// agent's switch sells none of it.
#[test]
fn a_fill_whose_broker_id_names_no_one_order_stays_external() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    for two_orders in [false, true] {
        let mut shell = reconciling(&ports);
        accepted_order(&mut shell, &ports, INTENT);
        let broker = if two_orders {
            accepted_order(&mut shell, &ports, OTHER_INTENT);
            "b-1"
        } else {
            "b-unknown"
        };
        let ran = fill_by_broker_id(&mut shell, &ports, broker, "10");
        assert!(
            ran.draft_types().contains(&"ExternalActivityIngested"),
            "two orders: {two_orders}, {:?}",
            ran.draft_types()
        );
        assert_eq!(
            ran.draft("FillApplied")
                .and_then(|applied| applied.payload.get("client_order_id")),
            None,
            "two orders: {two_orders}"
        );
        assert_eq!(
            shell.state.effective_mode(&agent(common::AGENT)),
            Mode::ExitsOnly,
            "two orders: {two_orders}"
        );
        assert!(
            switch_sells(&mut shell, &ports).is_empty(),
            "two orders: {two_orders}"
        );
    }
}

/// The agent's buy of 10 the broker reports in `status` with `filled` of it filled, after a
/// reconciliation, with its fill naming no order.
fn ended_without_its_fill(
    status: &str,
    filled: &str,
    fills: Vec<mandate_executor::BrokerFill>,
    held: &str,
) -> Shell {
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
        filled,
        status,
    )];
    taken.fills = fills;
    taken.positions = vec![broker_position(AAPL, held)];
    shell.run(Input::BrokerSnapshot(taken), &ports);
    shell
}

/// DEC-2347 item 3.6 (lane D1's finding): a buy the broker reports filled whose fill named no
/// order leaves no lots in the sub-ledger, but the shares the account holds are the agent's, so it
/// still holds lots; once the account holds none of the instrument, it holds none.
#[test]
fn a_filled_buy_whose_fill_named_no_order_still_holds_lots() {
    let bought = broker_fill("f-1", None, "10", "150");
    let mut sold = broker_fill("f-2", None, "10", "151");
    sold.side = Side::Sell;
    let holding = ended_without_its_fill("filled", "10", vec![bought.clone()], "10");
    assert!(
        holding
            .state
            .orders()
            .values()
            .all(|order| order.filled_qty == qty("0")),
        "the fill is applied to no order"
    );
    assert_eq!(holding.state.holds_lots(&agent(common::AGENT)), Ok(true));
    assert_eq!(holding.state.holds_lots(&agent(OTHER_AGENT)), Ok(false));
    let flat = ended_without_its_fill("filled", "10", vec![bought, sold], "0");
    assert_eq!(flat.state.holds_lots(&agent(common::AGENT)), Ok(false));
}

/// DEC-2347 item 3.6 (the review of #2270): a buy that filled 5 of its 10 and then ended canceled
/// or expired, its fill naming no order, still holds the 5 the account holds.
#[test]
fn a_partly_filled_buy_that_ended_with_its_fill_named_no_order_still_holds_lots() {
    for status in ["canceled", "expired"] {
        let fill = broker_fill("f-1", None, "5", "150");
        let ended = ended_without_its_fill(status, "5", vec![fill], "5");
        assert!(
            ended.state.orders().values().all(|order| {
                order.state.is_terminal()
                    && order.state != OrderState::Filled
                    && order.filled_qty == qty("0")
            }),
            "{status}: the order ended unfilled in the fold"
        );
        assert_eq!(
            ended.state.holds_lots(&agent(common::AGENT)),
            Ok(true),
            "{status}"
        );
    }
}

/// The owner's release of `qty` AAPL from `who`'s position (journal spec §9.27, mandate spec §3.1),
/// folded at the account stream's next position.
fn release(shell: &mut Shell, who: &str, qty: &str) {
    let released = event(
        ACCOUNT_STREAM,
        shell.head().0 + 1,
        "PositionReleased",
        with_clock(
            &[
                ("agent_id", text(who)),
                ("instrument", text(AAPL)),
                ("qty", text(qty)),
                ("cause", text("owner_release")),
            ],
            25,
        ),
    );
    shell.fold_one(&released).expect("the release folds");
}

/// The agent's switch over its protected position: the sells it sends once its OCO's cancel is
/// accepted.
fn protected_switch_sells(shell: &mut Shell, ports: &mandate_executor::Ports<'_>) -> Vec<String> {
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 30)), ports);
    shell.run(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(agent(common::AGENT)),
            initiator: Initiator::Owner,
            confirmation: Some(owner_confirmation()),
            cause: None,
        }),
        ports,
    );
    let after = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        ports,
    );
    after
        .submissions()
        .into_iter()
        .filter(|order| order.side == Side::Sell)
        .map(|order| order.qty.to_string())
        .collect()
}

/// DEC-2354 item 2: a release of part of the agent's 10 leaves its switch only the remainder,
/// and a release of all of it leaves the switch nothing to sell and the agent no lots.
#[test]
fn a_release_leaves_the_switch_only_what_the_agent_still_holds() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut partly = protected_position(&ports);
    release(&mut partly, common::AGENT, "4");
    assert_eq!(protected_switch_sells(&mut partly, &ports), ["6"]);
    let mut wholly = protected_position(&ports);
    release(&mut wholly, common::AGENT, "10");
    assert_eq!(wholly.state.holds_lots(&agent(common::AGENT)), Ok(false));
    assert!(protected_switch_sells(&mut wholly, &ports).is_empty());
}

/// DEC-2354 item 2: another agent's release takes nothing off this agent's lots.
#[test]
fn another_agents_release_leaves_this_agents_lots() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    release(&mut shell, OTHER_AGENT, "10");
    assert_eq!(shell.state.holds_lots(&agent(common::AGENT)), Ok(true));
    assert_eq!(protected_switch_sells(&mut shell, &ports), ["10"]);
}

/// DEC-2354 item 1: the release is folded from the journal, so a replay reads the agent's lots as
/// released too.
#[test]
fn a_release_is_read_again_on_replay() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    release(&mut shell, common::AGENT, "10");
    let replayed = shell.replay().expect("the stream replays");
    assert_eq!(replayed.holds_lots(&agent(common::AGENT)), Ok(false));
}

/// DEC-2354 item 3, the bound's terms: a buy the broker rejected, or one the executor abandoned,
/// bought nothing, so it gives its agent no lots in a position another agent holds, though it
/// never filled its whole quantity.
#[test]
fn a_rejected_or_abandoned_buy_gives_its_agent_no_lots() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    for ending in [OrderState::Rejected, OrderState::Abandoned] {
        let mut shell = protected_position(&ports);
        shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), &ports);
        let id = shell
            .run(
                handoff(
                    OTHER_INTENT,
                    OTHER_AGENT,
                    protected_opening(AAPL, "10", "150", "140", Some("170")),
                ),
                &ports,
            )
            .submissions()
            .first()
            .map(|o| o.client_order_id.as_str().to_owned())
            .expect("the other agent's buy is sent");
        if ending == OrderState::Rejected {
            shell.run(
                Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
                    "b-other",
                    Some(&id),
                    AAPL,
                    Side::Buy,
                    "10",
                    "0",
                    "rejected",
                ))),
                &ports,
            );
        } else {
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
            shell.run(absent(), &ports);
        }
        assert!(
            shell
                .state
                .orders()
                .values()
                .any(|order| order.agent == Some(agent(OTHER_AGENT)) && order.state == ending),
            "the other agent's buy ends {ending:?}"
        );
        assert_eq!(
            shell.state.holds_lots(&agent(OTHER_AGENT)),
            Ok(false),
            "{ending:?}"
        );
        assert_eq!(shell.state.holds_lots(&agent(common::AGENT)), Ok(true));
    }
}

/// DEC-2354 item 3 (the coordinator's guard): a buy that filled 5 of 10 with its fill naming no
/// order, then released, leaves the switch nothing to sell: it sells the attributed lots alone,
/// never the bound `holds_lots` reads, which still counts the buy whole and so keeps the exit.
#[test]
fn a_released_unattributed_partial_fill_leaves_the_switch_nothing() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let fill = broker_fill("f-1", None, "5", "150");
    let mut shell = ended_without_its_fill("canceled", "5", vec![fill], "5");
    release(&mut shell, common::AGENT, "5");
    assert_eq!(shell.state.holds_lots(&agent(common::AGENT)), Ok(true));
    let switched = shell.run(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(agent(common::AGENT)),
            initiator: Initiator::Owner,
            confirmation: Some(owner_confirmation()),
            cause: None,
        }),
        &ports,
    );
    assert!(
        switched
            .submissions()
            .into_iter()
            .all(|order| order.side != Side::Sell),
        "{:?}",
        switched.draft_types()
    );
}

/// The agent's owner switch over its protected position, its flatten raised and waiting on the
/// OCO's cancel, then the owner's release of `released` of the 10 shares before that cancel is
/// confirmed: what the confirmation sends.
fn released_while_the_flatten_waits(released: &str) -> common::Ran {
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
            cause: None,
        }),
        &ports,
    );
    release(&mut shell, common::AGENT, released);
    shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    )
}

/// DEC-2360 item 1, rule 13: a release written after the switch sized its flatten, before that
/// flatten was sent, shrinks it to the sub-ledger at send time, so the owner's 4 shares are never
/// offered, and the shorter send is journaled `rung_short`.
#[test]
fn a_release_while_the_flatten_waits_shrinks_its_sell() {
    let sent = released_while_the_flatten_waits("4");
    let sold: Vec<String> = sent
        .submissions()
        .into_iter()
        .filter(|order| order.side == Side::Sell)
        .map(|order| order.qty.to_string())
        .collect();
    assert_eq!(sold, ["6"], "{:?}", sent.draft_types());
    let short: Vec<&str> = sent
        .drafts
        .iter()
        .filter(|draft| {
            draft.event_type == "ProtectionChanged"
                && draft
                    .payload
                    .get("action")
                    .and_then(mandate_canon::Value::as_str)
                    == Some("rung_short")
        })
        .filter_map(|draft| {
            draft
                .payload
                .get("sent")
                .and_then(mandate_canon::Value::as_str)
        })
        .collect();
    assert_eq!(
        short,
        ["6"],
        "the shorter send is journaled (DEC-2360 item 1): {:?}",
        sent.draft_types()
    );
}

/// DEC-2360 item 2: a release of the whole position before the flatten was sent leaves it nothing
/// to sell, so it is abandoned, never sent, and the owner alerted (`exit_released`). The
/// protection the ended sequence re-places is DEC-2360 item 4's, not this test's.
#[test]
fn a_release_of_everything_while_the_flatten_waits_abandons_it() {
    let sent = released_while_the_flatten_waits("10");
    assert!(
        sent.submissions()
            .into_iter()
            .filter(|order| order.purpose != Purpose::Protective)
            .all(|order| order.side != Side::Sell),
        "{:?}",
        sent.draft_types()
    );
    let reasons: Vec<&str> = sent
        .drafts
        .iter()
        .filter(|draft| draft.event_type == "OrderAbandoned")
        .filter_map(|draft| {
            draft
                .payload
                .get("reason")
                .and_then(mandate_canon::Value::as_str)
        })
        .collect();
    assert_eq!(reasons, ["released_to_owner"], "{:?}", sent.draft_types());
    assert!(
        sent.notifications.contains(&"exit_released"),
        "the owner is told the switch has nothing left to sell there: {:?}",
        sent.notifications
    );
}

/// DEC-2360 item 1 on the ladder (§5.6 step 2): a release between rungs shrinks the next rung to
/// the sub-ledger, so the flatten's 10 becomes 6 once the owner takes 4.
#[test]
fn a_release_between_rungs_shrinks_the_next_rung() {
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
            initiator: Initiator::RiskLimit,
            confirmation: None,
            cause: None,
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
        .expect("the flatten's first rung");
    assert_eq!(first.qty, qty("10"));
    release(&mut shell, common::AGENT, "4");
    let (_, confirmed) = step_rung(&mut shell, &ports, 16, first.client_order_id.as_str());
    let next = confirmed
        .submissions()
        .first()
        .copied()
        .cloned()
        .expect("the step's confirmation submits the next rung");
    assert_eq!(next.qty, qty("6"), "{:?}", confirmed.draft_types());
}

/// DEC-2360 invariant 4, rule 13: the cap reaches a switch's own flatten only. After the agent
/// released 4 of its 10, a risk exit of 10 that is not a switch's still sends all 10, unshrunk.
#[test]
fn a_release_leaves_an_exit_other_than_a_switch_flatten_whole() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), &ports);
    release(&mut shell, common::AGENT, "4");
    shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "10", "155")),
        &ports,
    );
    let sent = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );
    let sold: Vec<String> = sent
        .submissions()
        .into_iter()
        .filter(|order| order.side == Side::Sell)
        .map(|order| order.qty.to_string())
        .collect();
    assert_eq!(sold, ["10"], "{:?}", sent.draft_types());
}

/// The agent's position of 10 whose OCO was cancelled before any switch, so no protective stop
/// is left, with no quote ever seen; then its risk-limit switch, with no floor, and a tick: the
/// shell and what the switch's step and the tick journaled and alerted.
fn unpriced_switch(ports: &mandate_executor::Ports<'_>) -> (Shell, common::Ran, common::Ran) {
    let mut shell = protected_position(ports);
    for gone in [
        event(
            ACCOUNT_STREAM,
            shell.head().0.saturating_add(1),
            "ProtectionChanged",
            with_clock(
                &[
                    ("instrument", text(AAPL)),
                    ("action", text("cancelled")),
                    ("orders", text("md-oco-1")),
                    ("qty", text("10")),
                ],
                20,
            ),
        ),
        event(
            ACCOUNT_STREAM,
            shell.head().0.saturating_add(2),
            "OrderStateChanged",
            with_clock(
                &[
                    ("client_order_id", text("md-oco-1")),
                    ("state", text("canceled")),
                ],
                20,
            ),
        ),
    ] {
        shell.fold_one(&gone).expect("the OCO's cancel folds");
    }
    let switched = shell.run(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(agent(common::AGENT)),
            initiator: Initiator::RiskLimit,
            confirmation: None,
            cause: None,
        }),
        ports,
    );
    let ticked = shell.run(Input::Tick(clock(40)), ports);
    (shell, switched, ticked)
}

/// How many `exit_unpriced` records `ran` journaled and alerts it sent.
fn unpriced_alerts(ran: &common::Ran) -> (usize, usize) {
    let records = ran
        .drafts
        .iter()
        .filter(|draft| {
            draft.event_type == "ProtectionChanged"
                && draft
                    .payload
                    .get("action")
                    .and_then(mandate_canon::Value::as_str)
                    == Some("exit_unpriced")
        })
        .count();
    let alerts = ran
        .notifications
        .iter()
        .filter(|key| **key == "exit_unpriced")
        .count();
    (records, alerts)
}

/// DEC-2361 item 1, rule 13: a switch's close that nothing but a missing price holds back, with
/// no quote, floor or protective stop to name a limit from, is journaled `exit_unpriced` and
/// alerted once, never silent, and later steps do not repeat it.
#[test]
fn a_switch_close_with_nothing_to_price_from_is_alerted_once() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (_, switched, ticked) = unpriced_switch(&ports);
    assert!(
        switched
            .submissions()
            .into_iter()
            .all(|order| order.side != Side::Sell),
        "nothing to name a limit from, so no sell goes: {:?}",
        switched.draft_types()
    );
    assert_eq!(
        unpriced_alerts(&switched),
        (1, 1),
        "{:?}",
        switched.draft_types()
    );
    let switch = switched
        .draft("KillSwitchActivated")
        .map(|draft| draft.event_id.clone())
        .expect("the switch is journaled");
    let causes: Vec<Option<EventId>> = switched
        .drafts
        .iter()
        .filter(|draft| {
            draft.event_type == "ProtectionChanged"
                && draft
                    .payload
                    .get("action")
                    .and_then(mandate_canon::Value::as_str)
                    == Some("exit_unpriced")
        })
        .map(|draft| draft.causation_id.clone())
        .collect();
    assert_eq!(
        causes,
        [Some(switch)],
        "the record names the switch it alerts for, which is what folds it once (DEC-2361 item 2)"
    );
    assert_eq!(
        unpriced_alerts(&ticked),
        (0, 0),
        "{:?}",
        ticked.draft_types()
    );
}

/// DEC-2361 item 2: the alert holds nothing back. Once a quote arrives the switch's close is
/// raised and sells the agent's whole sub-ledger.
#[test]
fn a_switch_close_alerted_unpriced_sells_once_a_quote_arrives() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, _, _) = unpriced_switch(&ports);
    let quoted = shell.run(Input::Market(quote(AAPL, "155", "155.1", 41)), &ports);
    let sold: Vec<String> = quoted
        .submissions()
        .into_iter()
        .filter(|order| order.side == Side::Sell)
        .map(|order| order.qty.to_string())
        .collect();
    assert_eq!(sold, ["10"], "{:?}", quoted.draft_types());
}

/// The buffer day's replacement of the protected 10 after `before` folds onto it: the quantity
/// re-placed once the expiring OCO's cancel is confirmed.
fn buffer_day_after(before: &[mandate_executor::FoldedEvent]) -> mandate_num::Qty {
    re_placed(&buffer_day_ran(before), None)
}

/// The step that confirms the expiring OCO's cancel on the buffer day, after `before` folds onto
/// the protected 10: what it journals, sends and alerts.
fn buffer_day_ran(before: &[mandate_executor::FoldedEvent]) -> common::Ran {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    for event in before {
        let mut event = event.clone();
        event.seq = mandate_executor::Seq(shell.head().0.saturating_add(1));
        shell.fold_one(&event).expect("folds");
    }
    let day = copied(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "TradingDayStarted",
        with_clock(&[("date", text("2026-12-14"))], 7_000_000),
        &EventId(format!("{CLOCK_STREAM}-9")),
    );
    shell.fold_one(&day).expect("the trading day folds");
    shell.run(Input::Journal(day), &ports);
    shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    )
}

/// The owner's release of `qty` of `instrument` from the agent (journal spec §9.27), unplaced.
fn released_of(instrument: &str, qty: &str) -> mandate_executor::FoldedEvent {
    event(
        ACCOUNT_STREAM,
        0,
        "PositionReleased",
        with_clock(
            &[
                ("agent_id", text(common::AGENT)),
                ("instrument", text(instrument)),
                ("qty", text(qty)),
                ("cause", text("owner_release")),
            ],
            25,
        ),
    )
}

/// DEC-2362 invariant 3: once the owner has sold the 4 shares they took, nothing of theirs is left
/// in the position, so the agent's 6 stay wholly protected.
#[test]
fn released_shares_the_owner_sold_leave_the_agents_protection_whole() {
    let sold = event(
        ACCOUNT_STREAM,
        0,
        "FillApplied",
        with_clock(
            &[
                ("fill_id", text("f-owner")),
                ("instrument", text(AAPL)),
                ("side", text("sell")),
                ("qty_gross", text("4")),
                ("price", text("150")),
            ],
            26,
        ),
    );
    assert_eq!(buffer_day_after(&[released_of(AAPL, "4"), sold]), qty("6"));
}

/// DEC-2362 item 1 reads the instrument's own releases only: a release in another instrument
/// leaves this one's protection whole.
#[test]
fn a_release_in_another_instrument_leaves_this_ones_protection_whole() {
    assert_eq!(buffer_day_after(&[released_of("MSFT", "4")]), qty("10"));
}

/// A filled order of `agent`'s in `instrument`, `side` `qty` at 150, named `id`: its submission
/// and its fill, unplaced.
fn filled_order(
    id: &str,
    agent: &str,
    instrument: &str,
    side: &str,
    qty: &str,
) -> [mandate_executor::FoldedEvent; 2] {
    [
        event(
            ACCOUNT_STREAM,
            0,
            "OrderSubmitted",
            with_clock(
                &[
                    ("client_order_id", text(id)),
                    ("agent", text(agent)),
                    ("instrument", text(instrument)),
                    ("side", text(side)),
                    ("qty", text(qty)),
                    ("limit", text("150")),
                ],
                26,
            ),
        ),
        event(
            ACCOUNT_STREAM,
            0,
            "FillApplied",
            with_clock(
                &[
                    ("fill_id", text(&format!("f-{id}"))),
                    ("client_order_id", text(id)),
                    ("instrument", text(instrument)),
                    ("side", text(side)),
                    ("qty_gross", text(qty)),
                    ("price", text("150")),
                ],
                26,
            ),
        ),
    ]
}

/// DEC-2362 item 1 counts the instrument's own lots only: the agent's 5 shares of another symbol
/// leave the AAPL release of 4 what it is, so the agent's 6 AAPL are covered.
#[test]
fn another_instruments_lots_leave_this_ones_release_as_it_is() {
    let mut before = vec![released_of(AAPL, "4")];
    before.extend(filled_order("md-msft-1", common::AGENT, "MSFT", "buy", "5"));
    assert_eq!(buffer_day_after(&before), qty("6"));
}

/// DEC-2362 item 1 floors each agent's kept lots at zero: another agent that sold 3 it never
/// bought adds nothing to the agents' shares, so the owner, holding 1 of the 4 released, keeps
/// that 1 out of the agent's protection, which covers the agent's 6.
#[test]
fn an_agent_that_sold_more_than_it_holds_adds_nothing_to_the_agents_shares() {
    let mut before = vec![released_of(AAPL, "4")];
    before.extend(filled_order(
        "md-other-sell",
        OTHER_AGENT,
        AAPL,
        "sell",
        "3",
    ));
    assert_eq!(buffer_day_after(&before), qty("6"));
}

/// The protected 10 with the owner's release of `released` of them folded and an agent's risk exit
/// of 2 that filled, so the executor placed its own protection again, which the broker accepts
/// and then reports expired: the re-placement step and the expiry step.
fn released_then_lost(released: &str) -> (common::Ran, common::Ran) {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    release(&mut shell, common::AGENT, released);
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
    let Some(placed) = ended
        .submissions()
        .into_iter()
        .find(|order| order.purpose == Purpose::Protective)
        .cloned()
    else {
        return (ended, common::Ran::default());
    };
    let report = |status| {
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-placed",
            Some(placed.client_order_id.as_str()),
            AAPL,
            Side::Sell,
            &placed.qty.to_string(),
            "0",
            status,
        )))
    };
    shell.run(report("accepted"), &ports);
    let lost = shell.run(report("expired"), &ports);
    (ended, lost)
}

/// DEC-2362 item 1 at `re_cover`: with the owner holding 4 released shares and the agent's exit of
/// 2 filled, the agent's re-placed protection, lost at the broker, is placed again for the
/// agent's 4, never for the owner's released shares.
#[test]
fn a_re_cover_after_a_release_protects_only_the_agents_shares() {
    let (_, lost) = released_then_lost("4");
    assert_eq!(re_placed(&lost, None), qty("4"), "{:?}", lost.draft_types());
}

/// DEC-2362 item 1: after the owner took 8 of the 10 and the agent's exit sold its other 2, nothing
/// of the agent's is left to protect, so no protection is placed on the owner's shares, at the
/// exit's end or at a later loss.
#[test]
fn a_release_of_all_the_agent_keeps_places_no_protection() {
    let (ended, lost) = released_then_lost("8");
    for ran in [&ended, &lost] {
        assert!(
            ran.submissions()
                .into_iter()
                .all(|order| order.purpose != Purpose::Protective),
            "{:?}",
            ran.draft_types()
        );
    }
}

/// DEC-2362 item 1 at the end of an exit sequence: with the owner holding 4 released shares, the
/// agent's risk exit of 2 leaves it 4, and that is what the re-placement covers.
#[test]
fn an_exits_re_placement_after_a_release_protects_only_the_agents_shares() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    release(&mut shell, common::AGENT, "4");
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
        qty("4"),
        "{:?}",
        ended.draft_types()
    );
}

/// DEC-2362 item 1 at `new_day`: the buffer day's replacement of the protected 10, after the owner
/// took 4, covers the agent's 6.
#[test]
fn a_buffer_day_re_placement_after_a_release_protects_only_the_agents_shares() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    release(&mut shell, common::AGENT, "4");
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
        qty("6"),
        "{:?}",
        confirmed.draft_types()
    );
}

/// DEC-2368's edge: the agent's buy of 2 that the broker reports filled but whose fill named no
/// order (so it is applied unattributed), the owner's release of 4 of the agent's 10, and the
/// owner's sale of 3 of them, unattributed too. The position is 9: the agent's 8 and the owner's 1.
/// With `owner_sells` false the owner keeps the 4, and the position is 12.
fn unattributed_edge(owner_sells: bool) -> Vec<mandate_executor::FoldedEvent> {
    let fill = |id: &str, side: &str, qty: &str| {
        event(
            ACCOUNT_STREAM,
            0,
            "FillApplied",
            with_clock(
                &[
                    ("fill_id", text(id)),
                    ("instrument", text(AAPL)),
                    ("side", text(side)),
                    ("qty_gross", text(qty)),
                    ("price", text("150")),
                ],
                26,
            ),
        )
    };
    let mut events = vec![
        released_of(AAPL, "4"),
        event(
            ACCOUNT_STREAM,
            0,
            "OrderSubmitted",
            with_clock(
                &[
                    ("client_order_id", text("md-unattributed")),
                    ("agent", text(common::AGENT)),
                    ("instrument", text(AAPL)),
                    ("side", text("buy")),
                    ("qty", text("2")),
                    ("limit", text("150")),
                ],
                26,
            ),
        ),
        event(
            ACCOUNT_STREAM,
            0,
            "OrderStateChanged",
            with_clock(
                &[
                    ("client_order_id", text("md-unattributed")),
                    ("state", text("filled")),
                    ("filled_qty", text("2")),
                ],
                26,
            ),
        ),
        fill("f-unattributed", "buy", "2"),
    ];
    if owner_sells {
        events.push(fill("f-owner", "sell", "3"));
    }
    events
}

/// The `protection_gap` records `ran` journaled, as their `qty`.
fn gaps(ran: &common::Ran) -> Vec<String> {
    ran.drafts
        .iter()
        .filter(|draft| {
            draft.event_type == "ProtectionChanged"
                && draft
                    .payload
                    .get("action")
                    .and_then(mandate_canon::Value::as_str)
                    == Some("protection_gap")
        })
        .filter_map(|draft| {
            draft
                .payload
                .get("qty")
                .and_then(mandate_canon::Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

/// A `ProtectionChanged` of `action` for AAPL, as the executor journals it, unplaced.
fn protection_record(
    action: &str,
    members: &[(&str, mandate_canon::Value)],
) -> mandate_executor::FoldedEvent {
    let mut all = vec![("instrument", text(AAPL)), ("action", text(action))];
    all.extend(members.iter().cloned());
    event(ACCOUNT_STREAM, 0, "ProtectionChanged", with_clock(&all, 27))
}

/// DEC-2368 items 1 to 3, the founder's option A: in the edge the buffer day's re-placement still
/// covers 6, never the owner's released share, and the 2 of the agent's it may leave uncovered are
/// journaled as `protection_gap` after the placement and alerted (`protection_gap`, rule 6).
#[test]
#[ignore = "pending E6-4"]
fn an_edge_under_cover_is_journaled_as_a_gap_and_alerted() {
    let ran = buffer_day_ran(&unattributed_edge(true));
    assert_eq!(re_placed(&ran, None), qty("6"), "{:?}", ran.draft_types());
    assert_eq!(gaps(&ran), ["2"], "{:?}", ran.draft_types());
    let placed = ran.drafts.iter().rposition(|draft| {
        draft.event_type == "ProtectionChanged"
            && draft
                .payload
                .get("action")
                .and_then(mandate_canon::Value::as_str)
                == Some("placed")
    });
    let gap = ran.drafts.iter().rposition(|draft| {
        draft
            .payload
            .get("action")
            .and_then(mandate_canon::Value::as_str)
            == Some("protection_gap")
    });
    assert!(
        placed < gap,
        "the gap follows its placement: {:?}",
        ran.draft_types()
    );
    assert_eq!(
        ran.notifications
            .iter()
            .filter(|key| **key == "protection_gap")
            .count(),
        1,
        "{:?}",
        ran.notifications
    );
}

/// DEC-2368 invariant 2: with the same unattributed 2 and no owner sale, the placement covers the
/// agent's 8, every share it can read as the agent's, so there is no gap to journal or alert.
#[test]
fn no_gap_is_journaled_while_the_owner_keeps_the_released_shares() {
    let ran = buffer_day_ran(&unattributed_edge(false));
    assert_eq!(re_placed(&ran, None), qty("8"), "{:?}", ran.draft_types());
    assert!(gaps(&ran).is_empty(), "{:?}", ran.draft_types());
    assert!(!ran.notifications.contains(&"protection_gap"));
}

/// DEC-2368 item 3: a gap journaled at an earlier placement, with no placement since, is the same
/// episode. The buffer day journals its gap again but does not alert again.
#[test]
#[ignore = "pending E6-4"]
fn a_gap_in_an_open_episode_is_journaled_without_another_alert() {
    let mut before = unattributed_edge(true);
    before.push(protection_record("protection_gap", &[("qty", text("2"))]));
    let ran = buffer_day_ran(&before);
    assert_eq!(gaps(&ran), ["2"], "{:?}", ran.draft_types());
    assert!(
        !ran.notifications.contains(&"protection_gap"),
        "{:?}",
        ran.notifications
    );
}

/// DEC-2368 item 3: a placement after the earlier gap ends that episode, so the buffer day's gap
/// is a new one and is alerted.
#[test]
#[ignore = "pending E6-4"]
fn a_placement_after_a_gap_ends_its_episode() {
    let mut before = unattributed_edge(true);
    before.push(protection_record("protection_gap", &[("qty", text("2"))]));
    before.push(protection_record(
        "placed",
        &[
            ("orders", text("md-oco-1")),
            ("qty", text("10")),
            ("stop", text("147")),
            ("take_profit", text("157.5")),
        ],
    ));
    let ran = buffer_day_ran(&before);
    assert_eq!(gaps(&ran), ["2"], "{:?}", ran.draft_types());
    assert!(
        ran.notifications.contains(&"protection_gap"),
        "{:?}",
        ran.notifications
    );
}

/// The edge folded onto the protected 10, then the agent's risk exit of 2 that fills, so the
/// executor places protection again, which the broker accepts and then reports expired: the
/// sequence end's step and the expiry's step.
fn edge_then_lost() -> (common::Ran, common::Ran) {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    for event in unattributed_edge(true) {
        let mut event = event.clone();
        event.seq = mandate_executor::Seq(shell.head().0.saturating_add(1));
        shell.fold_one(&event).expect("folds");
    }
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
    let Some(placed) = ended
        .submissions()
        .into_iter()
        .find(|order| order.purpose == Purpose::Protective)
        .cloned()
    else {
        return (ended, common::Ran::default());
    };
    let report = |status| {
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-placed",
            Some(placed.client_order_id.as_str()),
            AAPL,
            Side::Sell,
            &placed.qty.to_string(),
            "0",
            status,
        )))
    };
    shell.run(report("accepted"), &ports);
    let lost = shell.run(report("expired"), &ports);
    (ended, lost)
}

/// DEC-2368 item 2 at the end of an exit sequence: in the edge, the agent's risk exit of 2 that
/// fills leaves a position of 7. The fold reads the agent's attributed lots as 10 less the exit's
/// 2 and the release's 4, so 4, and the owner's as 3; the re-placement covers 4 and journals the
/// unattributed 2 it may leave uncovered, and alerts it.
#[test]
#[ignore = "pending E6-4"]
fn an_exits_re_placement_in_the_edge_journals_its_gap() {
    let (ended, _) = edge_then_lost();
    assert_eq!(
        re_placed(&ended, None),
        qty("4"),
        "{:?}",
        ended.draft_types()
    );
    assert_eq!(gaps(&ended), ["2"], "{:?}", ended.draft_types());
    assert!(ended.notifications.contains(&"protection_gap"));
}

/// DEC-2368 items 2 and 3 at `re_cover`: the re-placed protection, lost at the broker, is placed
/// again for the same 4 and journals the same gap, within the episode the sequence end opened, so
/// without another alert.
#[test]
#[ignore = "pending E6-4"]
fn a_re_cover_in_the_edge_journals_its_gap_without_another_alert() {
    let (_, lost) = edge_then_lost();
    assert_eq!(re_placed(&lost, None), qty("4"), "{:?}", lost.draft_types());
    assert_eq!(gaps(&lost), ["2"], "{:?}", lost.draft_types());
    assert!(
        !lost.notifications.contains(&"protection_gap"),
        "{:?}",
        lost.notifications
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
            cause: None,
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
fn an_account_kill_switch_uses_cancel_all() {
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
            cause: None,
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
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), &ports);

    let ran = shell.run(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Account(mandate_executor::AccountRef(common::ACCOUNT.to_owned())),
            initiator: Initiator::Owner,
            confirmation: Some(owner_confirmation()),
            cause: None,
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

/// A position of `qty` in `instrument` held by `holder`, folded as the journaled facts of its
/// filled buy from `seq` on: what an account kill switch finds to close.
fn held_by(shell: &mut Shell, seq: u64, holder: &str, instrument: &str, qty: &str, id: &str) {
    for fact in [
        event(
            ACCOUNT_STREAM,
            seq,
            "OrderSubmitted",
            with_clock(
                &[
                    ("client_order_id", text(id)),
                    ("agent", text(holder)),
                    ("instrument", text(instrument)),
                    ("side", text("buy")),
                    ("qty", text(qty)),
                    ("limit", text("100")),
                ],
                12,
            ),
        ),
        event(
            ACCOUNT_STREAM,
            seq.saturating_add(1),
            "FillApplied",
            with_clock(
                &[
                    ("fill_id", text(&format!("f-{id}"))),
                    ("client_order_id", text(id)),
                    ("instrument", text(instrument)),
                    ("side", text("buy")),
                    ("qty_gross", text(qty)),
                    ("price", text("100")),
                ],
                12,
            ),
        ),
        event(
            ACCOUNT_STREAM,
            seq.saturating_add(2),
            "OrderStateChanged",
            with_clock(
                &[("client_order_id", text(id)), ("state", text("filled"))],
                12,
            ),
        ),
    ] {
        shell.fold_one(&fact).expect("the held position folds");
    }
}

/// An account with two agents: `AGENT`'s protected `AAPL` and `OTHER_AGENT`'s `BTC`, in the
/// regular session with a fresh quote for each, where §5.5 lets a close go at the market.
fn two_agent_account(ports: &mandate_executor::Ports<'_>) -> Shell {
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    for event in protected_position_events(2) {
        shell
            .fold_one(&event)
            .expect("the protected position folds");
    }
    held_by(&mut shell, 6, OTHER_AGENT, BTC, "0.5", "md-held-btc");
    let mut shell = shell.restart_ready(ports);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), ports);
    shell.run(Input::Market(quote(BTC, "60000", "60010", 20)), ports);
    shell
}

fn account_switch(initiator: Initiator) -> Input {
    Input::Command(Command::KillSwitch {
        scope: KillScope::Account(mandate_executor::AccountRef(common::ACCOUNT.to_owned())),
        initiator,
        confirmation: (initiator == Initiator::Owner).then(owner_confirmation),
        cause: None,
    })
}

/// §5.5's sequence for the account scope: the final mode first, for every agent on the account,
/// then the cancel. It is one `AgentModeApplied` for `*`, every agent of the account, those the
/// executor has not yet seen included: an owner's switch stops them, a mandate limit's pauses them
/// (DEC-100), journaled before the broker's cancel-all goes, so nothing an agent hands in meanwhile
/// is gated under its old mode (`AGENTS.md` rule 13).
#[test]
fn an_account_kill_switch_applies_every_agents_mode_before_its_cancel_all() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    for (initiator, mode, name) in [
        (Initiator::Owner, Mode::Stopped, "stopped"),
        (Initiator::RiskLimit, Mode::Paused, "paused"),
    ] {
        let mut shell = two_agent_account(&ports);

        let ran = shell.run(account_switch(initiator), &ports);

        let cancel_all = ran
            .effects
            .iter()
            .position(|e| matches!(e, Effect::Broker(BrokerRequest::CancelAll(_))))
            .unwrap_or_else(|| panic!("{initiator:?}: the cancel-all goes"));
        let every = ran.effects.iter().position(|e| {
            matches!(e, Effect::Journal(d) if d.event_type == "AgentModeApplied"
                && d.payload.get("agent").and_then(mandate_canon::Value::as_str) == Some("*")
                && d.payload.get("to").and_then(mandate_canon::Value::as_str) == Some(name))
        });
        assert!(
            every.is_some_and(|at| at < cancel_all),
            "{initiator:?}: every agent `{name}` before the cancel-all: {:?}",
            ran.draft_types()
        );
        for holder in [common::AGENT, OTHER_AGENT, "agent-unseen"] {
            assert_eq!(
                shell.state.effective_mode(&agent(holder)),
                mode,
                "{initiator:?}: {holder} is {name}"
            );
        }
    }
}

/// A crypto position whose stop-limit the executor placed after its entry filled, acknowledged by
/// the broker, in the regular session: protection the fold knows as an order with its request,
/// which `re_cover` would place again once cancelled. Answers the shell and the stop as the broker
/// describes it in `status`.
fn crypto_stop_account(
    ports: &mandate_executor::Ports<'_>,
) -> (Shell, impl Fn(&str) -> mandate_executor::BrokerOrder) {
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(ports);
    shell.run(Input::Market(quote(BTC, "60000", "60010", 20)), ports);
    let submitted = shell.run(
        handoff(
            INTENT,
            common::AGENT,
            protected_opening(BTC, "0.5", "60000", "54000", None),
        ),
        ports,
    );
    let entry = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the crypto entry is sent");
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&entry),
            BTC,
            Side::Buy,
            "0.5",
            "0.5",
            "filled",
        ))),
        ports,
    );
    let filled = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(btc_fill(&entry, "0.5"))),
        ports,
    );
    let (id, size) = filled
        .submissions()
        .into_iter()
        .find(|o| o.purpose == Purpose::Protective)
        .map(|o| (o.client_order_id.as_str().to_owned(), o.qty.to_string()))
        .expect("the stop-limit is placed");
    let stop =
        move |status: &str| broker_order("b-stop", Some(&id), BTC, Side::Sell, &size, "0", status);
    shell.run(Input::BrokerUpdate(BrokerUpdate::Order(stop("new"))), ports);
    (shell, stop)
}

/// §5.5's cancel → confirm → close (DEC-2333 items 1 and 2): the account switch's step sends the
/// cancel-all and nothing else. No order is cancelled by its id beside it, and no close-position
/// goes before the cancel-all is confirmed, so no sell still working can fill beside a close.
#[test]
fn an_account_kill_switch_cancels_nothing_by_id_and_closes_nothing_before_its_cancel_all_is_confirmed()
 {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = two_agent_account(&ports);

    let ran = shell.run(account_switch(Initiator::Owner), &ports);

    let sent: Vec<&BrokerRequest> = ran.requests.iter().collect();
    assert!(
        matches!(sent[..], [BrokerRequest::CancelAll(_)]),
        "the cancel-all alone: {sent:?}"
    );
}

/// The cancel-all is confirmed by the broker's answer and then a fresh read of its open orders
/// (DEC-2333 item 2): once the broker accepts it, the executor lists the account's open orders,
/// and still closes nothing until that read answers.
#[test]
fn an_accepted_cancel_all_is_confirmed_by_reading_the_open_orders() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = two_agent_account(&ports);
    shell.run(account_switch(Initiator::Owner), &ports);

    let accepted = shell.run(
        Input::Broker(Ok(BrokerOutcome::AccountWideAccepted)),
        &ports,
    );

    assert!(
        matches!(accepted.requests[..], [BrokerRequest::ListOpenOrders]),
        "one open-orders read, and no close yet: {:?}",
        accepted.requests
    );
}

/// A cancel-all whose answer is lost is never sent again without its own idempotency key
/// (DEC-2333 item 3): the executor reads the open orders by listing instead, in that step and on
/// every step after it, and closes nothing meanwhile.
#[test]
fn a_lost_cancel_all_answer_is_read_by_listing_and_never_resent() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = two_agent_account(&ports);
    shell.run(account_switch(Initiator::Owner), &ports);

    let lost = shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);
    let later = shell.run(Input::Market(quote(AAPL, "155", "155.1", 30)), &ports);

    assert!(
        lost.requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::ListOpenOrders)),
        "the lost answer is read by listing the open orders: {:?}",
        lost.requests
    );
    let after: Vec<&BrokerRequest> = lost.requests.iter().chain(&later.requests).collect();
    assert!(
        !after.iter().any(|r| matches!(
            r,
            BrokerRequest::CancelAll(_) | BrokerRequest::ClosePosition(..)
        )),
        "never a second cancel-all, and no close before a read confirms: {after:?}"
    );
}

/// A sell the open-orders read shows still working after the cancel-all is cancelled by its own id
/// (DEC-2333 item 3), and that instrument closes nothing until the cancel is confirmed.
#[test]
fn a_sell_still_working_after_the_cancel_all_is_cancelled_by_its_id() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, stop) = crypto_stop_account(&ports);
    shell.run(account_switch(Initiator::Owner), &ports);
    shell.run(
        Input::Broker(Ok(BrokerOutcome::AccountWideAccepted)),
        &ports,
    );

    let read = shell.run(
        Input::Broker(Ok(BrokerOutcome::OpenOrders(vec![stop("new")]))),
        &ports,
    );

    let working = stop("new").client_order_id;
    assert!(
        read.requests.iter().any(|r| matches!(
            r,
            BrokerRequest::Cancel { client_order_id }
                if Some(client_order_id.as_str()) == working.as_deref()
        )),
        "the stop still working is cancelled by its id: {:?}",
        read.requests
    );
    assert!(
        !read
            .requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::ClosePosition(..))),
        "and its instrument does not close beside it: {:?}",
        read.requests
    );
}

/// The protection the cancel-all cancelled is never placed again while the switch waits for its
/// close (DEC-2333 item 5; §5.4, `AGENTS.md` rule 12): without a guard the executor re-places this
/// crypto stop-limit, which would sell the same coins twice once the close-position goes. The
/// cancel is folded, and nothing protective is sent, in that step or the next.
#[test]
fn an_account_kill_switch_never_re_places_the_protection_its_cancel_all_cancelled() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, stop) = crypto_stop_account(&ports);
    shell.run(account_switch(Initiator::Owner), &ports);

    let cancelled = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(stop("canceled"))),
        &ports,
    );
    let later = shell.run(Input::Market(quote(BTC, "60000", "60010", 40)), &ports);

    let protective: Vec<_> = cancelled
        .submissions()
        .into_iter()
        .chain(later.submissions())
        .filter(|o| o.purpose == Purpose::Protective)
        .map(|o| o.client_order_id.as_str().to_owned())
        .collect();
    assert!(
        protective.is_empty(),
        "no protection re-placed while the switch closes: {protective:?}"
    );
}

/// A bracket entry the cancel-all leaves partly filled gets no OCO (DEC-2333 item 5, rule 12):
/// filled 60 of 100 when the account switch goes, the entry is reported canceled at 60, and the
/// GTC OCO `bracket` would place for those 60 would sell them twice beside the switch's close.
#[test]
fn a_bracket_the_cancel_all_leaves_partly_filled_gets_no_oco() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
    shell.run(Input::Market(quote(AAPL, "150", "150.1", 20)), &ports);
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
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the bracket is sent");
    let reported = |status: &str| {
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&entry),
            AAPL,
            Side::Buy,
            "100",
            "60",
            status,
        )))
    };
    shell.run(reported("partially_filled"), &ports);
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-1",
            Some(&entry),
            "60",
            "150",
        ))),
        &ports,
    );
    shell.run(account_switch(Initiator::Owner), &ports);

    let cancelled = shell.run(reported("canceled"), &ports);

    let protective: Vec<_> = cancelled
        .submissions()
        .into_iter()
        .filter(|o| o.purpose == Purpose::Protective)
        .map(|o| (o.client_order_id.as_str().to_owned(), o.qty))
        .collect();
    assert!(
        protective.is_empty(),
        "no OCO for the 60 beside the switch's close: {protective:?}"
    );
}

/// A re-placement before expiry under way when the account switch goes never places protection
/// once its cancel is confirmed (DEC-2333 item 5): the buffer day cancels the OCO, the switch
/// goes, and the confirmed cancel places nothing beside the switch's close.
#[test]
fn a_re_placement_under_way_across_the_account_switch_places_nothing() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    let buffer = copied(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "TradingDayStarted",
        with_clock(&[("date", text("2026-12-14"))], 6_100_000),
        &EventId(format!("{CLOCK_STREAM}-9")),
    );
    shell.fold_one(&buffer).expect("the buffer day folds");
    let begun = shell.run(Input::Journal(buffer), &ports);
    assert!(
        begun.requests.iter().any(|r| matches!(
            r,
            BrokerRequest::Cancel { client_order_id } if client_order_id.as_str() == "md-oco-1"
        )),
        "the buffer day starts the re-placement: {:?}",
        begun.requests
    );
    shell.run(
        Input::Market(quote(AAPL, "155", "155.1", 6_100_010)),
        &ports,
    );
    shell.run(account_switch(Initiator::Owner), &ports);

    let confirmed = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );

    let protective: Vec<_> = confirmed
        .submissions()
        .into_iter()
        .filter(|o| o.purpose == Purpose::Protective)
        .map(|o| o.client_order_id.as_str().to_owned())
        .collect();
    assert!(
        protective.is_empty(),
        "no re-placement beside the switch's close: {protective:?}"
    );
}

/// §5.4's bound never sells beside the account switch's close (DEC-2333 items 3 and 5): an entry
/// partly filled when the switch goes leaves its interval open, and at `max_unprotected_s` the
/// interval is journaled and alerted, but nothing is sent to exit it.
#[test]
fn the_bound_never_sells_beside_the_account_switch() {
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
    let entry = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the crypto entry is sent");
    let order = |filled: &str, status: &str| {
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&entry),
            BTC,
            Side::Buy,
            "0.5",
            filled,
            status,
        )))
    };
    shell.run(order("0.2", "partially_filled"), &ports);
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(mandate_executor::BrokerFill {
            instrument: instrument(BTC),
            ..broker_fill("f-btc-1", Some(&entry), "0.2", "60000")
        })),
        &ports,
    );
    shell.run(account_switch(Initiator::Owner), &ports);
    shell.run(order("0.2", "canceled"), &ports);

    let bound = shell.run(Input::Tick(clock(20 + 3_600)), &ports);

    let sells: Vec<_> = bound
        .submissions()
        .into_iter()
        .filter(|o| o.side == Side::Sell)
        .map(|o| (o.client_order_id.as_str().to_owned(), o.purpose))
        .collect();
    assert!(
        sells.is_empty(),
        "the bound sells nothing beside the switch's close: {sells:?}"
    );
    assert!(
        !bound.draft_types().contains(&"IntentReceived"),
        "nor raises a flatten of its own, held or not: {:?}",
        bound.draft_types()
    );
}

/// Protection the cancel-all cancels leaves its position unprotected under §5.4's bound, journaled
/// and never silent (DEC-2333 item 3): the cancel starts an interval (`unprotected_start`), and at
/// `max_unprotected_s` the interval is journaled (`interval_limit`) and the owner alerted
/// (`unprotected_interval_limit`), while nothing is sold beside the switch's close. What the bound
/// places again is A1c's ([`protection_is_placed_again_at_the_bound_for_the_held_quantity`]).
#[test]
fn protection_the_cancel_all_cancels_opens_a_bounded_alerted_interval() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, stop) = crypto_stop_account(&ports);
    shell.run(account_switch(Initiator::Owner), &ports);

    let cancelled = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(stop("canceled"))),
        &ports,
    );
    let bound = shell.run(Input::Tick(clock(20 + 3_600)), &ports);

    let actions = |ran: &common::Ran| -> Vec<String> {
        ran.drafts
            .iter()
            .filter(|d| d.event_type == "ProtectionChanged")
            .filter_map(|d| {
                d.payload
                    .get("action")
                    .and_then(mandate_canon::Value::as_str)
            })
            .map(str::to_owned)
            .collect()
    };
    assert!(
        actions(&cancelled).iter().any(|a| a == "unprotected_start"),
        "the cancel starts the interval: {:?}",
        actions(&cancelled)
    );
    assert!(
        actions(&bound).iter().any(|a| a == "interval_limit")
            && bound.notifications.contains(&"unprotected_interval_limit"),
        "the bound journals the interval and alerts the owner: {:?} {:?}",
        actions(&bound),
        bound.notifications
    );
    let sold: Vec<_> = cancelled
        .submissions()
        .into_iter()
        .chain(bound.submissions())
        .filter(|o| o.purpose != Purpose::Protective)
        .map(|o| o.client_order_id.as_str().to_owned())
        .collect();
    assert!(sold.is_empty(), "nothing sold: {sold:?}");
    assert!(
        cancelled.submissions().is_empty(),
        "and nothing placed beside the cancel: {:?}",
        cancelled.submissions()
    );
}

/// The account switch on the BTC position whose stop the cancel-all cancelled, up to §5.4's bound
/// (DEC-2333 item 3, DEC-2356): if `foreign` names a quantity, the cancel-all is accepted and a
/// read shows a sell that is not ours working for it in BTC, so the close waits; then the stop's
/// cancel is confirmed, which starts BTC's interval, and the bound passes. The shell, the run at
/// the bound, and the position the fold holds, which §5.4 sizes protection against.
fn at_the_bound(
    ports: &mandate_executor::Ports<'_>,
    foreign: Option<&str>,
) -> (Shell, common::Ran, String) {
    let (mut shell, stop) = crypto_stop_account(ports);
    let held = shell
        .state
        .positions()
        .get(&instrument(BTC))
        .map(ToString::to_string)
        .expect("BTC is held");
    shell.run(account_switch(Initiator::Owner), ports);
    if let Some(working) = foreign {
        shell.run(Input::Broker(Ok(BrokerOutcome::AccountWideAccepted)), ports);
        shell.run(
            Input::Broker(Ok(BrokerOutcome::OpenOrders(vec![foreign_sell(working)]))),
            ports,
        );
    }
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(stop("canceled"))),
        ports,
    );
    let bound = shell.run(Input::Tick(clock(20 + 3_600)), ports);
    (shell, bound, held)
}

/// A sell of `qty` BTC working at the broker that the platform did not place.
fn foreign_sell(qty: &str) -> mandate_executor::BrokerOrder {
    broker_order("b-ext", None, BTC, Side::Sell, qty, "0", "new")
}

/// The protective orders a run sends: their ids and quantities.
fn protective_sent(ran: &common::Ran) -> Vec<(String, String)> {
    ran.submissions()
        .into_iter()
        .filter(|o| o.purpose == Purpose::Protective)
        .map(|o| (o.client_order_id.as_str().to_owned(), o.qty.to_string()))
        .collect()
}

/// DEC-2333 item 3 (A1c): at §5.4's bound, protection is placed again for the held quantity less
/// every sell not confirmed cancelled. The stop's cancel is confirmed and nothing else sells, so
/// one protective sell goes for the whole position, journaled first, and nothing else is sent: no
/// close, no exit.
#[test]
fn protection_is_placed_again_at_the_bound_for_the_held_quantity() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (_, bound, held) = at_the_bound(&ports, None);

    let placed = protective_sent(&bound);
    assert_eq!(
        placed
            .iter()
            .map(|(_, qty)| qty.as_str())
            .collect::<Vec<_>>(),
        vec![held.as_str()],
        "one protective sell for the held quantity: {placed:?}"
    );
    let submitted = bound
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Broker(BrokerRequest::Submit(_))))
        .expect("the protection is sent");
    let journaled = bound.effects.iter().position(|e| {
        matches!(e, Effect::Journal(d) if d.event_type == "ProtectionChanged"
            && d.payload.get("action").and_then(mandate_canon::Value::as_str) == Some("placed"))
    });
    assert!(
        journaled.is_some_and(|at| at < submitted),
        "journaled before it is sent"
    );
    assert!(
        !bound.requests.iter().any(|r| matches!(
            r,
            BrokerRequest::ClosePosition(..) | BrokerRequest::CancelAll(_)
        )),
        "and nothing else: {:?}",
        bound.requests
    );
    assert!(
        bound
            .submissions()
            .iter()
            .all(|o| o.purpose == Purpose::Protective),
        "no exit beside it"
    );
}

/// DEC-2356 item 1: a sell the read shows working that is not ours counts as not confirmed
/// cancelled, so the protection placed at the bound covers only what it leaves.
#[test]
fn the_bound_protection_leaves_out_a_sell_the_read_shows_working() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (_, bound, held) = at_the_bound(&ports, Some("0.2"));

    let left = qty(&held)
        .checked_sub(qty("0.2"))
        .expect("the position holds more than the foreign sell")
        .to_string();
    let placed = protective_sent(&bound);
    assert_eq!(
        placed
            .iter()
            .map(|(_, qty)| qty.as_str())
            .collect::<Vec<_>>(),
        vec![left.as_str()],
        "protection for what the foreign sell leaves: {placed:?}"
    );
}

/// DEC-2356 item 3: a read that still shows another sell working in BTC leaves the bound's
/// protection resting, since the close cannot go yet; a read showing it alone cancels it by its
/// id, and the close goes only once that cancel is confirmed, never beside it.
#[test]
fn the_bound_protection_is_cancelled_only_when_the_close_can_go() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, bound, _) = at_the_bound(&ports, Some("0.2"));
    let (id, size) = protective_sent(&bound)
        .into_iter()
        .next()
        .expect("the bound places protection");
    let protection =
        |status: &str| broker_order("b-bound", Some(&id), BTC, Side::Sell, &size, "0", status);
    let cancels = |ran: &common::Ran| {
        ran.requests.iter().any(|r| {
            matches!(
                r,
                BrokerRequest::Cancel { client_order_id } if client_order_id.as_str() == id
            )
        })
    };
    let closes = |ran: &common::Ran| {
        ran.requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::ClosePosition(..)))
    };
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(protection("new"))),
        &ports,
    );

    let blocked = shell.run(
        Input::Broker(Ok(BrokerOutcome::OpenOrders(vec![
            foreign_sell("0.2"),
            protection("new"),
        ]))),
        &ports,
    );
    assert!(
        !cancels(&blocked) && !closes(&blocked),
        "kept while another sell blocks the close: {:?}",
        blocked.requests
    );

    let clear = shell.run(
        Input::Broker(Ok(BrokerOutcome::OpenOrders(vec![protection("new")]))),
        &ports,
    );
    assert!(
        cancels(&clear),
        "cancelled by its id once the close can go: {:?}",
        clear.requests
    );
    assert!(
        !closes(&clear),
        "and the close waits on its cancel: {:?}",
        clear.requests
    );

    let confirmed = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(protection("canceled"))),
        &ports,
    );
    assert!(
        closes(&confirmed),
        "the close goes once it is confirmed: {:?}",
        confirmed.requests
    );
    assert!(
        protective_sent(&confirmed).is_empty(),
        "and nothing is placed beside it: {:?}",
        protective_sent(&confirmed)
    );
}

/// DEC-2356 item 3, replayed: the protection the bound placed is folded from its `placed` record,
/// so a restarted executor still keeps it while another sell blocks the close.
#[test]
fn a_restart_keeps_the_bound_protection_while_the_close_is_blocked() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (shell, bound, _) = at_the_bound(&ports, Some("0.2"));
    let (id, size) = protective_sent(&bound)
        .into_iter()
        .next()
        .expect("the bound places protection");

    let (mut restarted, _) = shell.restart(&ports);
    let read = restarted.run(
        Input::BrokerSnapshot({
            let mut taken = snapshot(restarted.head().0, ReconcileReason::Startup);
            taken.open_orders = vec![
                foreign_sell("0.2"),
                broker_order("b-bound", Some(&id), BTC, Side::Sell, &size, "0", "new"),
            ];
            taken
        }),
        &ports,
    );
    assert!(
        !read.requests.iter().any(|r| matches!(
            r,
            BrokerRequest::Cancel { client_order_id } if client_order_id.as_str() == id
        )),
        "the replayed executor keeps it: {:?}",
        read.requests
    );
}

/// DEC-2356 item 1: a sell of ours still working counts as not confirmed cancelled. An agent
/// switch's flatten rung in flight on the BTC position when the account switch goes, its stop's
/// cancel confirmed, covers the whole position at the bound, so nothing is placed there beside it.
#[test]
fn a_flatten_rung_in_flight_at_the_bound_covers_the_position() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, stop) = crypto_stop_account(&ports);
    shell.run(Input::Market(quote(BTC, "60000", "60010", 20)), &ports);
    shell.run(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(agent(common::AGENT)),
            initiator: Initiator::RiskLimit,
            confirmation: None,
            cause: None,
        }),
        &ports,
    );
    let stopped = stop("canceled");
    let sent = shell.run(Input::BrokerUpdate(BrokerUpdate::Order(stopped)), &ports);
    assert!(
        sent.submissions()
            .iter()
            .any(|o| o.side == Side::Sell && o.purpose != Purpose::Protective),
        "the agent switch's flatten sends its first rung: {:?}",
        sent.submissions()
    );
    shell.run(account_switch(Initiator::Owner), &ports);

    let bound = shell.run(Input::Tick(clock(20 + 3_600)), &ports);

    assert!(
        protective_sent(&bound).is_empty(),
        "the rung still covers the position: {:?}",
        protective_sent(&bound)
    );
}

/// DEC-2356 item 3: the protection placed at the bound takes its prices from its own instrument's
/// protection, never from another's, though that other's protective order is the later one.
#[test]
fn the_bound_protection_takes_its_own_instruments_prices() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC, FRAC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, stop) = crypto_stop_account(&ports);
    let btc_stop = Some(common::price("54000"));
    shell.run(Input::Market(quote(FRAC, "20", "20.1", 20)), &ports);
    let entry = shell
        .run(
            handoff(
                OTHER_INTENT,
                common::AGENT,
                protected_opening(FRAC, "10.4", "20", "18", Some("24")),
            ),
            &ports,
        )
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the FRAC entry is sent");
    let filled = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-frac",
            Some(&entry),
            FRAC,
            Side::Buy,
            "10.4",
            "10.4",
            "filled",
        ))),
        &ports,
    );
    assert!(
        !protective_sent(&filled).is_empty(),
        "FRAC's own protection is placed"
    );
    shell.run(account_switch(Initiator::Owner), &ports);
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(stop("canceled"))),
        &ports,
    );

    let bound = shell.run(Input::Tick(clock(20 + 3_600)), &ports);

    let placed: Vec<_> = bound
        .submissions()
        .into_iter()
        .filter(|o| o.purpose == Purpose::Protective)
        .map(|o| (o.instrument.as_str().to_owned(), o.stop_price))
        .collect();
    assert_eq!(
        placed,
        vec![(BTC.to_owned(), btc_stop)],
        "BTC's protection at BTC's own stop"
    );
}

/// DEC-2356 item 8 (the review of #2327): a read that omits the bound's protection, sent and not
/// yet acknowledged, lets no close go beside it, though it lists no other sell.
#[test]
fn a_read_that_omits_the_unacknowledged_bound_protection_closes_nothing() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, bound, _) = at_the_bound(&ports, Some("0.2"));
    assert!(
        !protective_sent(&bound).is_empty(),
        "the bound places protection"
    );

    let read = shell.run(
        Input::Broker(Ok(BrokerOutcome::OpenOrders(Vec::new()))),
        &ports,
    );

    assert!(
        !read
            .requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::ClosePosition(..))),
        "no close beside the protection the read could not show: {:?}",
        read.requests
    );
}

/// DEC-2356 items 2 and 6: beside an exit sequence that sells only part of the position, the bound
/// places protection for what the exit still working leaves, `whole(long − sum)`.
#[test]
fn the_bound_protects_what_an_exit_of_part_of_the_position_leaves() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, stop) = crypto_stop_account(&ports);
    let held = shell
        .state
        .positions()
        .get(&instrument(BTC))
        .map(ToString::to_string)
        .expect("BTC is held");
    shell.run(Input::Market(quote(BTC, "60000", "60010", 20)), &ports);
    shell.run(
        handoff(OTHER_INTENT, common::AGENT, risk_exit(BTC, "0.2", "60000")),
        &ports,
    );
    let sent = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(stop("canceled"))),
        &ports,
    );
    assert!(
        sent.submissions()
            .iter()
            .any(|o| o.side == Side::Sell && o.qty == qty("0.2")),
        "the exit of 0.2 is sent once the stop's cancel is confirmed: {:?}",
        sent.submissions()
    );
    shell.run(account_switch(Initiator::Owner), &ports);

    let bound = shell.run(Input::Tick(clock(20 + 3_600)), &ports);

    let left = qty(&held)
        .checked_sub(qty("0.2"))
        .expect("the position holds more than the exit")
        .to_string();
    let placed = protective_sent(&bound);
    assert_eq!(
        placed
            .iter()
            .map(|(_, qty)| qty.as_str())
            .collect::<Vec<_>>(),
        vec![left.as_str()],
        "protection for what the exit leaves: {placed:?}"
    );
}

/// DEC-2333 item 5: once BTC's close-position has gone out, nothing is placed there, at the bound
/// or anywhere else, even with the stop's cancel confirmed and the interval it opens at its bound.
#[test]
fn nothing_is_placed_at_the_bound_once_the_close_is_requested() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, stop) = crypto_stop_account(&ports);
    shell.run(account_switch(Initiator::Owner), &ports);
    shell.run(
        Input::Broker(Ok(BrokerOutcome::AccountWideAccepted)),
        &ports,
    );
    let read = shell.run(
        Input::Broker(Ok(BrokerOutcome::OpenOrders(Vec::new()))),
        &ports,
    );
    assert!(
        read.requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::ClosePosition(..))),
        "BTC's close is requested"
    );
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(stop("canceled"))),
        &ports,
    );

    let bound = shell.run(Input::Tick(clock(20 + 3_600)), &ports);

    assert!(
        bound.submissions().is_empty(),
        "nothing beside the close: {:?}",
        bound.submissions()
    );
}

/// DEC-2333 item 3, carried from A1b: the cancel-all's answer lost, then a restart, whose startup
/// reconciliation is the read that confirms it, so the close goes on it and the cancel-all is never
/// sent again.
#[test]
fn a_lost_cancel_all_then_a_restart_closes_on_the_startup_read() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, stop) = crypto_stop_account(&ports);
    shell.run(account_switch(Initiator::Owner), &ports);
    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);

    let (mut restarted, started) = shell.restart(&ports);
    let held = stop("new").qty.to_string();
    let read = restarted.run(
        Input::BrokerSnapshot({
            let mut taken = snapshot(restarted.head().0, ReconcileReason::Startup);
            taken.positions = vec![broker_position(BTC, &held)];
            taken
        }),
        &ports,
    );

    assert!(
        read.requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::ClosePosition(_, i) if i.as_str() == BTC)),
        "the startup read confirms the cancel-all and BTC closes: {:?}",
        read.requests
    );
    assert!(
        !started
            .requests
            .iter()
            .chain(&read.requests)
            .any(|r| matches!(r, BrokerRequest::CancelAll(_))),
        "never a second cancel-all"
    );
}

/// The interval protection's loss opens is the closing instrument's own, whatever other
/// instrument has one open (DEC-2333 item 3): with an AAPL bracket partly filled, its interval
/// open, the BTC stop the cancel-all cancels still starts BTC's interval.
#[test]
fn another_instruments_open_interval_does_not_hide_the_closing_ones_start() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, stop) = crypto_stop_account(&ports);
    shell.run(Input::Market(quote(AAPL, "150", "150.1", 20)), &ports);
    let submitted = shell.run(
        handoff(
            OTHER_INTENT,
            common::AGENT,
            protected_opening(AAPL, "100", "150", "140", Some("170")),
        ),
        &ports,
    );
    let entry = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the AAPL bracket is sent");
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-2",
            Some(&entry),
            AAPL,
            Side::Buy,
            "100",
            "60",
            "partially_filled",
        ))),
        &ports,
    );
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-aapl-1",
            Some(&entry),
            "60",
            "150",
        ))),
        &ports,
    );
    shell.run(account_switch(Initiator::Owner), &ports);

    let cancelled = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(stop("canceled"))),
        &ports,
    );

    let started: Vec<_> = cancelled
        .drafts
        .iter()
        .filter(|d| {
            d.event_type == "ProtectionChanged"
                && d.payload
                    .get("action")
                    .and_then(mandate_canon::Value::as_str)
                    == Some("unprotected_start")
        })
        .filter_map(|d| {
            d.payload
                .get("instrument_id")
                .and_then(mandate_canon::Value::as_str)
        })
        .collect();
    assert_eq!(started, vec![BTC], "BTC's own interval starts");
}

/// The confirming read is one taken after the switch (DEC-2333 item 6): a reconciliation whose
/// snapshot was taken at a head before the switch's record cannot show what the cancel-all did, so
/// it confirms nothing and cancels nothing; one taken at or after it does.
#[test]
fn only_a_read_taken_after_the_switch_confirms_its_cancel_all() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, stop) = crypto_stop_account(&ports);
    let before = shell.head().0;
    shell.run(account_switch(Initiator::Owner), &ports);
    let after = shell.head().0;
    let read = |head: u64| {
        let mut taken = snapshot(head, ReconcileReason::Scheduled);
        taken.open_orders = vec![stop("new")];
        Input::BrokerSnapshot(taken)
    };
    let working = stop("new").client_order_id;
    let cancels = |ran: &common::Ran| {
        ran.requests.iter().any(|r| {
            matches!(
                r,
                BrokerRequest::Cancel { client_order_id }
                    if Some(client_order_id.as_str()) == working.as_deref()
            )
        })
    };

    let stale = shell.run(read(before), &ports);
    let fresh = shell.run(read(after), &ports);

    assert!(
        !cancels(&stale),
        "a snapshot taken before the switch confirms nothing: {:?}",
        stale.requests
    );
    assert!(
        cancels(&fresh),
        "one taken after it confirms, and cancels the stop by its id: {:?}",
        fresh.requests
    );
}

/// An agent switch's flatten rung in flight when the account switch goes is never followed by
/// another (DEC-2333 item 6): the flatten is exempt from the agent's mode, so neither the mode nor
/// the account switch's own guard held it before; the cancel-all cancels the rung, and the
/// confirmed cancel steps no new rung out beside the switch's close, then or at the next step.
#[test]
fn a_ladder_rung_in_flight_is_not_followed_by_another_beside_the_close() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Market(quote(AAPL, "150", "150.2", 20)), &ports);
    shell.run(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(agent(common::AGENT)),
            initiator: Initiator::RiskLimit,
            confirmation: None,
            cause: None,
        }),
        &ports,
    );
    let sent = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        })),
        &ports,
    );
    let rung = sent
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the agent switch's flatten sends its first rung");
    shell.run(account_switch(Initiator::Owner), &ports);

    let confirmed = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: rung.clone(),
        })),
        &ports,
    );
    let later = shell.run(Input::Tick(clock(40)), &ports);

    let sent: Vec<_> = confirmed
        .submissions()
        .into_iter()
        .chain(later.submissions())
        .map(|o| o.client_order_id.as_str().to_owned())
        .collect();
    assert!(
        sent.is_empty(),
        "no rung after {rung} beside the switch's close: {sent:?}"
    );
}

/// The close goes once its instrument is confirmed clear (§5.5, DEC-2333 item 2): the cancel-all
/// accepted, a read showing nothing of ours working in BTC, and the broker's close-position for BTC
/// is sent, once.
#[test]
fn a_confirmed_instrument_is_closed_by_the_broker() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, _) = crypto_stop_account(&ports);
    shell.run(account_switch(Initiator::Owner), &ports);
    shell.run(
        Input::Broker(Ok(BrokerOutcome::AccountWideAccepted)),
        &ports,
    );

    let read = shell.run(
        Input::Broker(Ok(BrokerOutcome::OpenOrders(Vec::new()))),
        &ports,
    );

    let closes: Vec<&str> = read
        .requests
        .iter()
        .filter_map(|r| match r {
            BrokerRequest::ClosePosition(_, instrument) => Some(instrument.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(closes, vec![BTC], "BTC's close-position, once");
}

/// The account switch, accepted and read clear, up to the run that closes BTC: the shell, the
/// switch's `KillSwitchActivated` event id, that run, and the stop the cancel-all cancelled.
fn closed_btc(
    ports: &mandate_executor::Ports<'_>,
) -> (
    Shell,
    String,
    common::Ran,
    impl Fn(&str) -> mandate_executor::BrokerOrder,
) {
    let (mut shell, stop) = crypto_stop_account(ports);
    let switched = shell.run(account_switch(Initiator::Owner), ports);
    let switch = switched
        .draft("KillSwitchActivated")
        .map(|d| d.event_id.0.clone())
        .expect("the switch is journaled");
    shell.run(Input::Broker(Ok(BrokerOutcome::AccountWideAccepted)), ports);
    let read = shell.run(
        Input::Broker(Ok(BrokerOutcome::OpenOrders(Vec::new()))),
        ports,
    );
    (shell, switch, read, stop)
}

/// Each close-position is journaled before it is sent (DEC-2337 items 1 and 3, `AGENTS.md` rule
/// 5): one `AccountCloseRequested` naming BTC, the quantity held, and the switch it carries out,
/// ahead of the close-position.
#[test]
fn a_close_is_journaled_before_it_is_sent() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (shell, switch, read, _) = closed_btc(&ports);
    let switch_id = switch.clone();

    let record = read
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Journal(d) if d.event_type == "AccountCloseRequested"))
        .expect("the close is journaled");
    let close = read
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Broker(BrokerRequest::ClosePosition(..))))
        .expect("the close-position goes");
    assert!(record < close, "journaled before it is sent");
    let journaled = read.draft("AccountCloseRequested").expect("the record");
    let field = |name: &str| {
        journaled
            .payload
            .get(name)
            .and_then(mandate_canon::Value::as_str)
            .map(str::to_owned)
    };
    let held = shell
        .state
        .positions()
        .get(&instrument(BTC))
        .map(ToString::to_string);
    assert_eq!(
        (
            field("instrument_id"),
            field("qty"),
            field("kill_switch_event")
        ),
        (Some(BTC.to_owned()), held, Some(switch.clone())),
        "BTC, the quantity held, and the switch"
    );
    assert_eq!(
        journaled
            .payload
            .get("attempt")
            .and_then(mandate_canon::Value::as_int),
        Some(1),
        "the first request"
    );
    let named = shell
        .account_journal
        .iter()
        .find(|e| e.event_id.0 == switch_id)
        .filter(|e| e.event_type == "KillSwitchActivated")
        .and_then(|e| {
            e.payload
                .get("closes")
                .and_then(mandate_canon::Value::as_array)
        })
        .is_some_and(|closes| closes.iter().any(|name| name.as_str() == Some(BTC)));
    assert!(
        named,
        "the switch it names lists BTC in its closes (DEC-2337 item 5, until rule-checked)"
    );
}

/// A journaled close is read as sent, and asked again only on a later read (DEC-2337 item 3): a
/// restart resends nothing; a read taken before the request shows nothing new; a read taken after
/// it, showing BTC still held with nothing of ours working, journals `attempt` 2 before the
/// close-position goes again.
#[test]
fn a_close_is_asked_again_only_on_a_later_read_showing_it_still_held() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (shell, _, _, _) = closed_btc(&ports);
    let requested = shell
        .account_journal
        .iter()
        .find(|e| e.event_type == "AccountCloseRequested")
        .map(|e| e.seq.0)
        .expect("the first request is journaled");
    let held = shell
        .state
        .positions()
        .get(&instrument(BTC))
        .map(ToString::to_string)
        .unwrap_or_default();
    let read = |head: u64| {
        let mut taken = snapshot(head, ReconcileReason::Startup);
        taken.positions = vec![broker_position(BTC, &held)];
        Input::BrokerSnapshot(taken)
    };
    let closes = |ran: &common::Ran| {
        ran.requests
            .iter()
            .filter(|r| matches!(r, BrokerRequest::ClosePosition(..)))
            .count()
    };

    let (mut restarted, started) = shell.restart(&ports);
    let stale = restarted.run(read(requested - 1), &ports);
    let fresh = restarted.run(read(restarted.head().0), &ports);

    assert_eq!(
        (closes(&started), closes(&stale)),
        (0, 0),
        "nothing again before a read taken after the request"
    );
    let attempt = fresh.draft("AccountCloseRequested").and_then(|d| {
        d.payload
            .get("attempt")
            .and_then(mandate_canon::Value::as_int)
    });
    assert_eq!(attempt, Some(2), "the re-request is attempt 2");
    let record = fresh
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Journal(d) if d.event_type == "AccountCloseRequested"));
    let close = fresh
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Broker(BrokerRequest::ClosePosition(..))));
    assert!(
        record.is_some() && close.is_some() && record < close,
        "journaled before it goes again: record at {record:?}, close at {close:?}"
    );
}

/// A later read showing BTC no longer held asks no close again (DEC-2337 item 3): only a read that
/// shows the instrument still held does.
#[test]
fn a_later_read_showing_the_instrument_flat_asks_no_close_again() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (shell, _, _, _) = closed_btc(&ports);
    assert!(
        shell
            .account_journal
            .iter()
            .any(|e| e.event_type == "AccountCloseRequested"),
        "the first request is journaled"
    );

    let (mut restarted, _) = shell.restart(&ports);
    let flat = restarted.run(
        Input::BrokerSnapshot({
            let mut taken = snapshot(restarted.head().0, ReconcileReason::Startup);
            taken.positions = vec![broker_position(BTC, "0")];
            taken
        }),
        &ports,
    );

    assert!(
        !flat
            .requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::ClosePosition(..))),
        "nothing held, nothing asked again: {:?}",
        flat.requests
    );
}

/// The in-doubt re-read stops once nothing the switch closes is held (DEC-2333 item 3): after the
/// close's fill leaves BTC flat, a tick past the window reads nothing again.
#[test]
fn the_switch_reads_nothing_again_once_nothing_is_held() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, _, _, _) = closed_btc(&ports);
    assert!(
        shell
            .account_journal
            .iter()
            .any(|e| e.event_type == "AccountCloseRequested"),
        "the close is requested"
    );
    let held = shell
        .state
        .positions()
        .get(&instrument(BTC))
        .map(ToString::to_string)
        .unwrap_or_default();
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(mandate_executor::BrokerFill {
            instrument: instrument(BTC),
            side: Side::Sell,
            broker_order_id: Some("b-close".to_owned()),
            ..broker_fill("f-btc-close", None, &held, "59000")
        })),
        &ports,
    );

    let later = shell.run(
        Input::Tick(clock(20 + config.unknown_absent_window_s + 1)),
        &ports,
    );

    assert!(
        !later
            .requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::ListOpenOrders)),
        "nothing held, nothing read again: {:?}",
        later.requests
    );
}

/// The close ends only an interval that is open (DEC-2337 item 4): with the stop's cancel never
/// reported, no interval opened under the switch, so the close's fill leaving nothing held journals
/// no `unprotected_end`.
#[test]
fn a_close_with_no_interval_open_ends_none() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, _, _, _) = closed_btc(&ports);
    let close = shell
        .account_journal
        .iter()
        .position(|e| e.event_type == "AccountCloseRequested")
        .expect("the close is requested");
    let held = shell
        .state
        .positions()
        .get(&instrument(BTC))
        .map(ToString::to_string)
        .unwrap_or_default();

    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(mandate_executor::BrokerFill {
            instrument: instrument(BTC),
            side: Side::Sell,
            broker_order_id: Some("b-close".to_owned()),
            ..broker_fill("f-btc-close", None, &held, "59000")
        })),
        &ports,
    );

    let ended: Vec<usize> = shell
        .account_journal
        .iter()
        .enumerate()
        .skip(close)
        .filter(|(_, e)| {
            e.event_type == "ProtectionChanged"
                && e.payload
                    .get("action")
                    .and_then(mandate_canon::Value::as_str)
                    == Some("unprotected_end")
        })
        .map(|(at, _)| at)
        .collect();
    assert!(ended.is_empty(), "no interval to end: {ended:?}");
}

/// Whether a run sent a close-position.
fn sends_a_close(ran: &common::Ran) -> bool {
    ran.requests
        .iter()
        .any(|r| matches!(r, BrokerRequest::ClosePosition(..)))
}

/// A second account switch after a close confirms nothing by the first switch's read (rule 13,
/// DEC-2333 items 2 and 4): its batch sends the cancel-all and no close, its answer alone sends no
/// close, and only a read after it closes BTC again, as that switch's first request.
#[test]
fn a_second_account_switch_closes_nothing_until_its_own_cancel_all_is_read() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, first, _, _) = closed_btc(&ports);

    let second = shell.run(account_switch(Initiator::RiskLimit), &ports);
    let switch = second
        .draft("KillSwitchActivated")
        .map(|d| d.event_id.0.clone())
        .expect("the second switch is journaled");
    let accepted = shell.run(
        Input::Broker(Ok(BrokerOutcome::AccountWideAccepted)),
        &ports,
    );
    let read = shell.run(
        Input::Broker(Ok(BrokerOutcome::OpenOrders(Vec::new()))),
        &ports,
    );

    assert!(
        second
            .requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::CancelAll(_))),
        "the second switch sends its cancel-all: {:?}",
        second.requests
    );
    assert!(
        !sends_a_close(&second) && !sends_a_close(&accepted),
        "no close beside the cancel-all or on its answer alone: {:?} then {:?}",
        second.requests,
        accepted.requests
    );
    assert!(sends_a_close(&read), "the read after it closes BTC");
    let request = read
        .draft("AccountCloseRequested")
        .expect("journaled first");
    let payload = &request.payload;
    assert_eq!(
        (
            payload
                .get("kill_switch_event")
                .and_then(mandate_canon::Value::as_str),
            payload
                .get("attempt")
                .and_then(mandate_canon::Value::as_int)
        ),
        (Some(switch.as_str()), Some(1)),
        "the second switch's first request, not the first switch's ({first})"
    );
}

/// The close waits for the cancel-all's answer (DEC-2333 item 2): an open-orders read that comes
/// before the cancel-all is accepted or lost releases no close, and the read after the answer
/// does.
#[test]
fn a_read_before_the_cancel_all_is_answered_releases_no_close() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, _) = crypto_stop_account(&ports);
    shell.run(account_switch(Initiator::Owner), &ports);

    let early = shell.run(
        Input::Broker(Ok(BrokerOutcome::OpenOrders(Vec::new()))),
        &ports,
    );
    shell.run(
        Input::Broker(Ok(BrokerOutcome::AccountWideAccepted)),
        &ports,
    );
    let read = shell.run(
        Input::Broker(Ok(BrokerOutcome::OpenOrders(Vec::new()))),
        &ports,
    );

    assert!(
        !sends_a_close(&early),
        "no close before the cancel-all is answered: {:?}",
        early.requests
    );
    assert!(
        sends_a_close(&read),
        "the read after the answer closes BTC: {:?}",
        read.requests
    );
}

/// A reconciliation whose snapshot was taken before the switch releases no close (DEC-2333 item
/// 2), even after the cancel-all is answered; one taken at or after it does.
#[test]
fn a_read_taken_before_the_switch_releases_no_close() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, _) = crypto_stop_account(&ports);
    let held = shell
        .state
        .positions()
        .get(&instrument(BTC))
        .map(ToString::to_string)
        .unwrap_or_default();
    let before = shell.head().0;
    shell.run(account_switch(Initiator::Owner), &ports);
    shell.run(
        Input::Broker(Ok(BrokerOutcome::AccountWideAccepted)),
        &ports,
    );
    let read = |head: u64| {
        let mut taken = snapshot(head, ReconcileReason::Scheduled);
        taken.positions = vec![broker_position(BTC, &held)];
        Input::BrokerSnapshot(taken)
    };

    let stale = shell.run(read(before), &ports);
    let fresh = shell.run(read(shell.head().0), &ports);

    assert!(
        !sends_a_close(&stale),
        "a snapshot taken before the switch releases nothing: {:?}",
        stale.requests
    );
    assert!(
        sends_a_close(&fresh),
        "one taken after it closes BTC: {:?}",
        fresh.requests
    );
}

/// BTC's unprotected interval ends on its close (DEC-2337 item 4): the close is journaled, the
/// broker reports the stop the cancel-all cancelled, which opens BTC's interval, and the close's
/// fill, which names no order of ours, leaves nothing held, so the interval journals
/// `unprotected_end` after the `AccountCloseRequested`: the last end, since the entry's own interval
/// ended before the switch.
#[test]
fn the_interval_ends_once_the_close_leaves_nothing_held() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, _, _, stop) = closed_btc(&ports);
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(stop("canceled"))),
        &ports,
    );
    let held = shell
        .state
        .positions()
        .get(&instrument(BTC))
        .map(ToString::to_string)
        .unwrap_or_default();

    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(mandate_executor::BrokerFill {
            instrument: instrument(BTC),
            side: Side::Sell,
            broker_order_id: Some("b-close".to_owned()),
            ..broker_fill("f-btc-close", None, &held, "59000")
        })),
        &ports,
    );

    let close = shell
        .account_journal
        .iter()
        .position(|e| e.event_type == "AccountCloseRequested");
    let end = shell.account_journal.iter().rposition(|e| {
        e.event_type == "ProtectionChanged"
            && e.payload
                .get("action")
                .and_then(mandate_canon::Value::as_str)
                == Some("unprotected_end")
    });
    assert!(
        close.is_some() && end.is_some() && close < end,
        "the interval ends after the close: close at {close:?}, end at {end:?}"
    );
}

/// An instrument with a sell of ours still working waits for that cancel's confirmation, and
/// closes then (DEC-2333 item 3): never beside a sell that may still fill.
#[test]
fn a_working_sell_holds_the_close_until_its_cancel_is_confirmed() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, stop) = crypto_stop_account(&ports);
    shell.run(account_switch(Initiator::Owner), &ports);
    shell.run(
        Input::Broker(Ok(BrokerOutcome::AccountWideAccepted)),
        &ports,
    );
    let read = shell.run(
        Input::Broker(Ok(BrokerOutcome::OpenOrders(vec![stop("new")]))),
        &ports,
    );
    let closing = |ran: &common::Ran| {
        ran.requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::ClosePosition(..)))
    };
    assert!(!closing(&read), "no close beside the working stop");

    let confirmed = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: stop("new").client_order_id.unwrap_or_default(),
        })),
        &ports,
    );

    assert!(
        closing(&confirmed),
        "the stop's confirmed cancel releases BTC's close: {:?}",
        confirmed.requests
    );
}

/// The in-doubt hold reads again every `unknown_absent_window_s` while the cancel-all is
/// unconfirmed (DEC-2333 item 3): a lost answer and a read that never comes back are read again at
/// the window, never by sending the cancel-all again, and not before it.
#[test]
fn the_in_doubt_hold_reads_again_at_the_lookup_window() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, _) = crypto_stop_account(&ports);
    shell.run(Input::Tick(clock(20)), &ports);
    shell.run(account_switch(Initiator::Owner), &ports);
    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);

    let window = config.unknown_absent_window_s;
    let early = shell.run(Input::Tick(clock(20 + window - 1)), &ports);
    let again = shell.run(Input::Tick(clock(20 + window + 1)), &ports);

    assert!(
        !early
            .requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::ListOpenOrders)),
        "nothing read again inside the window: {:?}",
        early.requests
    );

    assert!(
        again
            .requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::ListOpenOrders)),
        "the open orders are read again at the window: {:?}",
        again.requests
    );
    assert!(
        !again
            .requests
            .iter()
            .any(|r| matches!(r, BrokerRequest::CancelAll(_))),
        "never the cancel-all again"
    );
}

/// At §5.4's bound, protection is placed again only for the held quantity less every sell not
/// confirmed cancelled (DEC-2333 item 3): a lost cancel-all confirms nothing, so the original stop,
/// covering the whole position, still counts, and nothing is placed beside it.
#[test]
fn a_lost_cancel_all_at_the_bound_places_nothing_beside_the_original_stop() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, _) = crypto_stop_account(&ports);
    shell.run(account_switch(Initiator::Owner), &ports);
    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);

    let bound = shell.run(Input::Tick(clock(20 + 3_600)), &ports);

    let placed: Vec<_> = bound
        .submissions()
        .into_iter()
        .map(|o| (o.client_order_id.as_str().to_owned(), o.qty))
        .collect();
    assert!(
        placed.is_empty(),
        "nothing beside the stop not confirmed cancelled: {placed:?}"
    );
}

/// A mode lifted after the account switch, as the account stream copies it: `agent` back to
/// `normal` from `restriction`, citing the control-stream record that lifted it.
fn lifted(shell: &mut Shell, agent: &str, restriction: &str) {
    let lift = copied(
        ACCOUNT_STREAM,
        shell.head().0.saturating_add(1),
        "AgentModeApplied",
        with_clock(
            &[
                ("agent", text(agent)),
                ("to", text("normal")),
                ("restriction", text(restriction)),
            ],
            30,
        ),
        &EventId(format!("{CLOCK_STREAM}-21")),
    );
    shell.fold_one(&lift).expect("the lift folds");
}

/// Whether a stop the broker cancels after the account switch, and a lift of `restriction` for
/// `agent`, is placed again: what decides whether the switch still guards its instrument.
fn re_placed_after(agent: &str, restriction: &str) -> bool {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, stop) = crypto_stop_account(&ports);
    shell.run(account_switch(Initiator::Owner), &ports);
    lifted(&mut shell, agent, restriction);
    let cancelled = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(stop("canceled"))),
        &ports,
    );
    cancelled
        .submissions()
        .into_iter()
        .any(|o| o.purpose == Purpose::Protective)
}

/// Lifting the account's kill switch, `*`'s `kill_switch` restriction, ends the switch's guard:
/// a stop the broker cancels afterwards is placed again, as it would be with no switch (§5.4,
/// DEC-2333 item 5).
#[test]
fn lifting_the_account_kill_switch_protects_the_position_again() {
    assert!(
        re_placed_after("*", "kill_switch"),
        "once the account's kill switch is lifted, lost protection is placed again"
    );
}

/// Only the account's own kill-switch lift ends the guard: one agent's kill switch lifted, or
/// another restriction of every agent lifted, leaves the account switch's close unfinished, and
/// nothing is placed beside it (DEC-2333 item 5, rule 12).
#[test]
fn only_the_account_kill_switch_lift_ends_its_guard() {
    for (agent, restriction) in [(common::AGENT, "kill_switch"), ("*", "daily_loss")] {
        assert!(
            !re_placed_after(agent, restriction),
            "{agent}'s {restriction} lifted: the account switch still guards its instrument"
        );
    }
}

/// The account switch closes only what is held: an instrument bought and sold back to flat, with
/// no fresh quote, is neither named in `closes` nor holds the switch back for want of a market
/// (§5.5, DEC-2333 item 1).
#[test]
fn an_account_kill_switch_passes_over_a_flat_instrument() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, CPHC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    for event in protected_position_events(2) {
        shell
            .fold_one(&event)
            .expect("the protected position folds");
    }
    held_by(&mut shell, 6, common::AGENT, CPHC, "5", "md-flat-buy");
    for fact in [
        event(
            ACCOUNT_STREAM,
            9,
            "OrderSubmitted",
            with_clock(
                &[
                    ("client_order_id", text("md-flat-sell")),
                    ("agent", text(common::AGENT)),
                    ("instrument", text(CPHC)),
                    ("side", text("sell")),
                    ("qty", text("5")),
                    ("limit", text("100")),
                ],
                12,
            ),
        ),
        event(
            ACCOUNT_STREAM,
            10,
            "FillApplied",
            with_clock(
                &[
                    ("fill_id", text("f-md-flat-sell")),
                    ("client_order_id", text("md-flat-sell")),
                    ("instrument", text(CPHC)),
                    ("side", text("sell")),
                    ("qty_gross", text("5")),
                    ("price", text("100")),
                ],
                12,
            ),
        ),
        event(
            ACCOUNT_STREAM,
            11,
            "OrderStateChanged",
            with_clock(
                &[
                    ("client_order_id", text("md-flat-sell")),
                    ("state", text("filled")),
                ],
                12,
            ),
        ),
    ] {
        shell.fold_one(&fact).expect("the round trip folds");
    }
    let mut shell = shell.restart_ready(&ports);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), &ports);

    let ran = shell.run(account_switch(Initiator::Owner), &ports);

    let closes: Vec<&str> = ran
        .draft("KillSwitchActivated")
        .expect("the switch goes, CPHC's missing quote notwithstanding")
        .payload
        .get("closes")
        .and_then(mandate_canon::Value::as_array)
        .map(|names| {
            names
                .iter()
                .filter_map(mandate_canon::Value::as_str)
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(closes, vec![AAPL], "only the held instrument");
}

/// Any order of ours the open-orders read shows still working after the cancel-all is cancelled
/// by its own id, a buy as much as a sell: a buy left working would add the risk the switch exists
/// to remove (DEC-2333 item 3).
#[test]
fn a_buy_still_working_after_the_cancel_all_is_cancelled_by_its_id() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, CPHC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, buy) = protected_position_with_a_resting_buy(&ports);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), &ports);
    shell.run(account_switch(Initiator::Owner), &ports);
    shell.run(
        Input::Broker(Ok(BrokerOutcome::AccountWideAccepted)),
        &ports,
    );

    let read = shell.run(
        Input::Broker(Ok(BrokerOutcome::OpenOrders(vec![broker_order(
            "b-buy",
            Some(&buy),
            AAPL,
            Side::Buy,
            "10",
            "0",
            "new",
        )]))),
        &ports,
    );

    assert!(
        read.requests.iter().any(|r| matches!(
            r,
            BrokerRequest::Cancel { client_order_id } if client_order_id.as_str() == buy
        )),
        "the buy still working is cancelled by its id: {:?}",
        read.requests
    );
}

/// A fill that arrives after the account switch never places protection: the entry was partly
/// filled when the switch went, so its instrument is one the switch closes, and the close on
/// confirmation sells the whole position then. A stop placed for the late fill beside that close
/// would sell the same coins twice (DEC-2333 item 5, rule 12).
#[test]
fn a_fill_after_the_account_switch_places_no_protection() {
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
    let entry = submitted
        .submissions()
        .first()
        .map(|o| o.client_order_id.as_str().to_owned())
        .expect("the crypto entry is sent");
    let order = |filled: &str, status: &str| {
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&entry),
            BTC,
            Side::Buy,
            "0.5",
            filled,
            status,
        )))
    };
    shell.run(order("0.2", "partially_filled"), &ports);
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(mandate_executor::BrokerFill {
            instrument: instrument(BTC),
            ..broker_fill("f-btc-1", Some(&entry), "0.2", "60000")
        })),
        &ports,
    );
    shell.run(account_switch(Initiator::Owner), &ports);

    let ended = shell.run(order("0.5", "filled"), &ports);
    let late = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(mandate_executor::BrokerFill {
            instrument: instrument(BTC),
            ..broker_fill("f-btc-2", Some(&entry), "0.3", "60000")
        })),
        &ports,
    );

    let protective: Vec<_> = ended
        .submissions()
        .into_iter()
        .chain(late.submissions())
        .filter(|o| o.purpose == Purpose::Protective)
        .map(|o| (o.client_order_id.as_str().to_owned(), o.qty))
        .collect();
    assert!(
        protective.is_empty(),
        "no stop beside the switch's close: {protective:?}"
    );
}

/// The account switch's records replay (ES-21), and the switch resumes where it stopped: a restart
/// folds every record it journaled, every agent stays stopped, the startup reconciliation's open
/// orders confirm the cancel-all, so the stop still working is cancelled by its id and never
/// placed again (DEC-2333 items 2, 3 and 5).
#[test]
fn an_account_kill_switch_replays_after_a_restart() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, stop) = crypto_stop_account(&ports);
    shell.run(account_switch(Initiator::Owner), &ports);
    assert!(
        shell
            .account_journal
            .iter()
            .any(|e| e.event_type == "KillSwitchActivated"),
        "the switch is on the journal"
    );

    let (mut restarted, started) = shell.restart(&ports);
    let read = restarted.run(
        Input::BrokerSnapshot({
            let mut taken = snapshot(restarted.head().0, ReconcileReason::Startup);
            taken.open_orders = vec![stop("new")];
            taken
        }),
        &ports,
    );

    assert_eq!(
        restarted.state.effective_mode(&agent(common::AGENT)),
        Mode::Stopped,
        "the agent stays stopped after the replay"
    );
    let working = stop("new").client_order_id;
    assert!(
        read.requests.iter().any(|r| matches!(
            r,
            BrokerRequest::Cancel { client_order_id }
                if Some(client_order_id.as_str()) == working.as_deref()
        )),
        "the startup read confirms the cancel-all and cancels the stop by its id: {:?}",
        read.requests
    );
    let sent: Vec<&BrokerRequest> = started.requests.iter().chain(&read.requests).collect();
    assert!(
        !sent
            .iter()
            .any(|r| matches!(r, BrokerRequest::CancelAll(_) | BrokerRequest::Submit(_))),
        "never a second cancel-all, and nothing placed: {sent:?}"
    );
}

/// The account switch is journaled before anything is sent (`AGENTS.md` rule 5): one
/// `KillSwitchActivated` of scope `account`, naming the account as its subject and the
/// instruments it closes, ahead of the cancel-all.
#[test]
fn an_account_kill_switch_is_journaled_naming_the_account_before_it_acts() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = two_agent_account(&ports);

    let ran = shell.run(account_switch(Initiator::Owner), &ports);

    let record = ran
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Journal(d) if d.event_type == "KillSwitchActivated"))
        .expect("the switch is journaled");
    let cancel_all = ran
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Broker(BrokerRequest::CancelAll(_))))
        .expect("the cancel-all goes");
    assert!(record < cancel_all, "journaled before it acts");
    let switch = ran.draft("KillSwitchActivated").expect("the record");
    let field = |name: &str| {
        switch
            .payload
            .get(name)
            .and_then(mandate_canon::Value::as_str)
    };
    assert_eq!(
        (field("scope"), field("subject")),
        (Some("account"), Some(common::ACCOUNT)),
        "scope account, subject the account"
    );
    let mut closes: Vec<&str> = switch
        .payload
        .get("closes")
        .and_then(mandate_canon::Value::as_array)
        .map(|names| {
            names
                .iter()
                .filter_map(mandate_canon::Value::as_str)
                .collect()
        })
        .unwrap_or_default();
    closes.sort_unstable();
    assert_eq!(closes, vec![AAPL, BTC], "naming every instrument it closes");
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
            cause: None,
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
                cause: None,
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
            cause: None,
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
fn an_owner_exit_outside_the_session_prices_from_the_confirmed_bid() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = protected_position(&ports);
    shell.run(Input::Tick(clock(PRE_MARKET)), &ports);

    shell.run(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(agent(common::AGENT)),
            initiator: Initiator::Owner,
            confirmation: Some(owner_confirmation()),
            cause: None,
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
                cause: None,
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
            cause: None,
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
            cause: None,
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
    let handed = fired
        .draft("IntentReceived")
        .expect("the watchdog hands in its exit");
    let agent_of = |payload: &mandate_canon::Value, member: &str| {
        payload
            .get(member)
            .and_then(mandate_canon::Value::as_str)
            .map(str::to_owned)
    };
    let intents = agent_of(&handed.payload, "agent_id");
    assert_eq!(
        [
            agent_of(&watchdog.payload, "agent").or_else(|| intents.clone()),
            intents,
            agent_of(&watchdog.payload, "agent_id"),
        ],
        [Some("*".to_owned()), Some("*".to_owned()), None],
        "five of fifteen is no single holder, so the exit belongs to no agent (§2.3): its intent \
         names `*`, as a legacy watchdog's `agent` does; §9.5's closed watchdog names no agent"
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
            cause: None,
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
            cause: None,
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
            cause: None,
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
        ExecutorError::OwnFillMismatch {
            client_order_id: "x".to_owned(),
            reason: "fills_short",
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
        25,
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

/// One command applied as the switch of two agents counts as applied for each of them, and for no
/// other agent or record (DEC-2343 item 4, `ExecutorState::applied_for`): the executor process
/// dedupes a command against the runtime's copy per agent.
#[test]
fn a_switch_copied_from_one_record_is_applied_for_its_own_agents_only() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = started();
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(&ports);
    let command = EventId("01JCMD000000000000000000AA".to_owned());
    for named in [common::AGENT, OTHER_AGENT] {
        shell.run(
            Input::Command(Command::KillSwitch {
                scope: KillScope::Agent(agent(named)),
                initiator: Initiator::RiskLimit,
                confirmation: None,
                cause: Some(command.clone()),
            }),
            &ports,
        );
    }
    let other = EventId("01JCMD000000000000000000AB".to_owned());
    assert_eq!(
        (
            shell.state.applied_for(&agent(common::AGENT), &command),
            shell.state.applied_for(&agent(OTHER_AGENT), &command),
            shell.state.applied_for(&agent("agent-c"), &command),
            shell.state.applied_for(&agent(common::AGENT), &other),
        ),
        (true, true, false, false),
        "each named agent once, no other agent or record"
    );
}
