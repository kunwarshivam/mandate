//! The runtime as a long-running process (E21-30 slice 1, DEC-1012): the agent stream on the real
//! in-memory journal, which validates every draft the process writes, and the control and clock
//! streams, and the account stream (DEC-1014), as rows a test lays down, as their writers (the
//! workspace services, the scheduler, and the account's executor) would have committed them.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::time::Duration;

use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_canon::{Digest, Key, Object, Value, parse};
use mandate_daemon::deployment::ApproverSource;
use mandate_daemon::models::{Evaluated, Models, Streams};
use mandate_daemon::{ArtifactSink, Config, ModelsWiring, Refs, RuntimeProcess, Wiring, fixture};
use mandate_journal::{AppendOutcome, Environment, StoredEvent, StreamId};
use mandate_num::{Conviction, Price, Qty, Unit};
use mandate_process::{Exit, State};
use mandate_runtime::{
    AgentId, ApprovalSettings, Autonomy, Classified, ConnectionId, Deferral, Deployment,
    DryRunVerdict, EventDraft, EventId, FlattenPlan, FlattenPlanner, FlattenRequest, GateDryRun,
    IdGen, MandateView, Mode, ModelDirection, ModelOutput, OrderExecution, OrderPlan, Proposal,
    Purpose, RiskClock, Seq, SignalInputs, TimeInForce, WorkspaceId, WriterEpoch,
};
use mandate_store::{MemoryStore, Store};
use mandate_time::UtcNanos;
use proptest::prelude::*;

const WORKSPACE: &str = "ws1";
const AGENT: &str = "agent-a";
const AGENT_STREAM: &str = "agent:ws1:agent-a";
const CONTROL: &str = "ctl:ws1";
const CLOCK: &str = "clock:ws1";
const CONNECTION: &str = "conn-1";
const ACCOUNT_REF: &str = "01J8Z3MACCOUNTREF000000001";
const ACCOUNT: &str = "acct:ws1:01J8Z3MACCOUNTREF000000001";
const OTHER_REF: &str = "01J8Z3MACCOUNTREF000000002";
const OTHER_ACCOUNT: &str = "acct:ws1:01J8Z3MACCOUNTREF000000002";
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
        research: None,
        models: None,
        facts: None,
        approvers: None,
    }
}

fn config() -> Config {
    Config {
        deployment: Deployment {
            agent: AgentId(AGENT.into()),
            connection: ConnectionId(CONNECTION.into()),
            workspace: WorkspaceId(WORKSPACE.into()),
        },
        environment: Environment::Paper,
        liveness: Duration::from_secs(10),
    }
}

/// The agent stream on the in-memory journal; the control, clock, and account streams as laid-down
/// rows, any of them able to be unreadable; a log of which stream each read was of; and a count of
/// appends to answer `Unavailable` after committing them, as a connection lost after the commit
/// would.
#[derive(Clone, Default)]
struct Rig {
    journal: MemoryStore,
    control: Rc<RefCell<Vec<StoredEvent>>>,
    control_down: Rc<Cell<bool>>,
    clock: Rc<RefCell<Vec<StoredEvent>>>,
    clock_down: Rc<Cell<bool>>,
    accounts: Rc<RefCell<Vec<StoredEvent>>>,
    account_down: Rc<Cell<bool>>,
    /// The agent stream read as unreadable, from the moment a test sets it.
    agent_down: Rc<Cell<bool>>,
    /// Rows a test lays over the agent stream's own when it is read, as a record a writer left.
    agent_extra: Rc<RefCell<Vec<StoredEvent>>>,
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
            CONTROL if self.control_down.get() => None,
            CONTROL => Some(self.control.borrow().clone()),
            CLOCK if self.clock_down.get() => None,
            CLOCK => Some(self.clock.borrow().clone()),
            id if id.starts_with("acct:") && self.account_down.get() => None,
            id if id.starts_with("acct:") => Some(
                self.accounts
                    .borrow()
                    .iter()
                    .filter(|r| r.stream_id == id)
                    .cloned()
                    .collect(),
            ),
            AGENT_STREAM if self.agent_down.get() => None,
            AGENT_STREAM => self.journal.rows(stream).map(|mut rows| {
                rows.extend(self.agent_extra.borrow().iter().cloned());
                rows
            }),
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
    versioned(stream, seq, event_type, 1, actor, payload)
}

fn versioned(
    stream: &str,
    seq: u64,
    event_type: &str,
    schema_version: u64,
    actor: &str,
    payload: &str,
) -> StoredEvent {
    let space = match stream {
        CONTROL => 'C',
        ACCOUNT => 'A',
        OTHER_ACCOUNT => 'B',
        _ => 'K',
    };
    let event_id = format!("01J8Z3M{space}{seq:018}");
    let body = format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{event_id}","stream_id":"{stream}","event_type":"{event_type}","schema_version":{schema_version},"event_time":"2026-10-16T13:00:00.000000000Z","clock_source":"scheduler","causation_id":null,"correlation_id":null,"actor":{{"kind":"{actor}","id":"x","version":"1","build":null}},"config_refs":{{}},"payload":{payload},"artifact_refs":[],"pii_refs":[]}}"#
    );
    StoredEvent {
        stream_id: stream.into(),
        seq,
        event_id,
        event_type: event_type.into(),
        schema_version,
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

/// A pass whose input in doubt is settled on its retry goes on to the streams in that same pass: a
/// tick the scheduler wrote meanwhile is the risk clock at once, not a pass later, and the batch
/// whose answer was lost is written once.
#[test]
fn a_retry_that_commits_goes_on_to_the_streams_in_the_same_pass() {
    let rig = Rig::default();
    let mut process = start(&rig, NOW);
    kill_switch(&rig);
    rig.lose_answers.set(1);
    process.step(at(NOW + 1));
    assert_eq!(risk_clock(&process), None, "the batch is in doubt");
    tick(&rig, NOW + 2);
    process.step(at(NOW + 2));
    assert_eq!(count(&rig, "KillSwitchActivated"), 1, "{:?}", kinds(&rig));
    assert_eq!(risk_clock(&process), Some(NOW + 2));
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

fn risk_clock<S: Store>(process: &RuntimeProcess<S>) -> Option<i64> {
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
        fixture::wiring("user-owner", AGENT_STREAM),
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

/// Lane DR's X9: two agents' runtimes on one journal, each on the binary's fixture wiring, both
/// open their streams and are ready. The journal refuses an `event_id` another stream already
/// holds (journal spec §3), so ids derived from the epoch, head and ordinal alone stopped the
/// second agent's runtime at its first batch.
#[test]
fn two_agents_runtimes_on_one_journal_both_open_their_streams() {
    let rig = Rig::default();
    let other = "agent-b";
    let other_stream = format!("agent:{WORKSPACE}:{other}");
    let mut config_b = config();
    config_b.deployment.agent = AgentId(other.into());
    let first = RuntimeProcess::start(
        rig.clone(),
        fixture::wiring("user-owner", AGENT_STREAM),
        &config(),
        at(NOW),
    );
    let second = RuntimeProcess::start(
        rig.clone(),
        fixture::wiring("user-owner", &other_stream),
        &config_b,
        at(NOW),
    );
    assert_eq!(first.lifecycle().state(), State::Ready);
    assert_eq!(second.lifecycle().state(), State::Ready);
    let opened = |stream: &str| {
        rig.journal
            .rows(&StreamId::parse(stream).unwrap())
            .unwrap_or_default()
            .iter()
            .filter(|row| row.event_type == "StreamOpened")
            .count()
    };
    assert_eq!((opened(AGENT_STREAM), opened(&other_stream)), (1, 1));
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

/// The control stream's `ConnectionEstablished` of `connection` at `schema_version`, naming
/// `reference` as its account (journal spec §9.8: version 2 carries `account_ref`).
fn established(rig: &Rig, connection: &str, reference: &str, schema_version: u64) {
    established_in(rig, connection, reference, schema_version, "paper");
}

fn established_in(
    rig: &Rig,
    connection: &str,
    reference: &str,
    schema_version: u64,
    environment: &str,
) {
    let seq = u64::try_from(rig.control.borrow().len()).unwrap() + 1;
    let payload = format!(
        r#"{{"connection_id":"{connection}","broker":"alpaca","environment":"{environment}","scopes":[],"account_ref":"{reference}"}}"#
    );
    rig.control.borrow_mut().push(versioned(
        CONTROL,
        seq,
        "ConnectionEstablished",
        schema_version,
        "system",
        &payload,
    ));
}

/// A record the account's executor committed on `stream`.
fn on_account(rig: &Rig, stream: &str, event_type: &str, payload: &str) {
    let seq = rig
        .accounts
        .borrow()
        .iter()
        .filter(|r| r.stream_id == stream)
        .count();
    let seq = u64::try_from(seq).unwrap() + 1;
    rig.accounts
        .borrow_mut()
        .push(row(stream, seq, event_type, "system", payload));
}

fn reconciled(rig: &Rig, stream: &str) {
    on_account(rig, stream, "ReconciliationRun", r#"{"result":"clean"}"#);
}

fn mode(process: &RuntimeProcess<Rig>) -> Mode {
    process.state().effective_mode()
}

/// A runtime starts held until its account's executor reconciles (trading spec §11, DEC-131 item
/// 13), and the reconciliation reaches it from the account stream its connection's establishment
/// names, so the hold lifts (DEC-1014 item 1).
#[test]
fn the_startup_hold_lifts_once_the_executor_reconciles_the_account() {
    let rig = Rig::default();
    established(&rig, CONNECTION, ACCOUNT_REF, 2);
    let mut process = start(&rig, NOW);
    assert_eq!(mode(&process), Mode::Paused);
    tick(&rig, NOW + 1);
    process.step(at(NOW + 1));
    assert_eq!(mode(&process), Mode::Paused, "nothing reconciled yet");
    reconciled(&rig, ACCOUNT);
    tick(&rig, NOW + 2);
    process.step(at(NOW + 2));
    assert_eq!(mode(&process), Mode::Normal, "{:?}", kinds(&rig));
    for second in 3..6 {
        tick(&rig, NOW + second);
        process.step(at(NOW + second));
    }
    assert_eq!(
        process.lifecycle().state(),
        State::Ready,
        "each row is folded once"
    );
    assert_eq!(mode(&process), Mode::Normal);
}

/// The account stream is folded before `Input::Started`, as the shell's open replays it, so the
/// start takes the hold on the whole fold (DEC-1014 item 2).
#[test]
fn an_account_reconciled_before_the_start_is_folded_before_the_hold_is_taken() {
    let rig = Rig::default();
    established(&rig, CONNECTION, ACCOUNT_REF, 2);
    reconciled(&rig, ACCOUNT);
    let process = start(&rig, NOW);
    assert_eq!(process.lifecycle().state(), State::Ready);
    assert_eq!(mode(&process), Mode::Normal);
}

/// The account stream as the cell's connect and record leave it (journal spec §9.8): the copy of
/// `ConnectionEstablished` the CLI's `connection record` appends after the executor's check and
/// reconciliation. The runtime folds it and starts ready; before, its fold refused the copy as not
/// interpreted, and every runtime in the cell exited `cycle_failed` at its start.
#[test]
fn the_account_streams_copy_of_the_establishment_folds_and_the_runtime_starts() {
    let rig = Rig::default();
    established(&rig, CONNECTION, ACCOUNT_REF, 2);
    reconciled(&rig, ACCOUNT);
    on_account(
        &rig,
        ACCOUNT,
        "ConnectionEstablished",
        &format!(
            r#"{{"connection_id":"{CONNECTION}","broker":"alpaca","environment":"paper","scopes":[],"account_ref":"{ACCOUNT_REF}","risk_clock":"2026-09-21T15:00:00.000000000Z"}}"#
        ),
    );
    let mut process = start(&rig, NOW);
    assert_eq!(
        process.lifecycle().state(),
        State::Ready,
        "{:?}",
        kinds(&rig)
    );
    assert_eq!(mode(&process), Mode::Normal);
    tick(&rig, NOW + 1);
    process.step(at(NOW + 1));
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// The latest establishment names the account: a reconnect to a new account is followed from the
/// restart, and the old account's reconciliation is not this connection's.
#[test]
fn the_latest_establishment_names_the_account_followed() {
    let rig = Rig::default();
    established(&rig, CONNECTION, OTHER_REF, 2);
    established(&rig, CONNECTION, ACCOUNT_REF, 2);
    reconciled(&rig, ACCOUNT);
    let process = start(&rig, NOW);
    assert_eq!(mode(&process), Mode::Normal);
    let rig = Rig::default();
    established(&rig, CONNECTION, ACCOUNT_REF, 2);
    established(&rig, CONNECTION, OTHER_REF, 2);
    reconciled(&rig, ACCOUNT);
    let process = start(&rig, NOW);
    assert_eq!(mode(&process), Mode::Paused);
}

/// Only this deployment's connection, at version 2, names its account: another connection's
/// account, or a version-1 record, lifts nothing (`AGENTS.md` rule 3).
#[test]
fn another_connection_s_account_never_lifts_the_hold() {
    let rig = Rig::default();
    established(&rig, "conn-2", OTHER_REF, 2);
    established(&rig, CONNECTION, ACCOUNT_REF, 1);
    reconciled(&rig, OTHER_ACCOUNT);
    reconciled(&rig, ACCOUNT);
    let mut process = start(&rig, NOW);
    tick(&rig, NOW + 1);
    process.step(at(NOW + 1));
    assert_eq!(process.lifecycle().state(), State::Ready);
    assert_eq!(mode(&process), Mode::Paused);
}

/// A connection established after the start is followed from then on.
#[test]
fn a_connection_established_after_the_start_is_followed() {
    let rig = Rig::default();
    let mut process = start(&rig, NOW);
    established(&rig, CONNECTION, ACCOUNT_REF, 2);
    reconciled(&rig, ACCOUNT);
    tick(&rig, NOW + 1);
    process.step(at(NOW + 1));
    assert_eq!(mode(&process), Mode::Normal);
}

/// The executor's `AgentModeApplied` for this agent tightens the runtime on the next pass.
#[test]
fn an_agent_paused_on_the_account_stream_is_paused() {
    let rig = Rig::default();
    established(&rig, CONNECTION, ACCOUNT_REF, 2);
    reconciled(&rig, ACCOUNT);
    let mut process = start(&rig, NOW);
    assert_eq!(mode(&process), Mode::Normal);
    on_account(
        &rig,
        ACCOUNT,
        "AgentModeApplied",
        &format!(r#"{{"agent":"{AGENT}","to":"paused"}}"#),
    );
    tick(&rig, NOW + 1);
    process.step(at(NOW + 1));
    assert_eq!(mode(&process), Mode::Paused);
}

#[test]
fn each_pass_reads_the_control_then_the_account_then_the_clock_stream() {
    let rig = Rig::default();
    established(&rig, CONNECTION, ACCOUNT_REF, 2);
    let mut process = start(&rig, NOW);
    kill_switch(&rig);
    tick(&rig, NOW + 1);
    rig.reads.borrow_mut().clear();
    process.step(at(NOW + 1));
    let reads = rig.reads.borrow().clone();
    let first = |stream: &str| reads.iter().position(|s| s == stream).unwrap();
    assert!(first(CONTROL) < first(ACCOUNT), "{reads:?}");
    assert!(first(ACCOUNT) < first(CLOCK), "{reads:?}");
}

/// An account stream that cannot be read folds nothing, and holds neither the owner's kill switch
/// (ES-06) nor the tick, so no risk exit or deadline waits on it (`AGENTS.md` rule 13).
#[test]
fn an_account_stream_that_cannot_be_read_holds_neither_the_kill_switch_nor_the_tick() {
    let rig = Rig::default();
    established(&rig, CONNECTION, ACCOUNT_REF, 2);
    reconciled(&rig, ACCOUNT);
    let mut process = start(&rig, NOW);
    rig.account_down.set(true);
    kill_switch(&rig);
    tick(&rig, NOW + 1);
    process.step(at(NOW + 1));
    assert_eq!(count(&rig, "KillSwitchActivated"), 1, "{:?}", kinds(&rig));
    assert_eq!(risk_clock(&process), Some(NOW + 1));
    assert_eq!(process.lifecycle().state(), State::Ready);
    on_account(
        &rig,
        ACCOUNT,
        "AgentModeApplied",
        &format!(r#"{{"agent":"{AGENT}","to":"paused"}}"#),
    );
    tick(&rig, NOW + 2);
    process.step(at(NOW + 2));
    let folded = |p: &RuntimeProcess<Rig>| p.state().head(ACCOUNT).map(|seq| seq.0);
    assert_eq!(folded(&process), Some(1), "nothing folded while unreadable");
    assert_eq!(risk_clock(&process), Some(NOW + 2));
    rig.account_down.set(false);
    tick(&rig, NOW + 3);
    process.step(at(NOW + 3));
    assert_eq!(folded(&process), Some(2), "folded once readable again");
}

/// What the start cannot read, it does not guess: an unreadable control or account stream stops the
/// process before it is ready, after its replay, and the next process starts on the whole fold.
#[test]
fn a_start_that_cannot_read_the_control_or_account_stream_is_never_ready() {
    let rig = Rig::default();
    rig.control_down.set(true);
    let process = start(&rig, NOW);
    assert_eq!(
        process.lifecycle().state(),
        State::Exited(Exit::CycleFailed)
    );
    let rig = Rig::default();
    established(&rig, CONNECTION, ACCOUNT_REF, 2);
    rig.account_down.set(true);
    let process = start(&rig, NOW);
    assert_eq!(
        process.lifecycle().state(),
        State::Exited(Exit::CycleFailed)
    );
}

/// A record the core cannot fold stops the process rather than being skipped (DEC-85).
#[test]
fn an_account_record_the_core_cannot_fold_stops_the_process() {
    let rig = Rig::default();
    established(&rig, CONNECTION, ACCOUNT_REF, 2);
    let mut process = start(&rig, NOW);
    on_account(&rig, ACCOUNT, "NoSuchRecord", "{}");
    process.step(at(NOW + 1));
    assert_eq!(
        process.lifecycle().state(),
        State::Exited(Exit::CycleFailed)
    );
    let restarted = start(&rig, NOW + 2);
    assert_eq!(
        restarted.lifecycle().state(),
        State::Exited(Exit::CycleFailed)
    );
}

/// The account a running process follows never changes under it: a later establishment naming
/// another account stops it, and the next process follows the new account from its start.
#[test]
fn an_establishment_naming_another_account_stops_the_process() {
    let rig = Rig::default();
    established(&rig, CONNECTION, ACCOUNT_REF, 2);
    let mut process = start(&rig, NOW);
    established(&rig, CONNECTION, ACCOUNT_REF, 2);
    process.step(at(NOW + 1));
    assert_eq!(
        process.lifecycle().state(),
        State::Ready,
        "the same account again"
    );
    established(&rig, CONNECTION, OTHER_REF, 2);
    process.step(at(NOW + 2));
    assert_eq!(
        process.lifecycle().state(),
        State::Exited(Exit::CycleFailed)
    );
}

/// A paper process follows only a paper connection's account, and folds only paper rows: an
/// establishment for another environment names nothing, and an account row of another environment
/// stops the process (`AGENTS.md` rule 8).
#[test]
fn a_paper_runtime_follows_only_a_paper_account() {
    let rig = Rig::default();
    established_in(&rig, CONNECTION, ACCOUNT_REF, 2, "live");
    reconciled(&rig, ACCOUNT);
    let mut process = start(&rig, NOW);
    tick(&rig, NOW + 1);
    process.step(at(NOW + 1));
    assert_eq!(process.lifecycle().state(), State::Ready);
    assert_eq!(
        mode(&process),
        Mode::Paused,
        "a live establishment names nothing"
    );

    let rig = Rig::default();
    established(&rig, CONNECTION, ACCOUNT_REF, 2);
    let mut process = start(&rig, NOW);
    reconciled(&rig, ACCOUNT);
    rig.accounts.borrow_mut().last_mut().unwrap().environment = "live".into();
    tick(&rig, NOW + 1);
    process.step(at(NOW + 1));
    assert_eq!(
        process.lifecycle().state(),
        State::Exited(Exit::CycleFailed)
    );
    assert_eq!(mode(&process), Mode::Paused);
}

#[derive(Debug, Clone)]
enum AccountOp {
    Establish,
    Reconcile { clean: bool },
    Submitted,
    Applied { here: bool, to: &'static str },
    Unreadable,
    Step,
    Restart,
}

fn account_op() -> impl Strategy<Value = AccountOp> {
    prop_oneof![
        Just(AccountOp::Establish),
        any::<bool>().prop_map(|clean| AccountOp::Reconcile { clean }),
        Just(AccountOp::Submitted),
        (
            any::<bool>(),
            prop_oneof![
                Just("normal"),
                Just("exits_only"),
                Just("paused"),
                Just("stopped")
            ]
        )
            .prop_map(|(here, to)| AccountOp::Applied { here, to }),
        Just(AccountOp::Unreadable),
        Just(AccountOp::Step),
        Just(AccountOp::Restart),
    ]
}

/// One account row as the oracle reads it, from the laid-down row alone.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Laid {
    Clean,
    Unclean,
    Submitted,
    Applied { agent: String, to: String },
}

fn laid(rig: &Rig) -> Vec<Laid> {
    rig.accounts
        .borrow()
        .iter()
        .map(|r| {
            let body = parse(&r.body).unwrap();
            let payload = body.get("payload").unwrap();
            let field = |name: &str| {
                payload
                    .get(name)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            match r.event_type.as_str() {
                "ReconciliationRun" if field("result") == "clean" => Laid::Clean,
                "ReconciliationRun" => Laid::Unclean,
                "OrderSubmitted" => Laid::Submitted,
                _ => Laid::Applied {
                    agent: field("agent"),
                    to: field("to"),
                },
            }
        })
        .collect()
}

/// Whether `rows` end with the broker's truth known: a clean reconciliation with no submission
/// after it (trading spec §11).
fn known(rows: &[Laid]) -> bool {
    rows.iter()
        .rposition(|r| *r == Laid::Clean)
        .is_some_and(|clean| !rows[clean..].contains(&Laid::Submitted))
}

/// The executor's latest mode for this agent in `rows`, `normal` with none.
fn copied(rows: &[Laid]) -> Mode {
    rows.iter()
        .rev()
        .find_map(|r| match r {
            Laid::Applied { agent, to } if agent == AGENT => Some(to.as_str()),
            _ => None,
        })
        .map_or(Mode::Normal, |to| match to {
            "exits_only" => Mode::ExitsOnly,
            "paused" => Mode::Paused,
            "stopped" => Mode::Stopped,
            _ => Mode::Normal,
        })
}

/// The oracle's own account of the hold: how many account rows the process has folded, how many
/// it had folded at its start, and whether the hold is held. Taken at a start unless the folded
/// rows already end known (no intent is ever outstanding here); lifted on a pass that folds a
/// clean reconciliation from after that start, with no submission after the last one
/// (DEC-1014 item 6).
struct Oracle {
    folded: usize,
    at_start: usize,
    held: bool,
}

impl Oracle {
    fn started(rows: &[Laid], established: bool) -> Self {
        let folded = if established { rows.len() } else { 0 };
        Self {
            folded,
            at_start: folded,
            held: !known(&rows[..folded]),
        }
    }

    fn passed(&mut self, rows: &[Laid], established: bool) {
        if established {
            self.folded = rows.len();
        }
        let since = &rows[self.at_start..self.folded];
        if self.held && since.contains(&Laid::Clean) && known(&rows[..self.folded]) {
            self.held = false;
        }
    }

    fn mode(&self, rows: &[Laid]) -> Mode {
        let copied = copied(&rows[..self.folded]);
        if self.held {
            copied.max(Mode::Paused)
        } else {
            copied
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Whatever the interleaving of the executor's records, the connection's establishment,
    /// unreadable account reads, passes, and restarts: the process stays ready, and after each start
    /// and each pass that reads the account its effective mode is the executor's latest mode for
    /// this agent, held at least `paused` from a start that did not find the broker's truth known
    /// until a clean reconciliation after that start, with no submission after it (trading spec
    /// §11, DEC-1014). The oracle reads only the rows the test laid down.
    #[test]
    fn the_runtime_s_mode_follows_its_account_stream_whatever_the_interleaving(
        ops in proptest::collection::vec(account_op(), 1..32),
    ) {
        let rig = Rig::default();
        let mut second = NOW;
        let mut process = start(&rig, second);
        let mut is_established = false;
        let mut oracle = Oracle::started(&laid(&rig), is_established);
        for op in ops {
            second += 1;
            match op {
                AccountOp::Establish => {
                    if !is_established {
                        established(&rig, CONNECTION, ACCOUNT_REF, 2);
                        is_established = true;
                    }
                }
                AccountOp::Reconcile { clean } => on_account(
                    &rig,
                    ACCOUNT,
                    "ReconciliationRun",
                    if clean { r#"{"result":"clean"}"# } else { r#"{"result":"mismatch"}"# },
                ),
                AccountOp::Submitted => on_account(
                    &rig,
                    ACCOUNT,
                    "OrderSubmitted",
                    r#"{"client_order_id":"o1","attempt":1}"#,
                ),
                AccountOp::Applied { here, to } => {
                    let agent = if here { AGENT } else { "agent-b" };
                    on_account(
                        &rig,
                        ACCOUNT,
                        "AgentModeApplied",
                        &format!(r#"{{"agent":"{agent}","to":"{to}"}}"#),
                    );
                }
                AccountOp::Unreadable => {
                    rig.account_down.set(true);
                    tick(&rig, second);
                    process.step(at(second));
                    rig.account_down.set(false);
                    oracle.passed(&laid(&rig), false);
                    prop_assert_eq!(mode(&process), oracle.mode(&laid(&rig)), "unreadable");
                }
                AccountOp::Step => {
                    tick(&rig, second);
                    process.step(at(second));
                    oracle.passed(&laid(&rig), is_established);
                    prop_assert_eq!(mode(&process), oracle.mode(&laid(&rig)), "after a pass");
                }
                AccountOp::Restart => {
                    process = start(&rig, second);
                    oracle = Oracle::started(&laid(&rig), is_established);
                    prop_assert_eq!(mode(&process), oracle.mode(&laid(&rig)), "at the start");
                }
            }
            prop_assert_eq!(process.lifecycle().state(), State::Ready, "{:?}", kinds(&rig));
        }
    }
}

/// The event ids of the control rows, when they read, and of the agent rows one ask was handed.
type Handed = (Option<Vec<String>>, Vec<String>);

/// The model host as a test lays it down: the evaluations it answers at any tick, how many times
/// it was asked, and what each ask was handed.
#[derive(Clone, Default)]
struct Host {
    answer: Rc<RefCell<Vec<Evaluated>>>,
    asked: Rc<Cell<u32>>,
    handed: Rc<RefCell<Vec<Handed>>>,
}
impl Models for Host {
    fn evaluate(&self, _: RiskClock, streams: Streams<'_>) -> Vec<Evaluated> {
        self.asked.set(self.asked.get() + 1);
        let ids = |rows: &[StoredEvent]| rows.iter().map(|row| row.event_id.clone()).collect();
        self.handed
            .borrow_mut()
            .push((streams.control.map(ids), ids(streams.agent)));
        self.answer.borrow().clone()
    }
}

/// An artifact store that keeps what it is given, or refuses everything.
#[derive(Clone, Default)]
struct Closes {
    kept: Rc<RefCell<Vec<Vec<u8>>>>,
    refuse: bool,
}
impl ArtifactSink for Closes {
    fn put(&mut self, bytes: &[u8]) -> bool {
        if !self.refuse {
            self.kept.borrow_mut().push(bytes.to_vec());
        }
        !self.refuse
    }
}

const SPY: &str = "b28f4066-5c6d-479b-a2af-85dc1a8f16fb";

/// The host's daily output for SPY cut off at `as_of`.
fn daily(as_of: i64) -> Evaluated {
    Evaluated {
        output: ModelOutput {
            model_id: "quant.ma_crossover".into(),
            model_version: "1.0.0".into(),
            content_hash: Digest::of(b"quant.ma_crossover"),
            instrument_id: InstrumentId::new(SPY).unwrap(),
            as_of: RiskClock::from_secs(as_of),
            expires_at: RiskClock::from_secs(as_of + 86_400),
            direction: ModelDirection::Long,
            conviction: Conviction::parse("1").unwrap(),
            confidence: Unit::parse("1").unwrap(),
            horizon_s: 86_400,
            thesis_ref: None,
            evidence: Vec::new(),
            invalidation: None,
            thesis_id: None,
            lineage_id: None,
            ignored: None,
        },
        closes: format!("closes through {as_of}").into_bytes(),
        source: "data.daily_bars".into(),
    }
}

fn modelled(rig: &Rig, host: &Host, closes: &Closes) -> RuntimeProcess<Rig> {
    RuntimeProcess::start(
        rig.clone(),
        Wiring {
            models: Some(ModelsWiring {
                models: Box::new(host.clone()),
                artifacts: Box::new(closes.clone()),
            }),
            ..wiring()
        },
        &config(),
        at(NOW),
    )
}

fn positions(rig: &Rig, kind: &str) -> Vec<usize> {
    kinds(rig)
        .iter()
        .enumerate()
        .filter(|(_, k)| *k == kind)
        .map(|(n, _)| n)
        .collect()
}

/// DEC-2383: on a tick the process asks the host once and records each new output as its
/// observation, the closes stored first under the digest it names, then the output; a tick
/// handed again asks nothing more, and a later tick with the same daily cut-off records nothing.
#[test]
fn a_tick_records_each_new_model_output_once_after_its_observation() {
    let rig = Rig::default();
    let (host, closes) = (Host::default(), Closes::default());
    host.answer.borrow_mut().push(daily(NOW - 3_600));
    let mut process = modelled(&rig, &host, &closes);
    tick(&rig, NOW + 5);
    process.step(at(NOW + 6));
    assert_eq!(host.asked.get(), 1);
    let ids = |rows: Vec<StoredEvent>| -> Vec<String> {
        rows.into_iter().map(|row| row.event_id).collect()
    };
    let control = rig.rows(&StreamId::parse(CONTROL).unwrap()).unwrap();
    let [(handed_control, handed_agent)] = host.handed.borrow().clone().try_into().unwrap();
    assert_eq!(
        handed_control,
        Some(ids(control)),
        "the host is handed the control rows (DEC-2827)"
    );
    assert!(
        handed_agent.is_empty() && !agent_rows(&rig).is_empty(),
        "the host is handed the agent stream's thesis records alone, here none (DEC-2852 item 4)"
    );
    let (observed, output) = (
        positions(&rig, "ObservationRecorded"),
        positions(&rig, "ModelOutputRecorded"),
    );
    assert_eq!((observed.len(), output.len()), (1, 1), "{:?}", kinds(&rig));
    assert!(
        observed[0] < output[0],
        "the observation first: {:?}",
        kinds(&rig)
    );
    assert_eq!(*closes.kept.borrow(), vec![daily(NOW - 3_600).closes]);
    let observation = String::from_utf8(agent_rows(&rig)[observed[0]].body.clone()).unwrap();
    assert!(
        observation.contains(&Digest::of(&daily(NOW - 3_600).closes).to_hex()),
        "its data_ref is the stored closes: {observation}"
    );
    process.step(at(NOW + 7));
    assert_eq!(host.asked.get(), 1, "the same tick asks nothing more");
    tick(&rig, NOW + 8);
    process.step(at(NOW + 9));
    assert_eq!(host.asked.get(), 2, "a new tick asks again");
    assert_eq!(
        count(&rig, "ModelOutputRecorded"),
        1,
        "the same daily output is recorded once"
    );
    host.answer.borrow_mut()[0] = daily(NOW + 7);
    tick(&rig, NOW + 10);
    process.step(at(NOW + 11));
    assert_eq!(
        count(&rig, "ModelOutputRecorded"),
        2,
        "the next cut-off is new"
    );
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// A restart on the same tick records no output a second time.
#[test]
fn a_restart_records_no_output_twice() {
    let rig = Rig::default();
    let (host, closes) = (Host::default(), Closes::default());
    host.answer.borrow_mut().push(daily(NOW - 3_600));
    tick(&rig, NOW + 5);
    let mut first = modelled(&rig, &host, &closes);
    first.step(at(NOW + 6));
    drop(first);
    let mut second = modelled(&rig, &host, &closes);
    second.step(at(NOW + 7));
    assert_eq!(host.asked.get(), 2, "the restart asks once");
    assert_eq!(count(&rig, "ModelOutputRecorded"), 1);
    assert_eq!(second.lifecycle().state(), State::Ready);
}

/// Without a tick the host is never asked (the runtime decides only on one); a host that answers
/// nothing, a refusal, records nothing; and closes that cannot be stored are no output (MI-10).
#[test]
fn no_tick_no_answer_or_unstored_closes_record_nothing() {
    let rig = Rig::default();
    let (host, closes) = (Host::default(), Closes::default());
    let mut process = modelled(&rig, &host, &closes);
    kill_switch(&rig);
    process.step(at(NOW + 1));
    assert_eq!(
        host.asked.get(),
        0,
        "no tick, no evaluation, and the kill switch asks nothing"
    );
    tick(&rig, NOW + 5);
    process.step(at(NOW + 6));
    assert_eq!(host.asked.get(), 1);
    assert_eq!(
        count(&rig, "ObservationRecorded") + count(&rig, "ModelOutputRecorded"),
        0
    );

    let rig = Rig::default();
    let refusing = Closes {
        refuse: true,
        ..Closes::default()
    };
    host.answer.borrow_mut().push(daily(NOW - 3_600));
    let mut process = modelled(&rig, &host, &refusing);
    tick(&rig, NOW + 5);
    process.step(at(NOW + 6));
    assert_eq!(
        count(&rig, "ObservationRecorded") + count(&rig, "ModelOutputRecorded"),
        0
    );
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// An order plan that proposes nothing and remembers, for each decision, the model outputs it was
/// handed.
#[derive(Clone, Default)]
struct Seen(Rc<RefCell<Vec<usize>>>);
impl OrderPlan for Seen {
    fn plan(&self, _: &MandateView, inputs: &SignalInputs) -> Option<Proposal> {
        self.0
            .borrow_mut()
            .push(inputs.outputs.values().map(BTreeMap::len).sum());
        None
    }
    fn classify(&self, _: &MandateView, _: &Proposal) -> Classified {
        Classified {
            autonomy: Autonomy::Deny,
            decided_by: None,
        }
    }
}

/// #2448's review (rule 13), seam 1: the tick that follows a new output decides with it folded,
/// so the observation, then the output, then the tick's decision is the journal's order.
#[test]
fn the_tick_decides_with_the_output_it_follows() {
    let rig = Rig::default();
    let (host, closes, seen) = (Host::default(), Closes::default(), Seen::default());
    host.answer.borrow_mut().push(daily(NOW - 3_600));
    let mut process = RuntimeProcess::start(
        rig.clone(),
        Wiring {
            plan: Box::new(seen.clone()),
            models: Some(ModelsWiring {
                models: Box::new(host.clone()),
                artifacts: Box::new(closes.clone()),
            }),
            ..wiring()
        },
        &config(),
        at(NOW),
    );
    tick(&rig, NOW + 5);
    process.step(at(NOW + 6));
    assert_eq!(count(&rig, "ModelOutputRecorded"), 1);
    assert_eq!(risk_clock(&process), Some(NOW + 5), "the tick was handed");
    assert_eq!(
        seen.0.borrow().last().copied(),
        Some(1),
        "the decision saw the output recorded before its tick: {:?}",
        seen.0.borrow()
    );
}

/// Seams 2 to 4: whatever the models' inputs read, the tick is handed, so its expiries and exits
/// never wait on a model (rule 13). With models wired the tick runs; an agent stream that cannot
/// be read, or an output record that does not read, records no new output, and the tick still
/// moves the risk clock with the process ready.
#[test]
fn the_tick_runs_whatever_the_models_inputs_read() {
    let rig = Rig::default();
    let (host, closes) = (Host::default(), Closes::default());
    let mut process = modelled(&rig, &host, &closes);
    tick(&rig, NOW + 5);
    process.step(at(NOW + 6));
    assert_eq!(
        risk_clock(&process),
        Some(NOW + 5),
        "models wired, the tick still runs"
    );

    host.answer.borrow_mut().push(daily(NOW - 3_600));
    rig.agent_down.set(true);
    tick(&rig, NOW + 7);
    process.step(at(NOW + 8));
    rig.agent_down.set(false);
    assert_eq!(
        count(&rig, "ModelOutputRecorded"),
        0,
        "an unreadable stream records nothing"
    );
    assert_eq!(
        risk_clock(&process),
        Some(NOW + 7),
        "and the tick still runs"
    );
    assert_eq!(process.lifecycle().state(), State::Ready);

    let broken = r#"{"model_id":"quant.ma_crossover"}"#;
    rig.agent_extra.borrow_mut().push(row(
        AGENT_STREAM,
        90,
        "ModelOutputRecorded",
        "runtime",
        broken,
    ));
    tick(&rig, NOW + 9);
    process.step(at(NOW + 10));
    assert_eq!(
        count(&rig, "ModelOutputRecorded"),
        0,
        "an unreadable output records nothing"
    );
    assert_eq!(
        risk_clock(&process),
        Some(NOW + 9),
        "and the tick still runs"
    );
    assert_eq!(process.lifecycle().state(), State::Ready);

    rig.agent_extra.borrow_mut().clear();
    drop(process);
    let mut process = modelled(&rig, &host, &closes);
    tick(&rig, NOW + 11);
    process.step(at(NOW + 12));
    assert_eq!(
        count(&rig, "ModelOutputRecorded"),
        1,
        "once readable, the output is recorded: a journal that took the record away is read \
         again whole at the next start or the next check, since a running process reads only \
         what follows its head between them (DEC-2847 item 4, DEC-2852 item 6)"
    );
    assert_eq!(risk_clock(&process), Some(NOW + 11));
}

/// The production ports' fold (DEC-2378 S2) is fed every row the process reads: the account the
/// control stream names, from its start, and the scheduler's ticks, each row once however many
/// passes read it.
#[test]
fn the_production_fold_is_fed_the_named_account_and_the_clock_once_each() {
    let rig = Rig::default();
    let facts = mandate_daemon::production::Facts::new(CLOCK, AGENT);
    established(&rig, CONNECTION, ACCOUNT_REF, 2);
    let stamp = at(NOW).to_string();
    on_account(
        &rig,
        ACCOUNT,
        "OrderRequestRecorded",
        &format!(r#"{{"agent_id":"{AGENT}","purpose":"open","risk_clock":"{stamp}"}}"#),
    );
    on_account(
        &rig,
        ACCOUNT,
        "OrderSubmitted",
        r#"{"client_order_id":"o1","instrument_id":"spy","side":"buy"}"#,
    );
    on_account(
        &rig,
        ACCOUNT,
        "FillApplied",
        &format!(
            r#"{{"fill_id":"f1","client_order_id":"o1","instrument_id":"spy","side":"buy","qty_gross":"3","price":"100","risk_clock":"{stamp}"}}"#
        ),
    );
    on_account(&rig, OTHER_ACCOUNT, "FillApplied", r#"{"fill_id":"f9"}"#);
    let mut process = RuntimeProcess::start(
        rig.clone(),
        Wiring {
            facts: Some(facts.clone()),
            approvers: None,
            ..wiring()
        },
        &config(),
        at(NOW),
    );
    tick(&rig, NOW + 5);
    process.step(at(NOW + 5));
    process.step(at(NOW + 6));
    let held = facts.read(|journaled| journaled.held());
    let three = mandate_num::Qty::parse("3").unwrap();
    assert_eq!(held, Some([("spy".to_owned(), three)].into()));
    assert_eq!(
        facts.read(|journaled| journaled.now()),
        Some(Some(at(NOW + 5)))
    );
}

/// The production flatten's unplanned holding reaches the process's alerts as the fixed key
/// `flatten_unplanned`, once (DEC-2378 item 8).
#[test]
fn a_kill_switch_with_an_unplanned_holding_raises_the_operator_alert_once() {
    let rig = Rig::default();
    let facts = mandate_daemon::production::Facts::new(CLOCK, AGENT);
    established(&rig, CONNECTION, ACCOUNT_REF, 2);
    let stamp = at(NOW).to_string();
    on_account(
        &rig,
        ACCOUNT,
        "OrderRequestRecorded",
        &format!(r#"{{"agent_id":"{AGENT}","purpose":"open","risk_clock":"{stamp}"}}"#),
    );
    on_account(
        &rig,
        ACCOUNT,
        "OrderSubmitted",
        r#"{"client_order_id":"o1","instrument_id":"spy","side":"buy"}"#,
    );
    on_account(
        &rig,
        ACCOUNT,
        "FillApplied",
        &format!(
            r#"{{"fill_id":"f1","client_order_id":"o1","instrument_id":"spy","side":"buy","qty_gross":"3","price":"100","risk_clock":"{stamp}"}}"#
        ),
    );
    let flatten =
        mandate_daemon::production::Flatten::new(facts.clone(), std::collections::BTreeMap::new());
    let mut process = RuntimeProcess::start(
        rig.clone(),
        Wiring {
            facts: Some(facts),
            approvers: None,
            flatten: Box::new(flatten),
            ..wiring()
        },
        &config(),
        at(NOW),
    );
    process.step(at(NOW + 1));
    kill_switch(&rig);
    let alerts = process.step(at(NOW + 2));
    assert_eq!(count(&rig, "KillSwitchActivated"), 1, "{:?}", kinds(&rig));
    let keys: Vec<&str> = alerts.iter().map(|alert| alert.key).collect();
    assert!(
        keys.contains(&mandate_daemon::FLATTEN_UNPLANNED),
        "{keys:?}"
    );
    let again = process.step(at(NOW + 3));
    assert!(
        again
            .iter()
            .all(|alert| alert.key != mandate_daemon::FLATTEN_UNPLANNED)
    );
}

/// The owner's exit of `spy`, as the workspace services journal it: unconfirmed, or with a
/// confirmed bid and CLI step-up `assertion` authenticated ten seconds before it was committed.
fn owner_exit(rig: &Rig, assertion: Option<&str>) {
    owner_exit_at(rig, assertion.map(|assertion| (assertion, NOW - 10)));
}

/// The owner's exit of `spy` committed at `NOW`, with a confirmed bid of 99 for 100 and floor 95
/// and CLI step-up `(assertion, authenticated_at)` when given, and no bid otherwise.
fn owner_exit_at(rig: &Rig, step_up: Option<(&str, i64)>) {
    let seq = u64::try_from(rig.control.borrow().len()).unwrap() + 1;
    let (bid, step_up) = match step_up {
        Some((assertion, authenticated_at)) => (
            r#""bid":"99","bid_size":"100","floor":"95""#.to_owned(),
            format!(
                r#"{{"assertion_id":"{assertion}","authenticated_at":{authenticated_at},"method":"cli_confirm"}}"#
            ),
        ),
        None => (
            r#""bid":null,"bid_size":null,"floor":null"#.to_owned(),
            "null".to_owned(),
        ),
    };
    let payload = format!(
        r#"{{"agent":"{AGENT}","command":"owner_exit","scope":"instrument","subject":"spy",{bid},"submitted_at":{NOW},"step_up":{step_up},"user":"user-owner"}}"#
    );
    rig.control
        .borrow_mut()
        .push(row(CONTROL, seq, "OwnerCommandIssued", "user", &payload));
}

/// The runtime over the production flatten, with the agent's fill of 3 `spy` on its account and
/// `spy` a US equity in its universe.
fn holding_spy(rig: &Rig, now: i64) -> RuntimeProcess<Rig> {
    let facts = mandate_daemon::production::Facts::new(CLOCK, AGENT);
    if rig.accounts.borrow().is_empty() {
        established(rig, CONNECTION, ACCOUNT_REF, 2);
        let stamp = at(NOW).to_string();
        on_account(
            rig,
            ACCOUNT,
            "OrderRequestRecorded",
            &format!(r#"{{"agent_id":"{AGENT}","purpose":"open","risk_clock":"{stamp}"}}"#),
        );
        on_account(
            rig,
            ACCOUNT,
            "OrderSubmitted",
            r#"{"client_order_id":"o1","instrument_id":"spy","side":"buy"}"#,
        );
        on_account(
            rig,
            ACCOUNT,
            "FillApplied",
            &format!(
                r#"{{"fill_id":"f1","client_order_id":"o1","instrument_id":"spy","side":"buy","qty_gross":"3","price":"100","risk_clock":"{stamp}"}}"#
            ),
        );
    }
    let classes = [("spy".to_owned(), mandate_domain::AssetClass::UsEquity)].into();
    let flatten = mandate_daemon::production::Flatten::new(facts.clone(), classes);
    RuntimeProcess::start(
        rig.clone(),
        Wiring {
            facts: Some(facts),
            approvers: None,
            flatten: Box::new(flatten),
            ..wiring()
        },
        &config(),
        at(now),
    )
}

fn payload_of(row: &StoredEvent) -> Value {
    parse(&row.body)
        .ok()
        .and_then(|body| body.get("payload").cloned())
        .unwrap_or(Value::Null)
}

fn causation_of(row: &StoredEvent) -> Option<String> {
    parse(&row.body)
        .ok()?
        .get("causation_id")?
        .as_str()
        .map(str::to_owned)
}

/// X7, DEC-2387: in the cell, where the journal is the only channel to the executor, the owner's
/// exit of one instrument is journaled on the real journal as its copy and one market sell of the
/// agent's whole sub-ledger caused by it, the record the executor reads (journal spec §9.1). With
/// valid step-up the copy's evidence carries `authenticated_at` as the timestamp the journal types
/// and the executor reads to bound the confirmation (DEC-2040 item 7).
#[test]
fn the_owner_s_exit_of_one_instrument_is_journaled_as_its_market_sell() {
    for assertion in [None, Some("assertion-exit")] {
        let rig = Rig::default();
        let mut process = holding_spy(&rig, NOW);
        process.step(at(NOW + 1));
        owner_exit(&rig, assertion);
        process.step(at(NOW + 2));
        assert_eq!(
            process.lifecycle().state(),
            State::Ready,
            "{:?}",
            kinds(&rig)
        );
        let rows = agent_rows(&rig);
        let copy = rows
            .iter()
            .find(|r| r.event_type == "OwnerExitRequested")
            .expect("the copy is journaled");
        let legs: Vec<&StoredEvent> = rows
            .iter()
            .filter(|r| r.event_type == "IntentProposed")
            .collect();
        assert_eq!(legs.len(), 1, "{:?}", kinds(&rig));
        assert_eq!(causation_of(legs[0]), Some(copy.event_id.clone()));
        let leg = payload_of(legs[0]);
        for (member, want) in [
            ("instrument_id", "spy"),
            ("side", "sell"),
            ("type", "market"),
            ("tif", "day"),
            ("qty", "3"),
            ("purpose", "owner_exit"),
        ] {
            assert_eq!(
                leg.get(member).and_then(Value::as_str),
                Some(want),
                "{member}"
            );
        }
        assert_eq!(leg.get("limit_price"), Some(&Value::Null));
        let stamped = payload_of(copy)
            .get("step_up")
            .and_then(|step_up| step_up.get("authenticated_at"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        assert_eq!(
            stamped,
            assertion.map(|_| at(NOW - 10).to_string()),
            "{assertion:?}"
        );
    }
}

#[derive(Debug, Clone, Copy)]
enum ExitOp {
    OwnerExit,
    Tick,
    LoseAnswer,
    Step,
    Restart,
}

fn exit_op() -> impl Strategy<Value = ExitOp> {
    prop_oneof![
        Just(ExitOp::OwnerExit),
        Just(ExitOp::Tick),
        Just(ExitOp::LoseAnswer),
        Just(ExitOp::Step),
        Just(ExitOp::Restart),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// Whatever the interleaving of the owner's exits, ticks, lost append answers and restarts,
    /// once a pass runs with nothing lost: every owner exit the control stream holds is copied
    /// exactly once, and each copy has exactly one market sell of the agent's 3 `spy` caused by it,
    /// so none is sold twice and none is never proposed (`AGENTS.md` rule 13). The oracle reads
    /// the control rows the test laid down and the agent stream's rows, not the process's state.
    #[test]
    fn every_owner_exit_is_proposed_once_whatever_the_interleaving(
        ops in proptest::collection::vec(exit_op(), 1..20),
    ) {
        let rig = Rig::default();
        let mut second = NOW;
        let mut process = holding_spy(&rig, second);
        for op in ops {
            second += 1;
            match op {
                ExitOp::OwnerExit => owner_exit(&rig, None),
                ExitOp::Tick => tick(&rig, second),
                ExitOp::LoseAnswer => rig.lose_answers.set(1),
                ExitOp::Step => {
                    process.step(at(second));
                }
                ExitOp::Restart => process = holding_spy(&rig, second),
            }
        }
        rig.lose_answers.set(0);
        for _ in 0..2 {
            second += 1;
            process.step(at(second));
        }
        prop_assert_eq!(process.lifecycle().state(), State::Ready, "{:?}", kinds(&rig));
        let commands: Vec<String> = rig
            .control
            .borrow()
            .iter()
            .filter(|r| r.event_type == "OwnerCommandIssued")
            .map(|r| r.event_id.clone())
            .collect();
        let rows = agent_rows(&rig);
        let copies: Vec<&StoredEvent> =
            rows.iter().filter(|r| r.event_type == "OwnerExitRequested").collect();
        let copied: Vec<String> = copies.iter().filter_map(|r| causation_of(r)).collect();
        prop_assert_eq!(&copied, &commands);
        for copy in copies {
            let legs: Vec<&StoredEvent> = rows
                .iter()
                .filter(|r| r.event_type == "IntentProposed")
                .filter(|r| causation_of(r).as_ref() == Some(&copy.event_id))
                .collect();
            prop_assert_eq!(legs.len(), 1, "{:?}", kinds(&rig));
            let leg = payload_of(legs[0]);
            prop_assert_eq!(leg.get("qty").and_then(Value::as_str), Some("3"));
        }
    }
}

/// Where the agent-stream rows the runtime writes for an owner's exit are kept, so the executor
/// process's own test reads exactly them (DEC-2387): the two processes may not link each other
/// (`xtask/layers.toml`), so this file is their contract.
const OWNER_EXIT_CONTRACT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/contracts/owner-exit-agent-stream.json"
);

/// The runtime's agent-stream rows for three owner exits of `spy` (unconfirmed, confirmed with
/// valid step-up, and confirmed with step-up already stale at commit) are byte for byte the
/// contract file `mandate-executord`'s test reads. `MANDATE_WRITE_CONTRACTS=1` rewrites it.
#[test]
fn the_owner_exit_rows_are_the_executors_contract() {
    let cases: [(&str, Option<(&str, i64)>); 3] = [
        ("unconfirmed", None),
        ("stepped_up", Some(("assertion-valid", NOW - 10))),
        ("stale", Some(("assertion-stale", NOW - 1_000))),
    ];
    let mut listed = Vec::new();
    for (name, step_up) in cases {
        let rig = Rig::default();
        let mut process = holding_spy(&rig, NOW);
        process.step(at(NOW + 1));
        owner_exit_at(&rig, step_up);
        process.step(at(NOW + 2));
        assert_eq!(process.lifecycle().state(), State::Ready, "{name}");
        let rows: Vec<Value> = agent_rows(&rig)
            .iter()
            .filter(|r| {
                matches!(
                    r.event_type.as_str(),
                    "OwnerExitRequested" | "IntentProposed"
                )
            })
            .map(|r| {
                let mut members = Object::new();
                let put = |members: &mut Object, key: &str, value: Value| {
                    members.insert(Key::new(key).unwrap(), value);
                };
                put(&mut members, "event_id", Value::Str(r.event_id.clone()));
                put(&mut members, "event_type", Value::Str(r.event_type.clone()));
                put(
                    &mut members,
                    "causation_id",
                    causation_of(r).map_or(Value::Null, Value::Str),
                );
                put(&mut members, "payload", payload_of(r));
                Value::Object(members)
            })
            .collect();
        assert_eq!(rows.len(), 2, "{name}: the copy and its one leg");
        let mut case = Object::new();
        case.insert(Key::new("name").unwrap(), Value::Str(name.to_owned()));
        case.insert(Key::new("rows").unwrap(), Value::Array(rows));
        listed.push(Value::Object(case));
    }
    let mut document = Object::new();
    document.insert(Key::new("cases").unwrap(), Value::Array(listed));
    let mut bytes = mandate_canon::to_canonical(&Value::Object(document));
    bytes.push(b'\n');
    if std::env::var_os("MANDATE_WRITE_CONTRACTS").is_some() {
        std::fs::write(OWNER_EXIT_CONTRACT, &bytes).unwrap();
    }
    let kept = std::fs::read(OWNER_EXIT_CONTRACT).unwrap_or_default();
    assert!(
        kept == bytes,
        "{OWNER_EXIT_CONTRACT} differs from what the runtime writes; rerun with \
         MANDATE_WRITE_CONTRACTS=1 and re-check mandate-executord's owner_exit_contract test"
    );
}

/// #2448's re-review (rule 13): an evaluation the core would refuse (an empty source, a negative
/// horizon, an `as_of` past what a stamp can write) stores no closes and hands nothing, so it
/// never stops the process; an evaluation beside it is still recorded and the tick still runs.
#[test]
fn an_evaluation_the_core_refuses_hands_nothing_and_the_tick_runs() {
    let rig = Rig::default();
    let (host, closes) = (Host::default(), Closes::default());
    let mut unsourced = daily(NOW - 7_200);
    unsourced.source = String::new();
    let mut backwards = daily(NOW - 10_800);
    backwards.output.horizon_s = u64::MAX;
    let mut unstamped = daily(NOW - 14_400);
    unstamped.output.as_of = RiskClock::from_secs(i64::MAX);
    let good = daily(NOW - 3_600);
    host.answer
        .borrow_mut()
        .extend([unsourced, backwards, unstamped, good.clone()]);
    let mut process = modelled(&rig, &host, &closes);
    tick(&rig, NOW + 5);
    process.step(at(NOW + 6));
    assert_eq!(
        process.lifecycle().state(),
        State::Ready,
        "{:?}",
        kinds(&rig)
    );
    assert_eq!(risk_clock(&process), Some(NOW + 5), "the tick was handed");
    assert_eq!(
        (
            count(&rig, "ObservationRecorded"),
            count(&rig, "ModelOutputRecorded")
        ),
        (1, 1),
        "only the evaluation the core takes: {:?}",
        kinds(&rig)
    );
    assert_eq!(*closes.kept.borrow(), vec![good.closes]);
}

/// DEC-131 item 6 through the models pass: an observation whose append is in doubt is handed
/// again first, then its output, then the tick, each recorded once, and the host is not asked
/// again for the same tick.
#[test]
fn an_observation_in_doubt_is_followed_by_its_output_before_the_tick() {
    let rig = Rig::default();
    let (host, closes) = (Host::default(), Closes::default());
    host.answer.borrow_mut().push(daily(NOW - 3_600));
    let mut process = modelled(&rig, &host, &closes);
    tick(&rig, NOW + 5);
    rig.lose_answers.set(1);
    process.step(at(NOW + 6));
    assert_eq!(risk_clock(&process), None, "the observation is in doubt");
    assert_eq!(count(&rig, "ModelOutputRecorded"), 0);
    process.step(at(NOW + 7));
    assert_eq!(host.asked.get(), 1, "the same tick asks nothing more");
    assert_eq!(
        (
            count(&rig, "ObservationRecorded"),
            count(&rig, "ModelOutputRecorded")
        ),
        (1, 1),
        "{:?}",
        kinds(&rig)
    );
    assert!(
        positions(&rig, "ObservationRecorded")[0] < positions(&rig, "ModelOutputRecorded")[0],
        "{:?}",
        kinds(&rig)
    );
    assert_eq!(risk_clock(&process), Some(NOW + 5), "then the tick");
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// The same output twice in one answer is recorded once.
#[test]
fn an_output_answered_twice_at_once_is_recorded_once() {
    let rig = Rig::default();
    let (host, closes) = (Host::default(), Closes::default());
    host.answer
        .borrow_mut()
        .extend([daily(NOW - 3_600), daily(NOW - 3_600)]);
    let mut process = modelled(&rig, &host, &closes);
    tick(&rig, NOW + 5);
    process.step(at(NOW + 6));
    assert_eq!(
        (
            count(&rig, "ObservationRecorded"),
            count(&rig, "ModelOutputRecorded")
        ),
        (1, 1),
        "{:?}",
        kinds(&rig)
    );
    assert_eq!(risk_clock(&process), Some(NOW + 5));
}

/// #2448's re-review: when the clock has moved on by the time an observation in doubt is retried,
/// the output queued behind it is handed before the new tick is evaluated, so the same daily
/// output, answered again at the new tick, is recorded once.
#[test]
fn a_tick_that_moves_on_while_an_observation_is_in_doubt_records_its_output_once() {
    let rig = Rig::default();
    let (host, closes) = (Host::default(), Closes::default());
    host.answer.borrow_mut().push(daily(NOW - 3_600));
    let mut process = modelled(&rig, &host, &closes);
    tick(&rig, NOW + 5);
    rig.lose_answers.set(1);
    process.step(at(NOW + 6));
    assert_eq!(count(&rig, "ModelOutputRecorded"), 0, "in doubt");
    tick(&rig, NOW + 7);
    process.step(at(NOW + 8));
    assert_eq!(host.asked.get(), 2, "the new tick asks again");
    assert_eq!(
        (
            count(&rig, "ObservationRecorded"),
            count(&rig, "ModelOutputRecorded")
        ),
        (1, 1),
        "{:?}",
        kinds(&rig)
    );
    assert_eq!(risk_clock(&process), Some(NOW + 7));
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// A dry run that allows every opening with the halt deferred to the binding gate and no bars read.
struct Deferring;
impl GateDryRun for Deferring {
    fn check(&self, _: &Proposal) -> DryRunVerdict {
        DryRunVerdict::AllowDeferring {
            deferred: BTreeSet::from([Deferral::Halt]),
            liquidity: None,
        }
    }
}

/// A plan that opens one share of AAPL at 155 whatever its inputs, and classifies it `auto`.
struct Opening;
impl OrderPlan for Opening {
    fn plan(&self, _: &MandateView, _: &SignalInputs) -> Option<Proposal> {
        Some(Proposal {
            instrument: InstrumentId::new("AAPL").ok()?,
            asset_class: AssetClass::UsEquity,
            side: Side::Buy,
            qty: Qty::parse("1").ok()?,
            limit: Price::parse("155").ok()?,
            purpose: Purpose::Open,
            exit_origin: None,
            exit_conviction: Some(Value::Str("1".into())),
            buy_conviction: Some(Value::Str("1".into())),
            combined_score: Value::Str("0.5".into()),
            outputs_used: BTreeSet::new(),
            model_weights: BTreeMap::new(),
            clips_applied: Vec::new(),
            execution: Some(OrderExecution {
                asset_class: AssetClass::UsEquity,
                tif: TimeInForce::Day,
                protection_required: false,
                protection: None,
            }),
        })
    }
    fn classify(&self, _: &MandateView, _: &Proposal) -> Classified {
        Classified {
            autonomy: Autonomy::Auto,
            decided_by: Some("default".into()),
        }
    }
}

/// DEC-2388 item 13, end to end over the journal's own append: an opening the dry run allows with
/// a check deferred is committed as `DecisionMade` version 3 naming it, and the owner is asked;
/// nothing is proposed to the executor.
#[test]
fn a_deferring_opening_is_journaled_at_version_3_and_asked_never_proposed() {
    let rig = Rig::default();
    established(&rig, CONNECTION, ACCOUNT_REF, 2);
    reconciled(&rig, ACCOUNT);
    on_account(
        &rig,
        ACCOUNT,
        "MarkUpdated",
        &format!(
            r#"{{"instrument_id":"AAPL","price":"155","source":"fixture","feed":"sip","risk_clock":{NOW}}}"#
        ),
    );
    let mut wiring = fixture::wiring("user-owner", AGENT_STREAM);
    wiring.gate = Box::new(Deferring);
    wiring.plan = Box::new(Opening);
    wiring.view.version = format!("sha256:{}", Digest::of(b"mandate-daemon fixture").to_hex());
    wiring.view.working_universe = BTreeSet::from([InstrumentId::new("AAPL").unwrap()]);
    let mut process = RuntimeProcess::start(rig.clone(), wiring, &config(), at(NOW));
    for second in 1..4 {
        tick(&rig, NOW + second);
        process.step(at(NOW + second));
    }
    assert_eq!(
        process.lifecycle().state(),
        State::Ready,
        "{:?}",
        kinds(&rig)
    );
    let rows = agent_rows(&rig);
    let decision = rows
        .iter()
        .find(|row| row.event_type == "DecisionMade")
        .unwrap_or_else(|| panic!("the opening was decided: {:?}", kinds(&rig)));
    assert_eq!(decision.schema_version, 3);
    let body = parse(&decision.body).unwrap();
    let named: Vec<&str> = body
        .get("payload")
        .and_then(|payload| payload.get("deferred_to_binding"))
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(named, vec!["halt", "liquidity_floor", "participation"]);
    assert_eq!(count(&rig, "ApprovalRequested"), 1, "{:?}", kinds(&rig));
    assert_eq!(count(&rig, "IntentProposed"), 0, "{:?}", kinds(&rig));
    let request = rows
        .iter()
        .find(|row| row.event_type == "ApprovalRequested")
        .unwrap();
    assert_eq!(
        request.schema_version, 3,
        "the request names the decision's deferrals (DEC-2515)"
    );
    let content = parse(&request.body).unwrap();
    let asked: Vec<&str> = content
        .get("payload")
        .and_then(|payload| payload.get("content"))
        .and_then(|content| content.get("deferred_to_binding"))
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(asked, named, "rule 292: exactly the decision's");
}

const RESEARCH_SPY: &str = "b28f4066-5c6d-479b-a2af-85dc1a8f16fb";

/// An admitted thesis on SPY (journal spec §9.4) as of `as_of`, expiring an hour later at
/// `expires_at`, appended to the agent stream through the journal's own append as the research
/// writer would leave it. It takes the stream's writer epoch, fencing any process before it.
fn thesis_at(rig: &Rig, as_of: &str, expires_at: &str) {
    thesis_on(
        rig,
        ("01J8Z3MTHE5000000000000001", "01J8Z3MTHE5000000000000002"),
        RESEARCH_SPY,
        (as_of, expires_at),
    );
}

/// As [`thesis_at`], for the thesis `thesis` recorded as the event `event` on `instrument`.
fn thesis_on(
    rig: &Rig,
    (thesis, event): (&str, &str),
    instrument: &str,
    (as_of, expires_at): (&str, &str),
) {
    let stream = StreamId::parse(AGENT_STREAM).unwrap();
    let mut journal = rig.journal.clone();
    let head = journal.rows(&stream).unwrap().len();
    let epoch = journal.take_ownership(&stream).unwrap();
    let hash = Digest::of(b"x").to_hex();
    let payload = parse(format!(
        r#"{{"model_id":"llm.research_agent","model_version":"0.1.0","content_hash":"sha256:{hash}","thesis_id":"{thesis}","lineage_id":"{thesis}","revision":0,"predecessor_thesis_id":null,"autopsy_ref":null,"instrument_id":"{instrument}","asset_class":"us_equity","direction":"long","as_of":"{as_of}","expires_at":"{expires_at}","horizon_s":3600,"conviction":"0.6","confidence":"0.7","evidence_ref":null,"evidence_sources":["src.newswire"],"corroboration":"independent_source","invalidation":"a guidance cut","allowlist_version":3,"prompt_ref":"sha256:{hash}","response_ref":"sha256:{hash}","admitted":true,"reason":null}}"#
    ).as_bytes())
    .unwrap();
    let mut refs = Object::new();
    refs.insert(
        Key::new("model_version").unwrap(),
        Value::Str(format!("sha256:{hash}")),
    );
    refs.insert(
        Key::new("mandate_version").unwrap(),
        Value::Str(format!(
            "sha256:{}",
            Digest::of(b"research mandate").to_hex()
        )),
    );
    let bytes = mandate_store::envelope::draft_bytes(
        &mandate_store::envelope::Envelope {
            stream: AGENT_STREAM,
            environment: Environment::Paper,
            actor_kind: "agent",
            actor_id: AGENT,
            build: "mandate-daemon/test",
            version: "0.1.0",
            event_time: at(NOW - 3_600),
        },
        &mandate_store::envelope::Fields {
            event_id: event,
            event_type: "ThesisProposed",
            schema_version: 1,
            causation_id: None,
            correlation_id: Some("01J8Z3MRVN0000000000000001"),
            config_refs: &refs,
            payload: &payload,
        },
    )
    .unwrap();
    let outcome = journal.append(
        &stream,
        u64::try_from(head).unwrap(),
        epoch,
        at(NOW - 3_600),
        &[bytes],
    );
    assert!(
        matches!(outcome, AppendOutcome::Committed(_)),
        "the journal takes the thesis: {outcome:?}"
    );
}

/// The demo's research deployment over the journal's own append, holding 3 SPY from its own filled
/// buy under a thesis that expires at `expires_at`, ticked three times from `NOW`; the process is
/// still ready. The first start only opens the stream, so the thesis lands after `StreamOpened`.
fn held_spy_under_a_thesis_expiring_at(expires_at: i64) -> Rig {
    let rig = Rig::default();
    established(&rig, CONNECTION, ACCOUNT_REF, 2);
    reconciled(&rig, ACCOUNT);
    let document = mandate_canon::parse(
        &std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../deploy/cell/demo/research-mandate.json"
        ))
        .unwrap(),
    )
    .unwrap();
    let version = format!("sha256:{}", Digest::of(b"research mandate").to_hex());
    let wire = || {
        mandate_daemon::production::wired(
            fixture::wiring("user-owner", AGENT_STREAM),
            &version,
            &document,
            (None, mandate_daemon::bars::BarsReader::default()),
            CLOCK,
            AGENT,
        )
    };
    let seq = u64::try_from(rig.control.borrow().len()).unwrap() + 1;
    rig.control.borrow_mut().push(row(
        CONTROL,
        seq,
        "AgentDeployed",
        "user",
        &format!(r#"{{"agent_id":"{AGENT}","mandate_version":"{version}"}}"#),
    ));
    drop(RuntimeProcess::start(
        rig.clone(),
        wire(),
        &config(),
        at(NOW),
    ));
    thesis_at(
        &rig,
        &at(expires_at - 3_600).to_string(),
        &at(expires_at).to_string(),
    );
    on_account(
        &rig,
        ACCOUNT,
        "AccountSnapshotRecorded",
        &format!(
            r#"{{"status":"ACTIVE","crypto_status":"INACTIVE","trading_blocked":false,"account_blocked":false,"trade_suspended_by_user":false,"multiplier":1,"equity":"100000","cash":"100000","buying_power":"100000","non_marginable_buying_power":"100000","risk_clock":"{}"}}"#,
            at(NOW)
        ),
    );
    on_account(
        &rig,
        ACCOUNT,
        "MarkUpdated",
        &format!(
            r#"{{"instrument_id":"{RESEARCH_SPY}","price":"100.05","source":"quote","feed":"fixture","bid":"99.95","sane":true,"session":"regular","risk_clock":"{}"}}"#,
            at(NOW)
        ),
    );
    on_account(
        &rig,
        ACCOUNT,
        "OrderRequestRecorded",
        &format!(
            r#"{{"agent_id":"{AGENT}","purpose":"open","risk_clock":"{}"}}"#,
            at(NOW)
        ),
    );
    on_account(
        &rig,
        ACCOUNT,
        "OrderSubmitted",
        &format!(r#"{{"client_order_id":"o1","instrument_id":"{RESEARCH_SPY}","side":"buy"}}"#),
    );
    on_account(
        &rig,
        ACCOUNT,
        "FillApplied",
        &format!(
            r#"{{"fill_id":"f1","client_order_id":"o1","instrument_id":"{RESEARCH_SPY}","side":"buy","qty_gross":"3","price":"99.95","risk_clock":"{}"}}"#,
            at(NOW)
        ),
    );
    on_account(
        &rig,
        ACCOUNT,
        "OrderStateChanged",
        r#"{"client_order_id":"o1","state":"filled","replaced_by":null,"ignored":false}"#,
    );
    reconciled(&rig, ACCOUNT);
    let mut process = RuntimeProcess::start(rig.clone(), wire(), &config(), at(NOW));
    for second in 1..4 {
        tick(&rig, NOW + second);
        process.step(at(NOW + second));
    }
    assert_eq!(
        process.lifecycle().state(),
        State::Ready,
        "{:?}",
        kinds(&rig)
    );
    rig
}

fn member<'a>(payload: &'a Value, name: &str) -> Option<&'a str> {
    payload.get(name).and_then(Value::as_str)
}

/// D1's ask on #2551, end to end over the journal's own append (journal spec rule 8, DEC-2391): a
/// held instrument whose thesis expired is sold whole at the bid by the deployed plan. The
/// `DecisionMade`, `exit_origin` `removed_instrument` with no convictions, score, outputs, weights
/// or clips, decided AUTO by the built-in risk-reducing rule, and the `IntentProposed` it causes
/// commit, once, with no ask.
#[test]
fn a_held_instrument_whose_thesis_ended_is_proposed_for_sale_through_the_journal() {
    let rig = held_spy_under_a_thesis_expiring_at(NOW - 60);
    assert_eq!(count(&rig, "DecisionMade"), 1, "{:?}", kinds(&rig));
    assert_eq!(count(&rig, "IntentProposed"), 1, "{:?}", kinds(&rig));
    assert_eq!(count(&rig, "ApprovalRequested"), 0, "{:?}", kinds(&rig));
    let rows = agent_rows(&rig);
    let row = |event_type: &str| {
        rows.iter()
            .find(|row| row.event_type == event_type)
            .unwrap_or_else(|| panic!("{event_type}: {:?}", kinds(&rig)))
    };
    let (decision, intent) = (row("DecisionMade"), row("IntentProposed"));
    let payload = |row: &StoredEvent| parse(&row.body).unwrap().get("payload").cloned().unwrap();
    let decided = payload(decision);
    for (name, want) in [
        ("instrument_id", RESEARCH_SPY),
        ("side", "sell"),
        ("qty", "3"),
        ("limit_price", "99.95"),
        ("purpose", "discretionary_exit"),
        ("exit_origin", "removed_instrument"),
        ("dry_run", "allow"),
        ("autonomy", "auto"),
        ("decided_by", "builtin_risk_reducing"),
    ] {
        assert_eq!(member(&decided, name), Some(want), "{name}: {decided:?}");
    }
    for name in ["exit_conviction", "buy_conviction", "combined_score"] {
        assert_eq!(decided.get(name), Some(&Value::Null), "{name}");
    }
    for name in ["outputs_used", "model_weights", "clips_applied"] {
        assert_eq!(
            decided
                .get(name)
                .and_then(Value::as_array)
                .map(<[Value]>::len),
            Some(0),
            "{name}"
        );
    }
    let proposed = payload(intent);
    for (name, want) in [
        ("instrument_id", RESEARCH_SPY),
        ("side", "sell"),
        ("qty", "3"),
        ("limit_price", "99.95"),
        ("purpose", "discretionary_exit"),
    ] {
        assert_eq!(member(&proposed, name), Some(want), "{name}: {proposed:?}");
    }
    assert_eq!(causation_of(intent), Some(decision.event_id.clone()));
}

/// The control: the same holding under a thesis still current decides and proposes nothing.
#[test]
fn a_held_instrument_whose_thesis_is_current_is_not_sold() {
    let rig = held_spy_under_a_thesis_expiring_at(NOW + 3_600);
    assert_eq!(count(&rig, "ThesisProposed"), 1, "{:?}", kinds(&rig));
    assert_eq!(count(&rig, "DecisionMade"), 0, "{:?}", kinds(&rig));
    assert_eq!(count(&rig, "IntentProposed"), 0, "{:?}", kinds(&rig));
}

/// The control stream's `ConnectionEstablished` of `CONNECTION` that `user` made, as the workspace
/// services journal it, and its `ConnectionRevoked`.
fn owned(rig: &Rig, user: &str) {
    let seq = u64::try_from(rig.control.borrow().len()).unwrap() + 1;
    let payload = format!(
        r#"{{"connection_id":"{CONNECTION}","broker":"alpaca","environment":"paper","scopes":[],"account_ref":"{ACCOUNT_REF}","user":"{user}"}}"#
    );
    rig.control.borrow_mut().push(versioned(
        CONTROL,
        seq,
        "ConnectionEstablished",
        2,
        "user",
        &payload,
    ));
}

fn revoked(rig: &Rig) {
    let seq = u64::try_from(rig.control.borrow().len()).unwrap() + 1;
    let payload = format!(r#"{{"connection_id":"{CONNECTION}","reason":"owner_revoked"}}"#);
    rig.control
        .borrow_mut()
        .push(row(CONTROL, seq, "ConnectionRevoked", "user", &payload));
}

fn approvers_of(process: &RuntimeProcess<Rig>) -> Vec<&str> {
    process
        .view()
        .approval
        .approvers
        .iter()
        .map(String::as_str)
        .collect()
}

/// Backlog `E21-30-daemon-approvers-reread` (DEC-2818): every control pass narrows the view's
/// approvers to those the stream still resolves to, before it hands the pass's records to the core,
/// so a connection revoked while the process runs leaves no one to approve until the stream
/// resolves its owner again, never beyond the set admitted at the start; the fixture wiring,
/// which has no source, keeps its view's approvers.
#[test]
fn a_connection_revoked_while_running_leaves_no_approver() {
    let rig = Rig::default();
    owned(&rig, "user-owner");
    let mut wiring = wiring();
    wiring.approvers = Some(ApproverSource::new(
        ["user-owner".to_owned()].into(),
        CONNECTION,
        &["user:user-owner"],
    ));
    let mut process = RuntimeProcess::start(rig.clone(), wiring, &config(), at(NOW));
    assert_eq!(approvers_of(&process), ["user-owner"]);
    tick(&rig, NOW + 1);
    process.step(at(NOW + 1));
    assert_eq!(
        approvers_of(&process),
        ["user-owner"],
        "unchanged rows keep them"
    );
    revoked(&rig);
    tick(&rig, NOW + 2);
    process.step(at(NOW + 2));
    assert_eq!(
        approvers_of(&process),
        Vec::<&str>::new(),
        "{:?}",
        kinds(&rig)
    );
    owned(&rig, "user-owner");
    tick(&rig, NOW + 3);
    process.step(at(NOW + 3));
    assert!(
        approvers_of(&process)
            .iter()
            .all(|user| *user == "user-owner"),
        "nobody outside the admitted set is added"
    );

    let fixture = Rig::default();
    owned(&fixture, "user-owner");
    let mut kept = start(&fixture, NOW);
    revoked(&fixture);
    tick(&fixture, NOW + 1);
    kept.step(at(NOW + 1));
    assert_eq!(
        approvers_of(&kept),
        ["user-owner"],
        "no source keeps the view"
    );
}

/// The owner's grant of the runtime's one pending request, as the CLI commits it to the control
/// stream at `at`, with step-up evidence from the same second.
fn owner_grants_pending(rig: &Rig, at: i64) {
    let rows = agent_rows(rig);
    let request = rows
        .iter()
        .find(|row| row.event_type == "ApprovalRequested")
        .unwrap_or_else(|| panic!("the opening was asked: {:?}", kinds(rig)));
    let body = parse(&request.body).unwrap();
    let hash = body
        .get("payload")
        .and_then(|payload| payload.get("content_hash"))
        .and_then(Value::as_str)
        .unwrap()
        .to_owned();
    let seq = u64::try_from(rig.control.borrow().len()).unwrap() + 1;
    let payload = format!(
        r#"{{"agent":"{AGENT}","approval":"{}","verdict":"approved","content_hash":"{hash}","submitted_at":{at},"step_up":{{"assertion_id":"assertion-{seq}","authenticated_at":{at},"method":"cli_confirm"}},"responder":"user-owner","role":"approver"}}"#,
        request.event_id
    );
    rig.control.borrow_mut().push(row(
        CONTROL,
        seq,
        "ApprovalResponseSubmitted",
        "user",
        &payload,
    ));
}

/// What the runtime judged the owner's grant of a deferring opening, when the connection was or
/// was not revoked in the same control pass the grant arrives in.
fn judged(revoke: bool) -> (String, String) {
    let rig = Rig::default();
    owned(&rig, "user-owner");
    reconciled(&rig, ACCOUNT);
    on_account(
        &rig,
        ACCOUNT,
        "MarkUpdated",
        &format!(
            r#"{{"instrument_id":"AAPL","price":"155","source":"fixture","feed":"sip","risk_clock":{NOW}}}"#
        ),
    );
    let mut wiring = fixture::wiring("user-owner", AGENT_STREAM);
    wiring.gate = Box::new(Deferring);
    wiring.plan = Box::new(Opening);
    wiring.view.version = format!("sha256:{}", Digest::of(b"mandate-daemon fixture").to_hex());
    wiring.view.working_universe = BTreeSet::from([InstrumentId::new("AAPL").unwrap()]);
    wiring.approvers = Some(ApproverSource::new(
        ["user-owner".to_owned()].into(),
        CONNECTION,
        &["user:user-owner"],
    ));
    let mut process = RuntimeProcess::start(rig.clone(), wiring, &config(), at(NOW));
    for second in 1..4 {
        tick(&rig, NOW + second);
        process.step(at(NOW + second));
    }
    assert_eq!(count(&rig, "ApprovalRequested"), 1, "{:?}", kinds(&rig));
    if revoke {
        revoked(&rig);
    }
    owner_grants_pending(&rig, NOW + 4);
    tick(&rig, NOW + 4);
    process.step(at(NOW + 4));
    let rows = agent_rows(&rig);
    let responded = rows
        .iter()
        .find(|row| row.event_type == "ApprovalResponded")
        .unwrap_or_else(|| panic!("the grant was judged: {:?}", kinds(&rig)));
    let body = parse(&responded.body).unwrap();
    let member = |name: &str| {
        body.get("payload")
            .and_then(|payload| payload.get(name))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    (member("result"), member("reason"))
}

/// DEC-2818 item 3, end to end: a grant in the same control pass as the revocation of its
/// approver's connection is judged against the narrowed set and refused, while the same grant with
/// no revocation is admitted, so narrowing after the pass's records are handed would fail here.
#[test]
fn a_grant_in_the_pass_that_revokes_its_approver_is_refused() {
    assert_eq!(judged(false).0, "admitted");
    assert_eq!(
        judged(true),
        ("refused".to_owned(), "not_an_approver".to_owned())
    );
}

/// The rule set the gate reads, the tracer's reviewed configuration, as the fold resolves it from
/// the control stream's registration.
fn gate_rules() -> mandate_risk::GateConfig {
    let rules = include_str!("../../mandate-shell/tests/fixtures/tracer/config/rule-set.json");
    mandate_gate_context::gate_rule_set(&parse(rules.trim().as_bytes()).unwrap()).unwrap()
}

/// The content hash the demo envelope pins `quant.ma_crossover` 1.0.0 at.
const DEMO_MODEL_HASH: &str = "cf36ebf8746a95c79443a2a0b5b49762809e082bced8a0a5bfdda7a3dd6c469d";

/// 2026-10-27T15:00:00Z, a Tuesday inside the regular session, with Monday's close behind it: the
/// window the demo's crossover can decide in (the output of a daily close is fresh for a day, and
/// an opening trades only in the regular session).
const IN_SESSION: i64 = 1_793_113_200;

/// The demo's own envelope, wired as the deployed runtime wires it, over the journaled facts an
/// in-session tick finds.
fn demo_in_session() -> (Rig, RuntimeProcess<Rig>) {
    let rig = Rig::default();
    established(&rig, CONNECTION, ACCOUNT_REF, 2);
    reconciled(&rig, ACCOUNT);
    on_account(
        &rig,
        ACCOUNT,
        "AccountSnapshotRecorded",
        &format!(
            r#"{{"status":"ACTIVE","crypto_status":"INACTIVE","trading_blocked":false,"account_blocked":false,"trade_suspended_by_user":false,"multiplier":1,"equity":"100000","cash":"100000","buying_power":"100000","non_marginable_buying_power":"100000","risk_clock":"{}"}}"#,
            at(IN_SESSION)
        ),
    );
    on_account(
        &rig,
        ACCOUNT,
        "MarkUpdated",
        &format!(
            r#"{{"instrument_id":"{SPY}","price":"580.05","source":"quote","feed":"fixture","bid":"579.95","sane":true,"session":"regular","risk_clock":"{}"}}"#,
            at(IN_SESSION)
        ),
    );
    let document = parse(
        &std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../deploy/cell/demo/mandate.json"
        ))
        .unwrap(),
    )
    .unwrap();
    let version = format!("sha256:{}", Digest::of(b"demo mandate").to_hex());
    let seq = u64::try_from(rig.control.borrow().len()).unwrap() + 1;
    rig.control.borrow_mut().push(row(
        CONTROL,
        seq,
        "AgentDeployed",
        "user",
        &format!(r#"{{"agent_id":"{AGENT}","mandate_version":"{version}"}}"#),
    ));
    let seq = u64::try_from(rig.control.borrow().len()).unwrap() + 1;
    rig.control.borrow_mut().push(row(
        CONTROL,
        seq,
        "ConfigSnapshotRegistered",
        "user",
        &format!(
            r#"{{"kind":"model_version","content_hash":"sha256:{DEMO_MODEL_HASH}","model_id":"quant.ma_crossover","model_version":"1.0.0","params":[],"admits_instruments":false}}"#
        ),
    ));
    let mut wiring = mandate_daemon::production::wired(
        fixture::wiring("user-owner", AGENT_STREAM),
        &version,
        &document,
        (
            Some(gate_rules()),
            mandate_daemon::bars::BarsReader::default(),
        ),
        CLOCK,
        AGENT,
    );
    wiring.models = Some(ModelsWiring {
        models: Box::new(Host {
            answer: Rc::new(RefCell::new(vec![Evaluated {
                output: ModelOutput {
                    content_hash: Digest::from_hex(DEMO_MODEL_HASH).unwrap(),
                    ..daily(IN_SESSION - 60).output
                },
                ..daily(IN_SESSION - 60)
            }])),
            ..Host::default()
        }),
        artifacts: Box::new(Closes::default()),
    });
    let process = RuntimeProcess::start(rig.clone(), wiring, &config(), at(IN_SESSION));
    (rig, process)
}

/// The owner's grant of `approval`, as the workspace services journal it on the control stream: the
/// members of `mandate-runtime`'s own grant fixture (`tests/common/escalation.rs`, `Answer::grant`),
/// which the core's approval tests read, so the two stay in step.
fn granted(rig: &Rig, approval: &str, content_hash: &str) {
    let seq = u64::try_from(rig.control.borrow().len()).unwrap() + 1;
    let payload = format!(
        r#"{{"agent":"{AGENT}","approval":"{approval}","verdict":"approved","content_hash":"{content_hash}","submitted_at":{IN_SESSION},"step_up":{{"assertion_id":"assertion-1","authenticated_at":{IN_SESSION},"method":"cli_confirm"}},"responder":"user-owner","role":"approver"}}"#
    );
    rig.control.borrow_mut().push(row(
        CONTROL,
        seq,
        "ApprovalResponseSubmitted",
        "user",
        &payload,
    ));
}

/// The demo's whole in-session loop on its own envelope and caps, over the journal's own append:
/// an in-session tick decides, the gate's dry run allows with its deferrals, the owner is asked,
/// and the owner's grant proposes the opening to the executor. The window itself is why the loop
/// cannot run on a Monday (the crossover's output from Friday's close has expired), so this test
/// is the in-session proof the cell cannot give outside it.
#[test]
fn the_demo_envelope_decides_asks_and_proposes_once_granted_in_session() {
    let (rig, mut process) = demo_in_session();
    for second in 1..4 {
        tick(&rig, IN_SESSION + second);
        process.step(at(IN_SESSION + second));
    }
    assert_eq!(count(&rig, "DecisionMade"), 1, "{:?}", kinds(&rig));
    assert_eq!(count(&rig, "ApprovalRequested"), 1, "{:?}", kinds(&rig));
    assert_eq!(count(&rig, "IntentProposed"), 0, "{:?}", kinds(&rig));
    let rows = agent_rows(&rig);
    let asked = rows
        .iter()
        .find(|row| row.event_type == "ApprovalRequested")
        .unwrap();
    let body = parse(&asked.body).unwrap();
    let payload = body.get("payload").unwrap();
    let hash = payload
        .get("content_hash")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("the request names its content hash: {payload:?}"));
    granted(&rig, asked.event_id.as_str(), hash);
    tick(&rig, IN_SESSION + 4);
    process.step(at(IN_SESSION + 4));
    assert_eq!(count(&rig, "ApprovalResponded"), 1, "{:?}", kinds(&rig));
    assert_eq!(count(&rig, "IntentProposed"), 1, "{:?}", kinds(&rig));
    let rows = agent_rows(&rig);
    let intent = rows
        .iter()
        .find(|row| row.event_type == "IntentProposed")
        .unwrap();
    let body = parse(&intent.body).unwrap();
    let payload = body.get("payload").unwrap();
    assert_eq!(
        payload.get("instrument_id").and_then(Value::as_str),
        Some(SPY),
        "the envelope's own instrument: {payload:?}"
    );
    assert_eq!(
        payload.get("side").and_then(Value::as_str),
        Some("buy"),
        "{payload:?}"
    );
    assert_eq!(
        process.lifecycle().state(),
        State::Ready,
        "{:?}",
        kinds(&rig)
    );
}

/// The research envelope's content hash for `llm.research_agent` 0.1.0.
const RESEARCH_MODEL_HASH: &str =
    "bff754a2e669add9a7928a4b8c71c764cfe61455e361621ae6f6c262af0824cd";

/// The demo's research deployment, an unpinned universe, wired as the deployed runtime wires it,
/// over the journaled facts an in-session tick finds: both of its models registered, an admitted
/// thesis on SPY current for the hour, and both models' `Long` on SPY, the research model's from
/// that thesis (conviction 0.6), journaled first, as the cell's replay of 2026-10-27 journals
/// them. The first start only opens
/// the agent stream, so the thesis lands after `StreamOpened`.
fn research_in_session() -> (Rig, RuntimeProcess<Rig>) {
    research_in_session_with(|_| {})
}

/// As [`research_in_session`], with `more` laid down after SPY's thesis.
fn research_in_session_with(more: impl Fn(&Rig)) -> (Rig, RuntimeProcess<Rig>) {
    let rig = Rig::default();
    established(&rig, CONNECTION, ACCOUNT_REF, 2);
    reconciled(&rig, ACCOUNT);
    on_account(
        &rig,
        ACCOUNT,
        "AccountSnapshotRecorded",
        &format!(
            r#"{{"status":"ACTIVE","crypto_status":"INACTIVE","trading_blocked":false,"account_blocked":false,"trade_suspended_by_user":false,"multiplier":1,"equity":"100000","cash":"100000","buying_power":"100000","non_marginable_buying_power":"100000","risk_clock":"{}"}}"#,
            at(IN_SESSION)
        ),
    );
    on_account(
        &rig,
        ACCOUNT,
        "MarkUpdated",
        &format!(
            r#"{{"instrument_id":"{SPY}","price":"580.05","source":"quote","feed":"fixture","bid":"579.95","sane":true,"session":"regular","risk_clock":"{}"}}"#,
            at(IN_SESSION)
        ),
    );
    let document = parse(
        &std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../deploy/cell/demo/research-mandate.json"
        ))
        .unwrap(),
    )
    .unwrap();
    let version = format!("sha256:{}", Digest::of(b"research mandate").to_hex());
    let seq = u64::try_from(rig.control.borrow().len()).unwrap() + 1;
    rig.control.borrow_mut().push(row(
        CONTROL,
        seq,
        "AgentDeployed",
        "user",
        &format!(r#"{{"agent_id":"{AGENT}","mandate_version":"{version}"}}"#),
    ));
    for (model, model_version, hash, admits) in [
        ("quant.ma_crossover", "1.0.0", DEMO_MODEL_HASH, false),
        ("llm.research_agent", "0.1.0", RESEARCH_MODEL_HASH, true),
    ] {
        let seq = u64::try_from(rig.control.borrow().len()).unwrap() + 1;
        rig.control.borrow_mut().push(row(
            CONTROL,
            seq,
            "ConfigSnapshotRegistered",
            "user",
            &format!(
                r#"{{"kind":"model_version","content_hash":"sha256:{hash}","model_id":"{model}","model_version":"{model_version}","params":[],"admits_instruments":{admits}}}"#
            ),
        ));
    }
    let wire = || {
        let mut wiring = mandate_daemon::production::wired(
            fixture::wiring("user-owner", AGENT_STREAM),
            &version,
            &document,
            (
                Some(gate_rules()),
                mandate_daemon::bars::BarsReader::default(),
            ),
            CLOCK,
            AGENT,
        );
        wiring.models = Some(ModelsWiring {
            models: Box::new(Host {
                answer: Rc::new(RefCell::new(vec![
                    Evaluated {
                        output: ModelOutput {
                            model_id: "llm.research_agent".into(),
                            model_version: "0.1.0".into(),
                            content_hash: Digest::from_hex(RESEARCH_MODEL_HASH).unwrap(),
                            conviction: Conviction::parse("0.6").unwrap(),
                            confidence: Unit::parse("0.6").unwrap(),
                            thesis_id: Some("01J8Z3MTHE5000000000000001".into()),
                            lineage_id: Some("01J8Z3MTHE5000000000000001".into()),
                            ..daily(IN_SESSION - 60).output
                        },
                        ..daily(IN_SESSION - 60)
                    },
                    Evaluated {
                        output: ModelOutput {
                            content_hash: Digest::from_hex(DEMO_MODEL_HASH).unwrap(),
                            ..daily(IN_SESSION - 60).output
                        },
                        ..daily(IN_SESSION - 60)
                    },
                ])),
                ..Host::default()
            }),
            artifacts: Box::new(Closes::default()),
        });
        wiring
    };
    drop(RuntimeProcess::start(
        rig.clone(),
        wire(),
        &config(),
        at(IN_SESSION),
    ));
    thesis_at(
        &rig,
        &at(IN_SESSION - 60).to_string(),
        &at(IN_SESSION + 3_540).to_string(),
    );
    more(&rig);
    reconciled(&rig, ACCOUNT);
    let process = RuntimeProcess::start(rig.clone(), wire(), &config(), at(IN_SESSION));
    (rig, process)
}

/// E21-2, DEC-2827, the cell's research loop end to end (lane GW's replay of 2026-10-27 found it
/// stop here): an unpinned research deployment with an admitted SPY thesis and the crossover's
/// `Long` on SPY decides once in session and asks its owner about the opening, both naming SPY;
/// nothing is proposed to the executor before the grant.
#[test]
fn a_research_deployment_decides_and_asks_about_its_working_universe_in_session() {
    let (rig, mut process) = research_in_session();
    for second in 1..4 {
        tick(&rig, IN_SESSION + second);
        process.step(at(IN_SESSION + second));
    }
    assert_eq!(count(&rig, "ThesisProposed"), 1, "{:?}", kinds(&rig));
    assert_eq!(count(&rig, "DecisionMade"), 1, "{:?}", kinds(&rig));
    assert_eq!(count(&rig, "ApprovalRequested"), 1, "{:?}", kinds(&rig));
    assert_eq!(count(&rig, "IntentProposed"), 0, "{:?}", kinds(&rig));
    let rows = agent_rows(&rig);
    let payload = |event_type: &str| {
        let row = rows
            .iter()
            .find(|row| row.event_type == event_type)
            .unwrap();
        parse(&row.body).unwrap().get("payload").cloned().unwrap()
    };
    let decided = payload("DecisionMade");
    for (name, want) in [
        ("instrument_id", SPY),
        ("side", "buy"),
        ("purpose", "open"),
        ("autonomy", "ask"),
    ] {
        assert_eq!(member(&decided, name), Some(want), "{name}: {decided:?}");
    }
    let asked = payload("ApprovalRequested");
    assert!(
        String::from_utf8(mandate_canon::to_canonical(&asked))
            .unwrap()
            .contains(SPY),
        "the request names SPY: {asked:?}"
    );
}

/// Another US equity, which the research envelope's theses may admit.
const RESEARCH_QQQ: &str = "3a9e1f0c-2b7d-4c55-8e61-0f4d2a6b9c13";

/// DEC-2819 item 2, as the executor opens: with two surviving admitted theses there is no one
/// working instrument, so the research deployment decides nothing in session, though both models'
/// `Long` on SPY are journaled.
#[test]
fn a_research_deployment_with_two_surviving_theses_opens_nothing() {
    let (rig, mut process) = research_in_session_with(|rig| {
        thesis_on(
            rig,
            ("01J8Z3MTHE5000000000000003", "01J8Z3MTHE5000000000000004"),
            RESEARCH_QQQ,
            (
                &at(IN_SESSION - 60).to_string(),
                &at(IN_SESSION + 3_540).to_string(),
            ),
        );
    });
    for second in 1..4 {
        tick(&rig, IN_SESSION + second);
        process.step(at(IN_SESSION + second));
    }
    assert_eq!(count(&rig, "ThesisProposed"), 2, "{:?}", kinds(&rig));
    assert!(count(&rig, "ModelOutputRecorded") >= 2, "{:?}", kinds(&rig));
    assert_eq!(count(&rig, "DecisionMade"), 0, "{:?}", kinds(&rig));
    assert_eq!(count(&rig, "ApprovalRequested"), 0, "{:?}", kinds(&rig));
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// `AGENTS.md` rule 13: a research deployment holding SPY from its own fill under a current thesis
/// has the sale planned on the owner's kill switch, as a pinned deployment's is. The flatten maps
/// every instrument a thesis record admitted, with the class its record names (DEC-2819, as the
/// executor maps exits), so no operator alert `flatten_unplanned` is raised for it. Today it maps
/// only the pinned universe, which a research mandate has none of.
#[test]
fn a_research_deployments_kill_switch_sells_its_held_instrument() {
    let rig = held_spy_under_a_thesis_expiring_at(NOW + 3_600);
    let mut process = RuntimeProcess::start(rig.clone(), research_wired(), &config(), at(NOW + 4));
    process.step(at(NOW + 5));
    kill_switch(&rig);
    let alerts = process.step(at(NOW + 6));
    assert_eq!(count(&rig, "KillSwitchActivated"), 1, "{:?}", kinds(&rig));
    let keys: Vec<&str> = alerts.iter().map(|alert| alert.key).collect();
    assert!(
        !keys.contains(&mandate_daemon::FLATTEN_UNPLANNED),
        "the held SPY is planned: {keys:?}"
    );
    assert_eq!(
        process.lifecycle().state(),
        State::Ready,
        "{:?}",
        kinds(&rig)
    );
}

/// The demo's research deployment as [`held_spy_under_a_thesis_expiring_at`] wires it.
fn research_wired() -> Wiring {
    let document = mandate_canon::parse(
        &std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../deploy/cell/demo/research-mandate.json"
        ))
        .unwrap(),
    )
    .unwrap();
    mandate_daemon::production::wired(
        fixture::wiring("user-owner", AGENT_STREAM),
        &format!("sha256:{}", Digest::of(b"research mandate").to_hex()),
        &document,
        (None, mandate_daemon::bars::BarsReader::default()),
        CLOCK,
        AGENT,
    )
}

/// DEC-2841: the owner's grant of a research deployment's opening is acted on. The runtime
/// revalidates a grant against the view's working universe (`in_working_universe`), which for an
/// unpinned mandate is read again from the agent stream at every pass, so the instrument its
/// single surviving thesis holds is proposed to the executor once granted, never skipped as
/// outside a pinned list it has none of.
#[test]
fn a_research_deployments_granted_opening_is_proposed_on_its_working_instrument() {
    let (rig, mut process) = research_in_session();
    for second in 1..4 {
        tick(&rig, IN_SESSION + second);
        process.step(at(IN_SESSION + second));
    }
    let rows = agent_rows(&rig);
    let asked = rows
        .iter()
        .find(|row| row.event_type == "ApprovalRequested")
        .unwrap_or_else(|| panic!("the opening is asked: {:?}", kinds(&rig)));
    let body = parse(&asked.body).unwrap();
    let hash = body
        .get("payload")
        .and_then(|payload| payload.get("content_hash"))
        .and_then(Value::as_str)
        .unwrap()
        .to_owned();
    granted(&rig, asked.event_id.as_str(), &hash);
    tick(&rig, IN_SESSION + 4);
    process.step(at(IN_SESSION + 4));
    assert_eq!(count(&rig, "ApprovalResponded"), 1, "{:?}", kinds(&rig));
    assert_eq!(
        count(&rig, "IntentProposed"),
        1,
        "the grant is acted on: {:?}",
        kinds(&rig)
    );
    let rows = agent_rows(&rig);
    let intent = rows
        .iter()
        .find(|row| row.event_type == "IntentProposed")
        .unwrap();
    let payload = parse(&intent.body)
        .unwrap()
        .get("payload")
        .cloned()
        .unwrap();
    assert_eq!(member(&payload, "instrument_id"), Some(SPY), "{payload:?}");
    assert_eq!(member(&payload, "side"), Some("buy"), "{payload:?}");
}

/// Mandate spec §5.3's re-entry cooldown over the process (lane DR's rehearsal, 2026-10-27): the
/// agent's position was sold by its owner's exit in the second the tick reads, so the opening the
/// crossover proposes is refused `reentry_cooldown` by the runtime's own dry run, reading the
/// exit fill from the fold as the binding gate does. The owner is never asked to buy back what
/// they just sold.
#[test]
fn an_owner_exit_just_filled_asks_nothing_inside_the_reentry_cooldown() {
    let (rig, mut process) = demo_in_session();
    let now = at(IN_SESSION);
    for (order, side, purpose) in [("o1", "buy", "open"), ("o2", "sell", "owner_exit")] {
        on_account(
            &rig,
            ACCOUNT,
            "OrderRequestRecorded",
            &format!(r#"{{"agent_id":"{AGENT}","purpose":"{purpose}","risk_clock":"{now}"}}"#),
        );
        on_account(
            &rig,
            ACCOUNT,
            "OrderSubmitted",
            &format!(
                r#"{{"client_order_id":"{order}","instrument_id":"{SPY}","side":"{side}","risk_clock":"{now}"}}"#
            ),
        );
        on_account(
            &rig,
            ACCOUNT,
            "FillApplied",
            &format!(
                r#"{{"fill_id":"f-{order}","client_order_id":"{order}","instrument_id":"{SPY}","side":"{side}","qty_gross":"1","price":"579.95","trade_date":"{}","risk_clock":"{now}","fees":[]}}"#,
                now.date()
            ),
        );
        on_account(
            &rig,
            ACCOUNT,
            "OrderStateChanged",
            &format!(
                r#"{{"client_order_id":"{order}","state":"filled","replaced_by":null,"ignored":false,"risk_clock":"{now}"}}"#
            ),
        );
    }
    for second in 1..4 {
        tick(&rig, IN_SESSION + second);
        process.step(at(IN_SESSION + second));
    }
    assert_eq!(count(&rig, "ApprovalRequested"), 0, "{:?}", kinds(&rig));
    assert_eq!(count(&rig, "IntentProposed"), 0, "{:?}", kinds(&rig));
    let rows = agent_rows(&rig);
    let decided = rows
        .iter()
        .find(|row| row.event_type == "DecisionMade")
        .unwrap_or_else(|| panic!("the opening is decided and refused: {:?}", kinds(&rig)));
    let payload = parse(&decided.body)
        .unwrap()
        .get("payload")
        .cloned()
        .unwrap();
    assert_eq!(member(&payload, "dry_run"), Some("deny"), "{payload:?}");
    assert_eq!(
        member(&payload, "reason_code"),
        Some("reentry_cooldown"),
        "{payload:?}"
    );
}

/// [`Rig`] read as a journal that follows each stream from a held head (DEC-2852): its
/// `rows_from` hands only the records at and past the asked seq, and counts every record it hands,
/// so a test sees how much a pass reads.
#[derive(Clone, Default)]
struct Following {
    rig: Rig,
    handed: Rc<Cell<usize>>,
    /// Every read's first sequence number, in order.
    starts: Rc<RefCell<Vec<u64>>>,
    /// A read from the first record refused, as a store refuses a chain broken before the head.
    whole_down: Rc<Cell<bool>>,
}

impl Store for Following {
    fn take_ownership(&mut self, stream: &StreamId) -> Option<u64> {
        self.rig.take_ownership(stream)
    }

    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        let rows = self.rig.rows(stream)?;
        self.handed.set(self.handed.get() + rows.len());
        Some(rows)
    }

    fn rows_from(
        &self,
        stream: &StreamId,
        start: mandate_journal::TrustedStart,
    ) -> Option<Vec<StoredEvent>> {
        self.starts.borrow_mut().push(start.from_seq);
        if self.whole_down.get() && start == mandate_journal::TrustedStart::GENESIS {
            return None;
        }
        let since: Vec<StoredEvent> = self
            .rig
            .rows(stream)?
            .into_iter()
            .filter(|row| row.seq >= start.from_seq)
            .collect();
        self.handed.set(self.handed.get() + since.len());
        Some(since)
    }

    fn append(
        &mut self,
        stream: &StreamId,
        head: u64,
        epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        self.rig.append(stream, head, epoch, recorded_at, drafts)
    }
}

/// DEC-2852: over a long run, a pass reads only what was appended since the last, never its
/// streams' whole length: here the one tick each pass adds.
#[test]
fn a_long_run_reads_only_what_each_pass_appended() {
    let following = Following::default();
    let mut process = RuntimeProcess::start(following.clone(), wiring(), &config(), at(NOW));
    assert_eq!(process.lifecycle().state(), State::Ready);
    for second in 1..=2_000 {
        tick(&following.rig, NOW + second);
        let before = following.handed.get();
        process.step(at(NOW + second));
        let read = following.handed.get() - before;
        assert_read(read, rechecks(1, second), &following, second);
    }
    assert_eq!(process.lifecycle().state(), State::Ready);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// DEC-2852: a process that reads each stream only past its head journals exactly what one
    /// reading every stream whole at every pass journals, whatever the interleaving of kill
    /// switches, ticks, the executor pausing and resuming the agent, lost append answers, steps and
    /// restarts, and folds to the same state. The whole reader is the plain
    /// [`Rig`], whose records follow no chain, so every read of it is read again whole.
    #[test]
    fn reading_past_the_head_journals_what_reading_whole_streams_does(
        ops in proptest::collection::vec((op(), any::<Option<bool>>()), 1..24),
    ) {
        let whole = Rig::default();
        let following = Following::default();
        for rig in [&whole, &following.rig] {
            established(rig, CONNECTION, ACCOUNT_REF, 2);
            reconciled(rig, ACCOUNT);
        }
        let mut second = NOW;
        let mut by_whole = start(&whole, second);
        let mut by_head =
            RuntimeProcess::start(following.clone(), wiring(), &config(), at(second));
        for (op, mode) in ops {
            second += 1;
            for rig in [&whole, &following.rig] {
                if let Some(paused) = mode {
                    let to = if paused { "paused" } else { "normal" };
                    on_account(
                        rig,
                        ACCOUNT,
                        "AgentModeApplied",
                        &format!(r#"{{"agent":"{AGENT}","to":"{to}"}}"#),
                    );
                }
                match op {
                    Op::KillSwitch => kill_switch(rig),
                    Op::Tick => tick(rig, second),
                    Op::LoseAnswer => rig.lose_answers.set(1),
                    Op::Step | Op::Restart => {}
                }
            }
            match op {
                Op::Step => {
                    by_whole.step(at(second));
                    by_head.step(at(second));
                }
                Op::Restart => {
                    by_whole = start(&whole, second);
                    by_head =
                        RuntimeProcess::start(following.clone(), wiring(), &config(), at(second));
                }
                _ => {}
            }
        }
        let bodies = |rig: &Rig| -> Vec<Vec<u8>> {
            agent_rows(rig).into_iter().map(|row| row.body).collect()
        };
        prop_assert_eq!(bodies(&following.rig), bodies(&whole), "{:?}", kinds(&whole));
        prop_assert_eq!(by_head.lifecycle().state(), by_whole.lifecycle().state());
        prop_assert_eq!(by_head.state(), by_whole.state(), "the folded state, its risk clock and mode");
    }
}

/// The production fold's answer for an opening in SPY: `Held::Unreadable` once a row it reads did
/// not read, as journal spec §2's clock rule makes a tick earlier than the last.
fn fold_unreadable(facts: &mandate_daemon::production::Facts) -> bool {
    facts
        .read(|journaled| {
            matches!(
                journaled.facts(SPY, mandate_daemon::production::FRESH),
                Err(mandate_gate_context::Held::Unreadable)
            )
        })
        .unwrap_or(false)
}

/// DEC-2852, #2828's review: a clock record that goes back holds the production fold, as a fold of
/// the whole clock stream at every pass holds it, over a store that reads only past the head, whether the fold followed the account from the
/// start or only after the clock had ticked, so no earlier record is left unfolded.
#[test]
fn a_clock_record_that_goes_back_holds_the_fold_as_a_whole_read_does() {
    for follows_late in [false, true] {
        let following = Following::default();
        let rig = following.rig.clone();
        if !follows_late {
            established(&rig, CONNECTION, ACCOUNT_REF, 2);
        }
        let facts = mandate_daemon::production::Facts::new(CLOCK, AGENT);
        let mut process = RuntimeProcess::start(
            following.clone(),
            Wiring {
                facts: Some(facts.clone()),
                ..wiring()
            },
            &config(),
            at(NOW),
        );
        tick(&rig, NOW + 5);
        tick(&rig, NOW + 3);
        process.step(at(NOW + 6));
        tick(&rig, NOW + 9);
        process.step(at(NOW + 10));
        if follows_late {
            established(&rig, CONNECTION, ACCOUNT_REF, 2);
            tick(&rig, NOW + 11);
            process.step(at(NOW + 12));
        }
        assert!(
            fold_unreadable(&facts),
            "follows late: {follows_late}; the tick at +3 after +5 holds the fold"
        );
    }
}

/// A clock stream that cannot be read is asked once a pass, never again from its first record when
/// no head was ever held.
#[test]
fn an_unreadable_clock_stream_is_asked_once_a_pass() {
    let rig = Rig::default();
    let mut process = start(&rig, NOW);
    rig.clock_down.set(true);
    rig.reads.borrow_mut().clear();
    process.step(at(NOW + 1));
    let asked = rig.reads.borrow().iter().filter(|s| *s == CLOCK).count();
    assert_eq!(asked, 1, "{:?}", rig.reads.borrow());
}

/// DEC-2852: with the production fold wired and following the account, a pass still reads only
/// what was appended since the last: the clock is read whole once, when the fold first follows,
/// and never again.
#[test]
fn a_long_run_with_the_fold_following_reads_only_what_each_pass_appended() {
    let following = Following::default();
    established(&following.rig, CONNECTION, ACCOUNT_REF, 2);
    let mut process = RuntimeProcess::start(
        following.clone(),
        Wiring {
            facts: Some(mandate_daemon::production::Facts::new(CLOCK, AGENT)),
            ..wiring()
        },
        &config(),
        at(NOW),
    );
    tick(&following.rig, NOW + 1);
    process.step(at(NOW + 1));
    for second in 2..=1_000 {
        tick(&following.rig, NOW + second);
        let before = following.handed.get();
        process.step(at(NOW + second));
        let read = following.handed.get() - before;
        assert_read(read, rechecks(1, second), &following, second);
    }
}

/// Whether the pass at `NOW + second` reads every stream again from its first record, the
/// first pass having been at `NOW + first` (DEC-2852 item 6).
fn rechecks(first: i64, second: i64) -> bool {
    second > first && (second - first) % 60 == 0
}

/// A pass reads its own tick and nothing else, unless it is a check, which reads each stream
/// whole: more than its tick, and no more than every record there is.
fn assert_read(read: usize, check: bool, following: &Following, second: i64) {
    if check {
        let all = following.rig.clock.borrow().len() + following.rig.control.borrow().len();
        assert!(
            read > 1 && read <= all + following.rig.accounts.borrow().len() + 64,
            "check pass {second} read {read} records, not each stream whole"
        );
    } else {
        assert!(
            read <= 1,
            "pass {second} read {read} records, not only its own tick"
        );
    }
}

/// DEC-2852 item 6: a record at the held head that no longer reads as it did, as a rewrite
/// that hashes its chain again leaves it, is found by the next check, at most `60` s of passes
/// later, and the process fails rather than fold on (`AGENTS.md` rule 3): the pass that finds it
/// hands no tick. Before the check, the process reads on past the head, as a tail does.
#[test]
fn a_rewritten_head_fails_the_process_at_the_next_check() {
    for stream in [CONTROL, CLOCK, ACCOUNT] {
        let following = Following::default();
        established(&following.rig, CONNECTION, ACCOUNT_REF, 2);
        reconciled(&following.rig, ACCOUNT);
        let mut process = RuntimeProcess::start(following.clone(), wiring(), &config(), at(NOW));
        for second in 1..=10 {
            tick(&following.rig, NOW + second);
            process.step(at(NOW + second));
        }
        rewrite_head(&following.rig, stream);
        for second in 11..=60 {
            process.step(at(NOW + second));
            assert_eq!(
                process.lifecycle().state(),
                State::Ready,
                "{stream} at {second}"
            );
        }
        tick(&following.rig, NOW + 61);
        process.step(at(NOW + 61));
        assert_eq!(
            process.lifecycle().state(),
            State::Exited(Exit::CycleFailed),
            "{stream}"
        );
        assert_eq!(risk_clock(&process), Some(NOW + 10), "{stream}");
    }
}

/// DEC-2852 items 3 and 6: a check reads each stream from its first record and hands on only
/// what follows the head, so the passes across several checks fold and tick exactly as the passes
/// between them do: the risk clock is each pass's own tick, and the kept control stream is the
/// stream, each record once.
#[test]
fn a_check_hands_on_only_what_follows_the_head() {
    let following = Following::default();
    established(&following.rig, CONNECTION, ACCOUNT_REF, 2);
    reconciled(&following.rig, ACCOUNT);
    let (host, closes) = (Host::default(), Closes::default());
    let mut process = RuntimeProcess::start(
        following.clone(),
        Wiring {
            models: Some(ModelsWiring {
                models: Box::new(host.clone()),
                artifacts: Box::new(closes),
            }),
            ..wiring()
        },
        &config(),
        at(NOW),
    );
    for second in 1..=190 {
        if second == 100 {
            kill_switch(&following.rig);
        }
        tick(&following.rig, NOW + second);
        process.step(at(NOW + second));
        assert_eq!(risk_clock(&process), Some(NOW + second), "pass {second}");
        let control: Vec<String> = following
            .rig
            .control
            .borrow()
            .iter()
            .map(|row| row.event_id.clone())
            .collect();
        let handed = host
            .handed
            .borrow()
            .last()
            .cloned()
            .map(|(control, _)| control);
        assert_eq!(handed, Some(Some(control)), "pass {second}");
    }
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// DEC-2852 item 6: a step back in time makes the check due at once, so a clock set back
/// never defers it by the step.
#[test]
fn a_step_back_in_time_makes_the_check_due() {
    let following = Following::default();
    established(&following.rig, CONNECTION, ACCOUNT_REF, 2);
    let mut process = RuntimeProcess::start(following.clone(), wiring(), &config(), at(NOW));
    for second in [1, 61] {
        tick(&following.rig, NOW + second);
        process.step(at(NOW + second));
    }
    rewrite_head(&following.rig, CONTROL);
    process.step(at(NOW + 62));
    assert_eq!(process.lifecycle().state(), State::Ready);
    process.step(at(NOW + 30));
    assert_eq!(
        process.lifecycle().state(),
        State::Exited(Exit::CycleFailed)
    );
}

/// DEC-2852 items 3 and 6: a store that refuses every read past the head, as the plain [`Rig`]
/// does, is read from the first record when a record follows the head, and that read too must
/// hold the head, so a rewritten head fails the process on the pass a record lands after it, with
/// no check due. The record here is the owner's kill switch: the process hands nothing, and the
/// executor's gate holds its own kill path (DEC-2852 item 6).
#[test]
fn a_refused_read_past_the_head_that_no_longer_holds_it_fails_the_process() {
    let rig = Rig::default();
    established(&rig, CONNECTION, ACCOUNT_REF, 2);
    let mut process = RuntimeProcess::start(rig.clone(), wiring(), &config(), at(NOW));
    tick(&rig, NOW + 1);
    process.step(at(NOW + 1));
    assert_eq!(process.lifecycle().state(), State::Ready);
    rewrite_head(&rig, CONTROL);
    kill_switch(&rig);
    tick(&rig, NOW + 2);
    process.step(at(NOW + 2));
    assert_eq!(
        process.lifecycle().state(),
        State::Exited(Exit::CycleFailed)
    );
    assert_eq!(risk_clock(&process), Some(NOW + 1));
}

/// Gives the last record of `stream` in `rig` another hash, as a rewrite that hashes the chain
/// again leaves the record at a held head.
fn rewrite_head(rig: &Rig, stream: &str) {
    let rows = match stream {
        CONTROL => &rig.control,
        CLOCK => &rig.clock,
        _ => &rig.accounts,
    };
    if let Some(head) = rows.borrow_mut().last_mut() {
        head.hash = Digest::of(b"rewritten");
    }
}

/// DEC-2852 item 6: once a check is due, a stream that will not read from its first record,
/// as a chain broken before the head will not, is read only from its first record, so nothing
/// past the head is read on a chain not verified again, and it is asked again each pass until it
/// reads; then the tail reads past its head again.
#[test]
fn a_check_that_will_not_read_reads_nothing_past_the_head_until_it_does() {
    let following = Following::default();
    established(&following.rig, CONNECTION, ACCOUNT_REF, 2);
    reconciled(&following.rig, ACCOUNT);
    let mut process = RuntimeProcess::start(following.clone(), wiring(), &config(), at(NOW));
    for second in 1..=60 {
        tick(&following.rig, NOW + second);
        process.step(at(NOW + second));
    }
    following.whole_down.set(true);
    for second in 61..=70 {
        tick(&following.rig, NOW + second);
        following.starts.borrow_mut().clear();
        process.step(at(NOW + second));
        let starts = following.starts.borrow().clone();
        assert!(
            !starts.is_empty() && starts.iter().all(|&from| from == 1),
            "pass {second} read {starts:?}"
        );
    }
    assert_eq!(process.lifecycle().state(), State::Ready);
    following.whole_down.set(false);
    tick(&following.rig, NOW + 71);
    process.step(at(NOW + 71));
    tick(&following.rig, NOW + 72);
    following.starts.borrow_mut().clear();
    process.step(at(NOW + 72));
    let starts = following.starts.borrow().clone();
    assert!(
        !starts.is_empty() && starts.iter().all(|&from| from > 1),
        "pass 72 read {starts:?}"
    );
    assert_eq!(process.lifecycle().state(), State::Ready);
}

fn modelled_following(
    following: &Following,
    host: &Host,
    closes: &Closes,
) -> RuntimeProcess<Following> {
    RuntimeProcess::start(
        following.clone(),
        Wiring {
            models: Some(ModelsWiring {
                models: Box::new(host.clone()),
                artifacts: Box::new(closes.clone()),
            }),
            ..wiring()
        },
        &config(),
        at(NOW),
    )
}

/// DEC-2852 item 4: over a long run read past the head, with a check every minute and a restart,
/// each distinct output the host answers is recorded once and no other, as a fold of the whole
/// agent stream would record it: the journaled outputs are folded a record at a time. The oracle
/// counts the agent stream's own records.
#[test]
fn a_long_run_records_each_output_once_across_checks_and_a_restart() {
    let following = Following::default();
    let (host, closes) = (Host::default(), Closes::default());
    let mut process = modelled_following(&following, &host, &closes);
    let mut answered = BTreeSet::new();
    for n in 1..=200 {
        if n % 50 == 1 {
            let as_of = NOW - 4 * 3_600 + (n / 50) * 3_600;
            *host.answer.borrow_mut() = vec![daily(as_of)];
            answered.insert(as_of);
        }
        if n == 120 {
            process = modelled_following(&following, &host, &closes);
        }
        tick(&following.rig, NOW + n);
        process.step(at(NOW + n));
    }
    assert_eq!(answered.len(), 4);
    assert_eq!(count(&following.rig, "ModelOutputRecorded"), answered.len());
    assert_eq!(count(&following.rig, "ObservationRecorded"), answered.len());
}

/// DEC-2852 item 4: the host is handed the agent stream's thesis records alone, the only records of
/// it a port reads, and the only ones the process keeps.
#[test]
fn the_host_is_handed_the_agent_streams_thesis_records_alone() {
    let rig = Rig::default();
    let (host, closes) = (Host::default(), Closes::default());
    host.answer.borrow_mut().push(daily(NOW - 3_600));
    let mut process = modelled(&rig, &host, &closes);
    let laid = row(AGENT_STREAM, 90, "ThesisProposed", "runtime", "{}");
    rig.agent_extra.borrow_mut().push(laid.clone());
    tick(&rig, NOW + 5);
    process.step(at(NOW + 6));
    let handed = host.handed.borrow().last().cloned().map(|(_, agent)| agent);
    assert_eq!(handed, Some(vec![laid.event_id]));
    assert!(agent_rows(&rig).len() > 1);
}
