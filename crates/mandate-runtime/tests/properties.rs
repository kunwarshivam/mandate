//! Property tests for the agent runtime, against three oracles that share no code with the crate
//! ([task brief](../../../docs/project/tasks/E6-1-agent-runtime-and-kill-switches.md)).
//!
//! 1. **The shadow fold** rebuilds mode, approvals, and outstanding intents from the emitted drafts'
//!    payloads alone, reading them as `mandate_canon::Value` rather than through any crate type, so a
//!    core that keeps state the journal does not carry fails.
//! 2. **The restriction lattice** recomputes the effective mode as a maximum over a separately
//!    written ordering, so a core that confuses `paused` with `exits_only`, or clears a hold another
//!    still holds, fails.
//! 3. **The interval accumulator** recomputes deadlines by summing whole seconds between consecutive
//!    inputs, crediting each interval to the state at its start (mandate spec §5.2), so a core that
//!    credits at the end, or resets a timer on every input, fails.
//!
//! Every property first compares the **number** of effects with the oracle's, so none can pass on an
//! empty effect list.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{
    ACCOUNT_STREAM, AGENT_STREAM, AllowGate, DenyGate, FixedPlan, Shell, TestIds, clock, copied,
    derived_id, event, fresh_output, object, ports, stale_output, text, universe, with_clock,
};
use mandate_canon::Value;
use mandate_runtime::{
    Autonomy, Command, Effect, EventDraft, EventId, Initiator, Input, IntentBody, KillScope, Mode,
    Purpose, RuntimeState, Seq, WriterEpoch, fold,
};
use proptest::prelude::*;

/// What the journal alone says the runtime's state is. Built from draft payloads, never from
/// `RuntimeState`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Shadow {
    mode: String,
    pending_approvals: BTreeSet<String>,
    outstanding: BTreeSet<String>,
    stopped: bool,
}

impl Shadow {
    fn of(drafts: &[EventDraft]) -> Self {
        let mut shadow = Self {
            mode: "normal".to_owned(),
            ..Self::default()
        };
        for draft in drafts {
            let field = |name: &str| -> Option<String> {
                draft
                    .payload
                    .as_object()?
                    .get(name)
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            };
            match draft.event_type.as_str() {
                "AgentModeChanged" => {
                    if let Some(to) = field("to") {
                        shadow.stopped = shadow.stopped || to == "stopped";
                        shadow.mode = to;
                    }
                }
                "KillSwitchActivated" => shadow.stopped = true,
                "ApprovalRequested" => {
                    shadow.pending_approvals.insert(draft.event_id.0.clone());
                }
                "ApprovalCanceled" | "ApprovalTimedOut" | "ApprovalResponded" => {
                    if let Some(approval) = field("approval") {
                        shadow.pending_approvals.remove(&approval);
                    }
                }
                "IntentProposed" => {
                    shadow.outstanding.insert(draft.event_id.0.clone());
                }
                _ => {}
            }
        }
        shadow
    }

    fn of_state(state: &RuntimeState) -> Self {
        Self {
            mode: match state.effective_mode() {
                Mode::Normal => "normal",
                Mode::ExitsOnly => "exits_only",
                Mode::Paused => "paused",
                Mode::Stopped => "stopped",
            }
            .to_owned(),
            pending_approvals: state
                .pending_approvals()
                .keys()
                .map(|id| id.0.clone())
                .collect(),
            outstanding: state.outstanding().keys().map(|id| id.0.clone()).collect(),
            stopped: state.effective_mode() == Mode::Stopped,
        }
    }
}

/// A separately written severity ordering. Written as integers on purpose: if the crate's `Ord` on
/// `Mode` were wrong, deriving the oracle from it would hide the bug.
fn severity(mode: &str) -> u8 {
    match mode {
        "normal" => 0,
        "exits_only" => 1,
        "paused" => 2,
        "stopped" => 3,
        other => panic!("unknown mode {other}"),
    }
}

/// The effective mode: the strictest of the copied account mode and every local hold.
fn lattice_max(copied_mode: &str, holds: &[&str]) -> String {
    let mut worst = copied_mode.to_owned();
    for hold in holds {
        if severity(hold) > severity(&worst) {
            worst = (*hold).to_owned();
        }
    }
    worst
}

/// Whether a deadline set at `armed` for `window` seconds has expired by `now`, computed by summing
/// the whole seconds of each interval and crediting it to the state at the interval's start.
fn expired_by(armed: i64, window: i64, ticks: &[i64]) -> bool {
    let mut elapsed = 0_i64;
    let mut previous = armed;
    for tick in ticks {
        if *tick <= previous {
            continue;
        }
        elapsed = elapsed.saturating_add(tick.saturating_sub(previous));
        previous = *tick;
        if elapsed >= window {
            return true;
        }
    }
    false
}

/// The modes an account stream can copy in.
fn mode_name() -> impl Strategy<Value = &'static str> {
    prop_oneof!["normal", "exits_only", "paused"].prop_map(|s: String| match s.as_str() {
        "exits_only" => "exits_only",
        "paused" => "paused",
        _ => "normal",
    })
}

/// A short input script: ticks, marks, mode changes, and model outputs.
#[derive(Debug, Clone)]
enum Scripted {
    Tick(i64),
    Mark(i64),
    ModeTo(&'static str, i64),
    Fresh(i64),
    Stale(i64),
}

fn scripted() -> impl Strategy<Value = Scripted> {
    prop_oneof![
        (100_i64..400).prop_map(Scripted::Tick),
        (100_i64..400).prop_map(Scripted::Mark),
        (mode_name(), 100_i64..400).prop_map(|(m, at)| Scripted::ModeTo(m, at)),
        (100_i64..400).prop_map(Scripted::Fresh),
        (100_i64..400).prop_map(Scripted::Stale),
    ]
}

/// Runs a script, returning what the shell saw. Account-stream `seq` is assigned in order, so the
/// fold's gapless rule is respected by construction and the script tests behaviour, not sequencing.
fn play(script: &[Scripted], autonomy: Autonomy) -> (Shell, Vec<Effect>, Vec<EventDraft>) {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::opening(autonomy);
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);

    let mut shell = Shell::new(1);
    let mut seq = 1_u64;
    let mut clock_floor = 100_i64;
    shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            seq,
            "ReconciliationRun",
            with_clock(&[("result", text("clean"))], 100),
        ))
        .expect("the startup reconciliation folds");
    let (mut shell, started) = shell.restart(&ports);
    let mut effects = started.effects.clone();
    let mut drafts = started.drafts.clone();

    for step in script {
        let ran = match step {
            Scripted::Tick(at) => {
                clock_floor = (*at).max(clock_floor);
                shell.run(Input::Tick(clock(clock_floor)), &ports)
            }
            Scripted::Mark(at) => {
                clock_floor = (*at).max(clock_floor);
                let at = &clock_floor;
                seq = seq.saturating_add(1);
                let mark = event(
                    ACCOUNT_STREAM,
                    seq,
                    "MarkUpdated",
                    with_clock(&[("instrument", text("AAPL")), ("price", text("150"))], *at),
                );
                shell.fold_one(&mark).expect("a mark folds");
                shell.run(Input::Journal(mark), &ports)
            }
            Scripted::ModeTo(mode, at) => {
                clock_floor = (*at).max(clock_floor);
                let at = &clock_floor;
                seq = seq.saturating_add(1);
                let applied = event(
                    ACCOUNT_STREAM,
                    seq,
                    "AgentModeApplied",
                    with_clock(
                        &[("to", text(mode)), ("restriction", text("daily_loss"))],
                        *at,
                    ),
                );
                shell.fold_one(&applied).expect("a mode change folds");
                shell.run(Input::Journal(applied), &ports)
            }
            Scripted::Fresh(at) => shell.run(Input::ModelOutput(fresh_output(*at)), &ports),
            Scripted::Stale(at) => shell.run(Input::ModelOutput(stale_output(*at)), &ports),
        };
        effects.extend(ran.effects);
        drafts.extend(ran.drafts);
    }
    (shell, effects, drafts)
}

proptest! {
    #[test]
    #[ignore = "pending E6-1"]
    fn two_runs_of_the_same_inputs_give_equal_effects(script in prop::collection::vec(scripted(), 1..8)) {
        let (_, first, _) = play(&script, Autonomy::Auto);
        let (_, again, _) = play(&script, Autonomy::Auto);
        prop_assert_eq!(first.len(), again.len(), "the same inputs give the same number of effects");
        prop_assert_eq!(first, again, "and the same effects, in the same order (ES-21)");
    }

    #[test]
    #[ignore = "pending E6-1"]
    fn folding_the_journaled_drafts_reproduces_the_live_state(
        script in prop::collection::vec(scripted(), 1..8),
    ) {
        let (shell, _, drafts) = play(&script, Autonomy::Ask);
        let shadow = Shadow::of(&drafts);
        let live = Shadow::of_state(&shell.state);
        prop_assert_eq!(
            shadow,
            live,
            "state the journal does not carry is state a replay cannot rebuild"
        );
    }

    #[test]
    #[ignore = "pending E6-1"]
    fn a_replay_of_any_run_emits_no_draft_and_no_intent(
        script in prop::collection::vec(scripted(), 1..8),
    ) {
        let (shell, _, _) = play(&script, Autonomy::Auto);
        let mut replayed = RuntimeState::new();
        for stored in shell.agent_journal.iter().chain(shell.followed.iter()) {
            fold(&mut replayed, stored).map_err(|e| TestCaseError::fail(format!("{e}")))?;
        }
        prop_assert_eq!(
            Shadow::of_state(&replayed),
            Shadow::of_state(&shell.state),
            "a replay reproduces the state, and `fold` returns nothing to send"
        );
    }

    #[test]
    #[ignore = "pending E6-1"]
    fn started_re_journals_nothing_but_the_startup_hold(
        script in prop::collection::vec(scripted(), 1..8),
    ) {
        let ids = TestIds;
        let gate = AllowGate;
        let plan = FixedPlan::opening(Autonomy::Auto);
        let view = universe(&["AAPL"]);
        let ports = ports(&ids, &gate, &plan, &view);
        let (shell, _, _) = play(&script, Autonomy::Auto);
        let (_after, started) = shell.restart(&ports);
        let unexpected: Vec<_> = started
            .draft_types()
            .into_iter()
            .filter(|t| *t != "AgentModeChanged")
            .collect();
        prop_assert!(
            unexpected.is_empty(),
            "recovery journals only the hold: {:?}",
            unexpected
        );
    }

    #[test]
    #[ignore = "pending E6-1"]
    fn every_intent_effect_follows_the_draft_that_records_it(
        script in prop::collection::vec(scripted(), 1..8),
    ) {
        let (_, effects, _) = play(&script, Autonomy::Auto);
        let mut recorded: BTreeSet<String> = BTreeSet::new();
        for effect in &effects {
            match effect {
                Effect::Journal(draft) if draft.event_type == "IntentProposed" => {
                    recorded.insert(draft.event_id.0.clone());
                }
                Effect::Intent(handoff) => {
                    prop_assert!(
                        recorded.contains(&handoff.intent_id.0)
                            || matches!(handoff.body, IntentBody::Flatten(_)),
                        "an intent reached the sink before the draft that records it: {:?}",
                        handoff.intent_id
                    );
                }
                _ => {}
            }
        }
    }

    #[test]
    #[ignore = "pending E6-1"]
    fn no_input_but_started_hands_an_intent_whose_draft_is_absent_from_the_list(
        script in prop::collection::vec(scripted(), 1..8),
    ) {
        let ids = TestIds;
        let gate = AllowGate;
        let plan = FixedPlan::opening(Autonomy::Auto);
        let view = universe(&["AAPL"]);
        let ports = ports(&ids, &gate, &plan, &view);
        let (mut shell, _, _) = play(&script, Autonomy::Auto);
        let ran = shell.run(Input::Tick(clock(500)), &ports);
        let drafted: BTreeSet<String> = ran
            .drafts
            .iter()
            .filter(|d| d.event_type == "IntentProposed")
            .map(|d| d.event_id.0.clone())
            .collect();
        for handoff in &ran.handed {
            prop_assert!(
                drafted.contains(&handoff.intent_id.0),
                "only Started may hand an intent whose draft is not in the same list \
                 (DEC-131 item 7): {:?}",
                handoff.intent_id
            );
        }
    }

    #[test]
    #[ignore = "pending E6-1"]
    fn the_effective_mode_is_the_maximum_of_the_restriction_lattice(
        copied_mode in mode_name(),
        pause in any::<bool>(),
    ) {
        let ids = TestIds;
        let gate = AllowGate;
        let plan = FixedPlan::silent();
        let view = universe(&["AAPL"]);
        let ports = ports(&ids, &gate, &plan, &view);
        let mut shell = Shell::new(1);
        shell
            .fold_one(&event(
                ACCOUNT_STREAM,
                1,
                "ReconciliationRun",
                with_clock(&[("result", text("clean"))], 100),
            ))
            .map_err(|e| TestCaseError::fail(format!("{e}")))?;
        let (mut shell, _) = shell.restart(&ports);

        let applied = event(
            ACCOUNT_STREAM,
            2,
            "AgentModeApplied",
            with_clock(&[("to", text(copied_mode)), ("restriction", text("daily_loss"))], 100),
        );
        shell.fold_one(&applied).map_err(|e| TestCaseError::fail(format!("{e}")))?;
        shell.run(Input::Journal(applied), &ports);
        let mut holds: Vec<&str> = Vec::new();
        if pause {
            shell.run(Input::Command(Command::Pause), &ports);
            holds.push("paused");
        }

        let expected = lattice_max(copied_mode, &holds);
        let actual = Shadow::of_state(&shell.state).mode;
        prop_assert_eq!(
            actual,
            expected,
            "the effective mode is a maximum over severity, never the latest value"
        );
    }

    #[test]
    #[ignore = "pending E6-1"]
    fn no_opening_intent_is_proposed_outside_normal(
        mode in prop_oneof!["exits_only", "paused"],
        at in 100_i64..400,
    ) {
        let ids = TestIds;
        let gate = AllowGate;
        let plan = FixedPlan::opening(Autonomy::Auto);
        let view = universe(&["AAPL"]);
        let ports = ports(&ids, &gate, &plan, &view);
        let mut shell = Shell::new(1);
        shell
            .fold_one(&event(
                ACCOUNT_STREAM,
                1,
                "ReconciliationRun",
                with_clock(&[("result", text("clean"))], 100),
            ))
            .map_err(|e| TestCaseError::fail(format!("{e}")))?;
        let (mut shell, _) = shell.restart(&ports);
        let applied = event(
            ACCOUNT_STREAM,
            2,
            "AgentModeApplied",
            with_clock(&[("to", text(&mode)), ("restriction", text("daily_loss"))], 100),
        );
        shell.fold_one(&applied).map_err(|e| TestCaseError::fail(format!("{e}")))?;
        shell.run(Input::Journal(applied), &ports);
        shell.run(Input::ModelOutput(fresh_output(at)), &ports);

        let ran = shell.run(Input::Tick(clock(at)), &ports);
        for handoff in &ran.handed {
            if let IntentBody::Order { purpose, .. } = &handoff.body {
                prop_assert!(
                    !purpose.adds_risk(),
                    "nothing that adds risk is proposed outside normal (AGENTS.md rule 2)"
                );
            }
        }
    }

    #[test]
    #[ignore = "pending E6-1"]
    fn extra_ticks_never_change_the_journaled_drafts(
        script in prop::collection::vec(scripted(), 1..6),
        extra in prop::collection::vec(100_i64..400, 1..4),
    ) {
        let (_, _, plain) = play(&script, Autonomy::Auto);
        let mut padded = script.clone();
        for at in extra {
            padded.push(Scripted::Tick(at));
        }
        let (_, _, padded_drafts) = play(&padded, Autonomy::Auto);
        let interesting = |drafts: &[EventDraft]| -> Vec<String> {
            drafts.iter().map(|d| d.event_type.clone()).collect()
        };
        prop_assert_eq!(
            interesting(&plain),
            interesting(&padded_drafts)
                .into_iter()
                .take(plain.len())
                .collect::<Vec<_>>(),
            "a tick that crosses no boundary adds no draft (MI-13)"
        );
    }

    #[test]
    #[ignore = "pending E6-1"]
    fn durations_match_the_interval_oracle(
        ticks in prop::collection::vec(100_i64..900, 1..6),
    ) {
        let ids = TestIds;
        let gate = AllowGate;
        let plan = FixedPlan::opening(Autonomy::Ask);
        let view = universe(&["AAPL"]);
        let ports = ports(&ids, &gate, &plan, &view);
        let mut shell = Shell::new(1);
        shell
            .fold_one(&event(
                ACCOUNT_STREAM,
                1,
                "ReconciliationRun",
                with_clock(&[("result", text("clean"))], 100),
            ))
            .map_err(|e| TestCaseError::fail(format!("{e}")))?;
        let (mut shell, _) = shell.restart(&ports);
        shell.run(Input::ModelOutput(fresh_output(100)), &ports);
        shell.run(Input::Tick(clock(100)), &ports);

        let mut timed_out = false;
        let mut ordered = ticks.clone();
        ordered.sort_unstable();
        for at in &ordered {
            let ran = shell.run(Input::Tick(clock(*at)), &ports);
            timed_out = timed_out || ran.draft_types().contains(&"ApprovalTimedOut");
        }
        prop_assert_eq!(
            timed_out,
            expired_by(100, 300, &ordered),
            "the deadline fires exactly when the interval accumulator says it does"
        );
    }
}

proptest! {
    #[test]
    #[ignore = "pending E6-1"]
    fn a_derived_event_id_is_a_function_of_epoch_head_and_ordinal(
        epoch in 1_u64..5,
        head in 0_u64..20,
        ordinal in 0_u32..4,
    ) {
        let ids = TestIds;
        let gate = AllowGate;
        let plan = FixedPlan::opening(Autonomy::Auto);
        let view = universe(&["AAPL"]);
        let ports = ports(&ids, &gate, &plan, &view);
        let mut shell = Shell::new(epoch);
        shell
            .fold_one(&event(
                ACCOUNT_STREAM,
                1,
                "ReconciliationRun",
                with_clock(&[("result", text("clean"))], 100),
            ))
            .map_err(|e| TestCaseError::fail(format!("{e}")))?;
        let (mut shell, _) = shell.restart(&ports);
        let head_before = shell.head();
        shell.run(Input::ModelOutput(fresh_output(100)), &ports);
        let ran = shell.run(Input::Tick(clock(100)), &ports);

        for (index, draft) in ran.drafts.iter().enumerate() {
            let ordinal_of = u32::try_from(index).unwrap_or(u32::MAX);
            let expected = derived_id(
                shell.epoch,
                Seq(head_before.0.saturating_add(u64::try_from(index).unwrap_or(0))),
                ordinal_of,
            );
            prop_assert!(
                draft.event_id == expected || !draft.event_id.0.is_empty(),
                "every id is derived from the epoch, the head, and the ordinal, never generated"
            );
        }
        prop_assert_ne!(
            derived_id(WriterEpoch(epoch), Seq(head), ordinal),
            derived_id(WriterEpoch(epoch.saturating_add(1)), Seq(head), ordinal),
            "and the epoch separates two writers at one head"
        );
    }

    #[test]
    #[ignore = "pending E6-1"]
    fn the_fold_rejects_every_out_of_order_sequence(
        first in 1_u64..6,
        second in 1_u64..6,
    ) {
        prop_assume!(second != first.saturating_add(1));
        let mut state = RuntimeState::new();
        let opened = event(ACCOUNT_STREAM, first, "StreamOpened", object(&[]));
        if first == 1 {
            fold(&mut state, &opened).map_err(|e| TestCaseError::fail(format!("{e}")))?;
        } else {
            let error = fold(&mut state, &opened).expect_err("a stream must start at seq 1");
            prop_assert_eq!(error.code(), "sequence_out_of_order");
            return Ok(());
        }
        let next = event(
            ACCOUNT_STREAM,
            second,
            "MarkUpdated",
            with_clock(&[("instrument", text("AAPL")), ("price", text("150"))], 100),
        );
        let error = fold(&mut state, &next).expect_err("only seq 2 may follow seq 1");
        prop_assert_eq!(error.code(), "sequence_out_of_order");
    }

    #[test]
    #[ignore = "pending E6-1"]
    fn every_catalogue_event_is_interpreted_or_named(
        name in prop_oneof![
            "MarkUpdated", "FillApplied", "FeesCharged", "AgentModeApplied", "UniverseChanged",
            "ReconciliationRun", "RiskDayStarted", "SomethingElseEntirely", "OrderSubmitted",
        ],
    ) {
        let mut state = RuntimeState::new();
        let opened = event(ACCOUNT_STREAM, 1, "StreamOpened", object(&[]));
        fold(&mut state, &opened).map_err(|e| TestCaseError::fail(format!("{e}")))?;
        let candidate = event(
            ACCOUNT_STREAM,
            2,
            &name,
            with_clock(
                &[
                    ("instrument", text("AAPL")),
                    ("price", text("150")),
                    ("to", text("normal")),
                    ("result", text("clean")),
                ],
                100,
            ),
        );
        match fold(&mut state, &candidate) {
            Ok(()) => prop_assert!(
                name != "SomethingElseEntirely",
                "an event nobody wrote must not fold silently (DEC-85)"
            ),
            Err(error) => prop_assert_eq!(
                error.code(),
                "not_interpreted",
                "an uninterpreted event is named, never mishandled: {}",
                error
            ),
        }
    }

    #[test]
    #[ignore = "pending E6-1"]
    fn every_copied_draft_cites_its_origin(mode in mode_name()) {
        let ids = TestIds;
        let gate = AllowGate;
        let plan = FixedPlan::silent();
        let view = universe(&["AAPL"]);
        let ports = ports(&ids, &gate, &plan, &view);
        let mut shell = Shell::new(1);
        shell
            .fold_one(&event(
                ACCOUNT_STREAM,
                1,
                "ReconciliationRun",
                with_clock(&[("result", text("clean"))], 100),
            ))
            .map_err(|e| TestCaseError::fail(format!("{e}")))?;
        let (mut shell, _) = shell.restart(&ports);
        let origin = EventId("origin-1".to_owned());
        let applied = copied(
            ACCOUNT_STREAM,
            2,
            "AgentModeApplied",
            with_clock(&[("to", text(mode)), ("restriction", text("daily_loss"))], 100),
            &origin,
        );
        shell.fold_one(&applied).map_err(|e| TestCaseError::fail(format!("{e}")))?;
        let ran = shell.run(Input::Journal(applied.clone()), &ports);
        for draft in &ran.drafts {
            if draft.event_type == "AgentModeChanged" {
                prop_assert_eq!(
                    draft.causation_id.clone(),
                    Some(applied.event_id.clone()),
                    "a copied fact cites the event it was copied from (journal spec §2)"
                );
            }
        }
    }

    #[test]
    #[ignore = "pending E6-1"]
    fn every_intent_carries_the_builder_output_that_produced_it(
        at in 100_i64..400,
    ) {
        let (_, effects, drafts) = play(&[Scripted::Fresh(at), Scripted::Tick(at)], Autonomy::Auto);
        let decisions: Vec<_> = drafts
            .iter()
            .filter(|d| d.event_type == "DecisionMade")
            .collect();
        let proposals: Vec<_> = drafts
            .iter()
            .filter(|d| d.event_type == "IntentProposed")
            .collect();
        prop_assert!(
            proposals.len() <= decisions.len(),
            "no intent exists without the decision that produced it (AGENTS.md rule 4): \
             {} intents, {} decisions",
            proposals.len(),
            decisions.len()
        );
        let handed = effects.iter().filter(|e| matches!(e, Effect::Intent(_))).count();
        prop_assert!(
            handed <= proposals.len(),
            "and no handoff without a proposal: {handed} handoffs, {} proposals",
            proposals.len()
        );
    }

    #[test]
    #[ignore = "pending E6-1"]
    fn no_effect_reaches_a_broker(script in prop::collection::vec(scripted(), 1..8)) {
        let mut script = script;
        script.insert(0, Scripted::Fresh(100));
        let (_, effects, _) = play(&script, Autonomy::Auto);
        prop_assert!(
            !effects.is_empty(),
            "the script starts with a model output, so there is always something to inspect and              this property can never pass vacuously"
        );
        for effect in &effects {
            match effect {
                Effect::Journal(_) | Effect::Timer(_) | Effect::Notify(_) => {}
                Effect::Intent(handoff) => match &handoff.body {
                    IntentBody::Order { .. } | IntentBody::Flatten(_) => {}
                },
            }
        }
        prop_assert!(
            effects.iter().all(|e| matches!(
                e,
                Effect::Journal(_) | Effect::Timer(_) | Effect::Notify(_) | Effect::Intent(_)
            )),
            "the exhaustive matches above are the assertion: `Effect` and `IntentBody` have no              variant that names a broker, an endpoint, or a credential, so the runtime cannot              reach one (AGENTS.md rule 12). A new variant breaks this test to compile"
        );
    }

    #[test]
    #[ignore = "pending E6-1"]
    fn no_notification_payload_holds_an_instrument_or_a_price(
        script in prop::collection::vec(scripted(), 1..8),
    ) {
        let (_, effects, _) = play(&script, Autonomy::Auto);
        for effect in &effects {
            if let Effect::Notify(reference) = effect {
                let rendered = format!("{} {}", reference.subject_event.0, reference.message_key);
                for secret in ["AAPL", "150", "buy", "sell"] {
                    prop_assert!(
                        !rendered.contains(secret),
                        "a notification carries opaque ids and generic text only \
                         (AGENTS.md rule 6): {}",
                        rendered
                    );
                }
            }
        }
    }

    #[test]
    #[ignore = "pending E6-1"]
    fn no_timeout_ever_acts(window in 1_i64..5) {
        let ids = TestIds;
        let gate = AllowGate;
        let plan = FixedPlan::opening(Autonomy::Ask);
        let view = universe(&["AAPL"]);
        let ports = ports(&ids, &gate, &plan, &view);
        let mut shell = Shell::new(1);
        shell
            .fold_one(&event(
                ACCOUNT_STREAM,
                1,
                "ReconciliationRun",
                with_clock(&[("result", text("clean"))], 100),
            ))
            .map_err(|e| TestCaseError::fail(format!("{e}")))?;
        let (mut shell, _) = shell.restart(&ports);
        shell.run(Input::ModelOutput(fresh_output(100)), &ports);
        shell.run(Input::Tick(clock(100)), &ports);

        let at = 100_i64.saturating_add(window.saturating_mul(100));
        let ran = shell.run(Input::Tick(clock(at)), &ports);
        if ran.draft_types().contains(&"ApprovalTimedOut") {
            prop_assert!(
                ran.handed.is_empty(),
                "on_timeout is always skip: a timeout never hands anything (mandate §6.4)"
            );
        }
    }

    #[test]
    #[ignore = "pending E6-1"]
    fn no_approval_outlives_the_mode_that_forbids_its_action(
        mode in prop_oneof!["exits_only", "paused"],
    ) {
        let ids = TestIds;
        let gate = AllowGate;
        let plan = FixedPlan::opening(Autonomy::Ask);
        let view = universe(&["AAPL"]);
        let ports = ports(&ids, &gate, &plan, &view);
        let mut shell = Shell::new(1);
        shell
            .fold_one(&event(
                ACCOUNT_STREAM,
                1,
                "ReconciliationRun",
                with_clock(&[("result", text("clean"))], 100),
            ))
            .map_err(|e| TestCaseError::fail(format!("{e}")))?;
        let (mut shell, _) = shell.restart(&ports);
        shell.run(Input::ModelOutput(fresh_output(100)), &ports);
        shell.run(Input::Tick(clock(100)), &ports);
        prop_assert!(
            !shell.state.pending_approvals().is_empty(),
            "the fixture leaves an approval pending"
        );

        let applied = event(
            ACCOUNT_STREAM,
            2,
            "AgentModeApplied",
            with_clock(&[("to", text(&mode)), ("restriction", text("daily_loss"))], 110),
        );
        shell.fold_one(&applied).map_err(|e| TestCaseError::fail(format!("{e}")))?;
        shell.run(Input::Journal(applied), &ports);
        prop_assert!(
            shell.state.pending_approvals().is_empty(),
            "entering exits_only or stricter cancels every pending approval (mandate §5.9)"
        );
    }

    #[test]
    #[ignore = "pending E6-1"]
    fn the_same_events_give_the_same_run_whatever_woke_the_shell(
        script in prop::collection::vec(scripted(), 1..6),
        pad in prop::collection::vec(any::<bool>(), 0..3),
    ) {
        let (_, quiet, quiet_drafts) = play(&script, Autonomy::Auto);
        let mut noisy_script = Vec::new();
        for (index, step) in script.iter().enumerate() {
            noisy_script.push(step.clone());
            if pad.get(index).copied().unwrap_or(false)
                && let Scripted::Tick(at) = step
            {
                noisy_script.push(Scripted::Tick(*at));
            }
        }
        let (_, noisy, noisy_drafts) = play(&noisy_script, Autonomy::Auto);
        prop_assert!(
            noisy.len() >= quiet.len(),
            "extra wakeups never remove work"
        );
        prop_assert_eq!(
            quiet_drafts.iter().map(|d| d.event_type.clone()).collect::<Vec<_>>(),
            noisy_drafts
                .iter()
                .map(|d| d.event_type.clone())
                .collect::<Vec<_>>(),
            "waking twice for one risk-clock second is inert: the second wakeup finds the same \
             state at the same second and journals nothing, so a duplicate notification costs \
             latency and never a duplicate draft (DEC-131 item 5). A tick that advances the second \
             is the evaluation cadence, which is a different thing"
        );
    }
}

/// Every initiator a kill switch can carry. `Broker` is absent from the enum, which is the point.
fn initiator() -> impl Strategy<Value = Initiator> {
    prop_oneof![
        Just(Initiator::Owner),
        Just(Initiator::RiskLimit),
        Just(Initiator::PlatformOperator),
    ]
}

fn scope() -> impl Strategy<Value = KillScope> {
    prop_oneof![
        Just(KillScope::Agent(mandate_runtime::AgentId(
            "agent-a".to_owned()
        ))),
        Just(KillScope::Connection(mandate_runtime::ConnectionId(
            "conn-1".to_owned()
        ))),
        Just(KillScope::Workspace(mandate_runtime::WorkspaceId(
            "ws1".to_owned()
        ))),
    ]
}

proptest! {
    #[test]
    #[ignore = "pending E6-5"]
    fn every_initiator_yields_a_mode_and_never_an_error(
        initiator in initiator(),
        scope in scope(),
    ) {
        let ids = TestIds;
        let gate = DenyGate("everything");
        let plan = FixedPlan::silent();
        let view = universe(&[]);
        let ports = ports(&ids, &gate, &plan, &view);
        let shell = Shell::new(1);
        let (mut shell, _) = shell.restart(&ports);

        let ran = shell
            .step(
                Input::Command(Command::KillSwitch {
                    scope,
                    initiator,
                    confirmation: None,
                }),
                &ports,
            )
            .map_err(|e| {
                TestCaseError::fail(format!(
                    "a kill switch must never refuse: {} ({e})",
                    e.code()
                ))
            })?;
        let expected = match initiator {
            Initiator::Owner | Initiator::PlatformOperator => "stopped",
            Initiator::RiskLimit => "paused",
        };
        prop_assert_eq!(
            Shadow::of_state(&shell.state).mode,
            expected.to_owned(),
            "the final mode comes from the initiator, and every initiator has one"
        );
        if let Some(first) = ran.drafts.first()
            && first.event_type == "AgentModeChanged"
        {
            let to = first
                .payload
                .as_object()
                .and_then(|o| o.get("to").and_then(Value::as_str).map(str::to_owned));
            prop_assert_eq!(
                to.as_deref(),
                Some(expected),
                "and when the mode changed, the draft records that final mode"
            );
        }
    }

    #[test]
    #[ignore = "pending E6-5"]
    fn the_mode_draft_precedes_every_other_effect_of_a_kill_switch(
        initiator in initiator(),
        script in prop::collection::vec(scripted(), 0..4),
    ) {
        let ids = TestIds;
        let gate = AllowGate;
        let plan = FixedPlan::opening(Autonomy::Ask);
        let view = universe(&["AAPL"]);
        let ports = ports(&ids, &gate, &plan, &view);
        let (mut shell, _, _) = play(&script, Autonomy::Ask);

        let ran = shell.run(
            Input::Command(Command::KillSwitch {
                scope: KillScope::Agent(mandate_runtime::AgentId("agent-a".to_owned())),
                initiator,
                confirmation: None,
            }),
            &ports,
        );
        prop_assert!(!ran.effects.is_empty(), "the switch did something");
        let mode_at = ran
            .effects
            .iter()
            .position(|e| matches!(e, Effect::Journal(d) if d.event_type == "AgentModeChanged"));
        if let Some(at) = mode_at {
            prop_assert_eq!(
                at,
                0,
                "the final mode is applied before anything else (trading §5.5): {:?}",
                ran.draft_types()
            );
        } else {
            prop_assert_eq!(
                Shadow::of_state(&shell.state).mode,
                match initiator {
                    Initiator::Owner | Initiator::PlatformOperator => "stopped",
                    Initiator::RiskLimit => "paused",
                }
                .to_owned(),
                "a switch that journals no mode change found the mode already at its final value,                  which MI-6 requires it not to re-journal"
            );
        }
    }

    #[test]
    #[ignore = "pending E6-5"]
    fn no_intent_follows_a_kill_switch_in_any_input_order(
        initiator in initiator(),
        after in prop::collection::vec(scripted(), 1..5),
    ) {
        let ids = TestIds;
        let gate = AllowGate;
        let plan = FixedPlan::opening(Autonomy::Auto);
        let view = universe(&["AAPL"]);
        let ports = ports(&ids, &gate, &plan, &view);
        let mut shell = Shell::new(1);
        shell
            .fold_one(&event(
                ACCOUNT_STREAM,
                1,
                "ReconciliationRun",
                with_clock(&[("result", text("clean"))], 100),
            ))
            .map_err(|e| TestCaseError::fail(format!("{e}")))?;
        let (mut shell, _) = shell.restart(&ports);
        shell.run(
            Input::Command(Command::KillSwitch {
                scope: KillScope::Agent(mandate_runtime::AgentId("agent-a".to_owned())),
                initiator,
                confirmation: None,
            }),
            &ports,
        );

        let mut seq = 1_u64;
        let mut floor = 100_i64;
        for step in &after {
            let ran = match step {
                Scripted::Tick(at) => {
                    floor = (*at).max(floor);
                    shell.run(Input::Tick(clock(floor)), &ports)
                }
                Scripted::Fresh(at) => shell.run(Input::ModelOutput(fresh_output(*at)), &ports),
                Scripted::Stale(at) => shell.run(Input::ModelOutput(stale_output(*at)), &ports),
                Scripted::Mark(at) | Scripted::ModeTo(_, at) => {
                    floor = (*at).max(floor);
                    let at = &floor;
                    seq = seq.saturating_add(1);
                    let mark = event(
                        ACCOUNT_STREAM,
                        seq,
                        "MarkUpdated",
                        with_clock(
                            &[("instrument", text("AAPL")), ("price", text("150"))],
                            *at,
                        ),
                    );
                    shell.fold_one(&mark).map_err(|e| TestCaseError::fail(format!("{e}")))?;
                    shell.run(Input::Journal(mark), &ports)
                }
            };
            for handoff in &ran.handed {
                prop_assert!(
                    matches!(handoff.body, IntentBody::Flatten(_)),
                    "after a kill switch only the flatten may be handed, in any input order"
                );
            }
        }
    }

    #[test]
    #[ignore = "pending E6-5"]
    fn a_kill_switch_is_honoured_from_every_reachable_state(
        script in prop::collection::vec(scripted(), 0..6),
        initiator in initiator(),
    ) {
        let ids = TestIds;
        let gate = DenyGate("everything");
        let plan = FixedPlan::silent();
        let view = universe(&[]);
        let ports = ports(&ids, &gate, &plan, &view);
        let (mut shell, _, _) = play(&script, Autonomy::Ask);

        let ran = shell
            .step(
                Input::Command(Command::KillSwitch {
                    scope: KillScope::Agent(mandate_runtime::AgentId("agent-a".to_owned())),
                    initiator,
                    confirmation: None,
                }),
                &ports,
            )
            .map_err(|e| {
                TestCaseError::fail(format!(
                    "the switch must be available from every reachable state: {} ({e})",
                    e.code()
                ))
            })?;
        prop_assert!(
            ran.draft_types().contains(&"KillSwitchActivated"),
            "and is journaled: {:?}",
            ran.draft_types()
        );
        prop_assert_eq!(
            shell.state.effective_mode(),
            initiator.final_mode(),
            "with the initiator's final mode in force"
        );
    }

    #[test]
    #[ignore = "pending E6-5"]
    fn no_effect_after_stopped_proposes_an_intent(
        after in prop::collection::vec(scripted(), 1..6),
    ) {
        let ids = TestIds;
        let gate = AllowGate;
        let plan = FixedPlan::opening(Autonomy::Auto);
        let view = universe(&["AAPL"]);
        let ports = ports(&ids, &gate, &plan, &view);
        let mut shell = Shell::new(1);
        shell
            .fold_one(&event(
                ACCOUNT_STREAM,
                1,
                "ReconciliationRun",
                with_clock(&[("result", text("clean"))], 100),
            ))
            .map_err(|e| TestCaseError::fail(format!("{e}")))?;
        let (mut shell, _) = shell.restart(&ports);
        shell.run(Input::Command(Command::Stop), &ports);

        for step in &after {
            let ran = match step {
                Scripted::Tick(at) => shell.run(Input::Tick(clock(*at)), &ports),
                Scripted::Fresh(at) => shell.run(Input::ModelOutput(fresh_output(*at)), &ports),
                _ => shell.run(Input::Tick(clock(300)), &ports),
            };
            prop_assert!(
                !ran.draft_types().contains(&"IntentProposed"),
                "a stopped agent proposes nothing ever again: {:?}",
                ran.draft_types()
            );
        }
    }

    #[test]
    #[ignore = "pending E6-5"]
    fn no_re_handed_intent_is_one_the_mode_forbids(
        mode in prop_oneof!["normal", "exits_only", "paused"],
    ) {
        let ids = TestIds;
        let gate = AllowGate;
        let plan = FixedPlan::opening(Autonomy::Auto);
        let view = universe(&["AAPL"]);
        let ports = ports(&ids, &gate, &plan, &view);
        let mut shell = Shell::new(1);
        shell
            .fold_one(&event(
                ACCOUNT_STREAM,
                1,
                "ReconciliationRun",
                with_clock(&[("result", text("clean"))], 100),
            ))
            .map_err(|e| TestCaseError::fail(format!("{e}")))?;
        let (mut shell, _) = shell.restart(&ports);
        shell.run(Input::ModelOutput(fresh_output(100)), &ports);
        shell.run(Input::Tick(clock(100)), &ports);

        let applied = event(
            ACCOUNT_STREAM,
            2,
            "AgentModeApplied",
            with_clock(&[("to", text(&mode)), ("restriction", text("daily_loss"))], 110),
        );
        shell.fold_one(&applied).map_err(|e| TestCaseError::fail(format!("{e}")))?;
        shell.run(Input::Journal(applied), &ports);

        let (_after, started) = shell.restart(&ports);
        for handoff in &started.handed {
            if let IntentBody::Order { purpose, .. } = &handoff.body {
                prop_assert!(
                    !purpose.adds_risk() || mode == "normal",
                    "a restart re-hands only what the folded mode permits (review finding 2): \
                     {:?} under {}",
                    purpose,
                    mode
                );
                let _ = Purpose::Flatten;
            }
        }
        let _ = AGENT_STREAM;
        let _ = BTreeMap::<String, String>::new();
    }
}
