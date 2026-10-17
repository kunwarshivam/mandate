//! The lifecycle against an independent oracle (E21-28, DEC-994 item 6). The fold keeps facts
//! as inputs arrive; the oracle recomputes every answer from the whole input history at once, by
//! searching it (the first drain request, the first terminal input, the facts after the epoch).
//! A property test over random sequences compares the two after every input: the state, every
//! action's permission (the kill switch in every state the process is up), and liveness at its
//! bound. A deterministic sweep shows every transition of the lifecycle is reached and none goes
//! backwards, and each seeded bug is caught.

use std::collections::BTreeSet;
use std::time::Duration;

use mandate_process::{Action, DRAIN_BOUND, Exit, Input, Kind, Lifecycle, State};
use mandate_time::UtcNanos;
use proptest::prelude::*;

const LIVENESS: Duration = Duration::from_secs(10);
const START_NS: i128 = 1_000_000_000_000;
const SEC: i128 = 1_000_000_000;
const KINDS: [Kind; 4] = [Kind::Executor, Kind::Runtime, Kind::Writer, Kind::Service];
const ACTIONS: [Action; 3] = [Action::Open, Action::RiskExit, Action::KillSwitch];

fn time(ns: i128) -> UtcNanos {
    let secs = i64::try_from(ns.div_euclid(SEC)).unwrap_or_else(|e| panic!("{e}"));
    let nanos = u32::try_from(ns.rem_euclid(SEC)).unwrap_or_else(|e| panic!("{e}"));
    UtcNanos::from_parts(secs, nanos).unwrap_or_else(|e| panic!("{e:?}"))
}

fn bound() -> i128 {
    i128::try_from(DRAIN_BOUND.as_nanos()).unwrap_or_else(|e| panic!("{e}"))
}

fn liveness() -> i128 {
    i128::try_from(LIVENESS.as_nanos()).unwrap_or_else(|e| panic!("{e}"))
}

/// What the oracle expects after a prefix of inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Expected {
    state: State,
    permits: [bool; 3],
    clock: i128,
}

fn writes_a_stream(kind: Kind) -> bool {
    match kind {
        Kind::Executor | Kind::Runtime | Kind::Writer => true,
        Kind::Service => false,
    }
}

fn checks_its_state(kind: Kind) -> bool {
    match kind {
        Kind::Executor | Kind::Runtime => true,
        Kind::Writer | Kind::Service => false,
    }
}

fn at_head(input: &Input) -> Option<bool> {
    match input {
        Input::Replayed { position, head } => Some(position >= head),
        _ => None,
    }
}

/// The inputs readiness may count: everything after the first epoch, or everything for a process
/// that takes none. `None` while a writer has not taken its epoch.
fn under_epoch(kind: Kind, history: &[(i128, Input)]) -> Option<&[(i128, Input)]> {
    if !writes_a_stream(kind) {
        return Some(history);
    }
    let epoch = history
        .iter()
        .position(|(_, input)| *input == Input::EpochTaken)?;
    history.get(epoch + 1..)
}

/// The state of a history that holds no terminal input.
fn running(kind: Kind, history: &[(i128, Input)]) -> State {
    if history
        .iter()
        .any(|(_, input)| *input == Input::DrainRequested)
    {
        return State::Draining;
    }
    if !history
        .iter()
        .any(|(_, input)| *input == Input::StartupPassed)
    {
        return State::Starting;
    }
    let Some(counted) = under_epoch(kind, history) else {
        return State::Replaying;
    };
    if writes_a_stream(kind)
        && !counted
            .iter()
            .any(|(_, input)| at_head(input) == Some(true))
    {
        return State::Replaying;
    }
    let head = !writes_a_stream(kind)
        || counted
            .iter()
            .rev()
            .find_map(|(_, input)| at_head(input))
            .unwrap_or(false);
    let checked = !checks_its_state(kind)
        || counted
            .iter()
            .rev()
            .find_map(|(_, input)| match input {
                Input::Reconciled { passed } => Some(*passed),
                _ => None,
            })
            .unwrap_or(false);
    if head && checked {
        State::Ready
    } else {
        State::NotReady
    }
}

fn oracle(kind: Kind, history: &[(i128, Input)]) -> Expected {
    let clocks: Vec<i128> = history
        .iter()
        .scan(START_NS, |latest, (at, _)| {
            *latest = (*latest).max(*at);
            Some(*latest)
        })
        .collect();
    let clock = clocks.last().copied().unwrap_or(START_NS);
    let index_of = |wanted: Input| history.iter().position(|(_, input)| *input == wanted);
    let drain = index_of(Input::DrainRequested);
    let abandoned =
        drain.and_then(|d| (d + 1..history.len()).find(|j| clocks[*j] - clocks[d] > bound()));
    let settled =
        drain.and_then(|d| (d + 1..history.len()).find(|j| history[*j].1 == Input::Settled));
    let terminal = [
        abandoned,
        index_of(Input::Fenced),
        index_of(Input::Failed),
        settled,
    ]
    .into_iter()
    .flatten()
    .min();
    let Some(end) = terminal else {
        let state = running(kind, history);
        let started = history
            .iter()
            .any(|(_, input)| *input == Input::StartupPassed);
        let replayed = !writes_a_stream(kind)
            || under_epoch(kind, history)
                .is_some_and(|counted| counted.iter().any(|(_, i)| at_head(i) == Some(true)));
        return Expected {
            state,
            permits: [state == State::Ready, started && replayed, true],
            clock,
        };
    };
    let exit = if abandoned == Some(end) {
        Exit::DrainIncomplete
    } else {
        match history[end].1 {
            Input::Fenced => Exit::Fenced,
            Input::Settled => Exit::Drained,
            _ if drain.is_some_and(|d| d < end) => Exit::DrainIncomplete,
            _ => match running(kind, &history[..end]) {
                State::Starting | State::Replaying => Exit::Refused,
                _ => Exit::CycleFailed,
            },
        }
    };
    Expected {
        state: State::Exited(exit),
        permits: [false; 3],
        clock,
    }
}

/// What a lifecycle under test answers; the real one, or one with a seeded bug.
trait Subject {
    fn observe(&mut self, at: UtcNanos, input: Input) -> State;
    fn state(&self) -> State;
    fn permits(&self, action: Action) -> bool;
    fn live(&self, now: UtcNanos) -> bool;
}

impl Subject for Lifecycle {
    fn observe(&mut self, at: UtcNanos, input: Input) -> State {
        Lifecycle::observe(self, at, input)
    }
    fn state(&self) -> State {
        Lifecycle::state(self)
    }
    fn permits(&self, action: Action) -> bool {
        Lifecycle::permits(self, action)
    }
    fn live(&self, now: UtcNanos) -> bool {
        Lifecycle::live(self, now)
    }
}

/// The first disagreement between `subject` and the oracle over `history`, if any.
fn disagreement(
    kind: Kind,
    mut subject: Box<dyn Subject>,
    history: &[(i128, Input)],
) -> Option<String> {
    for end in 1..=history.len() {
        let (at, input) = history[end - 1];
        let state = subject.observe(time(at), input);
        let expected = oracle(kind, &history[..end]);
        let up = !matches!(expected.state, State::Exited(_));
        let permits = ACTIONS.map(|action| subject.permits(action));
        let live = [
            subject.live(time(expected.clock + liveness())),
            subject.live(time(expected.clock + liveness() + 1)),
        ];
        if state != expected.state
            || subject.state() != expected.state
            || permits != expected.permits
            || live != [up, false]
        {
            return Some(format!(
                "{kind:?} after {:?}: got {state:?} {permits:?} {live:?}, expected {expected:?}",
                &history[..end]
            ));
        }
    }
    None
}

fn fresh(kind: Kind) -> Lifecycle {
    Lifecycle::new(kind, LIVENESS, time(START_NS))
}

fn real(kind: Kind) -> Box<dyn Subject> {
    Box::new(fresh(kind))
}

const INPUTS: [Input; 12] = [
    Input::Heartbeat,
    Input::StartupPassed,
    Input::EpochTaken,
    Input::Replayed {
        position: 2,
        head: 2,
    },
    Input::Replayed {
        position: 1,
        head: 2,
    },
    Input::Replayed {
        position: 3,
        head: 2,
    },
    Input::Reconciled { passed: true },
    Input::Reconciled { passed: false },
    Input::Fenced,
    Input::DrainRequested,
    Input::Settled,
    Input::Failed,
];

fn input() -> impl Strategy<Value = Input> {
    prop_oneof![
        3 => Just(Input::Heartbeat),
        3 => Just(Input::StartupPassed),
        3 => Just(Input::EpochTaken),
        6 => (0u64..4, 0u64..4).prop_map(|(position, head)| Input::Replayed { position, head }),
        4 => any::<bool>().prop_map(|passed| Input::Reconciled { passed }),
        1 => Just(Input::Fenced),
        2 => Just(Input::DrainRequested),
        2 => Just(Input::Settled),
        1 => Just(Input::Failed),
    ]
}

fn delta() -> impl Strategy<Value = i128> {
    prop_oneof![
        6 => 0i128..2 * SEC,
        1 => -3 * SEC..0,
        1 => Just(bound()),
        1 => Just(bound() + 1),
        1 => 80 * SEC..100 * SEC,
    ]
}

fn history(steps: Vec<(i128, Input)>) -> Vec<(i128, Input)> {
    steps
        .into_iter()
        .scan(START_NS, |at, (delta, input)| {
            *at += delta;
            Some((*at, input))
        })
        .collect()
}

proptest! {
    #[test]
    fn the_fold_matches_the_oracle_after_every_input(
        kind in prop::sample::select(KINDS.to_vec()),
        steps in prop::collection::vec((delta(), input()), 0..40),
    ) {
        let history = history(steps);
        let found = disagreement(kind, real(kind), &history);
        prop_assert!(found.is_none(), "{}", found.unwrap_or_default());
    }
}

/// A fixed pseudo-random sweep, so coverage and the seeded bugs are checked the same way at
/// every run.
fn sweep() -> Vec<(Kind, Vec<(i128, Input)>)> {
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move |below: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        usize::try_from(seed % u64::try_from(below).unwrap_or(1)).unwrap_or(0)
    };
    let deltas = [0, SEC, SEC, 2 * SEC, -SEC, bound(), bound() + 1];
    let mut cases = Vec::new();
    for round in 0..6_000 {
        let kind = KINDS[round % KINDS.len()];
        let len = next(14);
        let mut at = START_NS;
        let mut steps = Vec::new();
        for _ in 0..len {
            at += deltas[next(deltas.len())];
            steps.push((at, INPUTS[next(INPUTS.len())]));
        }
        cases.push((kind, steps));
    }
    cases
}

fn label(state: State) -> String {
    match state {
        State::Starting => "starting".into(),
        State::Replaying => "replaying".into(),
        State::Ready => "ready".into(),
        State::NotReady => "not_ready".into(),
        State::Draining => "draining".into(),
        State::Exited(exit) => format!("exited:{}", exit.code()),
    }
}

fn rank(state: State) -> u8 {
    match state {
        State::Starting => 0,
        State::Replaying => 1,
        State::Ready | State::NotReady => 2,
        State::Draining => 3,
        State::Exited(_) => 4,
    }
}

#[test]
fn every_transition_is_reached_and_none_goes_backwards() {
    let mut seen = BTreeSet::new();
    for (kind, history) in sweep() {
        let mut lifecycle = Lifecycle::new(kind, LIVENESS, time(START_NS));
        let mut before = lifecycle.state();
        for (at, input) in history {
            let after = lifecycle.observe(time(at), input);
            if after != before {
                assert!(
                    rank(after) > rank(before) || rank(after) == 2,
                    "{kind:?}: {before:?} -> {after:?}"
                );
                assert!(
                    !matches!(before, State::Exited(_)),
                    "{before:?} -> {after:?}"
                );
                seen.insert(format!("{} -> {}", label(before), label(after)));
            }
            before = after;
        }
    }
    let required = [
        "starting -> replaying",
        "starting -> ready",
        "starting -> draining",
        "starting -> exited:refused",
        "starting -> exited:fenced",
        "replaying -> ready",
        "replaying -> not_ready",
        "replaying -> draining",
        "replaying -> exited:refused",
        "replaying -> exited:fenced",
        "ready -> not_ready",
        "not_ready -> ready",
        "ready -> draining",
        "not_ready -> draining",
        "ready -> exited:cycle_failed",
        "ready -> exited:fenced",
        "not_ready -> exited:cycle_failed",
        "not_ready -> exited:fenced",
        "draining -> exited:drained",
        "draining -> exited:drain_incomplete",
        "draining -> exited:fenced",
    ];
    for transition in required {
        assert!(seen.contains(transition), "never reached: {transition}");
    }
}

#[test]
fn the_fold_matches_the_oracle_over_the_sweep() {
    for (kind, history) in sweep() {
        let found = disagreement(kind, real(kind), &history);
        assert!(found.is_none(), "{}", found.unwrap_or_default());
    }
}

/// A lifecycle that gates the kill switch on readiness.
struct KillSwitchOnlyWhenReady(Lifecycle);
/// A lifecycle that lets risk exits wait for readiness.
struct RiskExitOnlyWhenReady(Lifecycle);
/// A lifecycle that opens while not ready.
struct OpenWhileNotReady(Lifecycle);
/// A lifecycle that still applies the kill switch after it exited.
struct ActsAfterExit(Lifecycle);
/// A lifecycle that ignores a fenced append while draining.
struct FencedIgnoredWhileDraining(Lifecycle);
/// A lifecycle that waits for a drain however long it takes.
struct DrainNeverAbandoned(Lifecycle, Option<UtcNanos>);
/// A lifecycle that counts a reconciliation from before its epoch.
struct ReconciledBeforeTheEpoch(Lifecycle, bool, Option<bool>);

macro_rules! delegate {
    () => {
        fn state(&self) -> State {
            self.0.state()
        }
        fn live(&self, now: UtcNanos) -> bool {
            self.0.live(now)
        }
    };
}

impl Subject for KillSwitchOnlyWhenReady {
    fn observe(&mut self, at: UtcNanos, input: Input) -> State {
        self.0.observe(at, input)
    }
    fn permits(&self, action: Action) -> bool {
        match action {
            Action::KillSwitch => self.0.state() == State::Ready,
            _ => self.0.permits(action),
        }
    }
    delegate!();
}

impl Subject for RiskExitOnlyWhenReady {
    fn observe(&mut self, at: UtcNanos, input: Input) -> State {
        self.0.observe(at, input)
    }
    fn permits(&self, action: Action) -> bool {
        match action {
            Action::RiskExit => self.0.state() == State::Ready,
            _ => self.0.permits(action),
        }
    }
    delegate!();
}

impl Subject for OpenWhileNotReady {
    fn observe(&mut self, at: UtcNanos, input: Input) -> State {
        self.0.observe(at, input)
    }
    fn permits(&self, action: Action) -> bool {
        match action {
            Action::Open => matches!(self.0.state(), State::Ready | State::NotReady),
            _ => self.0.permits(action),
        }
    }
    delegate!();
}

impl Subject for ActsAfterExit {
    fn observe(&mut self, at: UtcNanos, input: Input) -> State {
        self.0.observe(at, input)
    }
    fn permits(&self, action: Action) -> bool {
        action == Action::KillSwitch || self.0.permits(action)
    }
    delegate!();
}

impl Subject for FencedIgnoredWhileDraining {
    fn observe(&mut self, at: UtcNanos, input: Input) -> State {
        if input == Input::Fenced && self.0.state() == State::Draining {
            return self.0.observe(at, Input::Heartbeat);
        }
        self.0.observe(at, input)
    }
    fn permits(&self, action: Action) -> bool {
        self.0.permits(action)
    }
    delegate!();
}

impl Subject for DrainNeverAbandoned {
    fn observe(&mut self, at: UtcNanos, input: Input) -> State {
        let at = self.1.unwrap_or(at);
        if input == Input::DrainRequested && self.1.is_none() {
            self.1 = Some(at);
        }
        self.0.observe(at, input)
    }
    fn permits(&self, action: Action) -> bool {
        self.0.permits(action)
    }
    delegate!();
}

impl Subject for ReconciledBeforeTheEpoch {
    fn observe(&mut self, at: UtcNanos, input: Input) -> State {
        let state = self.0.observe(at, input);
        match input {
            Input::Reconciled { passed } if !self.1 => self.2 = Some(passed),
            Input::EpochTaken if !self.1 => {
                self.1 = true;
                if let Some(passed) = self.2 {
                    return self.0.observe(at, Input::Reconciled { passed });
                }
            }
            _ => {}
        }
        state
    }
    fn permits(&self, action: Action) -> bool {
        self.0.permits(action)
    }
    delegate!();
}

/// Builds a subject of one kind, real or with a seeded bug.
type Maker = dyn Fn(Kind) -> Box<dyn Subject>;

#[test]
fn the_oracle_catches_every_seeded_bug() {
    let bugs: [(&str, &Maker); 7] = [
        ("kill switch gated on readiness", &|k| {
            Box::new(KillSwitchOnlyWhenReady(fresh(k)))
        }),
        ("risk exit gated on readiness", &|k| {
            Box::new(RiskExitOnlyWhenReady(fresh(k)))
        }),
        ("open while not ready", &|k| {
            Box::new(OpenWhileNotReady(fresh(k)))
        }),
        ("kill switch after exit", &|k| {
            Box::new(ActsAfterExit(fresh(k)))
        }),
        ("fenced ignored while draining", &|k| {
            Box::new(FencedIgnoredWhileDraining(fresh(k)))
        }),
        ("drain never abandoned", &|k| {
            Box::new(DrainNeverAbandoned(fresh(k), None))
        }),
        ("reconciliation before the epoch", &|k| {
            Box::new(ReconciledBeforeTheEpoch(fresh(k), false, None))
        }),
    ];
    let sweep = sweep();
    for (name, make) in bugs {
        let caught = sweep
            .iter()
            .any(|(kind, history)| disagreement(*kind, make(*kind), history).is_some());
        assert!(caught, "the oracle missed the seeded bug: {name}");
    }
}
