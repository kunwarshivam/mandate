//! The runtime as a long-running process (E21-30 slice 1, DEC-1012): the agent stream on the real
//! in-memory journal, which validates every draft the process writes, and the control and clock
//! streams, and the account stream (DEC-1014), as rows a test lays down, as their writers (the
//! workspace services, the scheduler, and the account's executor) would have committed them.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Duration;

use mandate_accounting::InstrumentId;
use mandate_canon::{Digest, Key, Object, Value, parse};
use mandate_daemon::models::{Evaluated, Models};
use mandate_daemon::{ArtifactSink, Config, ModelsWiring, Refs, RuntimeProcess, Wiring, fixture};
use mandate_journal::{AppendOutcome, Environment, StoredEvent, StreamId};
use mandate_num::{Conviction, Unit};
use mandate_process::{Exit, State};
use mandate_runtime::{
    AgentId, ApprovalSettings, Autonomy, Classified, ConnectionId, Deployment, DryRunVerdict,
    EventDraft, EventId, FlattenPlan, FlattenPlanner, FlattenRequest, GateDryRun, IdGen,
    MandateView, Mode, ModelDirection, ModelOutput, OrderPlan, Proposal, RiskClock, Seq,
    SignalInputs, WorkspaceId, WriterEpoch,
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

/// The model host as a test lays it down: the evaluations it answers at any tick, and how many
/// times it was asked.
#[derive(Clone, Default)]
struct Host {
    answer: Rc<RefCell<Vec<Evaluated>>>,
    asked: Rc<Cell<u32>>,
}
impl Models for Host {
    fn evaluate(&self, _: RiskClock) -> Vec<Evaluated> {
        self.asked.set(self.asked.get() + 1);
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

const SPY: &str = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415";

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
    tick(&rig, NOW + 11);
    process.step(at(NOW + 12));
    assert_eq!(
        count(&rig, "ModelOutputRecorded"),
        1,
        "once readable, the output is recorded"
    );
    assert_eq!(risk_clock(&process), Some(NOW + 11));
}
