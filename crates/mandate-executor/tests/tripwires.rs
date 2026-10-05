//! E6-13's account-stream tripwire fold (mandate spec §6.7 and MI-31).
//!
//! These are plain pending functions so `cargo xtask ci pending` executes each one against the
//! fail-closed stub. The property oracles derive their answers from generated histories without
//! calling production predicates.

use std::collections::BTreeSet;

use mandate_accounting::{InstrumentId, Side};
use mandate_approval::{
    AssertionId, Environment, RiskClock, STEP_UP_WINDOW_S, StepUp, StepUpMethod,
};
use mandate_executor::ExecutorError;
use mandate_executor::tripwire::{
    AcknowledgmentRefusal, TripwireAcknowledgment, TripwireEvent, TripwireFill, TripwireInput,
    TripwireOutcome, TripwireState, TripwireValue, fold,
};
use mandate_num::Usd;
use mandate_spec::SchemaDec;
use mandate_spec::document::{Tripwire, TripwireAction, TripwireId, TripwireMetric};
use mandate_time::Date;
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

use TripwireAction::{EndDelegations, ExitsOnly};
use TripwireMetric::{ConsecutiveLosingExits, NewInstruments, RealizedLossUsd};

fn id(text: &str) -> TripwireId {
    TripwireId::parse(text).expect("a tripwire id")
}

fn threshold(text: &str) -> SchemaDec {
    SchemaDec::parse(text, mandate_spec::DecGrammar::PositiveDecimal).expect("a positive decimal")
}

fn count(value: u32) -> TripwireValue {
    TripwireValue::Count(value)
}

fn count_text(value: &str) -> TripwireValue {
    count(value.parse().expect("an unsigned count"))
}

fn usd(text: &str) -> TripwireValue {
    TripwireValue::Usd(Usd::parse(text).expect("signed dollars"))
}

fn tripwire(name: &str, metric: TripwireMetric, at: &str, action: TripwireAction) -> Tripwire {
    Tripwire {
        id: id(name),
        metric,
        threshold: threshold(at),
        action,
    }
}

fn fill(instrument: &str, side: Side, net: &str) -> TripwireFill {
    TripwireFill {
        instrument: InstrumentId::new(instrument).expect("an instrument"),
        side,
        net_realized_usd: Usd::parse(net).expect("signed dollars"),
    }
}

fn applied(state: &TripwireState, input: TripwireInput) -> TripwireOutcome {
    fold(state, &input).expect("E6-13 applies the tripwire input")
}

fn armed(tripwires: Vec<Tripwire>) -> TripwireState {
    applied(
        &TripwireState::default(),
        TripwireInput::MandateVersionApplied { tripwires },
    )
    .state
}

fn step_up(assertion: &str, authenticated_at: i64) -> StepUp {
    StepUp {
        assertion: AssertionId(assertion.to_owned()),
        authenticated_at: RiskClock(authenticated_at),
        method: StepUpMethod::CliConfirm,
    }
}

fn acknowledgment(name: &str, assertion: &str) -> TripwireAcknowledgment {
    TripwireAcknowledgment {
        tripwire: id(name),
        step_up: Some(step_up(assertion, 1_000)),
        committed_at: RiskClock(1_001),
        processed_at: RiskClock(1_001),
        environment: Environment::Paper,
        requester: Some("user:u1".to_owned()),
        acknowledging_user: Some("user:u2".to_owned()),
        independent_required_at_request: false,
        independent_required_now: false,
    }
}

fn acknowledge(name: &str, assertion: &str) -> TripwireInput {
    TripwireInput::OwnerAcknowledged(acknowledgment(name, assertion))
}

#[test]
fn the_tripwire_fold_stub_fails_closed() {
    assert_eq!(
        fold(&TripwireState::default(), &TripwireInput::Clock),
        Err(ExecutorError::Unimplemented { story: "E6-13" })
    );
}

#[test]
fn acknowledgment_refusal_codes_are_exact_and_closed() {
    assert_eq!(
        [
            AcknowledgmentRefusal::StepUpMissing.as_str(),
            AcknowledgmentRefusal::StepUpStale.as_str(),
            AcknowledgmentRefusal::StepUpReused.as_str(),
            AcknowledgmentRefusal::StepUpMethod.as_str(),
            AcknowledgmentRefusal::NotIndependent.as_str(),
        ],
        [
            "step_up_missing",
            "step_up_stale",
            "step_up_reused",
            "step_up_method",
            "not_independent",
        ]
    );
}

/// `consecutive_losing_exits` counts each losing sell fill, a non-losing sell resets it, and a buy
/// neither increments nor resets it. Net realized includes the fill's fee.
#[test]
#[ignore = "pending E6-13"]
fn losing_exits_fire_at_the_threshold_and_only_there() {
    let mut state = armed(vec![tripwire(
        "streak",
        ConsecutiveLosingExits,
        "2",
        ExitsOnly,
    )]);
    for (event, value, fired) in [
        (
            TripwireInput::FillApplied {
                fill: fill("AAPL", Side::Buy, "0"),
            },
            "0",
            false,
        ),
        (
            TripwireInput::FillApplied {
                fill: fill("AAPL", Side::Sell, "-1"),
            },
            "1",
            false,
        ),
        (
            TripwireInput::FillApplied {
                fill: fill("AAPL", Side::Buy, "-0.01"),
            },
            "1",
            false,
        ),
        (
            TripwireInput::LateFillApplied {
                fill: fill("AAPL", Side::Sell, "-0.01"),
            },
            "2",
            true,
        ),
    ] {
        let outcome = applied(&state, event);
        assert_eq!(outcome.snapshot.metrics[&id("streak")], count_text(value));
        assert_eq!(outcome.snapshot.fired.contains_key(&id("streak")), fired);
        state = outcome.state;
    }
}

/// A winning or break-even exit ends a losing streak, including a half-even cost-basis result of
/// exactly zero; a risk-day boundary does not.
#[test]
#[ignore = "pending E6-13"]
fn only_a_non_losing_sell_resets_a_losing_streak() {
    let mut state = armed(vec![tripwire(
        "streak",
        ConsecutiveLosingExits,
        "2",
        ExitsOnly,
    )]);
    state = applied(
        &state,
        TripwireInput::FillApplied {
            fill: fill("AAPL", Side::Sell, "-1"),
        },
    )
    .state;
    state = applied(
        &state,
        TripwireInput::RiskDayStarted {
            day: Date::parse("2026-09-23").expect("a date"),
        },
    )
    .state;
    let reset = applied(
        &state,
        TripwireInput::FillApplied {
            fill: fill("AAPL", Side::Sell, "0"),
        },
    );
    assert_eq!(reset.snapshot.metrics[&id("streak")], count(0));
    assert!(reset.snapshot.fired.is_empty());
}

/// `realized_loss_usd` is max(0, minus the sum of net realized in the current risk day): buy fees
/// count, same-day gains offset losses, and a new risk day starts it at zero.
#[test]
#[ignore = "pending E6-13"]
fn realized_loss_is_the_current_risk_days_net_loss() {
    let mut state = armed(vec![tripwire(
        "day_loss",
        RealizedLossUsd,
        "100",
        EndDelegations,
    )]);
    for (side, net) in [
        (Side::Buy, "-0.01"),
        (Side::Sell, "-60"),
        (Side::Sell, "20"),
        (Side::Sell, "-59.98"),
    ] {
        state = applied(
            &state,
            TripwireInput::FillApplied {
                fill: fill("AAPL", side, net),
            },
        )
        .state;
    }
    let before = applied(&state, TripwireInput::Clock);
    assert_eq!(before.snapshot.metrics[&id("day_loss")], usd("99.99"));
    assert!(before.snapshot.fired.is_empty());
    let reset = applied(
        &before.state,
        TripwireInput::RiskDayStarted {
            day: Date::parse("2026-09-23").expect("a date"),
        },
    );
    assert_eq!(reset.snapshot.metrics[&id("day_loss")], usd("0"));
}

/// `new_instruments` counts the agent's first fill ever in an instrument, not the first fill in the
/// window or the first fill after a position was closed.
#[test]
#[ignore = "pending E6-13"]
fn new_instruments_counts_first_ever_fills_only() {
    let mut state = armed(vec![tripwire(
        "new_names",
        NewInstruments,
        "2",
        EndDelegations,
    )]);
    for instrument in ["AAPL", "AAPL", "MSFT"] {
        state = applied(
            &state,
            TripwireInput::FillApplied {
                fill: fill(instrument, Side::Buy, "0"),
            },
        )
        .state;
    }
    let snapshot = applied(&state, TripwireInput::Clock).snapshot;
    assert_eq!(snapshot.metrics[&id("new_names")], count(2));
    assert_eq!(snapshot.fired.get(&id("new_names")), Some(&EndDelegations));
}

/// Marks, clocks, restarts, and a risk-day boundary cannot fire or lift a streak tripwire. Replay
/// of the same stream reconstructs the same state and emits no second firing for an existing latch.
#[test]
#[ignore = "pending E6-13"]
fn unrelated_inputs_never_fire_or_lift_and_restart_replays_the_latch() {
    let mut state = armed(vec![tripwire(
        "streak",
        ConsecutiveLosingExits,
        "1",
        ExitsOnly,
    )]);
    state = applied(
        &state,
        TripwireInput::FillApplied {
            fill: fill("AAPL", Side::Sell, "-1"),
        },
    )
    .state;
    for event in [
        TripwireInput::Mark,
        TripwireInput::Clock,
        TripwireInput::RiskDayStarted {
            day: Date::parse("2026-09-23").expect("a date"),
        },
        TripwireInput::Restart,
    ] {
        let outcome = applied(&state, event);
        assert_eq!(outcome.snapshot.fired.get(&id("streak")), Some(&ExitsOnly));
        assert!(
            !outcome
                .journal
                .iter()
                .any(|event| matches!(event, TripwireEvent::RiskLimitTriggered { .. }))
        );
        state = outcome.state;
    }
}

/// Arming excludes the arming input and all earlier fills. Threshold and action changes preserve
/// the count; a metric change under the same id arms afresh.
#[test]
#[ignore = "pending E6-13"]
fn version_inputs_apply_the_arming_and_window_rules() {
    let prehistory = applied(
        &TripwireState::default(),
        TripwireInput::FillApplied {
            fill: fill("AAPL", Side::Sell, "-1"),
        },
    )
    .state;
    let armed = applied(
        &prehistory,
        TripwireInput::MandateVersionApplied {
            tripwires: vec![tripwire(
                "wire",
                ConsecutiveLosingExits,
                "2",
                EndDelegations,
            )],
        },
    );
    assert_eq!(armed.snapshot.metrics[&id("wire")], count(0));
    let one = applied(
        &armed.state,
        TripwireInput::FillApplied {
            fill: fill("AAPL", Side::Sell, "-1"),
        },
    );
    let tightened = applied(
        &one.state,
        TripwireInput::MandateVersionApplied {
            tripwires: vec![tripwire("wire", ConsecutiveLosingExits, "1", ExitsOnly)],
        },
    );
    assert_eq!(tightened.snapshot.fired.get(&id("wire")), Some(&ExitsOnly));
    let changed = applied(
        &tightened.state,
        TripwireInput::MandateVersionApplied {
            tripwires: vec![tripwire("wire", NewInstruments, "1", ExitsOnly)],
        },
    );
    assert_eq!(changed.snapshot.metrics[&id("wire")], count(0));
}

/// Firings are emitted in id order. Each trigger carries the metric, threshold, and reached value,
/// and is immediately followed by an alert carrying only an opaque reference and generic text.
#[test]
#[ignore = "pending E6-13"]
fn simultaneous_firings_are_ordered_and_alert_without_sensitive_content() {
    let state = armed(vec![
        tripwire("a_loss", RealizedLossUsd, "10", EndDelegations),
        tripwire("b_streak", ConsecutiveLosingExits, "1", ExitsOnly),
    ]);
    let outcome = applied(
        &state,
        TripwireInput::FillApplied {
            fill: fill("AAPL", Side::Sell, "-20"),
        },
    );
    assert_eq!(outcome.journal.len(), 4);
    assert!(matches!(
        &outcome.journal[0],
        TripwireEvent::RiskLimitTriggered { id: wire, .. } if wire == &id("a_loss")
    ));
    let TripwireEvent::OwnerAlertSent(alert) = &outcome.journal[1] else {
        panic!("a trigger must be followed immediately by its owner alert");
    };
    assert_eq!(alert.triggered_event_index(), 0);
    assert_eq!(alert.generic_text(), "tripwire_fired");
    assert!(matches!(
        &outcome.journal[2],
        TripwireEvent::RiskLimitTriggered { id: wire, .. } if wire == &id("b_streak")
    ));
    let TripwireEvent::OwnerAlertSent(alert) = &outcome.journal[3] else {
        panic!("the second trigger must be followed immediately by its owner alert");
    };
    assert_eq!(alert.triggered_event_index(), 2);
    assert_eq!(alert.generic_text(), "tripwire_fired");
}

/// Every fired action suspends delegations and blocks allocation increases. `end_delegations`
/// leaves the mode normal; any fired `exits_only` holds that stricter action and never `paused`.
#[test]
#[ignore = "pending E6-13"]
fn a_fired_tripwire_only_tightens_the_effective_envelope() {
    for (action, effective) in [
        (EndDelegations, Some(EndDelegations)),
        (ExitsOnly, Some(ExitsOnly)),
    ] {
        let state = armed(vec![tripwire("wire", ConsecutiveLosingExits, "1", action)]);
        let snapshot = applied(
            &state,
            TripwireInput::FillApplied {
                fill: fill("AAPL", Side::Sell, "-1"),
            },
        )
        .snapshot;
        assert!(snapshot.delegations_suspended);
        assert!(snapshot.allocation_increase_blocked);
        assert_eq!(snapshot.effective_action, effective);
    }
}

/// Removing, raising, or softening a fired tripwire never lifts it. It holds the stricter of its
/// fired action and the current version's action until a valid owner acknowledgment.
#[test]
#[ignore = "pending E6-13"]
fn versions_never_lift_or_soften_a_fired_tripwire() {
    let initial = armed(vec![tripwire(
        "wire",
        ConsecutiveLosingExits,
        "1",
        ExitsOnly,
    )]);
    let fired = applied(
        &initial,
        TripwireInput::FillApplied {
            fill: fill("AAPL", Side::Sell, "-1"),
        },
    );
    for tripwires in [
        Vec::new(),
        vec![tripwire(
            "wire",
            ConsecutiveLosingExits,
            "100",
            EndDelegations,
        )],
    ] {
        let changed = applied(
            &fired.state,
            TripwireInput::MandateVersionApplied { tripwires },
        );
        assert_eq!(changed.snapshot.fired.get(&id("wire")), Some(&ExitsOnly));
        assert_eq!(changed.snapshot.effective_action, Some(ExitsOnly));
    }
}

/// A fired tripwire keeps counting its metric but emits no second trigger, including when a later
/// fill takes the metric farther past the threshold.
#[test]
#[ignore = "pending E6-13"]
fn a_fired_tripwire_keeps_counting_without_firing_again() {
    let initial = armed(vec![tripwire(
        "streak",
        ConsecutiveLosingExits,
        "1",
        ExitsOnly,
    )]);
    let first = applied(
        &initial,
        TripwireInput::FillApplied {
            fill: fill("AAPL", Side::Sell, "-1"),
        },
    );
    let later = applied(
        &first.state,
        TripwireInput::LateFillApplied {
            fill: fill("AAPL", Side::Sell, "-2"),
        },
    );
    assert_eq!(later.snapshot.metrics[&id("streak")], count(2));
    assert_eq!(later.snapshot.fired.get(&id("streak")), Some(&ExitsOnly));
    assert!(
        !later
            .journal
            .iter()
            .any(|event| matches!(event, TripwireEvent::RiskLimitTriggered { .. }))
    );
}

/// Missing, stale, wrong-method, and non-independent acknowledgments have distinct exact refusal
/// codes. Under the stricter independence setting at request or processing, both user names are
/// required and must differ; every refusal stays latched.
#[test]
#[ignore = "pending E6-13"]
fn acknowledgment_fails_closed_with_the_exact_refusal_reason() {
    let initial = armed(vec![tripwire(
        "wire",
        ConsecutiveLosingExits,
        "1",
        ExitsOnly,
    )]);
    let fired = applied(
        &initial,
        TripwireInput::FillApplied {
            fill: fill("AAPL", Side::Sell, "-1"),
        },
    )
    .state;
    let invalid = [
        (
            TripwireAcknowledgment {
                step_up: None,
                ..acknowledgment("wire", "missing")
            },
            AcknowledgmentRefusal::StepUpMissing,
        ),
        (
            TripwireAcknowledgment {
                step_up: Some(step_up("stale", 1_000)),
                processed_at: RiskClock(1_000 + STEP_UP_WINDOW_S + 1),
                ..acknowledgment("wire", "stale")
            },
            AcknowledgmentRefusal::StepUpStale,
        ),
        (
            TripwireAcknowledgment {
                environment: Environment::Live,
                ..acknowledgment("wire", "wrong-method")
            },
            AcknowledgmentRefusal::StepUpMethod,
        ),
        (
            TripwireAcknowledgment {
                requester: Some("user:u1".to_owned()),
                acknowledging_user: Some("user:u1".to_owned()),
                independent_required_now: true,
                ..acknowledgment("wire", "same-user")
            },
            AcknowledgmentRefusal::NotIndependent,
        ),
        (
            TripwireAcknowledgment {
                requester: None,
                independent_required_at_request: true,
                ..acknowledgment("wire", "missing-user")
            },
            AcknowledgmentRefusal::NotIndependent,
        ),
    ];
    for (ack, reason) in invalid {
        let outcome = applied(&fired, TripwireInput::OwnerAcknowledged(ack));
        assert_eq!(outcome.snapshot.fired.get(&id("wire")), Some(&ExitsOnly));
        assert_eq!(
            outcome.journal,
            vec![TripwireEvent::OwnerCommandRefused { reason }]
        );
    }
}

/// Every supplied assertion id is spent even when its command is refused. A stale, wrong-method,
/// or non-independent assertion therefore reports `step_up_reused` when replayed; this includes
/// MC-W49's wrong-method then replay sequence.
#[test]
#[ignore = "pending E6-13"]
fn every_refused_step_up_assertion_is_spent_before_it_can_be_replayed() {
    let initial = armed(vec![tripwire(
        "wire",
        ConsecutiveLosingExits,
        "1",
        ExitsOnly,
    )]);
    let fired = applied(
        &initial,
        TripwireInput::FillApplied {
            fill: fill("AAPL", Side::Sell, "-1"),
        },
    )
    .state;
    let refused_inputs = [
        (
            TripwireAcknowledgment {
                step_up: Some(step_up("spent-stale", 1_000)),
                processed_at: RiskClock(1_000 + STEP_UP_WINDOW_S + 1),
                ..acknowledgment("wire", "spent-stale")
            },
            "spent-stale",
            AcknowledgmentRefusal::StepUpStale,
        ),
        (
            TripwireAcknowledgment {
                environment: Environment::Live,
                ..acknowledgment("wire", "spent-method")
            },
            "spent-method",
            AcknowledgmentRefusal::StepUpMethod,
        ),
        (
            TripwireAcknowledgment {
                requester: Some("user:u1".to_owned()),
                acknowledging_user: Some("user:u1".to_owned()),
                independent_required_now: true,
                ..acknowledgment("wire", "spent-independence")
            },
            "spent-independence",
            AcknowledgmentRefusal::NotIndependent,
        ),
    ];
    for (refused_input, assertion, first_reason) in refused_inputs {
        let refused = applied(&fired, TripwireInput::OwnerAcknowledged(refused_input));
        assert_eq!(
            refused.journal,
            vec![TripwireEvent::OwnerCommandRefused {
                reason: first_reason,
            }]
        );
        let replayed = applied(&refused.state, acknowledge("wire", assertion));
        assert_eq!(
            replayed.journal,
            vec![TripwireEvent::OwnerCommandRefused {
                reason: AcknowledgmentRefusal::StepUpReused,
            }]
        );
        assert_eq!(replayed.snapshot.fired.get(&id("wire")), Some(&ExitsOnly));
    }
}

/// A valid acknowledgment lifts, journals the lift, and re-arms a still-present tripwire at zero.
/// Acknowledging one that is not fired changes nothing.
#[test]
#[ignore = "pending E6-13"]
fn a_valid_owner_acknowledgment_is_the_only_lift_and_rearms_at_zero() {
    let initial = armed(vec![tripwire(
        "wire",
        ConsecutiveLosingExits,
        "1",
        ExitsOnly,
    )]);
    let no_op = applied(&initial, acknowledge("wire", "not-fired"));
    assert!(no_op.journal.is_empty());
    let fired = applied(
        &initial,
        TripwireInput::FillApplied {
            fill: fill("AAPL", Side::Sell, "-1"),
        },
    );
    let lifted = applied(&fired.state, acknowledge("wire", "lift-present"));
    assert!(lifted.snapshot.fired.is_empty());
    assert_eq!(lifted.snapshot.metrics[&id("wire")], count(0));
    assert_eq!(
        lifted.journal,
        vec![TripwireEvent::RiskLimitLifted { id: id("wire") }]
    );
    let removed = applied(
        &fired.state,
        TripwireInput::MandateVersionApplied {
            tripwires: Vec::new(),
        },
    );
    let lifted_removed = applied(&removed.state, acknowledge("wire", "lift-removed"));
    assert!(lifted_removed.snapshot.fired.is_empty());
    assert!(!lifted_removed.snapshot.metrics.contains_key(&id("wire")));
}

fn first_fire_oracle(losses: &[bool], threshold: usize) -> Option<usize> {
    let mut streak = 0usize;
    for (index, loss) in losses.iter().enumerate() {
        streak = if *loss { streak.saturating_add(1) } else { 0 };
        if streak >= threshold {
            return Some(index);
        }
    }
    None
}

/// All three metrics over random fill logs match independent accumulators: streak from sell signs,
/// day loss from a signed sum, and first-ever instruments from a separate set.
#[test]
#[ignore = "pending E6-13"]
fn property_every_metric_matches_an_independent_fill_log_oracle() {
    let strategy = prop::collection::vec((0u8..6, any::<bool>(), -10i16..=10), 0..80);
    let mut runner = TestRunner::default();
    runner
        .run(&strategy, |draws| {
            let mut state = armed(vec![
                tripwire("a_streak", ConsecutiveLosingExits, "1000", EndDelegations),
                tripwire("b_loss", RealizedLossUsd, "1000", EndDelegations),
                tripwire("c_new", NewInstruments, "1000", EndDelegations),
            ]);
            let mut streak = 0usize;
            let mut net = 0i64;
            let mut seen = BTreeSet::new();
            for (instrument, buy, realized) in draws {
                let instrument = format!("ASSET-{instrument}");
                seen.insert(instrument.clone());
                let side = if buy { Side::Buy } else { Side::Sell };
                let realized = if buy {
                    -i64::from(realized.unsigned_abs())
                } else {
                    i64::from(realized)
                };
                net = net.saturating_add(realized);
                if side == Side::Sell {
                    streak = if realized < 0 {
                        streak.saturating_add(1)
                    } else {
                        0
                    };
                }
                let outcome = applied(
                    &state,
                    TripwireInput::FillApplied {
                        fill: fill(&instrument, side, &realized.to_string()),
                    },
                );
                let day_loss = net.saturating_neg().max(0);
                prop_assert_eq!(
                    outcome.snapshot.metrics[&id("a_streak")],
                    count(u32::try_from(streak).expect("generated streak fits u32"))
                );
                prop_assert_eq!(
                    outcome.snapshot.metrics[&id("b_loss")],
                    usd(&day_loss.to_string())
                );
                prop_assert_eq!(
                    outcome.snapshot.metrics[&id("c_new")],
                    count(u32::try_from(seen.len()).expect("generated set fits u32"))
                );
                state = outcome.state;
            }
            Ok(())
        })
        .expect("the fold matches the three independent metric accumulators");
}

/// MI-31's exact firing edge over random histories, against a separate streak accumulator.
#[test]
#[ignore = "pending E6-13"]
fn property_a_streak_fires_at_the_first_reaching_input_and_no_other() {
    let strategy = (prop::collection::vec(any::<bool>(), 0..80), 1usize..=20);
    let mut runner = TestRunner::default();
    runner
        .run(&strategy, |(losses, at)| {
            let expected = first_fire_oracle(&losses, at);
            let mut state = match fold(
                &TripwireState::default(),
                &TripwireInput::MandateVersionApplied {
                    tripwires: vec![tripwire(
                        "wire",
                        ConsecutiveLosingExits,
                        &at.to_string(),
                        ExitsOnly,
                    )],
                },
            ) {
                Ok(outcome) => outcome.state,
                Err(error) => return Err(TestCaseError::fail(error.to_string())),
            };
            let mut got = None;
            for (index, loss) in losses.iter().enumerate() {
                let outcome = match fold(
                    &state,
                    &TripwireInput::FillApplied {
                        fill: fill("AAPL", Side::Sell, if *loss { "-1" } else { "0" }),
                    },
                ) {
                    Ok(outcome) => outcome,
                    Err(error) => return Err(TestCaseError::fail(error.to_string())),
                };
                if got.is_none()
                    && outcome
                        .journal
                        .iter()
                        .any(|event| matches!(event, TripwireEvent::RiskLimitTriggered { .. }))
                {
                    got = Some(index);
                }
                state = outcome.state;
            }
            prop_assert_eq!(got, expected);
            Ok(())
        })
        .expect("the fold matches the independent first-fire oracle");
}

/// MI-3 and MI-31 over random later inputs: after firing, no non-acknowledgment input clears the
/// latch, whatever version, risk day, mark, clock, or restart follows.
#[test]
#[ignore = "pending E6-13"]
fn property_only_an_owner_acknowledgment_lifts_a_fired_tripwire() {
    let strategy = prop::collection::vec(0u8..6, 0..80);
    let mut runner = TestRunner::default();
    runner
        .run(&strategy, |steps| {
            let initial = armed(vec![tripwire(
                "wire",
                ConsecutiveLosingExits,
                "1",
                ExitsOnly,
            )]);
            let mut state = applied(
                &initial,
                TripwireInput::FillApplied {
                    fill: fill("AAPL", Side::Sell, "-1"),
                },
            )
            .state;
            for step in steps {
                let input = match step {
                    0 => TripwireInput::Mark,
                    1 => TripwireInput::Clock,
                    2 => TripwireInput::Restart,
                    3 => TripwireInput::RiskDayStarted {
                        day: Date::parse("2026-09-23").expect("a date"),
                    },
                    4 => TripwireInput::MandateVersionApplied {
                        tripwires: Vec::new(),
                    },
                    _ => TripwireInput::FillApplied {
                        fill: fill("AAPL", Side::Sell, "10"),
                    },
                };
                let outcome = applied(&state, input);
                prop_assert_eq!(outcome.snapshot.fired.get(&id("wire")), Some(&ExitsOnly));
                state = outcome.state;
            }
            Ok(())
        })
        .expect("only an acknowledgment lifts the latch");
}

/// MI-31's version monotonicity: lowering a threshold or strengthening an action on the same
/// history can only fire no later and can only hold an equal or stricter action.
#[test]
#[ignore = "pending E6-13"]
fn property_a_reducing_tripwire_change_never_fires_later_or_holds_less() {
    let strategy = (
        prop::collection::vec(any::<bool>(), 0..80),
        1usize..=20,
        1usize..=20,
    );
    let mut runner = TestRunner::default();
    runner
        .run(&strategy, |(losses, a, b)| {
            let loose = a.max(b);
            let tight = a.min(b);
            let loose_fire = first_fire_oracle(&losses, loose);
            let tight_fire = first_fire_oracle(&losses, tight);
            prop_assert!(
                tight_fire.is_some() || loose_fire.is_none(),
                "a lower threshold cannot miss a history the higher threshold reaches"
            );
            if let (Some(tight_at), Some(loose_at)) = (tight_fire, loose_fire) {
                prop_assert!(tight_at <= loose_at);
            }
            let old = vec![tripwire(
                "wire",
                ConsecutiveLosingExits,
                &loose.to_string(),
                EndDelegations,
            )];
            let new = vec![tripwire(
                "wire",
                ConsecutiveLosingExits,
                &tight.to_string(),
                ExitsOnly,
            )];
            let mut state = fold(
                &TripwireState::default(),
                &TripwireInput::MandateVersionApplied { tripwires: old },
            )
            .map_err(|error| TestCaseError::fail(error.to_string()))?
            .state;
            for loss in &losses {
                state = fold(
                    &state,
                    &TripwireInput::FillApplied {
                        fill: fill("AAPL", Side::Sell, if *loss { "-1" } else { "0" }),
                    },
                )
                .map_err(|error| TestCaseError::fail(error.to_string()))?
                .state;
            }
            let changed = fold(
                &state,
                &TripwireInput::MandateVersionApplied { tripwires: new },
            )
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
            let ending_streak = losses.iter().rev().take_while(|loss| **loss).count();
            let expected_action =
                (loose_fire.is_some() || ending_streak >= tight).then_some(ExitsOnly);
            prop_assert_eq!(changed.snapshot.effective_action, expected_action);
            Ok(())
        })
        .expect("tightening is monotone against the independent history oracle");
}
