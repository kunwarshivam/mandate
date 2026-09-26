//! Hand-calculated cases for the agent runtime
//! ([task brief](../../../docs/project/tasks/E6-1-agent-runtime-and-kill-switches.md)).
//!
//! Every case states one clause of the mandate, trading-domain, or journal spec and computes its
//! expectation by hand, so the case is evidence about the spec rather than a restatement of the code.
//! Cases whose subject is a kill switch are `pending E6-5`; the rest are `pending E6-1`.

mod common;

use common::{
    ACCOUNT_STREAM, AGENT_STREAM, AllowGate, AppendOutcome, CLOCK_STREAM, CONTROL_STREAM, DenyGate,
    FixedPlan, OTHER_AGENT_STREAM, Shell, TestIds, clock, derived_id, event, fresh_output,
    instrument, int, object, ports, price, qty, stale_output, text, universe,
    view_with_restriction, with_clock,
};
use mandate_accounting::AssetClass;
use mandate_runtime::{
    Autonomy, Command, Effect, EventId, FOLD_VERSION, Initiator, Input, IntentBody, KillScope,
    Mode, OwnerConfirmation, Purpose, RuntimeError, RuntimeState, Seq, TimerId, TimerRequest,
    WriterEpoch, fold,
};

/// A clean startup reconciliation on the account stream: what lifts the startup hold.
fn reconciliation(seq: u64, at: i64) -> mandate_runtime::FoldedEvent {
    event(
        ACCOUNT_STREAM,
        seq,
        "ReconciliationRun",
        with_clock(&[("result", text("clean"))], at),
    )
}

/// An `AgentModeApplied` on the account stream and the id it was copied from.
fn mode_applied(seq: u64, mode: &str, at: i64) -> mandate_runtime::FoldedEvent {
    event(
        ACCOUNT_STREAM,
        seq,
        "AgentModeApplied",
        with_clock(
            &[("to", text(mode)), ("restriction", text("daily_loss"))],
            at,
        ),
    )
}

fn owner_confirmation() -> OwnerConfirmation {
    OwnerConfirmation {
        bid: price("155"),
        bid_size: qty("100"),
        floor: price("154"),
        user: "user-1".to_owned(),
        step_up: "assertion-1".to_owned(),
    }
}

#[test]
#[ignore = "pending E6-1"]
fn a_gap_in_seq_fails_the_fold() {
    let mut state = RuntimeState::new(common::deployment());
    let first = event(ACCOUNT_STREAM, 1, "StreamOpened", object(&[]));
    fold(&mut state, &first).expect("seq 1 folds");
    let gap = reconciliation(3, 10);
    let error = fold(&mut state, &gap).expect_err("a gap must fail");
    assert_eq!(error.code(), "sequence_out_of_order", "{error}");
    assert!(
        matches!(
            error,
            RuntimeError::SequenceOutOfOrder {
                expected: 2,
                found: 3,
                ..
            }
        ),
        "the refusal names the seq it wanted and the one it got: {error}"
    );
}

#[test]
#[ignore = "pending E6-1"]
fn a_repeated_seq_fails_the_fold() {
    let mut state = RuntimeState::new(common::deployment());
    let first = event(ACCOUNT_STREAM, 1, "StreamOpened", object(&[]));
    fold(&mut state, &first).expect("seq 1 folds");
    let repeat = event(ACCOUNT_STREAM, 1, "StreamOpened", object(&[]));
    let error = fold(&mut state, &repeat).expect_err("a repeat must fail");
    assert_eq!(error.code(), "sequence_out_of_order", "{error}");
}

#[test]
#[ignore = "pending E6-1"]
fn an_unknown_event_type_fails_the_fold() {
    let mut state = RuntimeState::new(common::deployment());
    let unknown = event(ACCOUNT_STREAM, 1, "SomethingNobodyWrote", object(&[]));
    let error = fold(&mut state, &unknown).expect_err("an uninterpreted event must fail loudly");
    assert_eq!(error.code(), "not_interpreted", "{error}");
    assert!(
        format!("{error}").contains("SomethingNobodyWrote"),
        "the refusal names what it could not interpret: {error}"
    );
}

#[test]
#[ignore = "pending E6-1"]
fn a_copied_mode_change_points_at_the_originating_event() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::silent();
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 10)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);

    let origin = mode_applied(2, "exits_only", 20);
    shell.fold_one(&origin).expect("folds");
    let ran = shell.run(Input::Journal(origin.clone()), &ports);

    let copy = ran
        .drafts
        .iter()
        .find(|d| d.event_type == "AgentModeChanged")
        .expect("the copy is journaled on the agent stream");
    assert_eq!(
        copy.causation_id.as_ref(),
        Some(&origin.event_id),
        "the copy cites the account-stream event it came from (journal spec §2)"
    );
}

#[test]
#[ignore = "pending E6-1"]
fn the_golden_journal_folds_to_the_committed_state() {
    let committed = include_str!("golden-journal.json");
    let golden =
        mandate_canon::parse(committed.as_bytes()).expect("the golden journal is canonical");
    let object = golden.as_object().expect("an object");
    let events = object
        .get("events")
        .and_then(|v| v.as_array())
        .expect("an event list");
    let expected = object
        .get("expected")
        .and_then(|v| v.as_object())
        .expect("the expected fold output");

    let mut state = RuntimeState::new(common::deployment());
    for value in events {
        let row = value.as_object().expect("each event is an object");
        let folded = mandate_runtime::FoldedEvent {
            stream: row
                .get("stream")
                .and_then(|v| v.as_str())
                .expect("a stream")
                .to_owned(),
            seq: Seq(row.get("seq").and_then(|v| v.as_int()).expect("a seq")),
            event_id: EventId(format!(
                "{}-{}",
                row.get("stream")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default(),
                row.get("seq").and_then(|v| v.as_int()).unwrap_or_default()
            )),
            event_type: row
                .get("event_type")
                .and_then(|v| v.as_str())
                .expect("an event type")
                .to_owned(),
            causation_id: None,
            payload: row.get("payload").cloned().expect("a payload"),
        };
        fold(&mut state, &folded)
            .unwrap_or_else(|e| panic!("{} at seq {} fails: {e}", folded.event_type, folded.seq.0));
    }

    let mode = match state.effective_mode() {
        Mode::Normal => "normal",
        Mode::ExitsOnly => "exits_only",
        Mode::Paused => "paused",
        Mode::Stopped => "stopped",
    };
    assert_eq!(
        Some(mode),
        expected.get("effective_mode").and_then(|v| v.as_str()),
        "the committed fold output pins the mode"
    );
    assert_eq!(
        state.risk_clock().map(|c| c.secs()),
        expected
            .get("risk_clock")
            .and_then(|v| v.as_int())
            .and_then(|n| i64::try_from(n).ok()),
        "and the last risk-clock second"
    );
    assert_eq!(
        state.head(ACCOUNT_STREAM).map(|seq| seq.0),
        expected.get("account_head").and_then(|v| v.as_int()),
        "and the folded position"
    );
    assert_eq!(
        u64::try_from(state.pending_approvals().len()).unwrap_or(u64::MAX),
        expected
            .get("pending_approvals")
            .and_then(|v| v.as_int())
            .unwrap_or_default(),
        "and that nothing is pending"
    );
    assert_eq!(
        u64::try_from(state.outstanding().len()).unwrap_or(u64::MAX),
        expected
            .get("outstanding")
            .and_then(|v| v.as_int())
            .unwrap_or_default(),
        "and that nothing is outstanding"
    );
    assert_eq!(
        FOLD_VERSION,
        u32::try_from(
            golden
                .as_object()
                .and_then(|o| o.get("fold_version"))
                .and_then(|v| v.as_int())
                .unwrap_or_default()
        )
        .unwrap_or_default(),
        "a change to fold output bumps FOLD_VERSION and regenerates this file in the same commit"
    );
}

#[test]
#[ignore = "pending E6-1"]
fn the_fold_version_is_pinned_with_the_golden_journal() {
    assert_eq!(
        FOLD_VERSION, 1,
        "a change to fold output bumps this and regenerates the golden journal (ES-21)"
    );
    let mut state = RuntimeState::new(common::deployment());
    fold(&mut state, &reconciliation(1, 10)).expect("the golden journal's first event folds");
}

#[test]
#[ignore = "pending E6-1"]
fn an_unhandled_command_names_its_story() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::silent();
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    let error = shell
        .step(Input::Tick(clock(10)), &ports)
        .expect_err("a tick before Started must fail");
    assert_eq!(
        error.code(),
        "not_started",
        "an input before the runtime started is refused, not guessed: {error}"
    );
}

#[test]
#[ignore = "pending E6-1"]
fn every_error_code_is_stable_and_unique() {
    let codes = [
        RuntimeError::Unimplemented { story: "E6-1" }.code(),
        RuntimeError::NotInterpreted {
            what: "X".to_owned(),
            story: "E6-1",
        }
        .code(),
        RuntimeError::SequenceOutOfOrder {
            stream: AGENT_STREAM.to_owned(),
            expected: 2,
            found: 3,
        }
        .code(),
        RuntimeError::ForeignStream {
            stream: "acct:other:1".to_owned(),
        }
        .code(),
        RuntimeError::CopyWithoutCausation {
            event_type: "AgentModeApplied".to_owned(),
        }
        .code(),
        RuntimeError::RiskClockWentBackwards { last: 10, found: 9 }.code(),
        RuntimeError::AppendUnresolved { head: 4 }.code(),
        RuntimeError::EpochMismatch {
            folded: 1,
            found: 2,
        }
        .code(),
        RuntimeError::AlreadyStarted.code(),
        RuntimeError::NotStarted.code(),
        RuntimeError::NonCanonicalPayload {
            field: "price".to_owned(),
        }
        .code(),
        RuntimeError::UnknownApproval {
            approval: "a".to_owned(),
        }
        .code(),
    ];
    let mut sorted = codes.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        codes.len(),
        "every code is distinct: {codes:?}"
    );
    assert!(
        codes
            .iter()
            .all(|c| !c.is_empty() && c.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')),
        "codes are snake_case and non-empty: {codes:?}"
    );

    let mut state = RuntimeState::new(common::deployment());
    fold(
        &mut state,
        &event(ACCOUNT_STREAM, 1, "StreamOpened", object(&[])),
    )
    .expect("this deployment's own account stream binds the workspace");
    let foreign = event("acct:other-workspace:9", 1, "StreamOpened", object(&[]));
    let error = fold(&mut state, &foreign).expect_err("a stream of another workspace is refused");
    assert_eq!(
        error.code(),
        "foreign_stream",
        "the code a caller matches on comes from the same set: {error}"
    );
}

#[test]
#[ignore = "pending E6-1"]
fn a_tick_that_changes_nothing_emits_no_effect() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::silent();
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 10)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);

    let ran = shell.run(Input::Tick(clock(11)), &ports);
    assert!(
        ran.is_empty(),
        "a tick that crosses no boundary journals nothing (MI-13): {:?}",
        ran.draft_types()
    );
}

#[test]
#[ignore = "pending E6-1"]
fn a_deadline_is_measured_in_whole_seconds_of_risk_clock() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Ask);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::ModelOutput(fresh_output(100)), &ports);
    let asked = shell.run(Input::Tick(clock(100)), &ports);
    assert!(
        asked.draft_types().contains(&"ApprovalRequested"),
        "an ASK proposal asks: {:?}",
        asked.draft_types()
    );

    let one_second_early = shell.run(Input::Tick(clock(399)), &ports);
    assert!(
        one_second_early.draft_types().is_empty(),
        "at 299 s of a 300 s deadline nothing has expired: {:?}",
        one_second_early.draft_types()
    );
    let on_the_second = shell.run(Input::Tick(clock(400)), &ports);
    assert_eq!(
        on_the_second.draft_types(),
        vec!["ApprovalTimedOut"],
        "the deadline fires on the 300th whole second and skips the action"
    );
}

#[test]
#[ignore = "pending E6-1"]
fn event_time_never_moves_a_deadline() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Ask);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::ModelOutput(fresh_output(100)), &ports);
    shell.run(Input::Tick(clock(100)), &ports);

    let a_mark_whose_wording_claims_a_later_instant_than_its_risk_clock = event(
        ACCOUNT_STREAM,
        2,
        "MarkUpdated",
        with_clock(
            &[
                ("instrument", text("AAPL")),
                ("price", text("151")),
                ("event_time", text("2026-09-26T23:59:59Z")),
            ],
            100,
        ),
    );
    shell
        .fold_one(&a_mark_whose_wording_claims_a_later_instant_than_its_risk_clock)
        .expect("folds");
    let ran = shell.run(
        Input::Journal(a_mark_whose_wording_claims_a_later_instant_than_its_risk_clock),
        &ports,
    );
    assert!(
        !ran.draft_types().contains(&"ApprovalTimedOut"),
        "only the risk clock moves a deadline: {:?}",
        ran.draft_types()
    );
}

#[test]
#[ignore = "pending E6-1"]
fn a_tick_during_an_unresolved_append_does_not_change_the_drafts() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::ModelOutput(fresh_output(100)), &ports);

    shell.next_append = AppendOutcome::Unresolved;
    let first = shell.run(Input::Tick(clock(100)), &ports);
    assert!(
        !first.drafts.is_empty(),
        "the step produced a draft whose append is now in doubt"
    );

    let error = shell
        .step(Input::Tick(clock(101)), &ports)
        .expect_err("a new input at an unresolved head must be refused");
    assert_eq!(error.code(), "append_unresolved", "{error}");
}

#[test]
#[ignore = "pending E6-1"]
fn a_retried_append_derives_the_same_event_id() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);

    let mut first = Shell::new(1);
    first.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut first, _) = first.restart(&ports);
    first.run(Input::ModelOutput(fresh_output(100)), &ports);
    let before = first.head();
    let attempt = first.run(Input::Tick(clock(100)), &ports);

    let mut again = Shell::new(1);
    again.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut again, _) = again.restart(&ports);
    again.run(Input::ModelOutput(fresh_output(100)), &ports);
    let retry = again.run(Input::Tick(clock(100)), &ports);

    assert_eq!(
        attempt
            .drafts
            .iter()
            .map(|d| d.event_id.clone())
            .collect::<Vec<_>>(),
        retry
            .drafts
            .iter()
            .map(|d| d.event_id.clone())
            .collect::<Vec<_>>(),
        "the same epoch, head, and ordinals derive the same ids, so a retry is AlreadyCommitted"
    );
    let expected_first = derived_id(again.epoch, before, 0);
    assert_eq!(
        retry.drafts.first().map(|d| d.event_id.clone()),
        Some(expected_first),
        "and the derivation is the one the oracle computes independently"
    );
}

#[test]
#[ignore = "pending E6-1"]
fn two_epochs_never_derive_one_event_id() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);

    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut old_writer, _) = shell.restart(&ports);
    old_writer.run(Input::ModelOutput(fresh_output(100)), &ports);
    let head = old_writer.head();
    let old = old_writer.run(Input::Tick(clock(100)), &ports);

    let (mut new_writer, _) = old_writer.restart(&ports);
    new_writer.run(Input::ModelOutput(fresh_output(200)), &ports);
    let new = new_writer.run(Input::Tick(clock(200)), &ports);

    let old_ids: Vec<_> = old.drafts.iter().map(|d| d.event_id.clone()).collect();
    let new_ids: Vec<_> = new.drafts.iter().map(|d| d.event_id.clone()).collect();
    assert!(
        old_ids.iter().all(|id| !new_ids.contains(id)),
        "the epoch is in the derivation, so a fenced writer's retry cannot collide with the new \
         writer's id and turn a Fenced into an IdempotencyConflict: {old_ids:?} versus {new_ids:?}"
    );
    assert_ne!(
        derived_id(WriterEpoch(1), head, 0),
        derived_id(WriterEpoch(2), head, 0),
        "which the oracle's own derivation also separates"
    );
}

#[test]
#[ignore = "pending E6-1"]
fn a_fenced_append_stops_the_runtime() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::ModelOutput(fresh_output(100)), &ports);

    shell.next_append = AppendOutcome::Fenced;
    let ran = shell.run(Input::Tick(clock(100)), &ports);
    assert!(
        shell.stopped,
        "a newer epoch owns the stream, so this process exits"
    );
    assert!(
        ran.handed.is_empty(),
        "and nothing it computed reaches the sink: {:?}",
        ran.handed
    );
}

#[test]
#[ignore = "pending E6-1"]
fn an_unresolved_append_is_retried_before_any_new_input() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::ModelOutput(fresh_output(100)), &ports);

    let drafted_ids = |ran: &common::Ran| -> Vec<EventId> {
        ran.effects
            .iter()
            .filter_map(|e| match e {
                Effect::Journal(draft) => Some(draft.event_id.clone()),
                _ => None,
            })
            .collect()
    };

    shell.next_append = AppendOutcome::Unresolved;
    let doubted = shell.run(Input::Tick(clock(100)), &ports);
    let drafted = drafted_ids(&doubted);
    assert!(!drafted.is_empty(), "the doubted batch had drafts to retry");

    shell.next_append = AppendOutcome::Committed;
    let resolved = shell.run(Input::Tick(clock(100)), &ports);
    assert_eq!(
        drafted_ids(&resolved),
        drafted,
        "the retry sends the same drafts with the same derived ids, which is what makes the          append answer AlreadyCommitted rather than appending a second event"
    );
}

#[test]
#[ignore = "pending E6-1"]
fn an_unchanged_mode_journals_nothing() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::silent();
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 10)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);

    let first = mode_applied(2, "exits_only", 20);
    shell.fold_one(&first).expect("folds");
    let changed = shell.run(Input::Journal(first), &ports);
    assert_eq!(changed.draft_types(), vec!["AgentModeChanged"]);

    let same = mode_applied(3, "exits_only", 30);
    shell.fold_one(&same).expect("folds");
    let unchanged = shell.run(Input::Journal(same), &ports);
    assert!(
        unchanged.draft_types().is_empty(),
        "AgentModeChanged is journaled only when the mode changes (MI-6): {:?}",
        unchanged.draft_types()
    );
}

#[test]
#[ignore = "pending E6-1"]
fn a_restriction_that_lifts_while_another_is_active_does_not_restore_normal() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::silent();
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let shell = Shell::new(1);
    let (mut shell, _) = shell.restart(&ports);
    assert_eq!(
        shell.state.effective_mode(),
        Mode::Paused,
        "a replay with nothing reconciled holds awaiting_reconciliation"
    );
    shell.run(Input::Command(Command::Pause), &ports);
    shell.run(Input::Command(Command::Resume), &ports);
    assert_eq!(
        shell.state.effective_mode(),
        Mode::Paused,
        "lifting the owner's pause must not lift the startup hold with it. Each restriction lifts \
         independently (mandate §5.9), so a lift that cleared the whole set would restore normal \
         while the broker's truth is still unconfirmed"
    );

    let lifts_the_startup_hold = reconciliation(1, 30);
    shell.fold_one(&lifts_the_startup_hold).expect("folds");
    shell.run(Input::Journal(lifts_the_startup_hold), &ports);
    assert_eq!(
        shell.state.effective_mode(),
        Mode::Normal,
        "and once the last hold lifts, and only then, the agent is normal again"
    );
}

#[test]
#[ignore = "pending E6-1"]
fn an_owner_resume_does_not_lift_the_copied_account_mode() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::silent();
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 10)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);

    let paused = mode_applied(2, "paused", 20);
    shell.fold_one(&paused).expect("folds");
    shell.run(Input::Journal(paused), &ports);
    shell.run(Input::Command(Command::Resume), &ports);
    assert_eq!(
        shell.state.effective_mode(),
        Mode::Paused,
        "the runtime never lifts what the account stream set (DEC-131 item 9)"
    );
}

#[test]
#[ignore = "pending E6-1"]
fn a_broker_restriction_arrives_as_a_mode_copy_not_a_flatten() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::silent();
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 10)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);

    let restricted = event(
        ACCOUNT_STREAM,
        2,
        "AgentModeApplied",
        with_clock(
            &[
                ("to", text("exits_only")),
                ("restriction", text("account_restricted")),
            ],
            20,
        ),
    );
    shell.fold_one(&restricted).expect("folds");
    let ran = shell.run(Input::Journal(restricted), &ports);
    assert_eq!(
        ran.draft_types(),
        vec!["AgentModeChanged"],
        "a broker-driven restriction is a mode change, never a kill switch (trading §7.3)"
    );
    assert!(
        ran.handed.is_empty(),
        "and it hands no flatten: {:?}",
        ran.handed
    );
}

#[test]
#[ignore = "pending E6-1"]
fn exits_only_still_proposes_an_exit() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::exiting(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let exits_only = mode_applied(2, "exits_only", 100);
    shell.fold_one(&exits_only).expect("folds");
    shell.run(Input::Journal(exits_only), &ports);
    shell.run(Input::ModelOutput(fresh_output(100)), &ports);

    let ran = shell.run(Input::Tick(clock(100)), &ports);
    assert!(
        ran.draft_types().contains(&"IntentProposed"),
        "exits_only allows a risk-reducing intent: {:?}",
        ran.draft_types()
    );
}

#[test]
#[ignore = "pending E6-1"]
fn a_removed_instrument_proposes_no_opening() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = view_with_restriction(&["AAPL"], &["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::ModelOutput(fresh_output(100)), &ports);

    let ran = shell.run(Input::Tick(clock(100)), &ports);
    assert!(
        !ran.draft_types().contains(&"IntentProposed"),
        "an instrument under removed_instrument takes no opening order (mandate §5.9): {:?}",
        ran.draft_types()
    );
}

#[test]
#[ignore = "pending E6-1"]
fn a_removed_instrument_still_exits() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::exiting(Autonomy::Auto);
    let view = view_with_restriction(&["AAPL"], &["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::ModelOutput(fresh_output(100)), &ports);

    let ran = shell.run(Input::Tick(clock(100)), &ports);
    assert!(
        ran.draft_types().contains(&"IntentProposed"),
        "protection stays and exits still work in a removed instrument (mandate §2.3): {:?}",
        ran.draft_types()
    );
}

#[test]
#[ignore = "pending E6-1"]
fn a_stopped_agent_is_terminal() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::Command(Command::Stop), &ports);
    shell.run(Input::Command(Command::Resume), &ports);
    assert_eq!(
        shell.state.effective_mode(),
        Mode::Stopped,
        "nothing resumes a stopped deployment (DEC-131 item 12)"
    );

    shell.run(Input::ModelOutput(fresh_output(100)), &ports);
    let ran = shell.run(Input::Tick(clock(100)), &ports);
    assert!(
        !ran.draft_types().contains(&"IntentProposed") && ran.handed.is_empty(),
        "and it proposes nothing ever again: {:?} {:?}",
        ran.draft_types(),
        ran.handed
    );
}

#[test]
#[ignore = "pending E6-1"]
fn a_proposal_journals_before_it_reaches_the_sink() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::ModelOutput(fresh_output(100)), &ports);

    let ran = shell.run(Input::Tick(clock(100)), &ports);
    let intent_at = ran
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Intent(_)))
        .expect("the proposal reaches the sink");
    let draft_at = ran
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Journal(d) if d.event_type == "IntentProposed"))
        .expect("and is journaled");
    assert!(
        draft_at < intent_at,
        "journal before acting (AGENTS.md rule 5): draft at {draft_at}, handoff at {intent_at}"
    );
    let handed = ran.handed.first().expect("one handoff");
    let draft = ran
        .drafts
        .iter()
        .find(|d| d.event_type == "IntentProposed")
        .expect("one draft");
    assert_eq!(
        handed.intent_id, draft.event_id,
        "the intent id is the IntentProposed event id (journal spec §2)"
    );
}

#[test]
#[ignore = "pending E6-1"]
fn a_dry_run_deny_skips_the_proposal() {
    let ids = TestIds;
    let gate = DenyGate("position_cap");
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::ModelOutput(fresh_output(100)), &ports);

    let ran = shell.run(Input::Tick(clock(100)), &ports);
    assert!(
        ran.draft_types().contains(&"DecisionMade"),
        "the decision and its dry-run verdict are recorded: {:?}",
        ran.draft_types()
    );
    assert!(
        !ran.draft_types().contains(&"IntentProposed") && ran.handed.is_empty(),
        "a dry-run deny narrows the proposal away: {:?}",
        ran.draft_types()
    );
}

#[test]
#[ignore = "pending E6-1"]
fn a_stale_model_output_proposes_nothing() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 1000)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::ModelOutput(stale_output(1000)), &ports);

    let ran = shell.run(Input::Tick(clock(1000)), &ports);
    assert!(
        !ran.draft_types().contains(&"IntentProposed"),
        "an expired output is no basis for an order (mandate §8.1): {:?}",
        ran.draft_types()
    );
}

/// The `ApprovalRequested` this deployment is waiting on, after one ASK decision.
fn asked(shell: &mut Shell, ports: &mandate_runtime::Ports<'_>, at: i64) -> EventId {
    shell.run(Input::ModelOutput(fresh_output(at)), ports);
    let ran = shell.run(Input::Tick(clock(at)), ports);
    ran.drafts
        .iter()
        .find(|d| d.event_type == "ApprovalRequested")
        .map(|d| d.event_id.clone())
        .expect("an ASK decision requests an approval")
}

#[test]
#[ignore = "pending E6-1"]
fn an_approval_binds_the_quantity_and_the_version() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Ask);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::ModelOutput(fresh_output(100)), &ports);
    let ran = shell.run(Input::Tick(clock(100)), &ports);

    let request = ran
        .drafts
        .iter()
        .find(|d| d.event_type == "ApprovalRequested")
        .expect("the ASK asks");
    let bound = request
        .payload
        .as_object()
        .expect("the payload is an object");
    for field in ["qty", "limit", "mandate_version", "deadline"] {
        assert!(
            bound.contains_key(field),
            "an approval binds {field} (mandate §6.4): {:?}",
            bound.keys().map(|k| k.as_str()).collect::<Vec<_>>()
        );
    }
    assert_eq!(
        bound.get("mandate_version").and_then(|v| v.as_str()),
        Some(common::VERSION),
        "and binds the version it was proposed under"
    );
}

#[test]
#[ignore = "pending E6-1"]
fn an_approval_under_a_changed_version_is_skipped() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Ask);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let approval = asked(&mut shell, &ports, 100);

    let applied = event(
        ACCOUNT_STREAM,
        2,
        "MandateVersionApplied",
        with_clock(
            &[
                ("classification", text("risk_reducing")),
                ("new_version", text("v2")),
                ("result", text("applied")),
            ],
            110,
        ),
    );
    shell.fold_one(&applied).expect("folds");
    shell.run(Input::Journal(applied), &ports);

    let ran = shell.run(
        Input::ApprovalResponse(mandate_runtime::ApprovalOutcome {
            approval,
            verdict: mandate_runtime::ApprovalVerdict::Approved {
                responder: "user-1".to_owned(),
                step_up: "assertion-1".to_owned(),
            },
            at: clock(120),
        }),
        &ports,
    );
    assert!(
        ran.handed.is_empty(),
        "an approval bound to a superseded version is skipped, never re-priced: {:?}",
        ran.handed
    );
}

#[test]
#[ignore = "pending E6-1"]
fn an_approval_deadline_skips_the_action() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Ask);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let _ = asked(&mut shell, &ports, 100);

    let ran = shell.run(Input::Tick(clock(400)), &ports);
    assert_eq!(
        ran.draft_types(),
        vec!["ApprovalTimedOut"],
        "on_timeout is always skip (mandate §6.4)"
    );
    assert!(
        ran.handed.is_empty(),
        "a timeout never acts: {:?}",
        ran.handed
    );
}

#[test]
#[ignore = "pending E6-1"]
fn a_copied_exits_only_cancels_every_pending_approval() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Ask);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let _ = asked(&mut shell, &ports, 100);

    let exits_only = mode_applied(2, "exits_only", 110);
    shell.fold_one(&exits_only).expect("folds");
    let ran = shell.run(Input::Journal(exits_only), &ports);
    assert!(
        ran.draft_types().contains(&"ApprovalCanceled"),
        "entering exits_only cancels pending approvals (mandate §5.9): {:?}",
        ran.draft_types()
    );
    assert!(
        shell.state.pending_approvals().is_empty(),
        "and none is left waiting"
    );
}

#[test]
#[ignore = "pending E6-1"]
fn an_owner_pause_cancels_every_pending_approval() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Ask);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let _ = asked(&mut shell, &ports, 100);

    let ran = shell.run(Input::Command(Command::Pause), &ports);
    assert!(
        ran.draft_types().contains(&"ApprovalCanceled"),
        "an owner pause is a tightening too: {:?}",
        ran.draft_types()
    );
}

#[test]
#[ignore = "pending E6-1"]
fn a_reducing_version_applies_at_once_and_cancels_approvals() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Ask);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let _ = asked(&mut shell, &ports, 100);

    let reducing = event(
        ACCOUNT_STREAM,
        2,
        "MandateVersionApplied",
        with_clock(
            &[
                ("classification", text("risk_reducing")),
                ("new_version", text("v2")),
                ("result", text("applied")),
            ],
            110,
        ),
    );
    shell.fold_one(&reducing).expect("folds");
    let ran = shell.run(Input::Journal(reducing), &ports);
    assert!(
        ran.draft_types().contains(&"ApprovalCanceled"),
        "a reducing version applies at once and cancels pending approvals: {:?}",
        ran.draft_types()
    );
}

#[test]
#[ignore = "pending E6-1"]
fn a_risk_increasing_version_waits_for_a_safe_point() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::ModelOutput(fresh_output(100)), &ports);
    let proposed = shell.run(Input::Tick(clock(100)), &ports);
    assert!(
        !proposed.handed.is_empty(),
        "an Unknown-order window is opened by an outstanding intent"
    );

    let increasing = event(
        ACCOUNT_STREAM,
        2,
        "MandateVersionApplied",
        with_clock(
            &[
                ("classification", text("risk_increasing")),
                ("new_version", text("v2")),
                ("result", text("applied")),
            ],
            110,
        ),
    );
    shell.fold_one(&increasing).expect("folds");
    let ran = shell.run(Input::Journal(increasing), &ports);
    assert!(
        !ran.draft_types().contains(&"DecisionMade"),
        "a risk-increasing version waits for an evaluation with no Unknown orders: {:?}",
        ran.draft_types()
    );
}

#[test]
#[ignore = "pending E6-1"]
fn started_journals_the_startup_hold_before_its_first_handoff() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut first = Shell::new(1);
    first.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut first, _) = first.restart(&ports);
    first.run(Input::ModelOutput(fresh_output(100)), &ports);
    first.run(Input::Tick(clock(100)), &ports);

    let (_after, started) = first.restart(&ports);
    let hold_at = started
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Journal(d) if d.event_type == "AgentModeChanged"))
        .expect("the startup hold is journaled");
    assert_eq!(hold_at, 0, "and it is the first effect of Started");
    if let Some(handoff_at) = started
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Intent(_)))
    {
        assert!(
            hold_at < handoff_at,
            "nothing is re-handed before the hold exists in the journal (review finding 5)"
        );
    }
}

#[test]
#[ignore = "pending E6-1"]
fn a_restart_stays_paused_until_the_account_reconciles() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let shell = Shell::new(1);
    let (mut shell, _) = shell.restart(&ports);
    assert_eq!(
        shell.state.effective_mode(),
        Mode::Paused,
        "after a replay the runtime holds awaiting_reconciliation (trading §11, HLD)"
    );

    shell.run(Input::ModelOutput(fresh_output(100)), &ports);
    let held = shell.run(Input::Tick(clock(100)), &ports);
    assert!(
        held.handed.is_empty(),
        "and proposes nothing while held: {:?}",
        held.handed
    );

    let clean = reconciliation(1, 110);
    shell.fold_one(&clean).expect("folds");
    shell.run(Input::Journal(clean), &ports);
    assert_eq!(
        shell.state.effective_mode(),
        Mode::Normal,
        "a clean reconciliation at or after the last submission lifts the hold"
    );
}

#[test]
#[ignore = "pending E6-1"]
fn an_earlier_reconciliation_does_not_lift_the_startup_hold() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    let early = reconciliation(1, 50);
    shell.fold_one(&early).expect("folds");
    let submitted = event(
        ACCOUNT_STREAM,
        2,
        "OrderSubmitted",
        object(&[("client_order_id", text("o1")), ("attempt", int(1))]),
    );
    shell.fold_one(&submitted).expect("folds");

    let (shell, _) = shell.restart(&ports);
    assert_eq!(
        shell.state.effective_mode(),
        Mode::Paused,
        "a reconciliation older than the last submission proves nothing about it"
    );
}

#[test]
#[ignore = "pending E6-1"]
fn an_unexplained_position_keeps_the_runtime_paused() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    let mismatch = event(
        ACCOUNT_STREAM,
        1,
        "ReconciliationRun",
        with_clock(
            &[("result", text("mismatch")), ("difference", text("5"))],
            100,
        ),
    );
    shell.fold_one(&mismatch).expect("folds");
    let paused = mode_applied(2, "paused", 100);
    shell.fold_one(&paused).expect("folds");

    let (shell, _) = shell.restart(&ports);
    assert_eq!(
        shell.state.effective_mode(),
        Mode::Paused,
        "HLD sends Recovering to Paused on anything unexplained, and only owner acknowledgment \
         with step-up lifts a reconciliation pause (trading §11)"
    );
}

#[test]
#[ignore = "pending E6-1"]
fn a_restart_mid_run_journals_nothing_new() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::ModelOutput(fresh_output(100)), &ports);
    shell.run(Input::Tick(clock(100)), &ports);
    let before = shell.agent_journal.len();

    let (after, started) = shell.restart(&ports);
    let re_journaled: Vec<_> = started
        .draft_types()
        .into_iter()
        .filter(|t| *t != "AgentModeChanged")
        .collect();
    assert!(
        re_journaled.is_empty(),
        "a restart journals only the startup hold: {re_journaled:?}"
    );
    assert_eq!(
        after.agent_journal.len(),
        before.saturating_add(1),
        "one new event, the hold, and nothing else"
    );
}

#[test]
#[ignore = "pending E6-1"]
fn a_restart_after_an_intent_committed_re_hands_it_without_re_journaling() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::exiting(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::ModelOutput(fresh_output(100)), &ports);
    let first = shell.run(Input::Tick(clock(100)), &ports);
    let intent_id = first
        .handed
        .first()
        .map(|h| h.intent_id.clone())
        .expect("the exit was handed once");

    let (_after, started) = shell.restart(&ports);
    assert_eq!(
        started
            .handed
            .iter()
            .map(|h| h.intent_id.clone())
            .collect::<Vec<_>>(),
        vec![intent_id],
        "the outstanding exit is handed again, by the same intent id, so the executor dedupes it"
    );
    assert!(
        !started.draft_types().contains(&"IntentProposed"),
        "and it is not proposed a second time: {:?}",
        started.draft_types()
    );
}

#[test]
#[ignore = "pending E6-1"]
fn a_started_handoff_names_a_draft_the_fold_already_saw() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::exiting(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::ModelOutput(fresh_output(100)), &ports);
    shell.run(Input::Tick(clock(100)), &ports);

    let (after, started) = shell.restart(&ports);
    for handoff in &started.handed {
        assert!(
            after
                .agent_journal
                .iter()
                .any(|e| e.event_id == handoff.intent_id && e.event_type == "IntentProposed"),
            "Started's exception to write-before-acting is exactly this: the recording draft was \
             folded as committed before the call (DEC-131 item 7)"
        );
    }
}

#[test]
#[ignore = "pending E6-1"]
fn a_restart_under_stopped_re_hands_no_opening_intent() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::ModelOutput(fresh_output(100)), &ports);
    shell.run(Input::Tick(clock(100)), &ports);
    shell.run(Input::Command(Command::Stop), &ports);

    let (_after, started) = shell.restart(&ports);
    assert!(
        started.handed.is_empty(),
        "a stopped agent re-hands no opening intent (review finding 2): {:?}",
        started.handed
    );
}

#[test]
#[ignore = "pending E6-1"]
fn a_restart_under_exits_only_still_re_hands_an_exit() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::exiting(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::ModelOutput(fresh_output(100)), &ports);
    shell.run(Input::Tick(clock(100)), &ports);
    let exits_only = mode_applied(2, "exits_only", 110);
    shell.fold_one(&exits_only).expect("folds");
    shell.run(Input::Journal(exits_only), &ports);
    shell
        .fold_one(&reconciliation(3, 120))
        .expect("a clean reconciliation lifts the startup hold, so the copied mode governs");

    let (_after, started) = shell.restart(&ports);
    assert_eq!(
        started.handed.len(),
        1,
        "exits_only still permits the exit, so it is re-handed: {:?}",
        started.handed
    );
}

/// A started shell with a pending approval and an outstanding opening intent, which is the state a
/// kill switch has the most to undo.
fn armed_shell(ports: &mandate_runtime::Ports<'_>) -> Shell {
    let mut shell = Shell::new(1);
    shell
        .fold_one(&reconciliation(1, 100))
        .expect("the reconciliation folds");
    let (mut shell, _) = shell.restart(ports);
    shell.run(Input::ModelOutput(fresh_output(100)), ports);
    shell.run(Input::Tick(clock(100)), ports);
    shell
}

fn kill(scope: KillScope, initiator: Initiator, confirmation: Option<OwnerConfirmation>) -> Input {
    Input::Command(Command::KillSwitch {
        scope,
        initiator,
        confirmation,
    })
}

fn this_agent() -> KillScope {
    KillScope::Agent(mandate_runtime::AgentId("agent-a".to_owned()))
}

#[test]
#[ignore = "pending E6-5"]
fn an_owner_kill_switch_applies_stopped_before_anything_else() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Ask);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = armed_shell(&ports);

    let ran = shell.run(
        kill(this_agent(), Initiator::Owner, Some(owner_confirmation())),
        &ports,
    );
    let first = ran.drafts.first().expect("the switch journals something");
    assert_eq!(
        first.event_type,
        "AgentModeChanged",
        "the final mode is the first effect (trading §5.5): {:?}",
        ran.draft_types()
    );
    assert_eq!(
        first
            .payload
            .as_object()
            .and_then(|o| o.get("to"))
            .and_then(|v| v.as_str()),
        Some("stopped"),
        "and an owner kill switch is stopped, not paused"
    );
}

#[test]
#[ignore = "pending E6-5"]
fn an_automated_flatten_applies_paused_first() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = armed_shell(&ports);

    let ran = shell.run(kill(this_agent(), Initiator::RiskLimit, None), &ports);
    let first = ran.drafts.first().expect("the flatten journals something");
    assert_eq!(
        first
            .payload
            .as_object()
            .and_then(|o| o.get("to"))
            .and_then(|v| v.as_str()),
        Some("paused"),
        "a mandate limit's flatten pauses, so the owner can acknowledge it (mandate §5.5)"
    );
}

#[test]
#[ignore = "pending E6-5"]
fn a_platform_operator_stop_applies_stopped_as_a_risk_exit() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = armed_shell(&ports);

    let ran = shell.run(
        kill(this_agent(), Initiator::PlatformOperator, None),
        &ports,
    );
    let first = ran.drafts.first().expect("the stop journals something");
    assert_eq!(
        first
            .payload
            .as_object()
            .and_then(|o| o.get("to"))
            .and_then(|v| v.as_str()),
        Some("stopped"),
        "an operator stop is terminal (DEC-100)"
    );
    let plan_handed = ran
        .handed
        .iter()
        .find_map(|h| match &h.body {
            IntentBody::Flatten(plan) => Some(plan.clone()),
            IntentBody::Order { .. } => None,
        })
        .expect("it hands a flatten");
    assert_eq!(
        plan_handed.purpose,
        Purpose::RiskExit,
        "and sells on the automated schedule, because an operator cannot confirm a bid \
         (Decisions needed 6)"
    );
    assert!(
        plan_handed.confirmation.is_none(),
        "with no owner confirmation attached"
    );
}

#[test]
#[ignore = "pending E6-5"]
fn an_owner_kill_switch_journals_the_owner_exit_request_before_the_handoff() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = armed_shell(&ports);

    let ran = shell.run(
        kill(this_agent(), Initiator::Owner, Some(owner_confirmation())),
        &ports,
    );
    let request_at = ran
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Journal(d) if d.event_type == "OwnerExitRequested"))
        .expect("an owner-initiated stop records the owner's instruction (journal §9)");
    let handoff_at = ran
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Intent(_)))
        .expect("and hands the flatten");
    assert!(
        request_at < handoff_at,
        "the executor needs the confirmed bid and floor price before it prices the exit"
    );
    let payload = ran
        .drafts
        .iter()
        .find(|d| d.event_type == "OwnerExitRequested")
        .and_then(|d| d.payload.as_object().cloned())
        .expect("the request carries a payload");
    for field in ["bid", "bid_size", "floor", "user", "step_up", "confirmed"] {
        assert!(
            payload.contains_key(field),
            "OwnerExitRequested carries {field} (mandate §5.10, trading §5.5)"
        );
    }
}

#[test]
#[ignore = "pending E6-5"]
fn an_unconfirmed_owner_exit_leaves_the_equity_sells_for_the_session() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = armed_shell(&ports);

    let ran = shell.run(kill(this_agent(), Initiator::Owner, None), &ports);
    let payload = ran
        .drafts
        .iter()
        .find(|d| d.event_type == "OwnerExitRequested")
        .and_then(|d| d.payload.as_object().cloned())
        .expect("the request is still recorded");
    assert_eq!(
        payload.get("confirmed").and_then(|v| match v {
            mandate_canon::Value::Bool(b) => Some(*b),
            _ => None,
        }),
        Some(false),
        "without a confirmed bid the request says so, and trading §5.5 makes equity sells wait"
    );
    let handed = ran
        .handed
        .iter()
        .find_map(|h| match &h.body {
            IntentBody::Flatten(plan) => Some(plan.clone()),
            IntentBody::Order { .. } => None,
        })
        .expect("the flatten is still handed: a kill switch is never blocked");
    assert!(
        handed.confirmation.is_none(),
        "and it carries no confirmation the owner did not give"
    );
}

#[test]
#[ignore = "pending E6-5"]
fn a_kill_switch_cancels_every_pending_approval() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Ask);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = armed_shell(&ports);
    assert!(
        !shell.state.pending_approvals().is_empty(),
        "the fixture leaves an approval pending"
    );

    let ran = shell.run(kill(this_agent(), Initiator::Owner, None), &ports);
    assert!(
        ran.draft_types().contains(&"ApprovalCanceled"),
        "an approval that outlives the switch is an order after the stop: {:?}",
        ran.draft_types()
    );
    assert!(
        shell.state.pending_approvals().is_empty(),
        "so none is left"
    );
}

#[test]
#[ignore = "pending E6-5"]
fn an_approval_that_arrives_after_a_kill_switch_proposes_nothing() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Ask);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let approval = asked(&mut shell, &ports, 100);
    shell.run(kill(this_agent(), Initiator::Owner, None), &ports);

    let late = shell.run(
        Input::ApprovalResponse(mandate_runtime::ApprovalOutcome {
            approval,
            verdict: mandate_runtime::ApprovalVerdict::Approved {
                responder: "user-1".to_owned(),
                step_up: "assertion-1".to_owned(),
            },
            at: clock(200),
        }),
        &ports,
    );
    assert!(
        late.handed.is_empty() && !late.draft_types().contains(&"IntentProposed"),
        "a response after the stop proposes nothing: {:?} {:?}",
        late.draft_types(),
        late.handed
    );
}

#[test]
#[ignore = "pending E6-5"]
fn a_kill_switch_hands_one_flatten_plan_to_the_sink() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = armed_shell(&ports);

    let ran = shell.run(kill(this_agent(), Initiator::RiskLimit, None), &ports);
    let flattens: Vec<_> = ran
        .handed
        .iter()
        .filter(|h| matches!(h.body, IntentBody::Flatten(_)))
        .collect();
    assert_eq!(
        flattens.len(),
        1,
        "exactly one flatten, however many positions it covers: {:?}",
        ran.handed
    );
    assert!(
        ran.draft_types().contains(&"KillSwitchActivated"),
        "and the switch is journaled on the agent stream: {:?}",
        ran.draft_types()
    );
}

#[test]
#[ignore = "pending E6-5"]
fn the_runtime_never_emits_a_cancel_all_or_a_close_position() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let flatten = common::FixedFlatten::one_equity();
    let view = universe(&["AAPL"]);
    let ports = common::ports_with_flatten(&ids, &gate, &plan, &flatten, &view);

    for initiator in [
        Initiator::Owner,
        Initiator::RiskLimit,
        Initiator::PlatformOperator,
    ] {
        let mut shell = armed_shell(&ports);
        let known: Vec<String> = shell
            .state
            .outstanding()
            .keys()
            .map(|id| id.0.clone())
            .collect();
        let ran = shell.run(kill(this_agent(), initiator, None), &ports);
        let handed = ran
            .handed
            .iter()
            .find_map(|h| match &h.body {
                IntentBody::Flatten(plan) => Some(plan.clone()),
                IntentBody::Order { .. } => None,
            })
            .expect("every initiator hands a flatten");

        for id in &handed.cancel_client_order_ids {
            assert!(
                !id.is_empty() && !id.contains('*') && !id.eq_ignore_ascii_case("all"),
                "cancels name one client_order_id at a time, never a wildcard: {id}"
            );
            assert!(
                known.contains(id),
                "and only orders this agent knows about, never the account's (trading §5.5): \
                 {id} is not one of {known:?}"
            );
        }
        assert!(
            !handed.sells.is_empty(),
            "a flatten sells the agent's sub-ledger quantity"
        );
        for leg in &handed.sells {
            assert!(
                !leg.qty.is_zero(),
                "a sell names an exact quantity, never a close-position endpoint"
            );
            assert!(
                matches!(leg.asset_class, AssetClass::UsEquity | AssetClass::Crypto),
                "and an asset class whose schedule trading §5.5 defines"
            );
        }
    }
}

#[test]
#[ignore = "pending E6-5"]
fn a_kill_switch_for_another_agent_changes_nothing() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = armed_shell(&ports);
    let mode_before = shell.state.effective_mode();

    let ran = shell.run(
        kill(
            KillScope::Agent(mandate_runtime::AgentId("agent-b".to_owned())),
            Initiator::Owner,
            None,
        ),
        &ports,
    );
    assert!(
        ran.is_empty(),
        "a deployment outside the scope emits nothing at all: {:?}",
        ran.draft_types()
    );
    assert_eq!(
        shell.state.effective_mode(),
        mode_before,
        "and its mode is untouched"
    );
}

#[test]
#[ignore = "pending E6-5"]
fn a_connection_kill_switch_stops_every_agent_on_that_connection() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = armed_shell(&ports);

    let ran = shell.run(
        kill(
            KillScope::Connection(mandate_runtime::ConnectionId("conn-1".to_owned())),
            Initiator::Owner,
            None,
        ),
        &ports,
    );
    assert_eq!(
        shell.state.effective_mode(),
        Mode::Stopped,
        "this deployment is on the connection, so it gets the agent-scoped treatment"
    );
    assert_eq!(
        ran.handed
            .iter()
            .filter(|h| matches!(h.body, IntentBody::Flatten(_)))
            .count(),
        1,
        "one agent-scoped flatten; the account-wide cancel-all is the executor's"
    );
}

#[test]
#[ignore = "pending E6-5"]
fn a_workspace_kill_switch_stops_every_agent_in_the_workspace() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = armed_shell(&ports);

    shell.run(
        kill(
            KillScope::Workspace(mandate_runtime::WorkspaceId("ws1".to_owned())),
            Initiator::Owner,
            None,
        ),
        &ports,
    );
    assert_eq!(
        shell.state.effective_mode(),
        Mode::Stopped,
        "a workspace switch reaches every deployment in it, this one included"
    );
}

#[test]
#[ignore = "pending E6-5"]
fn a_kill_switch_with_no_model_output_still_stops_the_agent() {
    let ids = TestIds;
    let gate = DenyGate("everything");
    let plan = FixedPlan::silent();
    let view = universe(&[]);
    let ports = ports(&ids, &gate, &plan, &view);
    let shell = Shell::new(1);
    let (mut shell, _) = shell.restart(&ports);

    let ran = shell.run(kill(this_agent(), Initiator::Owner, None), &ports);
    assert_eq!(
        shell.state.effective_mode(),
        Mode::Stopped,
        "the switch needs no model state, no gate, and no approval (AGENTS.md rule 13)"
    );
    assert!(
        ran.draft_types().contains(&"KillSwitchActivated"),
        "and is journaled even from a bare state: {:?}",
        ran.draft_types()
    );
}

#[test]
#[ignore = "pending E6-5"]
fn the_runtime_never_lifts_a_risk_limit_restriction_itself() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = armed_shell(&ports);
    let flattened = mode_applied(2, "paused", 110);
    shell.fold_one(&flattened).expect("folds");
    shell.run(Input::Journal(flattened), &ports);

    let an_acknowledgment_that_is_not_the_account_streams_lift = event(
        CONTROL_STREAM,
        1,
        "OwnerAcknowledged",
        object(&[
            ("subject", text("drawdown_flatten")),
            ("user", text("user-1")),
        ]),
    );
    shell
        .fold_one(&an_acknowledgment_that_is_not_the_account_streams_lift)
        .expect("folds");
    shell.run(
        Input::Journal(an_acknowledgment_that_is_not_the_account_streams_lift),
        &ports,
    );
    assert_eq!(
        shell.state.effective_mode(),
        Mode::Paused,
        "only a copied AgentModeApplied changes the runtime's view, whichever limit latched:          drawdown_flatten lifts by acknowledgment once flat (mandate §5.8), daily_loss          automatically on a new risk day (§5.4), and lifetime_floor only by a loosened version          (§5.7). The runtime lifts none of them itself"
    );
}

#[test]
#[ignore = "pending E6-1"]
fn a_notification_carries_only_opaque_ids() {
    let ids = TestIds;
    let gate = DenyGate("position_cap");
    let plan = FixedPlan::opening(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::ModelOutput(fresh_output(100)), &ports);
    let ran = shell.run(Input::Tick(clock(100)), &ports);

    for effect in &ran.effects {
        if let Effect::Notify(reference) = effect {
            let rendered = format!("{} {}", reference.subject_event.0, reference.message_key);
            for secret in ["AAPL", "150", "10", "buy"] {
                assert!(
                    !rendered.contains(secret),
                    "a notification carries opaque ids and generic text only: {rendered}"
                );
            }
        }
    }
    let _ = OTHER_AGENT_STREAM;
    let _ = CLOCK_STREAM;
}

#[test]
#[ignore = "pending E6-5"]
fn a_kill_switch_cancels_the_working_order_the_fold_knows() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Auto);
    let flatten = common::FixedFlatten::one_equity();
    let view = universe(&["AAPL"]);
    let ports = common::ports_with_flatten(&ids, &gate, &plan, &flatten, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::ModelOutput(fresh_output(100)), &ports);
    let proposed = shell.run(Input::Tick(clock(100)), &ports);
    let working = proposed
        .handed
        .first()
        .map(|h| h.intent_id.0.clone())
        .expect("one order is working");

    let ran = shell.run(kill(this_agent(), Initiator::RiskLimit, None), &ports);
    let handed = ran
        .handed
        .iter()
        .find_map(|h| match &h.body {
            IntentBody::Flatten(plan) => Some(plan.clone()),
            IntentBody::Order { .. } => None,
        })
        .expect("the switch hands a flatten");
    assert!(
        handed.cancel_client_order_ids.contains(&working),
        "a kill switch that left a working order uncancelled would be the worst defect this crate \
         can have, so the order the fold knows about must appear in the plan's cancel list: \
         {working} is not in {:?}",
        handed.cancel_client_order_ids
    );
    assert!(
        !handed.sells.is_empty(),
        "and the plan sells the agent's sub-ledger, so an empty plan cannot pass this case"
    );
}

#[test]
#[ignore = "pending E6-1"]
fn a_restart_re_hands_nothing_for_an_intent_the_account_already_took() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::exiting(Autonomy::Auto);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::ModelOutput(fresh_output(100)), &ports);
    let first = shell.run(Input::Tick(clock(100)), &ports);
    let intent = first
        .handed
        .first()
        .map(|h| h.intent_id.0.clone())
        .expect("the exit was handed");

    let taken = event(
        ACCOUNT_STREAM,
        2,
        "IntentReceived",
        object(&[
            ("intent_id", text(&intent)),
            ("instrument", text("AAPL")),
            ("purpose", text("discretionary_exit")),
        ]),
    );
    shell.fold_one(&taken).expect("folds");

    let (_after, started) = shell.restart(&ports);
    assert!(
        started.handed.is_empty(),
        "an intent the account stream has taken has a terminal outcome, so it is no longer \
         outstanding and a restart must not hand it again: {:?}",
        started.handed
    );
}

#[test]
#[ignore = "pending E6-1"]
fn a_restart_re_arms_the_deadline_the_fold_carries() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(Autonomy::Ask);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);
    let approval = asked(&mut shell, &ports, 100);
    let armed_before = shell.armed.clone();
    assert!(
        armed_before.contains_key(&TimerId::ApprovalDeadline(approval.clone())),
        "asking arms the deadline"
    );

    let (after, started) = shell.restart(&ports);
    assert!(
        started.timers.iter().any(|request| matches!(
            request,
            TimerRequest::Arm { id, .. }
                if *id == TimerId::ApprovalDeadline(approval.clone())
        )),
        "Started re-arms every deadline the fold carries, so a crash costs a wakeup and never a \
         deadline: {:?}",
        started.timers
    );
    assert_eq!(
        after
            .armed
            .get(&TimerId::ApprovalDeadline(approval.clone())),
        armed_before.get(&TimerId::ApprovalDeadline(approval)),
        "and re-arms it at the same second, because the deadline is folded state and not the timer"
    );
}

#[test]
#[ignore = "pending E6-1"]
fn an_owner_exit_of_one_instrument_names_its_story() {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::silent();
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell.fold_one(&reconciliation(1, 100)).expect("folds");
    let (mut shell, _) = shell.restart(&ports);

    let error = shell
        .step(
            Input::Command(Command::OwnerExit {
                instrument: instrument("AAPL"),
                confirmation: Some(owner_confirmation()),
            }),
            &ports,
        )
        .expect_err("an owner exit of one instrument is not this story's");
    assert_eq!(
        error.code(),
        "not_interpreted",
        "an uninterpreted command fails loudly and names the story that owns it (DEC-85): {error}"
    );
    assert!(
        format!("{error}").contains("E7-2"),
        "and the story it names is the executor's: {error}"
    );
}
