//! The cell's research source (DEC-2214 items 2 to 4): each pass's parts from the recorded bundle,
//! the deployed mandate document, and the control and agent streams, and no context at all when a
//! fact cannot be read. The envelope's own reading is `envelope.rs`'s.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use mandate_canon::{Digest, parse};
use mandate_daemon::ResearchSource;
use mandate_daemon::meter::CellMeter;
use mandate_daemon::recorded::Bundle;
use mandate_daemon::source::FoldError;
use mandate_daemon::source::{CellSource, Rows, SourceFor, envelope, last_research_reservation};
use mandate_gateway::Outcome as CallOutcome;
use mandate_gateway::kinds::{KeyOwner, Purpose};
use mandate_gateway::meter::{MeterRecord, MeterStore, Reservation, Settlement, SharedMeter};
use mandate_gateway::money::{Usage, Usd6};
use mandate_gateway::registry::ModelRef;
use mandate_journal::{AppendOutcome, ArtifactRef, Environment, StoredEvent, StreamId, StreamType};
use mandate_num::Usd;
use mandate_research::{AssetId, LineageId, LineageState, WorkingUniverse};
use mandate_runtime::{
    ActorKind, AgentId, ConnectionId, Deployment, EventId, FoldedEvent, RiskClock, RuntimeState,
    Seq, WorkspaceId, fold,
};
use mandate_spec::document::Mandate;
use mandate_store::Store;
use mandate_time::{Date, UtcNanos};
use proptest::prelude::*;
use serde_json::{Value as Json, json};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/research");
const MANDATE: &str = include_str!("fixtures/mandate.json");
const AGENT: &str = "agent-a";
const AAPL: &str = "7b4a1c2e-1111-4a2b-9c3d-000000000001";
const MSFT: &str = "7b4a1c2e-2222-4a2b-9c3d-000000000002";

/// When every record of a test happens unless it says otherwise: 09:00 in New York on the risk day
/// the state's clock reads.
const EVENT_TIME: &str = "2026-10-16T13:00:00.000000000Z";

/// The state's risk clock: 11:00 in New York, the same risk day as [`EVENT_TIME`].
const RISK_CLOCK: &str = "2026-10-16T15:00:00Z";

/// A record of `stream`, with `payload` at `payload`, at [`EVENT_TIME`].
fn row(stream: &str, seq: u64, event_type: &str, environment: &str, payload: &str) -> StoredEvent {
    row_at(stream, seq, event_type, environment, payload, EVENT_TIME)
}

/// A record of `stream`, with `payload` at `payload`, at `event_time`.
fn row_at(
    stream: &str,
    seq: u64,
    event_type: &str,
    environment: &str,
    payload: &str,
    event_time: &str,
) -> StoredEvent {
    let event_id = format!("01J8Z3MC{seq:018}");
    let body = format!(
        r#"{{"envelope_version":1,"environment":"{environment}","event_id":"{event_id}","stream_id":"{stream}","event_type":"{event_type}","schema_version":1,"event_time":"{event_time}","clock_source":"system","causation_id":null,"correlation_id":null,"actor":{{"kind":"user","id":"user-owner","version":"1","build":null}},"config_refs":{{}},"payload":{payload},"artifact_refs":[],"pii_refs":[]}}"#
    );
    StoredEvent {
        stream_id: stream.into(),
        seq,
        event_id,
        event_type: event_type.into(),
        schema_version: 1,
        environment: environment.into(),
        recorded_at: "2026-10-16T13:00:00.000000000Z".into(),
        prev_hash: Digest::of(b"prev"),
        hash: Digest::of(body.as_bytes()),
        body: body.into_bytes(),
    }
}

/// The workspace's control and agent streams, either of which a test can make unreadable.
#[derive(Clone, Default)]
struct Journal {
    control: Rc<RefCell<Vec<StoredEvent>>>,
    agent: Rc<RefCell<Vec<StoredEvent>>>,
    meter: Rc<RefCell<Vec<StoredEvent>>>,
    unreadable: Rc<RefCell<BTreeSet<String>>>,
}

impl Rows for Journal {
    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        if self.unreadable.borrow().contains(stream.as_str()) {
            return None;
        }
        match stream.as_str() {
            "ctl:ws1" => Some(self.control.borrow().clone()),
            "agent:ws1:agent-a" => Some(self.agent.borrow().clone()),
            "meter:ws1" => Some(self.meter.borrow().clone()),
            _ => None,
        }
    }
}

impl Journal {
    fn control(&self, event_type: &str, environment: &str, payload: &str) {
        let seq = u64::try_from(self.control.borrow().len()).unwrap() + 1;
        self.control
            .borrow_mut()
            .push(row("ctl:ws1", seq, event_type, environment, payload));
    }

    fn agent(&self, event_type: &str, payload: &str) {
        self.agent_at(event_type, payload, EVENT_TIME);
    }

    fn agent_at(&self, event_type: &str, payload: &str, event_time: &str) {
        let seq = u64::try_from(self.agent.borrow().len()).unwrap() + 1;
        self.agent.borrow_mut().push(row_at(
            "agent:ws1:agent-a",
            seq,
            event_type,
            "paper",
            payload,
            event_time,
        ));
    }

    /// A `MeterReserved` on the workspace's meter stream for `agent` (JSON: a quoted id or `null`)
    /// and `purpose`, at `event_time`.
    fn reserved_at(&self, agent: &str, purpose: &str, event_time: &str) {
        let seq = u64::try_from(self.meter.borrow().len()).unwrap() + 1;
        self.meter.borrow_mut().push(row_at(
            "meter:ws1",
            seq,
            "MeterReserved",
            "paper",
            &format!(r#"{{"agent_id":{agent},"purpose":"{purpose}","max_cost_usd":"0.4"}}"#),
            event_time,
        ));
    }

    fn deployed(&self, agent: &str, version: &str, environment: &str) {
        self.control(
            "AgentDeployed",
            environment,
            &format!(
                r#"{{"agent_id":"{agent}","mandate_version":"{version}","record_ref":"{version}"}}"#
            ),
        );
    }

    fn stopped(&self, agent: &str) {
        self.control(
            "AgentStopped",
            "paper",
            &format!(
                r#"{{"agent_id":"{agent}","connection_id":"conn_alpaca_paper_01","reason":"owner_stop","retired_on":"2026-10-16","loss_added":"0"}}"#
            ),
        );
    }
}

/// The fixture mandate made one the research agent may work under: an unpinned universe of up to
/// five instruments, an admitting signal model, and a research envelope.
fn researching() -> Json {
    let mut document: Json = serde_json::from_str(MANDATE).unwrap();
    document["universe"]["pinned"] = json!(false);
    document["universe"]["pinned_instruments"] = json!([]);
    document["universe"]["max_instruments"] = json!(5);
    document["behavior"]["signal_models"][0]["admits_instruments"] = json!(true);
    document["behavior"]["research"] = json!({
        "interval_s": 3600,
        "cost_cap_usd_per_day": "5",
        "max_revisions_per_lineage": 3
    });
    document
}

/// Stores `document` canonically, as the workspace services do, and gives its version.
fn store(documents: &mut BTreeMap<Digest, Vec<u8>>, document: &Json) -> String {
    let mandate = Mandate::parse(&parse(document.to_string().as_bytes()).unwrap()).unwrap();
    let bytes = mandate.canonical_bytes().unwrap();
    let reference = ArtifactRef::of(&bytes);
    documents.insert(reference.digest(), bytes);
    reference.to_string()
}

/// A state with no risk clock yet: no tick or reconciliation folded.
fn unclocked() -> RuntimeState {
    RuntimeState::new(Deployment {
        agent: AgentId(AGENT.into()),
        connection: ConnectionId("conn-1".into()),
        workspace: WorkspaceId("ws1".into()),
    })
}

/// A state whose risk clock reads [`RISK_CLOCK`], from a clean reconciliation of the account.
fn state() -> RuntimeState {
    let mut state = unclocked();
    let secs = UtcNanos::parse_rfc3339(RISK_CLOCK).unwrap().secs();
    let reconciled = FoldedEvent {
        stream: "acct:ws1:acct-1".to_owned(),
        seq: Seq(1),
        event_id: EventId("01J8Z3MR000000000000000099".to_owned()),
        event_type: "ReconciliationRun".to_owned(),
        causation_id: None,
        actor: ActorKind::System,
        payload: parse(format!(r#"{{"result":"clean","risk_clock":{secs}}}"#).as_bytes()).unwrap(),
    };
    fold(&mut state, &reconciled).unwrap();
    assert!(
        state.risk_clock().is_some(),
        "the reconciliation sets the risk clock"
    );
    state
}

fn who() -> SourceFor {
    SourceFor {
        workspace: "ws1".to_owned(),
        agent: AGENT.to_owned(),
        environment: Environment::Paper,
    }
}

/// A journal with `document` deployed for the agent, and the source over `bundle`.
fn deployed(bundle: &Bundle, document: &Json) -> (Journal, CellSource) {
    let journal = Journal::default();
    let mut documents = BTreeMap::new();
    let version = store(&mut documents, document);
    journal.deployed(AGENT, &version, "paper");
    let source = CellSource::new(
        bundle,
        who(),
        Box::new(journal.clone()),
        Box::new(documents),
    );
    (journal, source)
}

fn bundle() -> Bundle {
    Bundle::load(Path::new(FIXTURE)).unwrap()
}

/// A copy of the fixture bundle with `edit` applied to `file`'s JSON.
fn bundle_with(name: &str, file: &str, edit: impl FnOnce(&mut Json)) -> Bundle {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("daemon-{}-source-{name}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    for entry in fs::read_dir(FIXTURE).unwrap() {
        let path = entry.unwrap().path();
        fs::copy(&path, dir.join(path.file_name().unwrap())).unwrap();
    }
    let mut value: Json = serde_json::from_slice(&fs::read(dir.join(file)).unwrap()).unwrap();
    edit(&mut value);
    fs::write(dir.join(file), value.to_string()).unwrap();
    Bundle::load(&dir).unwrap()
}

fn ids(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}

/// A store the source reads through, holding one row on every stream.
struct OneRow;

impl Store for OneRow {
    fn take_ownership(&mut self, _: &StreamId) -> Option<u64> {
        None
    }
    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        Some(vec![row(
            stream.as_str(),
            1,
            "AgentDeployed",
            "paper",
            "{}",
        )])
    }
    fn append(
        &mut self,
        _: &StreamId,
        _: u64,
        _: u64,
        _: UtcNanos,
        _: &[Vec<u8>],
    ) -> AppendOutcome {
        AppendOutcome::Unavailable
    }
}

/// Any store is a source of rows, read as the store reads them.
#[test]
fn a_store_is_read_as_its_own_rows() {
    let stream = StreamId::parse("ctl:ws1").unwrap();
    assert_eq!(Rows::rows(&OneRow, &stream), Store::rows(&OneRow, &stream));
    assert_eq!(Rows::rows(&OneRow, &stream).unwrap().len(), 1);
}

/// On a fresh agent stream, the pass gets the deployed mandate's envelope and the bundle's
/// recorded facts, an empty working universe, and no theses, lineages or reservation.
#[test]
fn a_fresh_agent_gets_its_mandate_and_the_bundle_s_facts() {
    let bundle = bundle();
    let document = researching();
    let (_, source) = deployed(&bundle, &document);
    let parts = source.parts(&state()).unwrap();
    let expected = envelope(&parse(document.to_string().as_bytes()).unwrap()).unwrap();
    assert_eq!(*parts.mandate.envelope(), expected);
    let facts = &parts.facts;
    assert_eq!(facts.allowlist, bundle.allowlist);
    assert_eq!(
        facts
            .data_universe
            .as_ref()
            .unwrap()
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect::<Vec<_>>(),
        ids(&[AAPL, MSFT])
    );
    assert!(facts.eligibility_failures.is_empty());
    assert!(facts.halted_instruments.is_empty());
    assert!(facts.claimed_by_other_agents.is_empty());
    assert!(facts.disclosures_accepted.is_empty());
    assert!(facts.instrument_groups.is_empty());
    assert_eq!(facts.research_spend_usd_today, Usd::parse("0").unwrap());
    assert_eq!(
        parts.universe,
        WorkingUniverse::Known {
            instruments: BTreeSet::new(),
            pinned: false
        }
    );
    assert_eq!(parts.lineages, LineageState::default());
    assert!(parts.active.is_empty());
    assert_eq!(parts.last_reservation, None);
    assert_eq!(
        parts
            .data_symbols
            .iter()
            .map(|s| s.as_str().to_owned())
            .collect::<Vec<_>>(),
        ids(&["AAPL", "MSFT"])
    );
}

/// The bundle's halts and ineligible instruments reach the eligibility floor, by asset.
#[test]
fn the_bundle_s_halts_and_ineligible_instruments_reach_the_floor() {
    let halted = bundle_with("halted", "research.json", |r| {
        r["halted_instruments"] = json!(["MSFT"])
    });
    let (_, source) = deployed(&halted, &researching());
    let facts = source.parts(&state()).unwrap().facts;
    let halted: Vec<_> = facts
        .halted_instruments
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect();
    assert_eq!(halted, ids(&[MSFT]));

    let ineligible = bundle_with("ineligible", "instruments.json", |rows| {
        rows[0]["eligible"] = json!(false)
    });
    let (_, source) = deployed(&ineligible, &researching());
    let facts = source.parts(&state()).unwrap().facts;
    let failures: Vec<_> = facts
        .eligibility_failures
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect();
    assert_eq!(failures, ids(&[AAPL]));
}

/// Without a deployed mandate it can read, the pass has no context: the agent never deployed or
/// since stopped, its document missing, or a stream unreadable.
#[test]
fn no_context_without_a_deployed_mandate_it_can_read() {
    let bundle = bundle();
    let journal = Journal::default();
    let source = CellSource::new(
        &bundle,
        who(),
        Box::new(journal.clone()),
        Box::new(BTreeMap::<Digest, Vec<u8>>::new()),
    );
    assert!(source.parts(&state()).is_none(), "never deployed");
    journal.deployed(AGENT, &format!("sha256:{}", "ab".repeat(32)), "paper");
    assert!(source.parts(&state()).is_none(), "document missing");

    let (journal, source) = deployed(&bundle, &researching());
    journal.stopped(AGENT);
    assert!(source.parts(&state()).is_none(), "stopped");

    let (journal, source) = deployed(&bundle, &researching());
    assert!(source.parts(&state()).is_some());
    for stream in ["ctl:ws1", "agent:ws1:agent-a"] {
        journal.unreadable.borrow_mut().insert(stream.to_owned());
        assert!(source.parts(&state()).is_none(), "{stream} unreadable");
        journal.unreadable.borrow_mut().clear();
    }
}

/// Another agent deployed in the environment may claim instruments this pass cannot see, so the
/// pass has no context until it stops (DEC-2214 item 2).
#[test]
fn no_context_while_another_agent_is_deployed() {
    let (journal, source) = deployed(&bundle(), &researching());
    journal.deployed("agent-b", &format!("sha256:{}", "cd".repeat(32)), "paper");
    assert!(source.parts(&state()).is_none());
    journal.stopped("agent-b");
    assert!(source.parts(&state()).is_some(), "agent-b stopped");
    journal.deployed("agent-b", &format!("sha256:{}", "cd".repeat(32)), "live");
    assert!(
        source.parts(&state()).is_some(),
        "another environment's deployment is not this one's"
    );
}

/// A record the source cannot fold gives the pass no context, so research pauses (DEC-2251): a
/// thesis or an invalidation it cannot read, or a call the meter reserved that the source's own
/// meter does not hold. A run's start and observations do not pause it, so the first run is never
/// held mid-way.
#[test]
fn research_pauses_once_the_stream_holds_a_record_it_cannot_fold() {
    let (journal, source) = deployed(&bundle(), &researching());
    journal.agent("RunStarted", r#"{"run_id":"01RUN"}"#);
    journal.agent("ObservationRecorded", r#"{"source":"src.newswire"}"#);
    assert!(source.parts(&state()).is_some(), "a run under way");

    for kind in ["ThesisProposed", "ThesisRevised", "ThesisInvalidated"] {
        let (journal, source) = deployed(&bundle(), &researching());
        journal.agent(kind, r#"{}"#);
        assert!(source.parts(&state()).is_none(), "{kind}");
    }

    let (journal, source) = deployed(&bundle(), &researching());
    journal.agent(
        "ModelInvocationRecorded",
        r#"{"cost_usd":"0.25","reservation":"01J8Z3MR000000000000000001"}"#,
    );
    assert!(source.parts(&state()).is_none(), "a metered call");
}

/// Today's spend is at most every unmetered call's recorded cost, summed; a cost it cannot read
/// gives no context.
#[test]
fn spend_sums_the_recorded_costs_and_refuses_one_it_cannot_read() {
    let (journal, source) = deployed(&bundle(), &researching());
    for cost in ["0.25", "0.5"] {
        journal.agent(
            "ModelInvocationRecorded",
            &format!(r#"{{"cost_usd":"{cost}","reservation":null}}"#),
        );
    }
    let facts = source.parts(&state()).unwrap().facts;
    assert_eq!(facts.research_spend_usd_today, Usd::parse("0.75").unwrap());
    journal.agent(
        "ModelInvocationRecorded",
        r#"{"cost_usd":"a lot","reservation":null}"#,
    );
    assert!(source.parts(&state()).is_none());
}

/// Today's spend is the risk day's (DEC-2214 items 2 and 3): a cost recorded on an earlier New York
/// day does not count, a cost recorded after the clock's day began counts even when the clock has
/// not caught up with it, and without a risk clock there is no today, so no context.
#[test]
fn spend_counts_only_the_risk_day_s_recorded_costs() {
    let (journal, source) = deployed(&bundle(), &researching());
    journal.agent_at(
        "ModelInvocationRecorded",
        r#"{"cost_usd":"0.25","reservation":null}"#,
        "2026-10-16T03:59:59.000000000Z",
    );
    journal.agent_at(
        "ModelInvocationRecorded",
        r#"{"cost_usd":"0.5","reservation":null}"#,
        "2026-10-16T04:00:00.000000000Z",
    );
    journal.agent_at(
        "ModelInvocationRecorded",
        r#"{"cost_usd":"2","reservation":null}"#,
        "2026-10-17T04:00:00.000000000Z",
    );
    let facts = source.parts(&state()).unwrap().facts;
    assert_eq!(
        facts.research_spend_usd_today,
        Usd::parse("2.5").unwrap(),
        "23:59:59 the evening before is yesterday's; midnight in New York and anything after it, \
         the next midnight included, counts while the clock still reads today"
    );
    assert!(
        source.parts(&unclocked()).is_none(),
        "no risk clock, no today"
    );
}

/// Every cost record is read strictly, an earlier day's too (DEC-2214 item 2): a cost or time it
/// cannot read, a record without a `reservation` member, or a negative cost gives no context.
#[test]
fn every_cost_record_is_read_strictly() {
    let yesterday = "2026-10-15T13:00:00.000000000Z";
    let cases: [(&str, &str, &str); 5] = [
        (
            "an earlier day's unreadable cost",
            r#"{"cost_usd":"a lot","reservation":null}"#,
            yesterday,
        ),
        (
            "an earlier day's unreadable time",
            r#"{"cost_usd":"0.25","reservation":null}"#,
            "yesterday",
        ),
        (
            "no reservation member",
            r#"{"cost_usd":"0.25"}"#,
            EVENT_TIME,
        ),
        (
            "a negative cost",
            r#"{"cost_usd":"-0.25","reservation":null}"#,
            EVENT_TIME,
        ),
        (
            "a negative cost on an earlier day",
            r#"{"cost_usd":"-0.25","reservation":null}"#,
            yesterday,
        ),
    ];
    for (case, payload, event_time) in cases {
        let (journal, source) = deployed(&bundle(), &researching());
        journal.agent_at("ModelInvocationRecorded", payload, event_time);
        assert!(source.parts(&state()).is_none(), "{case}");
    }
    let (journal, source) = deployed(&bundle(), &researching());
    journal.agent(
        "ModelInvocationRecorded",
        r#"{"cost_usd":"0","reservation":null}"#,
    );
    assert!(
        source.parts(&state()).is_some(),
        "a null reservation and a zero cost are read"
    );
}

/// A state whose risk clock reads `text`.
fn state_at(text: &str) -> RuntimeState {
    let mut state = unclocked();
    let secs = UtcNanos::parse_rfc3339(text).unwrap().secs();
    let reconciled = FoldedEvent {
        stream: "acct:ws1:acct-1".to_owned(),
        seq: Seq(1),
        event_id: EventId("01J8Z3MR000000000000000098".to_owned()),
        event_type: "ReconciliationRun".to_owned(),
        causation_id: None,
        actor: ActorKind::System,
        payload: parse(format!(r#"{{"result":"clean","risk_clock":{secs}}}"#).as_bytes()).unwrap(),
    };
    fold(&mut state, &reconciled).unwrap();
    state
}

/// A journal with `document` deployed, and the source over `bundle` counting the agent's spend
/// from a meter over `records` (DEC-2249 item 3).
fn metered(bundle: &Bundle, document: &Json, records: &SharedMeter) -> (Journal, CellSource) {
    let journal = Journal::default();
    let mut documents = BTreeMap::new();
    let version = store(&mut documents, document);
    journal.deployed(AGENT, &version, "paper");
    let clock = || UtcNanos::parse_rfc3339(RISK_CLOCK).unwrap();
    let meter = CellMeter::new(
        records.clone(),
        Rc::new(journal.clone()),
        who(),
        Rc::new(clock),
    );
    let source = CellSource::metered(
        bundle,
        who(),
        Rc::new(journal.clone()),
        Box::new(documents),
        meter,
    );
    (journal, source)
}

/// A research reservation `id` of the agent on 2026-10-16, at most `max`.
fn reservation(id: &str, max: &str) -> MeterRecord {
    MeterRecord::Reserved(Reservation {
        event_id: id.to_owned(),
        call_id: format!("call-{id}"),
        agent_id: Some(AGENT.to_owned()),
        purpose: Purpose::Research,
        model: ModelRef {
            model_id: "llm.research_agent".to_owned(),
            model_version: "0.1.0".to_owned(),
            content_hash: Digest::of(b"entry"),
        },
        endpoint: "openrouter:anthropic".to_owned(),
        key_owner: KeyOwner::Platform,
        price_table_ref: Digest::of(b"prices"),
        max_cost: Usd6::parse(max).unwrap(),
        risk_day: Date::new(2026, 10, 16).unwrap(),
    })
}

/// DEC-2249 item 3: a call the meter reserved no longer pauses research. It counts at what the
/// meter holds for it, its settled cost here, beside the unmetered calls' recorded costs, which
/// still count as before: 0.25 settled plus 0.5 recorded.
#[test]
fn a_call_the_meter_holds_counts_at_its_settled_cost_and_research_goes_on() {
    let store = SharedMeter::new();
    let (journal, source) = metered(&bundle(), &researching(), &store);
    let mut writer = store.clone();
    writer.append(reservation("R1", "0.40")).unwrap();
    writer
        .append(MeterRecord::Settled(Settlement {
            event_id: "S1".to_owned(),
            call_id: "call-R1".to_owned(),
            reservation: "R1".to_owned(),
            outcome: CallOutcome::Ok,
            attempts: 1,
            tokens: Usage {
                input: 100,
                output: 10,
                cached: 0,
            },
            cost: Usd6::parse("0.25").unwrap(),
        }))
        .unwrap();
    journal.agent(
        "ModelInvocationRecorded",
        r#"{"cost_usd":"0.25","reservation":"R1"}"#,
    );
    journal.agent(
        "ModelInvocationRecorded",
        r#"{"cost_usd":"0.5","reservation":null}"#,
    );
    let parts = source.parts(&state()).expect("a held call gives context");
    assert_eq!(
        parts.facts.research_spend_usd_today,
        Usd::parse("0.75").unwrap()
    );
    assert_eq!(parts.last_reservation, None, "DEC-2249 item 3");
}

/// After a restart the meter is empty while the agent stream records a reserved call earlier on
/// the risk day: no context, until the risk clock reaches the next risk day, when the earlier call
/// is another day's and the day's spend starts again from nothing.
#[test]
fn a_restarted_source_gives_no_context_until_the_next_risk_day() {
    let store = SharedMeter::new();
    let (journal, source) = metered(&bundle(), &researching(), &store);
    journal.agent(
        "ModelInvocationRecorded",
        r#"{"cost_usd":"0.25","reservation":"R9"}"#,
    );
    assert!(source.parts(&state()).is_none(), "R9 is not in the meter");
    assert!(
        source.parts(&state_at("2026-10-17T03:59:59Z")).is_none(),
        "the 16th's last second in New York"
    );
    let parts = source
        .parts(&state_at("2026-10-17T04:00:00Z"))
        .expect("the next risk day");
    assert_eq!(
        parts.facts.research_spend_usd_today,
        Usd::parse("0").unwrap()
    );
}

/// A thesis record (journal spec §9.4) with every member the schema names; `reason` is JSON.
fn thesis_payload(
    thesis: &str,
    lineage: &str,
    revision: u32,
    instrument: &str,
    admitted: bool,
    reason: &str,
    expires_at: &str,
) -> String {
    let predecessor = if revision == 0 {
        "null".to_owned()
    } else {
        format!(r#""{lineage}""#)
    };
    format!(
        r#"{{"model_id":"llm.research_agent","model_version":"0.1.0","content_hash":"sha256:{hash}","thesis_id":"{thesis}","lineage_id":"{lineage}","revision":{revision},"predecessor_thesis_id":{predecessor},"autopsy_ref":null,"instrument_id":"{instrument}","asset_class":"us_equity","direction":"long","as_of":"2026-10-16T13:00:00.000000000Z","expires_at":"{expires_at}","horizon_s":3600,"conviction":"0.6","confidence":"0.7","evidence_ref":null,"evidence_sources":["src.newswire"],"corroboration":"independent_source","invalidation":"a guidance cut","allowlist_version":3,"prompt_ref":"sha256:{hash}","response_ref":"sha256:{hash}","admitted":{admitted},"reason":{reason}}}"#,
        hash = Digest::of(b"x").to_hex(),
    )
}

/// `RISK_CLOCK` moved by `secs`, as the journal writes a timestamp.
fn after_clock(secs: i64) -> String {
    UtcNanos::from_parts(
        UtcNanos::parse_rfc3339(RISK_CLOCK).unwrap().secs() + secs,
        0,
    )
    .unwrap()
    .to_string()
}

/// E21-59, DEC-2249 items 1 and 2: the agent's own theses reach the pass. An admitted proposal
/// holds its instrument as the active thesis until its horizon, its lineage is replayed, and a
/// refusal for another reason admits nothing, where before any thesis record paused research.
#[test]
fn the_agent_s_own_theses_reach_the_pass() {
    let (journal, source) = deployed(&bundle(), &researching());
    journal.agent(
        "ThesisProposed",
        &thesis_payload("th-1", "th-1", 0, AAPL, true, "null", &after_clock(3_600)),
    );
    journal.agent(
        "ThesisProposed",
        &thesis_payload(
            "th-2",
            "th-2",
            0,
            MSFT,
            false,
            r#""universe_full""#,
            &after_clock(3_600),
        ),
    );
    let parts = source.parts(&state()).expect("the theses fold");
    assert_eq!(
        parts.universe,
        WorkingUniverse::Known {
            instruments: [AssetId::new(AAPL).unwrap()].into(),
            pinned: false,
        }
    );
    let active: Vec<(&str, &str, i64)> = parts
        .active
        .iter()
        .map(|t| (t.thesis_id.as_str(), t.symbol.as_str(), t.expires_at.secs()))
        .collect();
    let horizon = UtcNanos::parse_rfc3339(&after_clock(3_600)).unwrap().secs();
    assert_eq!(active, [("th-1", "AAPL", horizon)]);
    let lineage = |id: &str| {
        parts
            .lineages
            .lineage(&LineageId::new(id).unwrap())
            .copied()
    };
    assert_eq!(
        lineage("th-1").map(|l| (l.revisions, l.admitted, l.retired)),
        Some((0, 1, false))
    );
    assert_eq!(lineage("th-2").map(|l| l.admitted), Some(0));
    let expired = source
        .parts(&state_at(&after_clock(3_600)))
        .expect("an expired thesis folds too");
    assert!(expired.active.is_empty(), "at its horizon, removed");
}

/// DEC-2249 item 2: `UniverseChanged` is the account stream's (journal spec §9.3), never closed on
/// the agent stream, so the source does not read it there: a stray one pauses nothing.
#[test]
fn a_universe_change_never_reaches_the_agent_stream_and_pauses_nothing() {
    assert!(!mandate_journal::closes(
        "UniverseChanged",
        1,
        StreamType::Agent
    ));
    assert!(mandate_journal::closes(
        "UniverseChanged",
        1,
        StreamType::Account
    ));
    let (journal, source) = deployed(&bundle(), &researching());
    journal.agent("UniverseChanged", r#"{}"#);
    assert!(source.parts(&state()).is_some());
}

/// A pinned envelope researches nothing, so a thesis record under one, admitted or refused, is not
/// a state the core judged into it: no context (DEC-2251).
#[test]
fn a_pinned_envelope_with_a_thesis_record_gives_no_context() {
    let document: Json = serde_json::from_str(MANDATE).unwrap();
    assert_eq!(
        document["universe"]["pinned"],
        json!(true),
        "the fixture pins SPY"
    );
    let (journal, source) = deployed(&bundle(), &document);
    assert!(
        source.parts(&state()).is_some(),
        "pinned, with nothing to fold"
    );
    journal.agent(
        "ThesisProposed",
        &thesis_payload("th-1", "th-1", 0, AAPL, true, "null", &after_clock(3_600)),
    );
    assert!(source.parts(&state()).is_none());
    let (journal, source) = deployed(&bundle(), &document);
    journal.agent(
        "ThesisProposed",
        &thesis_payload(
            "th-2",
            "th-2",
            0,
            AAPL,
            false,
            r#""universe_pinned""#,
            &after_clock(3_600),
        ),
    );
    assert!(
        source.parts(&state()).is_none(),
        "a refused thesis leaves a known lineage and no active one, and still gives no context"
    );
}

/// One generated record of a pass's agent stream and meter.
#[derive(Debug, Clone)]
enum Record {
    /// A thesis of lineage `lineage` at `revision` on AAPL or MSFT, admitted, or refused
    /// `lineage_retired` when `retires`, else `not_in_data_universe`, its horizon `expires`
    /// seconds after the risk clock (negative: already passed).
    Thesis {
        lineage: u8,
        revision: u32,
        msft: bool,
        admitted: bool,
        retires: bool,
        expires: i64,
    },
    /// An invalidation of the `n`th thesis so far, modulo how many there are.
    Invalidated { n: usize },
    /// A reserved call of at most `max` cents, settled at `cost` cents and recorded, or still open.
    Reserved { max: u64, cost: Option<u64> },
    /// A call no reservation backed, recorded at `cost` cents.
    Unmetered { cost: u64 },
}

fn record() -> impl Strategy<Value = Record> {
    prop_oneof![
        4 => (0_u8..3, 0_u32..4, any::<bool>(), any::<bool>(), any::<bool>(), -3_600_i64..7_200)
            .prop_map(|(lineage, revision, msft, admitted, retires, expires)| Record::Thesis {
                lineage,
                revision,
                msft,
                admitted,
                retires,
                expires,
            }),
        1 => (0_usize..16).prop_map(|n| Record::Invalidated { n }),
        2 => (1_u64..200, prop::option::of(0_u64..200)).prop_map(|(max, cost)| Record::Reserved {
            max,
            cost: cost.map(|c| c.min(max)),
        }),
        1 => (0_u64..200).prop_map(|cost| Record::Unmetered { cost }),
    ]
}

/// `cents` as the canonical decimal text `Usd::parse` and `Usd6::parse` read.
fn dollars(cents: u64) -> String {
    match (cents / 100, cents % 100) {
        (whole, 0) => whole.to_string(),
        (whole, part) if part % 10 == 0 => format!("{whole}.{}", part / 10),
        (whole, part) => format!("{whole}.{part:02}"),
    }
}

/// An instrument's entry in the accumulator: its thesis, lineage, revision, horizon in seconds
/// after the risk clock, and whether it was invalidated.
type Entry = (String, String, u32, i64, bool);

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// E21-59 over random journaled runs: the pass the source gives, its universe, lineages, active
    /// theses and today's research spend, equals an accumulator rebuilt from the generated records
    /// alone. Spend is every settled call at its cost, every open reservation at its maximum, and
    /// every unmetered call at its recorded cost (DEC-2249 item 3).
    #[test]
    fn the_pass_equals_an_independent_accumulator_over_random_runs(
        records in prop::collection::vec(record(), 1..24),
    ) {
        let store = SharedMeter::new();
        let (journal, source) = metered(&bundle(), &researching(), &store);
        let mut writer = store.clone();
        let mut lineages: BTreeMap<String, (u32, u32, bool)> = BTreeMap::new();
        let mut holders: BTreeMap<String, &str> = BTreeMap::new();
        let mut entries: BTreeMap<&str, Entry> = BTreeMap::new();
        let mut theses: Vec<(String, String, &str)> = Vec::new();
        let mut spend_cents = 0_u64;
        for (index, record) in records.iter().enumerate() {
            match *record {
                Record::Thesis { lineage, revision, msft, admitted, retires, expires } => {
                    let thesis = format!("th-{index}");
                    let lineage = format!("ln-{lineage}");
                    let instrument = if msft { MSFT } else { AAPL };
                    let kind = if revision == 0 { "ThesisProposed" } else { "ThesisRevised" };
                    let reason = match (admitted, retires) {
                        (true, _) => "null",
                        (false, true) => r#""lineage_retired""#,
                        (false, false) => r#""not_in_data_universe""#,
                    };
                    journal.agent(kind, &thesis_payload(
                        &thesis, &lineage, revision, instrument, admitted, reason,
                        &after_clock(expires),
                    ));
                    let entry = lineages.entry(lineage.clone()).or_default();
                    if admitted {
                        entry.0 = entry.0.max(revision);
                        entry.1 += 1;
                        holders.retain(|_, held| *held != instrument);
                        holders.insert(lineage.clone(), instrument);
                        entries.insert(instrument, (thesis.clone(), lineage.clone(), revision, expires, false));
                    } else if retires {
                        entry.2 = true;
                    }
                    theses.push((thesis, lineage, instrument));
                }
                Record::Invalidated { n } => {
                    let Some((thesis, lineage, instrument)) = theses.get(n % theses.len().max(1)).cloned() else {
                        continue;
                    };
                    journal.agent("ThesisInvalidated", &format!(
                        r#"{{"thesis_id":"{thesis}","lineage_id":"{lineage}","instrument_id":"{instrument}","cause":"model","candidate":0,"evidence":["01OBS"],"source":"src.newswire"}}"#
                    ));
                    for entry in entries.values_mut() {
                        if entry.0 == thesis {
                            entry.4 = true;
                        }
                    }
                }
                Record::Reserved { max, cost } => {
                    let id = format!("R{index}");
                    writer.append(reservation(&id, &dollars(max))).unwrap();
                    match cost {
                        Some(cost) => {
                            writer.append(MeterRecord::Settled(Settlement {
                                event_id: format!("S{index}"),
                                call_id: format!("call-{id}"),
                                reservation: id.clone(),
                                outcome: CallOutcome::Ok,
                                attempts: 1,
                                tokens: Usage { input: 1, output: 1, cached: 0 },
                                cost: Usd6::parse(&dollars(cost)).unwrap(),
                            })).unwrap();
                            journal.agent("ModelInvocationRecorded", &format!(
                                r#"{{"cost_usd":"{}","reservation":"{id}"}}"#, dollars(cost)
                            ));
                            spend_cents += cost;
                        }
                        None => spend_cents += max,
                    }
                }
                Record::Unmetered { cost } => {
                    journal.agent("ModelInvocationRecorded", &format!(
                        r#"{{"cost_usd":"{}","reservation":null}}"#, dollars(cost)
                    ));
                    spend_cents += cost;
                }
            }
        }
        let parts = source.parts(&state()).expect("every generated record folds");
        let survivors: Vec<(&&str, &Entry)> = entries
            .iter()
            .filter(|(_, (_, lineage, _, expires, invalidated))| {
                !invalidated && !lineages[lineage].2 && 0 < *expires
            })
            .collect();
        prop_assert_eq!(
            &parts.universe,
            &WorkingUniverse::Known {
                instruments: survivors.iter().map(|(i, _)| AssetId::new(i).unwrap()).collect(),
                pinned: false,
            }
        );
        let active: Vec<(String, String, u32, String)> = parts
            .active
            .iter()
            .map(|t| (t.thesis_id.clone(), t.lineage_id.clone(), t.revision, t.instrument.as_str().to_owned()))
            .collect();
        let expected: Vec<(String, String, u32, String)> = survivors
            .iter()
            .map(|(i, (t, l, r, _, _))| (t.clone(), l.clone(), *r, (**i).to_owned()))
            .collect();
        prop_assert_eq!(active, expected);
        for (lineage, (revisions, admitted, retired)) in &lineages {
            let folded = parts.lineages.lineage(&LineageId::new(lineage).unwrap()).copied();
            prop_assert_eq!(
                folded.map(|l| (l.revisions, l.admitted, l.retired)),
                Some((*revisions, *admitted, *retired)),
                "{}", lineage
            );
        }
        for (lineage, instrument) in &holders {
            prop_assert_eq!(
                parts.lineages.holder_of(&LineageId::new(lineage).unwrap()),
                Some(&AssetId::new(instrument).unwrap())
            );
        }
        prop_assert_eq!(
            parts.facts.research_spend_usd_today,
            Usd::parse(&dollars(spend_cents)).unwrap()
        );
    }
}

/// DEC-2259 item 3: the last reservation is the `event_time` of the agent's latest research
/// `MeterReserved` on the meter stream, passing over other agents', the compiler's and other
/// purposes'; `None` before the first; and a reservation whose agent, purpose or time cannot be
/// read gives no answer, naming the member.
#[test]
fn the_last_research_reservation_is_the_agent_s_latest_on_the_meter_stream() {
    let secs = |text: &str| RiskClock::from_secs(UtcNanos::parse_rfc3339(text).unwrap().secs());
    assert_eq!(last_research_reservation(&[], AGENT), Ok(None));
    let journal = Journal::default();
    journal.reserved_at(r#""agent-a""#, "research", "2026-10-16T13:00:00.000000000Z");
    journal.reserved_at(r#""agent-a""#, "research", "2026-10-16T14:10:00.000000000Z");
    journal.reserved_at(r#""agent-b""#, "research", "2026-10-16T14:20:00.000000000Z");
    journal.reserved_at(
        r#""agent-a""#,
        "llm_signal",
        "2026-10-16T14:30:00.000000000Z",
    );
    journal.reserved_at("null", "compiler", "2026-10-16T14:40:00.000000000Z");
    let rows = journal.meter.borrow().clone();
    assert_eq!(
        last_research_reservation(&rows, AGENT),
        Ok(Some(secs("2026-10-16T14:10:00Z")))
    );
    assert_eq!(last_research_reservation(&rows, "agent-c"), Ok(None));
    for (member, from, to) in [
        ("agent_id", r#""agent_id":"agent-a","#, ""),
        ("purpose", r#""purpose":"research""#, r#""purpose":7"#),
        (
            "event_time",
            r#""event_time":"2026-10-16T13:00:00.000000000Z""#,
            r#""event_time":"soon""#,
        ),
    ] {
        let mut bad = rows.clone();
        bad[0].body = String::from_utf8(bad[0].body.clone())
            .unwrap()
            .replace(from, to)
            .into_bytes();
        assert_eq!(
            last_research_reservation(&bad, AGENT),
            Err(FoldError::Unreadable { member }),
            "{member}"
        );
    }
}

/// DEC-2259 item 3, wired: the pass carries the agent's last research reservation from the
/// workspace's meter stream, and gives no context while that stream cannot be read.
#[test]
fn the_pass_carries_the_last_research_reservation_from_the_meter_stream() {
    let (journal, source) = deployed(&bundle(), &researching());
    assert_eq!(source.parts(&state()).unwrap().last_reservation, None);
    journal.reserved_at(r#""agent-a""#, "research", "2026-10-16T14:10:00.000000000Z");
    assert_eq!(
        source.parts(&state()).unwrap().last_reservation,
        Some(RiskClock::from_secs(
            UtcNanos::parse_rfc3339("2026-10-16T14:10:00Z")
                .unwrap()
                .secs()
        ))
    );
    journal
        .unreadable
        .borrow_mut()
        .insert("meter:ws1".to_owned());
    assert!(source.parts(&state()).is_none());
}
