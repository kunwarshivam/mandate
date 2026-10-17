//! The runtime as a long-running process (E21-30 slice 1, DEC-1012): the agent stream on the real
//! in-memory journal, which validates every draft the process writes, and the control and clock
//! streams as rows a test lays down, as their writers (the workspace services and the scheduler)
//! would have committed them.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use mandate_canon::{Digest, Key, Object, Value, parse};
use mandate_daemon::{Config, Refs, RuntimeProcess, Wiring, fixture};
use mandate_journal::{AppendOutcome, Environment, StoredEvent, StreamId};
use mandate_process::{Exit, State};
use mandate_runtime::{
    AgentId, ApprovalSettings, Autonomy, Classified, ConnectionId, Deployment, DryRunVerdict,
    EventDraft, EventId, FlattenPlan, FlattenPlanner, FlattenRequest, GateDryRun, IdGen,
    MandateView, OrderPlan, Proposal, Seq, SignalInputs, WorkspaceId, WriterEpoch,
};
use mandate_store::{MemoryStore, Store};
use mandate_time::UtcNanos;
use proptest::prelude::*;

const WORKSPACE: &str = "ws1";
const AGENT: &str = "agent-a";
const AGENT_STREAM: &str = "agent:ws1:agent-a";
const CONTROL: &str = "ctl:ws1";
const CLOCK: &str = "clock:ws1";
const NOW: i64 = 1_792_155_600;

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap()
}

struct Ids;
impl IdGen for Ids {
    fn event_id(&self, epoch: WriterEpoch, head: Seq, ordinal: u32) -> EventId {
        EventId(format!("0{:05}{:014}{ordinal:06}", epoch.0, head.0))
    }
}

struct Deny;
impl GateDryRun for Deny {
    fn check(&self, _: &Proposal) -> DryRunVerdict {
        DryRunVerdict::Deny {
            reason_code: "fixture".into(),
        }
    }
}

struct Nothing;
impl OrderPlan for Nothing {
    fn plan(&self, _: &MandateView, _: &SignalInputs) -> Option<Proposal> {
        None
    }
    fn classify(&self, _: &MandateView, _: &Proposal) -> Classified {
        Classified {
            autonomy: Autonomy::Deny,
            decided_by: None,
        }
    }
}

struct Empty;
impl FlattenPlanner for Empty {
    fn plan(&self, request: &FlattenRequest) -> FlattenPlan {
        FlattenPlan {
            cancel_client_order_ids: Vec::new(),
            sells: Vec::new(),
            purpose: request.initiator.sell_purpose(),
            confirmation: request.confirmation.clone(),
        }
    }
}

/// The deployment's mandate version, the one reference the drafts here need (journal spec §9).
struct MandateRef;
impl Refs for MandateRef {
    fn refs(&self, _: &EventDraft) -> Option<(u64, Object)> {
        let version = format!("sha256:{}", Digest::of(b"the mandate version").to_hex());
        let mut refs = Object::new();
        refs.insert(Key::new("mandate_version").ok()?, Value::Str(version));
        Some((1, refs))
    }
}

fn wiring() -> Wiring {
    Wiring {
        ids: Box::new(Ids),
        gate: Box::new(Deny),
        plan: Box::new(Nothing),
        flatten: Box::new(Empty),
        refs: Box::new(MandateRef),
        view: MandateView {
            version: "v1".into(),
            working_universe: Default::default(),
            restricted_instruments: Default::default(),
            approval: ApprovalSettings {
                approvers: ["user-owner".to_owned()].into(),
                author: "user-author".into(),
                timeout_s: 300,
                environment: Environment::Paper,
                live_cli_confirm: None,
            },
        },
    }
}

fn config() -> Config {
    Config {
        deployment: Deployment {
            agent: AgentId(AGENT.into()),
            connection: ConnectionId("conn-1".into()),
            workspace: WorkspaceId(WORKSPACE.into()),
        },
        environment: Environment::Paper,
        liveness: Duration::from_secs(10),
    }
}

/// The agent stream on the in-memory journal; the control and clock streams as laid-down rows; a
/// log of which stream each read was of; and a count of appends to answer `Unavailable` after
/// committing them, as a connection lost after the commit would.
#[derive(Clone, Default)]
struct Rig {
    journal: MemoryStore,
    control: Rc<RefCell<Vec<StoredEvent>>>,
    clock: Rc<RefCell<Vec<StoredEvent>>>,
    clock_down: Rc<Cell<bool>>,
    reads: Rc<RefCell<Vec<String>>>,
    lose_answers: Rc<Cell<u32>>,
}

impl Store for Rig {
    fn take_ownership(&mut self, stream: &StreamId) -> Option<u64> {
        self.journal.take_ownership(stream)
    }
    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        self.reads.borrow_mut().push(stream.as_str().to_owned());
        match stream.as_str() {
            CONTROL => Some(self.control.borrow().clone()),
            CLOCK if self.clock_down.get() => None,
            CLOCK => Some(self.clock.borrow().clone()),
            _ => self.journal.rows(stream),
        }
    }
    fn append(
        &mut self,
        stream: &StreamId,
        head: u64,
        epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        let outcome = self
            .journal
            .append(stream, head, epoch, recorded_at, drafts);
        if self.lose_answers.get() > 0 {
            self.lose_answers.set(self.lose_answers.get() - 1);
            return AppendOutcome::Unavailable;
        }
        outcome
    }
}

fn row(stream: &str, seq: u64, event_type: &str, actor: &str, payload: &str) -> StoredEvent {
    let space = if stream == CONTROL { 'C' } else { 'K' };
    let event_id = format!("01J8Z3M{space}{seq:018}");
    let body = format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{event_id}","stream_id":"{stream}","event_type":"{event_type}","schema_version":1,"event_time":"2026-10-16T13:00:00.000000000Z","clock_source":"scheduler","causation_id":null,"correlation_id":null,"actor":{{"kind":"{actor}","id":"x","version":"1","build":null}},"config_refs":{{}},"payload":{payload},"artifact_refs":[],"pii_refs":[]}}"#
    );
    StoredEvent {
        stream_id: stream.into(),
        seq,
        event_id,
        event_type: event_type.into(),
        schema_version: 1,
        environment: "paper".into(),
        recorded_at: "2026-10-16T13:00:00.000000000Z".into(),
        prev_hash: Digest::of(b"prev"),
        hash: Digest::of(body.as_bytes()),
        body: body.into_bytes(),
    }
}

/// The owner's kill switch for this agent, as the workspace services journal it.
fn kill_switch(rig: &Rig) {
    let seq = u64::try_from(rig.control.borrow().len()).unwrap() + 1;
    let payload = format!(
        r#"{{"agent":"{AGENT}","command":"kill_switch","scope":"agent","subject":"{AGENT}","bid":null,"bid_size":null,"floor":null,"submitted_at":{NOW},"step_up":null,"user":"user-owner"}}"#
    );
    rig.control
        .borrow_mut()
        .push(row(CONTROL, seq, "OwnerCommandIssued", "user", &payload));
}

/// A scheduler tick at `secs`.
fn tick(rig: &Rig, secs: i64) {
    let seq = u64::try_from(rig.clock.borrow().len()).unwrap() + 1;
    let payload = format!(r#"{{"clock":"{}"}}"#, at(secs));
    rig.clock
        .borrow_mut()
        .push(row(CLOCK, seq, "ClockAdvanced", "system", &payload));
}

fn agent_rows(rig: &Rig) -> Vec<StoredEvent> {
    rig.journal
        .rows(&StreamId::parse(AGENT_STREAM).unwrap())
        .unwrap()
}

fn kinds(rig: &Rig) -> Vec<String> {
    agent_rows(rig)
        .iter()
        .map(|r| r.event_type.clone())
        .collect()
}

fn count(rig: &Rig, kind: &str) -> usize {
    kinds(rig).iter().filter(|k| *k == kind).count()
}

fn start(rig: &Rig, now: i64) -> RuntimeProcess<Rig> {
    RuntimeProcess::start(rig.clone(), wiring(), &config(), at(now))
}

#[test]
fn a_started_runtime_is_ready_and_writes_only_what_its_core_journals() {
    let rig = Rig::default();
    let process = start(&rig, NOW);
    assert_eq!(process.lifecycle().state(), State::Ready);
    assert!(
        agent_rows(&rig)
            .iter()
            .all(|r| r.event_type != "KillSwitchActivated"),
        "{:?}",
        kinds(&rig)
    );
}

#[test]
fn the_owner_s_kill_switch_is_journaled_by_the_runtime_on_its_agent_stream() {
    let rig = Rig::default();
    let mut process = start(&rig, NOW);
    kill_switch(&rig);
    process.step(at(NOW + 1));
    assert_eq!(
        process.lifecycle().state(),
        State::Ready,
        "{:?}",
        kinds(&rig)
    );
    assert_eq!(count(&rig, "KillSwitchActivated"), 1, "{:?}", kinds(&rig));
}

#[test]
fn each_pass_reads_the_control_stream_before_the_clock_stream() {
    let rig = Rig::default();
    let mut process = start(&rig, NOW);
    kill_switch(&rig);
    tick(&rig, NOW + 1);
    rig.reads.borrow_mut().clear();
    process.step(at(NOW + 1));
    let reads = rig.reads.borrow().clone();
    let control = reads.iter().position(|s| s == CONTROL).unwrap();
    let clock = reads.iter().position(|s| s == CLOCK).unwrap();
    assert!(control < clock, "{reads:?}");
}

#[test]
fn a_clock_stream_that_cannot_be_read_never_holds_the_kill_switch() {
    let rig = Rig::default();
    let mut process = start(&rig, NOW);
    rig.clock_down.set(true);
    kill_switch(&rig);
    process.step(at(NOW + 1));
    assert_eq!(count(&rig, "KillSwitchActivated"), 1);
    assert_eq!(process.lifecycle().state(), State::Ready);
}

#[test]
fn a_control_stream_handed_again_writes_nothing_twice() {
    let rig = Rig::default();
    let mut process = start(&rig, NOW);
    kill_switch(&rig);
    for second in 1..5 {
        process.step(at(NOW + second));
    }
    assert_eq!(count(&rig, "KillSwitchActivated"), 1);
    let mut restarted = start(&rig, NOW + 10);
    restarted.step(at(NOW + 11));
    assert_eq!(count(&rig, "KillSwitchActivated"), 1, "{:?}", kinds(&rig));
}

#[test]
fn an_append_whose_answer_was_lost_is_handed_again_and_written_once() {
    let rig = Rig::default();
    let mut process = start(&rig, NOW);
    kill_switch(&rig);
    rig.lose_answers.set(1);
    process.step(at(NOW + 1));
    process.step(at(NOW + 2));
    process.step(at(NOW + 3));
    assert_eq!(count(&rig, "KillSwitchActivated"), 1, "{:?}", kinds(&rig));
    assert_eq!(process.lifecycle().state(), State::Ready);
}

#[test]
fn whether_a_hint_came_changes_no_outcome() {
    let hinted = Rig::default();
    let polled = Rig::default();
    let mut eager = start(&hinted, NOW);
    let mut lazy = start(&polled, NOW);
    for second in 1..4 {
        kill_switch(&hinted);
        tick(&hinted, NOW + second);
        eager.step(at(NOW + 10));
        kill_switch(&polled);
        tick(&polled, NOW + second);
    }
    lazy.step(at(NOW + 10));
    let bodies = |rig: &Rig| -> Vec<(String, Value)> {
        agent_rows(rig)
            .iter()
            .map(|r| {
                let body = parse(&r.body).unwrap();
                (r.event_type.clone(), body.get("payload").cloned().unwrap())
            })
            .collect()
    };
    assert_eq!(bodies(&hinted), bodies(&polled));
}

#[test]
fn a_second_process_fences_the_first_before_it_writes_anything() {
    let rig = Rig::default();
    let mut first = start(&rig, NOW);
    let mut second = start(&rig, NOW + 1);
    kill_switch(&rig);
    let before = kinds(&rig);
    first.step(at(NOW + 2));
    assert_eq!(first.lifecycle().state(), State::Exited(Exit::Fenced));
    assert_eq!(kinds(&rig), before, "the fenced process wrote nothing");
    second.step(at(NOW + 2));
    assert_eq!(count(&rig, "KillSwitchActivated"), 1);
}

#[test]
fn a_runtime_for_any_environment_but_paper_refuses_before_taking_its_stream() {
    let rig = Rig::default();
    for environment in [Environment::Live, Environment::Backtest] {
        let process = RuntimeProcess::start(
            rig.clone(),
            wiring(),
            &Config {
                environment,
                ..config()
            },
            at(NOW),
        );
        assert_eq!(process.lifecycle().state(), State::Exited(Exit::Refused));
    }
    let agent = StreamId::parse(AGENT_STREAM).unwrap();
    assert_eq!(rig.journal.0.borrow().head(&agent).writer_epoch, 0);
}

#[test]
fn a_drain_hands_nothing_new_and_exits_drained() {
    let rig = Rig::default();
    let mut process = start(&rig, NOW);
    process.drain(at(NOW + 1));
    assert_eq!(process.lifecycle().state(), State::Exited(Exit::Drained));
    kill_switch(&rig);
    let before = kinds(&rig);
    process.step(at(NOW + 2));
    assert_eq!(kinds(&rig), before);
}

#[test]
fn a_draft_the_wiring_has_no_references_for_stops_the_process_rather_than_being_written() {
    struct Refuse;
    impl Refs for Refuse {
        fn refs(&self, _: &EventDraft) -> Option<(u64, Object)> {
            None
        }
    }
    let rig = Rig::default();
    kill_switch(&rig);
    let mut refusing = RuntimeProcess::start(
        rig.clone(),
        Wiring {
            refs: Box::new(Refuse),
            ..wiring()
        },
        &config(),
        at(NOW),
    );
    refusing.step(at(NOW + 1));
    assert_eq!(
        refusing.lifecycle().state(),
        State::Exited(Exit::CycleFailed)
    );
    assert_eq!(count(&rig, "KillSwitchActivated"), 0);
}

/// A `StreamOpened` whose answer was lost stops the process before the core starts, and the
/// restart finds the stream opened and opens it no second time (DEC-1012 item 7).
#[test]
fn a_stream_opening_in_doubt_exits_and_the_restart_opens_it_once() {
    let rig = Rig::default();
    rig.lose_answers.set(1);
    let first = start(&rig, NOW);
    assert_eq!(first.lifecycle().state(), State::Exited(Exit::CycleFailed));
    let restarted = start(&rig, NOW + 1);
    assert_eq!(restarted.lifecycle().state(), State::Ready);
    assert_eq!(count(&rig, "StreamOpened"), 1, "{:?}", kinds(&rig));
}

/// A row on the clock stream that is not a tick: the trading day the scheduler starts.
fn day_started(rig: &Rig) {
    let seq = u64::try_from(rig.clock.borrow().len()).unwrap() + 1;
    rig.clock.borrow_mut().push(row(
        CLOCK,
        seq,
        "TradingDayStarted",
        "system",
        r#"{"date":"2026-10-16"}"#,
    ));
}

fn risk_clock(process: &RuntimeProcess<Rig>) -> Option<i64> {
    process.state().risk_clock().map(|clock| clock.secs())
}

/// The scheduler's latest `ClockAdvanced` is the core's risk clock on the pass that reads it, and
/// a later tick moves it on; a clock-stream record that is not a tick is no tick.
#[test]
fn the_scheduler_s_latest_tick_is_the_runtime_s_risk_clock() {
    let rig = Rig::default();
    let mut process = start(&rig, NOW);
    day_started(&rig);
    process.step(at(NOW + 1));
    assert_eq!(risk_clock(&process), None, "a day started is no tick");
    tick(&rig, NOW + 5);
    process.step(at(NOW + 6));
    assert_eq!(risk_clock(&process), Some(NOW + 5));
    tick(&rig, NOW + 6);
    tick(&rig, NOW + 7);
    day_started(&rig);
    process.step(at(NOW + 8));
    assert_eq!(risk_clock(&process), Some(NOW + 7));
    process.step(at(NOW + 9));
    assert_eq!(
        risk_clock(&process),
        Some(NOW + 7),
        "a tick handed again moves nothing"
    );
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// The binary's fixture wiring carries the references every draft needs, so the owner's kill
/// switch is journaled through it rather than stopping the process.
#[test]
fn the_fixture_wiring_journals_the_owner_s_kill_switch() {
    let rig = Rig::default();
    let mut process = RuntimeProcess::start(
        rig.clone(),
        fixture::wiring("user-owner"),
        &config(),
        at(NOW),
    );
    kill_switch(&rig);
    process.step(at(NOW + 1));
    assert_eq!(
        process.lifecycle().state(),
        State::Ready,
        "{:?}",
        kinds(&rig)
    );
    assert_eq!(count(&rig, "KillSwitchActivated"), 1);
    let refs = parse(&agent_rows(&rig)[1].body)
        .unwrap()
        .get("config_refs")
        .and_then(|r| r.get("mandate_version"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    assert!(refs.is_some_and(|v| v.starts_with("sha256:")));
}

#[derive(Debug, Clone, Copy)]
enum Op {
    KillSwitch,
    Tick,
    LoseAnswer,
    Step,
    Restart,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        Just(Op::KillSwitch),
        Just(Op::Tick),
        Just(Op::LoseAnswer),
        Just(Op::Step),
        Just(Op::Restart),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Whatever the interleaving of the owner's kill switches, ticks, lost append answers, and
    /// restarts, once a pass runs with nothing lost: the process is ready; every kill switch copied
    /// names a distinct control row, with its owner's instruction beside it; and the first kill
    /// switch the control stream holds is among them. The oracle reads the control rows the test
    /// laid down and the agent stream's causation ids, not the process's state.
    #[test]
    fn every_kill_switch_is_copied_once_whatever_the_interleaving(
        ops in proptest::collection::vec(op(), 1..24),
    ) {
        let rig = Rig::default();
        let mut second = NOW;
        let mut process = start(&rig, second);
        for op in ops {
            second += 1;
            match op {
                Op::KillSwitch => kill_switch(&rig),
                Op::Tick => tick(&rig, second),
                Op::LoseAnswer => rig.lose_answers.set(1),
                Op::Step => {
                    process.step(at(second));
                }
                Op::Restart => process = start(&rig, second),
            }
        }
        rig.lose_answers.set(0);
        for _ in 0..2 {
            second += 1;
            process.step(at(second));
        }
        prop_assert_eq!(process.lifecycle().state(), State::Ready, "{:?}", kinds(&rig));
        let caused = |kind: &str| -> Vec<String> {
            agent_rows(&rig)
                .iter()
                .filter(|r| r.event_type == kind)
                .filter_map(|r| {
                    let body = parse(&r.body).ok()?;
                    body.get("causation_id")?.as_str().map(str::to_owned)
                })
                .collect()
        };
        let switched = caused("KillSwitchActivated");
        let distinct: std::collections::BTreeSet<&String> = switched.iter().collect();
        prop_assert_eq!(distinct.len(), switched.len(), "{:?}", switched);
        prop_assert_eq!(caused("OwnerExitRequested"), switched.clone());
        let first = rig.control.borrow().first().map(|r| r.event_id.clone());
        if let Some(first) = first {
            prop_assert!(switched.contains(&first), "{:?} misses {}", switched, first);
        }
    }
}
