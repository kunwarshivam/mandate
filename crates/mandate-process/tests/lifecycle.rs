//! The lifecycle's transitions, permissions, liveness, drain bound, and crash-loop bound, case by
//! case (E21-28, DEC-994 items 1 to 4; infrastructure design §3.4, §7.3).

use std::time::Duration;

use mandate_journal::AppendOutcome;
use mandate_process::{
    Action, CRASH_LOOP_RESTARTS, CRASH_LOOP_WINDOW, DRAIN_BOUND, Exit, Input, Kind, Lifecycle,
    State, crash_loop,
};
use mandate_time::UtcNanos;

const LIVENESS: Duration = Duration::from_secs(10);

fn at(secs: i64, nanos: u32) -> UtcNanos {
    UtcNanos::from_parts(secs, nanos).unwrap_or_else(|e| panic!("time: {e:?}"))
}

fn run(kind: Kind, inputs: &[Input]) -> Lifecycle {
    let mut lifecycle = Lifecycle::new(kind, LIVENESS, at(0, 0));
    for input in inputs {
        lifecycle.observe(at(1, 0), *input);
    }
    lifecycle
}

const UP_TO_READY: [Input; 4] = [
    Input::StartupPassed,
    Input::EpochTaken,
    Input::Replayed {
        position: 7,
        head: 7,
    },
    Input::Reconciled { passed: true },
];

#[test]
fn an_executor_is_ready_only_with_its_epoch_its_replay_at_the_head_and_its_reconciliation() {
    let mut lifecycle = Lifecycle::new(Kind::Executor, LIVENESS, at(0, 0));
    assert_eq!(lifecycle.state(), State::Starting);
    let expected = [
        State::Replaying,
        State::Replaying,
        State::NotReady,
        State::Ready,
    ];
    for (input, state) in UP_TO_READY.into_iter().zip(expected) {
        assert_eq!(lifecycle.observe(at(1, 0), input), state, "{input:?}");
    }
    let behind = Input::Replayed {
        position: 7,
        head: 9,
    };
    assert_eq!(lifecycle.observe(at(1, 0), behind), State::NotReady);
    let caught_up = Input::Replayed {
        position: 9,
        head: 9,
    };
    assert_eq!(lifecycle.observe(at(1, 0), caught_up), State::Ready);
    let failed = Input::Reconciled { passed: false };
    assert_eq!(lifecycle.observe(at(1, 0), failed), State::NotReady);
}

#[test]
fn each_kind_asks_only_its_own_gates() {
    let replayed_unreconciled = [
        Input::StartupPassed,
        Input::EpochTaken,
        Input::Replayed {
            position: 1,
            head: 1,
        },
    ];
    assert_eq!(
        run(Kind::Executor, &replayed_unreconciled).state(),
        State::NotReady
    );
    assert_eq!(
        run(Kind::Runtime, &replayed_unreconciled).state(),
        State::NotReady
    );
    assert_eq!(
        run(Kind::Writer, &replayed_unreconciled).state(),
        State::Ready
    );
    assert_eq!(
        run(Kind::Writer, &[Input::StartupPassed]).state(),
        State::Replaying
    );
    assert_eq!(run(Kind::Service, &[]).state(), State::Starting);
    assert_eq!(
        run(Kind::Service, &[Input::StartupPassed]).state(),
        State::Ready
    );
}

#[test]
fn replay_and_reconciliation_before_the_epoch_do_not_count() {
    let warm = [
        Input::StartupPassed,
        Input::Replayed {
            position: 3,
            head: 3,
        },
        Input::Reconciled { passed: true },
    ];
    let mut standby = run(Kind::Executor, &warm);
    assert_eq!(standby.state(), State::Replaying);
    assert!(!standby.permits(Action::RiskExit));
    assert_eq!(
        standby.observe(at(1, 0), Input::EpochTaken),
        State::Replaying
    );
    assert!(!standby.permits(Action::RiskExit));
    let replayed = Input::Replayed {
        position: 4,
        head: 4,
    };
    assert_eq!(standby.observe(at(1, 0), replayed), State::NotReady);
    assert_eq!(
        standby.observe(at(1, 0), Input::Reconciled { passed: true }),
        State::Ready
    );
    assert_eq!(standby.observe(at(1, 0), Input::EpochTaken), State::Ready);
}

#[test]
fn a_fenced_append_exits_from_every_state_and_nothing_is_permitted_after() {
    let prefixes: [&[Input]; 5] = [
        &[],
        &[Input::StartupPassed],
        &UP_TO_READY,
        &[
            Input::StartupPassed,
            Input::EpochTaken,
            Input::Replayed {
                position: 1,
                head: 1,
            },
        ],
        &[Input::DrainRequested],
    ];
    for prefix in prefixes {
        let mut lifecycle = run(Kind::Executor, prefix);
        let fenced = Input::from_append(&AppendOutcome::Fenced { current_epoch: 9 })
            .unwrap_or_else(|| panic!("a fenced outcome is an input"));
        assert_eq!(
            lifecycle.observe(at(1, 0), fenced),
            State::Exited(Exit::Fenced)
        );
        for action in [Action::Open, Action::RiskExit, Action::KillSwitch] {
            assert!(!lifecycle.permits(action), "{prefix:?} {action:?}");
        }
        assert!(!lifecycle.live(at(1, 0)));
        for input in UP_TO_READY {
            assert_eq!(
                lifecycle.observe(at(1, 0), input),
                State::Exited(Exit::Fenced)
            );
        }
    }
    assert_eq!(Input::from_append(&AppendOutcome::Unavailable), None);
    assert_eq!(Input::from_append(&AppendOutcome::Ambiguous), None);
}

/// DEC-2347 item 1: the executor's `Completed` exits from every state not yet exited, starting,
/// replaying, ready, not ready and draining, and nothing is permitted after; a process already
/// exited keeps its exit.
#[test]
#[ignore = "pending E7-4"]
fn completed_exits_from_every_state_not_yet_exited() {
    let prefixes: [&[Input]; 5] = [
        &[],
        &[Input::StartupPassed],
        &UP_TO_READY,
        &[
            Input::StartupPassed,
            Input::EpochTaken,
            Input::Replayed {
                position: 1,
                head: 1,
            },
        ],
        &[Input::DrainRequested],
    ];
    for prefix in prefixes {
        let mut lifecycle = run(Kind::Executor, prefix);
        assert_eq!(
            lifecycle.observe(at(1, 0), Input::Completed),
            State::Exited(Exit::Completed),
            "{prefix:?}"
        );
        for action in [Action::Open, Action::RiskExit, Action::KillSwitch] {
            assert!(!lifecycle.permits(action), "{prefix:?} {action:?}");
        }
        assert!(!lifecycle.live(at(1, 0)));
        for input in UP_TO_READY {
            assert_eq!(
                lifecycle.observe(at(1, 0), input),
                State::Exited(Exit::Completed)
            );
        }
    }
    for (exited, exit) in [
        (Input::Fenced, Exit::Fenced),
        (Input::Failed, Exit::Refused),
    ] {
        let mut lifecycle = run(Kind::Executor, &[exited]);
        assert_eq!(
            lifecycle.observe(at(1, 0), Input::Completed),
            State::Exited(exit)
        );
    }
}

#[test]
fn a_failure_exits_by_how_far_the_process_got() {
    assert_eq!(
        run(Kind::Executor, &[Input::Failed]).state(),
        State::Exited(Exit::Refused)
    );
    assert_eq!(
        run(Kind::Executor, &[Input::StartupPassed, Input::Failed]).state(),
        State::Exited(Exit::Refused)
    );
    let mut ready = UP_TO_READY.to_vec();
    ready.push(Input::Failed);
    assert_eq!(
        run(Kind::Executor, &ready).state(),
        State::Exited(Exit::CycleFailed)
    );
    let mut not_ready = UP_TO_READY.to_vec();
    not_ready.extend([Input::Reconciled { passed: false }, Input::Failed]);
    assert_eq!(
        run(Kind::Executor, &not_ready).state(),
        State::Exited(Exit::CycleFailed)
    );
    assert_eq!(
        run(Kind::Executor, &[Input::DrainRequested, Input::Failed]).state(),
        State::Exited(Exit::DrainIncomplete)
    );
}

#[test]
fn the_kill_switch_is_never_gated_on_readiness_and_risk_exits_wait_only_for_the_replay() {
    let cases: [(&[Input], State, bool); 5] = [
        (&[], State::Starting, false),
        (
            &[Input::StartupPassed, Input::EpochTaken],
            State::Replaying,
            false,
        ),
        (
            &[
                Input::StartupPassed,
                Input::EpochTaken,
                Input::Replayed {
                    position: 1,
                    head: 1,
                },
            ],
            State::NotReady,
            true,
        ),
        (&UP_TO_READY, State::Ready, true),
        (
            &[
                Input::StartupPassed,
                Input::EpochTaken,
                Input::Replayed {
                    position: 1,
                    head: 1,
                },
                Input::DrainRequested,
            ],
            State::Draining,
            true,
        ),
    ];
    for (inputs, state, risk_exit) in cases {
        let lifecycle = run(Kind::Executor, inputs);
        assert_eq!(lifecycle.state(), state);
        assert!(lifecycle.permits(Action::KillSwitch), "{state:?}");
        assert_eq!(lifecycle.permits(Action::RiskExit), risk_exit, "{state:?}");
        assert_eq!(
            lifecycle.permits(Action::Open),
            state == State::Ready,
            "{state:?}"
        );
    }
}

#[test]
fn a_drain_settles_to_drained_and_a_settle_outside_a_drain_changes_nothing() {
    let mut lifecycle = run(Kind::Executor, &UP_TO_READY);
    assert_eq!(lifecycle.observe(at(1, 0), Input::Settled), State::Ready);
    assert_eq!(
        lifecycle.observe(at(1, 0), Input::DrainRequested),
        State::Draining
    );
    assert!(!lifecycle.permits(Action::Open));
    let replayed = Input::Replayed {
        position: 9,
        head: 9,
    };
    assert_eq!(lifecycle.observe(at(1, 0), replayed), State::Draining);
    assert_eq!(
        lifecycle.observe(at(1, 0), Input::Settled),
        State::Exited(Exit::Drained)
    );
}

#[test]
fn a_drain_past_its_bound_is_abandoned_and_a_repeated_request_never_extends_it() {
    let bound = i64::try_from(DRAIN_BOUND.as_secs()).unwrap_or_else(|e| panic!("{e}"));
    let mut lifecycle = Lifecycle::new(Kind::Executor, LIVENESS, at(0, 0));
    for input in UP_TO_READY {
        lifecycle.observe(at(100, 0), input);
    }
    assert_eq!(
        lifecycle.observe(at(100, 0), Input::DrainRequested),
        State::Draining
    );
    assert_eq!(
        lifecycle.observe(at(150, 0), Input::DrainRequested),
        State::Draining
    );
    assert_eq!(
        lifecycle.observe(at(100 + bound, 0), Input::Heartbeat),
        State::Draining
    );
    assert_eq!(
        lifecycle.observe(at(100 + bound, 1), Input::Settled),
        State::Exited(Exit::DrainIncomplete)
    );
}

#[test]
fn an_input_past_the_drain_bound_finds_the_drain_already_abandoned() {
    let bound = i64::try_from(DRAIN_BOUND.as_secs()).unwrap_or_else(|e| panic!("{e}"));
    for input in [Input::Fenced, Input::Settled, Input::Failed] {
        let mut lifecycle = Lifecycle::new(Kind::Writer, LIVENESS, at(0, 0));
        lifecycle.observe(at(0, 0), Input::DrainRequested);
        assert!(lifecycle.permits(Action::KillSwitch));
        assert_eq!(
            lifecycle.observe(at(bound, 1), input),
            State::Exited(Exit::DrainIncomplete),
            "{input:?}"
        );
        assert!(!lifecycle.permits(Action::KillSwitch));
    }
}

#[test]
fn an_earlier_time_never_moves_the_fold_back() {
    let mut lifecycle = Lifecycle::new(Kind::Service, LIVENESS, at(0, 0));
    lifecycle.observe(at(500, 0), Input::DrainRequested);
    lifecycle.observe(at(1, 0), Input::Heartbeat);
    let bound = i64::try_from(DRAIN_BOUND.as_secs()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        lifecycle.observe(at(500 + bound, 1), Input::Heartbeat),
        State::Exited(Exit::DrainIncomplete)
    );
    let mut stale = Lifecycle::new(Kind::Service, LIVENESS, at(0, 0));
    stale.observe(at(100, 0), Input::Heartbeat);
    stale.observe(at(50, 0), Input::Heartbeat);
    assert!(stale.live(at(105, 0)));
    assert!(!stale.live(at(110, 1)));
}

#[test]
fn liveness_holds_within_its_bound_after_the_last_input() {
    let mut lifecycle = Lifecycle::new(Kind::Writer, LIVENESS, at(0, 0));
    assert!(lifecycle.live(at(0, 0)));
    assert!(lifecycle.live(at(10, 0)));
    assert!(!lifecycle.live(at(10, 1)));
    lifecycle.observe(at(8, 0), Input::Heartbeat);
    assert!(lifecycle.live(at(18, 0)));
    assert!(!lifecycle.live(at(18, 1)));
    assert!(lifecycle.live(at(5, 0)));
}

#[test]
fn exits_keep_their_codes_and_statuses() {
    let table = [
        (Exit::Drained, "drained", 0),
        (Exit::Refused, "refused", 2),
        (Exit::Fenced, "fenced", 3),
        (Exit::CycleFailed, "cycle_failed", 4),
        (Exit::DrainIncomplete, "drain_incomplete", 5),
        (Exit::Completed, "completed", 0),
    ];
    for (exit, code, status) in table {
        assert_eq!(exit.code(), code);
        assert_eq!(exit.exit_status(), status);
    }
}

#[test]
fn the_crash_loop_bound_counts_restarts_within_the_window_both_ends_included() {
    let window = i64::try_from(CRASH_LOOP_WINDOW.as_secs()).unwrap_or_else(|e| panic!("{e}"));
    let now = at(10_000, 0);
    let restarts =
        |offsets: &[i64]| -> Vec<UtcNanos> { offsets.iter().map(|o| at(10_000 - o, 0)).collect() };
    let four = restarts(&[0, 1, 2, 3]);
    assert_eq!(CRASH_LOOP_RESTARTS, 5);
    assert!(!crash_loop(four.clone(), now));
    let mut five = four.clone();
    five.push(at(10_000 - window, 0));
    assert!(crash_loop(five, now));
    let mut outside = four.clone();
    outside.push(at(10_000 - window - 1, 999_999_999));
    assert!(!crash_loop(outside, now));
    let mut ahead = four;
    ahead.push(at(10_500, 0));
    assert!(crash_loop(ahead, now));
    assert!(!crash_loop(Vec::new(), now));
}
