//! The approval path's invariants over random scripts (M7 tests PR 3 of 4; the brief's "Oracles":
//! the transition table, the causation walker, the field comparer, and the clock accumulator;
//! EI-1 to EI-7, EI-10, EI-12, EI-15; DEC-173 item 10).
//!
//! A script asks, answers (timely or late, with a right or wrong hash, fresh, stale, reused or no
//! step-up, from listed or unlisted principals), re-tails answers, moves marks, tightens and
//! relaxes the copied mode, applies versions, and restarts, and finally ticks past every deadline.
//! Every oracle reads the committed agent journal, each payload re-parsed from its canonical bytes,
//! and keeps its own clock and its own record of what it generated; none reads the runtime's state
//! or calls `mandate-approval`.
//!
//! The two lifecycle properties are live. The quorum property reads `ApprovalResponded.quorum` off
//! the journaled record (DEC-488); before the runtime wrote it, it failed on the member's absence
//! rather than at a stub, which no stub can report from a path the live properties exercise
//! (DEC-489).

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::escalation::{
    ASKED_AT, Answer, evidence, mark, mode_applied, next_account_seq, next_control_seq,
    reconciliation, version_applied,
};
use common::{
    AUTHOR, AllowGate, FixedPlan, OWNER, Shell, TestIds, clock, fresh_output, ports, universe,
};
use mandate_canon::Value;
use mandate_runtime::{
    ActorKind, Autonomy, Command, EventId, FoldedEvent, Initiator, Input, KillScope, Ports,
    RuntimeState, fold,
};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

/// The fixture mandate's `timeout_s`, which the oracle adds to the request's second itself.
const TIMEOUT_S: i64 = 300;
const STEP_UP_WINDOW_S: i64 = 300;

#[derive(Debug, Clone)]
enum Step {
    /// Advance the clock, with a fresh model output first so the plan can ask.
    Tick(i64),
    Answer(Answering),
    /// Hand the last control-stream answer to the runtime again without folding it again.
    Retail,
    Mark(&'static str),
    Mode(&'static str),
    /// Fold a copied exits-only without handing it in, then answer: the answer's own step applies
    /// the tightening and cancels in the same batch as it judges the answer (DEC-131 item 25(j),
    /// PB-21).
    AnswerAfterTightening(Answering),
    Version,
    /// A risk-limit kill switch, or the owner's Stop: each cancels every pending approval.
    KillSwitch,
    Stop,
    Restart,
}

#[derive(Debug, Clone)]
struct Answering {
    approve: bool,
    /// Seconds after the oracle's clock the answer says it was submitted.
    submitted_after: i64,
    /// `None` for no evidence; otherwise its age at submission.
    step_up_age: Option<i64>,
    reuse_assertion: bool,
    right_hash: bool,
    listed: bool,
    actor: ActorKind,
}

fn answering() -> impl Strategy<Value = Answering> {
    (
        prop::bool::weighted(0.8),
        prop_oneof![Just(0i64), 0i64..40],
        prop_oneof![
            4 => Just(Some(0i64)),
            1 => Just(Some(STEP_UP_WINDOW_S)),
            1 => Just(Some(STEP_UP_WINDOW_S + 1)),
            1 => Just(None),
        ],
        prop::bool::weighted(0.1),
        prop::bool::weighted(0.85),
        prop::bool::weighted(0.85),
        prop_oneof![
            6 => Just(ActorKind::User),
            1 => prop::sample::select(vec![
                ActorKind::System,
                ActorKind::Agent,
                ActorKind::Broker,
                ActorKind::PlatformOperator,
            ]),
        ],
    )
        .prop_map(
            |(
                approve,
                submitted_after,
                step_up_age,
                reuse_assertion,
                right_hash,
                listed,
                actor,
            )| {
                Answering {
                    approve,
                    submitted_after,
                    step_up_age,
                    reuse_assertion,
                    right_hash,
                    listed,
                    actor,
                }
            },
        )
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        4 => (1i64..200).prop_map(Step::Tick),
        4 => answering().prop_map(Step::Answer),
        1 => Just(Step::Retail),
        1 => prop::sample::select(vec!["155", "156.55", "157", "150"]).prop_map(Step::Mark),
        1 => prop::sample::select(vec!["exits_only", "normal"]).prop_map(Step::Mode),
        1 => answering().prop_map(Step::AnswerAfterTightening),
        1 => Just(Step::Version),
        1 => Just(Step::KillSwitch),
        1 => Just(Step::Stop),
        1 => Just(Step::Restart),
    ]
}

/// A script always opens by asking once and answering it, so no run passes without an answer to
/// judge.
fn script() -> impl Strategy<Value = Vec<Step>> {
    (answering(), prop::collection::vec(step(), 0..14)).prop_map(|(first, rest)| {
        let mut all = vec![Step::Tick(0), Step::Answer(first)];
        all.extend(rest);
        all
    })
}

/// What the oracle generated for one control-stream answer, kept by its event id.
#[derive(Debug, Clone)]
struct Generated {
    approval: EventId,
    approve: bool,
    /// Whether every admission check the oracle can judge from its own record passes: the target
    /// is a request it saw, the hash is that request's, the principal is a listed user, the
    /// effective time is before the deadline, and a grant's evidence is present, fresh and new.
    admissible: bool,
}

/// The oracle's own record of the run.
struct Record {
    clock: i64,
    requests: BTreeMap<EventId, (i64, String)>,
    answers: BTreeMap<EventId, Generated>,
    used: BTreeSet<String>,
    /// The requests no answer can be admitted to any more, by the oracle's own reading: those a
    /// tightening to exits-only or a version applied after them has cancelled (mandate spec §5.9 and
    /// §2.2, EI-7), and those an answer it judged admissible has already ended (quorum 1).
    closed: BTreeSet<EventId>,
    /// Approvals a kill switch or a Stop left open past its own step.
    late_cancels: Vec<String>,
    last_answer: Option<FoldedEvent>,
    assertions: u64,
}

struct Run<'a> {
    shell: Shell,
    ports: &'a Ports<'a>,
    record: Record,
}

impl Run<'_> {
    fn fold_and_step(&mut self, event: &FoldedEvent) -> Result<(), TestCaseError> {
        self.shell
            .fold_one(event)
            .map_err(|e| TestCaseError::fail(format!("{}: {e}", event.event_type)))?;
        self.step(Input::Journal(event.clone()))
    }

    fn step(&mut self, input: Input) -> Result<(), TestCaseError> {
        let ran = self
            .shell
            .step(input, self.ports)
            .map_err(|e| TestCaseError::fail(format!("step: {e}")))?;
        for draft in &ran.drafts {
            if draft.event_type == "ApprovalRequested" {
                let hash = draft
                    .payload
                    .get("content_hash")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                self.record.requests.insert(
                    draft.event_id.clone(),
                    (self.record.clock.saturating_add(TIMEOUT_S), hash),
                );
            }
        }
        Ok(())
    }

    fn play(&mut self, step: &Step) -> Result<(), TestCaseError> {
        match step {
            Step::Tick(dt) => {
                self.record.clock = self.record.clock.saturating_add(*dt);
                let now = self.record.clock;
                self.step(Input::ModelOutput(fresh_output(now)))?;
                self.step(Input::Tick(clock(now)))
            }
            Step::Answer(a) => self.answer(a),
            Step::Retail => match self.record.last_answer.clone() {
                Some(event) => self.step(Input::Journal(event)),
                None => Ok(()),
            },
            Step::Mark(price) => {
                let event = mark(next_account_seq(&self.shell), price, self.record.clock);
                self.fold_and_step(&event)
            }
            Step::Mode(mode) => {
                if *mode == "exits_only" {
                    self.close_every_request();
                }
                let event = mode_applied(next_account_seq(&self.shell), mode, self.record.clock);
                self.fold_and_step(&event)
            }
            Step::AnswerAfterTightening(a) => {
                self.close_every_request();
                let event = mode_applied(
                    next_account_seq(&self.shell),
                    "exits_only",
                    self.record.clock,
                );
                self.shell
                    .fold_one(&event)
                    .map_err(|e| TestCaseError::fail(format!("{}: {e}", event.event_type)))?;
                self.answer(a)
            }
            Step::KillSwitch => {
                self.close_every_request();
                self.cancelling(Input::Command(Command::KillSwitch {
                    scope: KillScope::Agent(common::deployment().agent),
                    initiator: Initiator::RiskLimit,
                    confirmation: None,
                }))
            }
            Step::Stop => {
                self.close_every_request();
                self.cancelling(Input::Command(Command::Stop))
            }
            Step::Version => {
                self.close_every_request();
                let event = version_applied(next_account_seq(&self.shell), "v1", self.record.clock);
                self.fold_and_step(&event)
            }
            Step::Restart => {
                let mut next = Shell::new(self.shell.epoch.0.saturating_add(1));
                next.followed = self.shell.followed.clone();
                next.agent_journal = self.shell.agent_journal.clone();
                for event in next
                    .agent_journal
                    .clone()
                    .iter()
                    .chain(next.followed.clone().iter())
                {
                    fold(&mut next.state, event)
                        .map_err(|e| TestCaseError::fail(format!("replay: {e}")))?;
                }
                self.shell = next;
                let epoch = self.shell.epoch;
                self.step(Input::Started(epoch))
            }
        }
    }

    /// A kill switch or a Stop, after which every approval the journal still holds open must have
    /// its `ApprovalCanceled` in that same step's batch (DEC-131 items 11 and 23): an approval that
    /// outlives the switch by even one step is an order the switch did not stop.
    fn cancelling(&mut self, input: Input) -> Result<(), TestCaseError> {
        let before = self.shell.agent_journal.len();
        let open = open_approvals(&self.shell.agent_journal);
        self.step(input)?;
        let canceled: BTreeSet<String> = self.shell.agent_journal[before..]
            .iter()
            .filter(|e| e.event_type == "ApprovalCanceled")
            .filter_map(|e| e.payload.get("approval").and_then(Value::as_str))
            .map(str::to_owned)
            .collect();
        for approval in open {
            if !canceled.contains(&approval) {
                self.record.late_cancels.push(approval);
            }
        }
        Ok(())
    }

    fn close_every_request(&mut self) {
        let known: Vec<EventId> = self.record.requests.keys().cloned().collect();
        self.record.closed.extend(known);
    }

    fn answer(&mut self, a: &Answering) -> Result<(), TestCaseError> {
        let target = self
            .record
            .requests
            .iter()
            .next_back()
            .map(|(id, v)| (id.clone(), v.clone()));
        let (approval, (deadline, hash)) = target.unwrap_or_else(|| {
            (
                EventId(format!("0{}", "Z".repeat(25))),
                (i64::MIN, String::new()),
            )
        });
        let submitted_at = self.record.clock.saturating_add(a.submitted_after);
        let effective = submitted_at.max(self.record.clock);
        self.record.assertions = self.record.assertions.saturating_add(1);
        let assertion = if a.reuse_assertion {
            self.record
                .used
                .iter()
                .next()
                .cloned()
                .unwrap_or_else(|| format!("assertion-{}", self.record.assertions))
        } else {
            format!("assertion-{}", self.record.assertions)
        };
        let step_up = if a.approve {
            a.step_up_age
                .and_then(|age| evidence(&assertion, submitted_at.saturating_sub(age)))
        } else {
            None
        };
        let fresh_step_up = !a.approve
            || step_up.as_ref().is_some_and(|e| {
                let age = effective.saturating_sub(e.authenticated_at);
                (0..=STEP_UP_WINDOW_S).contains(&age) && !self.record.used.contains(&e.assertion)
            });
        let answer = Answer {
            seq: next_control_seq(&self.shell),
            approval: approval.clone(),
            approved: a.approve,
            content_hash: if a.right_hash {
                hash.clone()
            } else {
                format!("sha256:{}", "b".repeat(64))
            },
            submitted_at,
            step_up: step_up.clone(),
            responder: if a.listed { OWNER } else { AUTHOR }.to_owned(),
            actor: a.actor,
        };
        let admissible = deadline != i64::MIN
            && a.right_hash
            && !hash.is_empty()
            && a.listed
            && a.actor == ActorKind::User
            && !self.record.closed.contains(&approval)
            && effective < deadline
            && fresh_step_up;
        if let Some(e) = &step_up {
            self.record.used.insert(e.assertion.clone());
        }
        if admissible {
            self.record.closed.insert(approval.clone());
        }
        let event = answer.event();
        self.record.answers.insert(
            event.event_id.clone(),
            Generated {
                approval,
                approve: a.approve,
                admissible,
            },
        );
        self.record.last_answer = Some(event.clone());
        self.fold_and_step(&event)
    }
}

/// One committed agent-stream event as the oracles read it: its type, ids, and its payload
/// re-parsed from canonical bytes.
#[derive(Debug, Clone)]
struct Read {
    event_type: String,
    event_id: EventId,
    causation: Option<EventId>,
    payload: Value,
}

impl Read {
    fn text(&self, key: &str) -> Option<&str> {
        self.payload.get(key).and_then(Value::as_str)
    }
}

fn read(journal: &[FoldedEvent]) -> Result<Vec<Read>, TestCaseError> {
    journal
        .iter()
        .map(|e| {
            let payload =
                mandate_canon::parse(&mandate_canon::to_canonical(&e.payload)).map_err(|err| {
                    TestCaseError::fail(format!("{} is not canonical: {err:?}", e.event_type))
                })?;
            Ok(Read {
                event_type: e.event_type.clone(),
                event_id: e.event_id.clone(),
                causation: e.causation_id.clone(),
                payload,
            })
        })
        .collect()
}

fn played(script: &[Step]) -> Result<(Vec<Read>, Record, Shell), TestCaseError> {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Ask),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell
        .fold_one(&reconciliation(1, ASKED_AT))
        .map_err(|e| TestCaseError::fail(format!("{e}")))?;
    shell
        .fold_one(&mark(2, "155", ASKED_AT))
        .map_err(|e| TestCaseError::fail(format!("{e}")))?;
    let (shell, _) = shell.restart(&ports);
    let mut run = Run {
        shell,
        ports: &ports,
        record: Record {
            clock: ASKED_AT,
            requests: BTreeMap::new(),
            answers: BTreeMap::new(),
            used: BTreeSet::new(),
            closed: BTreeSet::new(),
            late_cancels: Vec::new(),
            last_answer: None,
            assertions: 0,
        },
    };
    for step in script {
        run.play(step)?;
    }
    run.record.clock = run
        .record
        .clock
        .saturating_add(TIMEOUT_S.saturating_mul(10));
    let end = run.record.clock;
    run.step(Input::Tick(clock(end)))?;
    let journal = read(&run.shell.agent_journal)?;
    Ok((journal, run.record, run.shell))
}

/// Every answer is judged and copied once, and admitted exactly when the oracle's own record
/// judges it admissible: a request it saw, its hash, a listed `user`, before the deadline by its
/// own clock, fresh and unused step-up for a grant, and not cancelled by an exits-only, a version,
/// a kill switch, or a Stop applied after it, nor ended by an earlier admitted answer (EI-7,
/// EI-10, EI-14, EI-15).
fn admitted_exactly_the_admissible(journal: &[Read], record: &Record) -> Result<(), TestCaseError> {
    let copied = journal
        .iter()
        .filter(|e| e.event_type == "ApprovalResponded")
        .count();
    prop_assert_eq!(
        copied,
        record.answers.len(),
        "every answer is judged and copied once"
    );
    for event in journal
        .iter()
        .filter(|e| e.event_type == "ApprovalResponded")
    {
        let source = event.causation.clone().unwrap_or(EventId(String::new()));
        let generated = record.answers.get(&source).ok_or_else(|| {
            TestCaseError::fail(format!(
                "ApprovalResponded copies no generated answer: {source:?}"
            ))
        })?;
        prop_assert_eq!(event.text("approval"), Some(generated.approval.0.as_str()));
        prop_assert_eq!(
            event.text("result") == Some("admitted"),
            generated.admissible,
            "admitted exactly the answers the oracle judges admissible: {:?}",
            generated
        );
        if event.text("result") == Some("admitted") {
            prop_assert_eq!(
                event.text("verdict"),
                Some(if generated.approve {
                    "approved"
                } else {
                    "skipped"
                })
            );
        }
    }
    Ok(())
}

/// The approvals a journal holds open: requested, and with no terminal event yet, read from the
/// journaled events alone.
fn open_approvals(journal: &[FoldedEvent]) -> BTreeSet<String> {
    let mut open = BTreeSet::new();
    for e in journal {
        let approval = e
            .payload
            .get("approval")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let text = |k: &str| e.payload.get(k).and_then(Value::as_str);
        match e.event_type.as_str() {
            "ApprovalRequested" => {
                open.insert(e.event_id.0.clone());
            }
            "ApprovalTimedOut" | "ApprovalCanceled" | "ApprovalRevalidated" => {
                if let Some(a) = approval {
                    open.remove(&a);
                }
            }
            "ApprovalResponded"
                if text("verdict") == Some("skipped") && text("result") == Some("admitted") =>
            {
                if let Some(a) = approval {
                    open.remove(&a);
                }
            }
            _ => {}
        }
    }
    open
}

/// The lifecycle's states, from the brief's diagram, and the moves the table allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Created,
    Delivered,
    Granted,
    Terminal,
}

fn moved(state: State, event: &Read) -> Option<State> {
    let result = event.text("result");
    let verdict = event.text("verdict");
    match (state, event.event_type.as_str(), verdict, result) {
        (State::Created, "ApprovalDelivered", _, _)
            if event.text("status") == Some("delivered") =>
        {
            Some(State::Delivered)
        }
        (State::Created, "ApprovalDelivered", _, _) => Some(State::Created),
        (State::Created | State::Delivered, "ApprovalTimedOut" | "ApprovalCanceled", _, _) => {
            Some(State::Terminal)
        }
        (s @ (State::Created | State::Delivered), "ApprovalResponded", _, Some("refused")) => {
            Some(s)
        }
        (State::Delivered, "ApprovalResponded", Some("approved"), Some("counted")) => {
            Some(State::Delivered)
        }
        (State::Delivered, "ApprovalResponded", Some("approved"), Some("admitted")) => {
            Some(State::Granted)
        }
        (State::Delivered, "ApprovalResponded", Some("skipped"), Some("admitted")) => {
            Some(State::Terminal)
        }
        (State::Granted, "ApprovalRevalidated", _, Some("act" | "skip")) => Some(State::Terminal),
        (State::Terminal, "ApprovalResponded", _, Some("refused"))
            if event.text("reason") == Some("not_pending") =>
        {
            Some(State::Terminal)
        }
        _ => None,
    }
}

/// Runs `body` over `script()`; a failure reports proptest's minimal case, whose message is the
/// runtime's own error when it refuses.
fn check(body: impl Fn(Vec<Step>) -> Result<(), TestCaseError>) {
    let config = ProptestConfig {
        cases: 128,
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    if let Err(failure) = proptest::test_runner::TestRunner::new(config).run(&script(), body) {
        panic!("{failure}");
    }
}

/// EI-6, EI-7, DEC-173 item 10, the transition-table oracle: every approval event follows a
/// move the brief's lifecycle allows, every approval ends in exactly one terminal event once
/// every deadline has passed, every control-stream answer is copied exactly once however often
/// it is tailed and admitted exactly when the oracle's record allows (so no answer to an approval
/// an exits-only, a version, a kill switch, or a Stop cancelled is ever admitted), and replaying
/// the journal folds to the live pending set (EI-12).
#[test]
fn every_approval_moves_by_the_table_and_ends_once() {
    check(|script| {
        let (journal, record, shell) = played(&script)?;
        let mut states: BTreeMap<String, State> = BTreeMap::new();
        let mut terminals: BTreeMap<String, u32> = BTreeMap::new();
        let mut copies: BTreeMap<EventId, u32> = BTreeMap::new();
        for event in &journal {
            if event.event_type == "ApprovalRequested" {
                prop_assert!(
                    states
                        .insert(event.event_id.0.clone(), State::Created)
                        .is_none()
                );
                continue;
            }
            if !event.event_type.starts_with("Approval") {
                continue;
            }
            let approval = event.text("approval").unwrap_or_default().to_owned();
            if event.event_type == "ApprovalResponded" {
                let source = event.causation.clone().unwrap_or(EventId(String::new()));
                *copies.entry(source).or_default() += 1;
                if !states.contains_key(&approval) {
                    prop_assert_eq!(event.text("result"), Some("refused"));
                    prop_assert_eq!(event.text("reason"), Some("not_pending"));
                    continue;
                }
            }
            let state = *states.get(&approval).ok_or_else(|| {
                TestCaseError::fail(format!("{} names no request: {approval}", event.event_type))
            })?;
            let next = moved(state, event).ok_or_else(|| {
                TestCaseError::fail(format!("no move from {state:?} on {event:?}"))
            })?;
            if next == State::Terminal && state != State::Terminal {
                *terminals.entry(approval.clone()).or_default() += 1;
            }
            states.insert(approval, next);
        }
        prop_assert!(!states.is_empty(), "every script asks at least once");
        for (approval, state) in &states {
            prop_assert_eq!(*state, State::Terminal, "{} still open", approval);
            prop_assert_eq!(terminals.get(approval).copied(), Some(1), "{}", approval);
        }
        prop_assert!(!record.answers.is_empty());
        admitted_exactly_the_admissible(&journal, &record)?;
        prop_assert!(
            record.late_cancels.is_empty(),
            "a kill switch or Stop leaves no approval open past its own step: {:?}",
            record.late_cancels
        );
        for source in record.answers.keys() {
            prop_assert_eq!(
                copies.get(source).copied(),
                Some(1),
                "answer {:?} copied once",
                source
            );
        }
        let mut replayed = RuntimeState::new(common::deployment());
        for event in shell.agent_journal.iter().chain(shell.followed.iter()) {
            fold(&mut replayed, event).map_err(|e| TestCaseError::fail(format!("replay: {e}")))?;
        }
        prop_assert_eq!(
            replayed.pending_approvals(),
            shell.state.pending_approvals()
        );
        Ok(())
    });
}

/// EI-1, EI-3, EI-4, EI-7, EI-10, EI-15, the causation walker, field comparer and clock
/// accumulator: every opening intent's causation is an `act` re-validation of an approval the
/// oracle generated a request for, admitted from an answer the oracle's own clock and record judge
/// admissible, with exactly the bound action; at most one intent per approval; and an answer is
/// admitted exactly when the oracle judges it admissible. Inadmissible includes an answer to a request a
/// tightening to exits-only or a version applied after it has cancelled (EI-7), and one from any
/// `actor.kind` but `user` (EI-10).
#[test]
fn every_opening_walks_back_to_one_timely_admitted_grant() {
    check(|script| {
        let (journal, record, _) = played(&script)?;
        let by_id: BTreeMap<EventId, &Read> =
            journal.iter().map(|e| (e.event_id.clone(), e)).collect();
        admitted_exactly_the_admissible(&journal, &record)?;
        let mut intents_per_approval: BTreeMap<String, u32> = BTreeMap::new();
        for intent in journal.iter().filter(|e| e.event_type == "IntentProposed") {
            prop_assert_eq!(intent.text("purpose"), Some("open"));
            let cause = intent
                .causation
                .as_ref()
                .and_then(|c| by_id.get(c))
                .ok_or_else(|| {
                    TestCaseError::fail("an opening's causation is in this journal".to_owned())
                })?;
            prop_assert_eq!(cause.event_type.as_str(), "ApprovalRevalidated");
            prop_assert_eq!(cause.text("result"), Some("act"));
            let approval = cause.text("approval").unwrap_or_default().to_owned();
            let request = by_id
                .get(&EventId(approval.clone()))
                .filter(|r| r.event_type == "ApprovalRequested")
                .ok_or_else(|| TestCaseError::fail(format!("no request {approval}")))?;
            prop_assert!(record.requests.contains_key(&request.event_id));
            let action = request.payload.get("content").and_then(|c| c.get("action"));
            for (intent_key, action_key) in [
                ("instrument_id", "instrument"),
                ("side", "side"),
                ("qty", "qty"),
                ("limit_price", "limit"),
                ("purpose", "purpose"),
            ] {
                prop_assert_eq!(
                    intent.payload.get(intent_key),
                    action.and_then(|a| a.get(action_key)),
                    "the intent repeats the bound {}",
                    action_key
                );
            }
            let granted = journal.iter().any(|r| {
                r.event_type == "ApprovalResponded"
                    && r.text("approval") == Some(approval.as_str())
                    && r.text("verdict") == Some("approved")
                    && r.text("result") == Some("admitted")
            });
            prop_assert!(granted, "an act follows an admitted grant of {}", approval);
            *intents_per_approval.entry(approval).or_default() += 1;
        }
        for (approval, n) in &intents_per_approval {
            prop_assert_eq!(*n, 1, "one approval, at most one intent: {}", approval);
        }
        Ok(())
    });
}

/// Journal spec §9, mandate spec §6.4 check 7, DEC-488: every `ApprovalResponded` carries a
/// `quorum` member exactly when check 7 judged the grant it copies, and that member is exactly
/// `{independent, required}` as the journaled request bound them, with no overlay folded; a skip
/// and every refusal before check 7 carry none.
///
/// Whether check 7 judged a response is decided by the oracle's own generated answer, resolved
/// through the response's `causation` as [`admitted_exactly_the_admissible`] resolves it: check 7
/// is reached exactly when the generated answer both approves and is admissible. Reading the
/// runtime's own `verdict`, `result` and `reason` back would let the runtime choose which
/// responses it is judged on, so a swap of the labels between two responses would pass. The
/// pairing is asserted per response rather than by a count, for the same reason, and
/// `record.answers` is asserted non-empty so a script that generated nothing cannot pass by
/// walking nothing.
#[test]
fn the_quorum_is_recorded_exactly_when_check_7_judged_a_grant() {
    check(|script| {
        let (journal, record, _) = played(&script)?;
        let by_id: BTreeMap<EventId, &Read> =
            journal.iter().map(|e| (e.event_id.clone(), e)).collect();
        prop_assert!(
            !record.answers.is_empty(),
            "the script answered at least once, so the walk below judges something"
        );
        for event in journal
            .iter()
            .filter(|e| e.event_type == "ApprovalResponded")
        {
            let source = event.causation.clone().unwrap_or(EventId(String::new()));
            let generated = record.answers.get(&source).ok_or_else(|| {
                TestCaseError::fail(format!(
                    "ApprovalResponded copies no generated answer: {source:?}"
                ))
            })?;
            let reached_check_7 = generated.approve && generated.admissible;
            let expected = if reached_check_7 {
                let approval = event.text("approval").unwrap_or_default().to_owned();
                let request = by_id
                    .get(&EventId(approval.clone()))
                    .filter(|r| r.event_type == "ApprovalRequested")
                    .ok_or_else(|| {
                        TestCaseError::fail(format!("a judged grant names no request: {approval}"))
                    })?;
                let bound =
                    |name: &str| {
                        request.payload.get(name).cloned().ok_or_else(|| {
                            TestCaseError::fail(format!("the request binds `{name}`"))
                        })
                    };
                Some(common::object(&[
                    ("independent", bound("independent_required")?),
                    ("required", bound("approvers_required")?),
                ]))
            } else {
                None
            };
            prop_assert_eq!(
                event
                    .payload
                    .get("quorum")
                    .filter(|recorded| !matches!(recorded, Value::Null)),
                expected.as_ref(),
                "`quorum` is non-null exactly when check 7 judged the grant, as the request bound \
                 it; its presence as `null` otherwise is answer_records.rs's (DEC-533 item 3): {:?}",
                event.payload
            );
        }
        Ok(())
    });
}
