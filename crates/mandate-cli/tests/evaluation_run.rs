//! `mandate evaluation run` (DEC-2244 items 1 and 5): the forward-paper evaluation of an internal
//! paper workspace's thesis records, stored and journaled as `EvaluationRunRecorded` by the
//! `evaluator` system. The workspace holds the `research_equity` reference base as its stored
//! mandate, one deployed agent whose stream holds theses that pass the record check, and the
//! hand-calculated inputs `mandate-evaluation`'s own tests decode.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{AGENT, CONTROL, Journal, agent_stream, at, owner, stream};
use mandate_canon::{Digest, Int, Key, Object, Value, parse, to_canonical};
use mandate_cli::control::{ControlError, ControlJournal};
use mandate_cli::evaluation::{Evaluated, Request, run};
use mandate_cli::evaluation::{internal_paper, thesis_records};
use mandate_evaluation::InputFiles;
use mandate_journal::{
    AppendOutcome, ArtifactRef, ArtifactStore, Draft, Head, StoredEvent, StreamId, get_artifact,
};
use mandate_spec::Mandate;
use mandate_time::UtcNanos;

type Store = BTreeMap<Digest, Vec<u8>>;

const NOW: i64 = 1_790_000_000;
const CUT_OFF: &str = "2026-09-24T00:00:00Z";
const RULE_SET: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const DECISION: &str = include_str!("../../mandate-evaluation/tests/fixtures/inputs/decision.json");
const CLOSES: &str = include_str!("../../mandate-evaluation/tests/fixtures/inputs/closes.json");
const COST: &str = include_str!("../../mandate-evaluation/tests/fixtures/inputs/cost.json");
const BASELINES: &str =
    include_str!("../../mandate-evaluation/tests/fixtures/inputs/baselines.json");
const ENVELOPE: &str = r#"{"actor":{"build":null,"id":"@A","kind":"system","version":"0.1.0"},"artifact_refs":[],"causation_id":null,"clock_source":"local","config_refs":@C,"correlation_id":null,"envelope_version":1,"environment":"paper","event_id":"@I","event_time":"2026-10-08T13:00:00.000000000Z","event_type":"@T","payload":@P,"pii_refs":[],"schema_version":1,"stream_id":"@S"}"#;

fn instant(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
}

fn refused(reason: &'static str) -> Result<Evaluated, ControlError> {
    Err(ControlError::Refused { reason })
}

fn text(value: &Value, name: &str) -> String {
    value
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{name}"))
        .to_owned()
}

/// A §9.4 payload that passes `check_thesis_record` against the `research_equity` base: as of
/// Monday 2026-09-21 14:00Z, its horizon `expires_at`.
fn thesis(id: &str, expires_at: &str) -> String {
    let as_of = instant("2026-09-21T14:00:00Z");
    let expires = instant(expires_at);
    let horizon = u64::try_from(expires.secs() - as_of.secs()).unwrap();
    let s = |v: &str| Value::Str(v.to_owned());
    let digest = |c: char| s(&format!("sha256:{}", c.to_string().repeat(64)));
    let members = [
        ("model_id", s("llm.research_agent")),
        ("model_version", s("0.1.0")),
        ("content_hash", digest('4')),
        ("thesis_id", s(id)),
        ("lineage_id", s(&format!("ln_{id}"))),
        ("revision", Value::Int(Int::new(0).unwrap())),
        ("predecessor_thesis_id", Value::Null),
        ("autopsy_ref", Value::Null),
        ("instrument_id", s("7b4a1c2e-5555-4a2b-9c3d-000000000005")),
        ("asset_class", s("us_equity")),
        ("direction", s("long")),
        ("as_of", s(&as_of.to_string())),
        ("expires_at", s(&expires.to_string())),
        ("horizon_s", Value::Int(Int::new(horizon).unwrap())),
        ("conviction", s("0.7")),
        ("confidence", s("0.8")),
        ("evidence_ref", Value::Null),
        ("evidence_sources", Value::Array(vec![s("src.filings")])),
        ("corroboration", s("market_data")),
        ("invalidation", s("Guidance is cut.")),
        ("allowlist_version", Value::Int(Int::new(1).unwrap())),
        ("prompt_ref", digest('a')),
        ("response_ref", digest('b')),
        ("admitted", Value::Bool(true)),
        ("reason", Value::Null),
    ];
    let mut out = Object::new();
    for (name, value) in members {
        out.insert(Key::new(name).unwrap(), value);
    }
    String::from_utf8(to_canonical(&Value::Object(out))).unwrap()
}

/// An internal paper workspace: its control stream, one deployed agent, the stored mandate, and
/// the agent's two theses, one resolved by the cut-off and one still open.
struct World {
    journal: Journal,
    store: Store,
    version: String,
    setting: bool,
}

impl World {
    fn new() -> Self {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/refcases/mandate.json")).unwrap();
        let document = fixture.pointer("/bases/research_equity/mandate").unwrap();
        let value = parse(&serde_json::to_vec(document).unwrap()).unwrap();
        let canonical = Mandate::parse(&value).unwrap().canonical().unwrap();
        let mut store = Store::new();
        let version = store
            .put_artifact(&to_canonical(&canonical))
            .unwrap()
            .to_string();
        let mut world = Self {
            journal: Journal::default(),
            store,
            version,
            setting: true,
        };
        world.seed(
            CONTROL,
            "StreamOpened",
            r#"{"stream_type":"control","workspace_id":"ws1"}"#,
            "{}",
        );
        let deployed = format!(
            r#"{{"agent_id":"{AGENT}","mandate_version":"{}"}}"#,
            world.version
        );
        world.seed(CONTROL, "AgentDeployed", &deployed, "{}");
        let rule_set = format!(
            r#"{{"admits_instruments":null,"content_hash":"{RULE_SET}","kind":"rule_set","model_id":null,"model_version":null,"params":[]}}"#
        );
        world.seed(CONTROL, "ConfigSnapshotRegistered", &rule_set, "{}");
        world.thesis("th_1", "2026-09-23T21:00:00Z");
        world.thesis("th_2", "2026-09-30T21:00:00Z");
        world
    }

    fn thesis(&mut self, id: &str, expires_at: &str) {
        let refs = format!(r#"{{"mandate_version":"{}"}}"#, self.version);
        let payload = thesis(id, expires_at);
        self.seed(&agent_stream(AGENT), "ThesisProposed", &payload, &refs);
    }

    /// Appends one event as its stream's writer would; each stream's event ids are its own.
    fn seed(&mut self, on: &str, event_type: &str, payload: &str, config_refs: &str) {
        let target = stream(on);
        let head = self.journal.head(&target).unwrap().seq;
        let tag = if on == CONTROL { 9 } else { 8 };
        let draft = ENVELOPE
            .replace("@A", "control_services")
            .replace("@C", config_refs)
            .replace("@I", &format!("{tag}{head:025}"))
            .replace("@T", event_type)
            .replace("@P", payload)
            .replace("@S", on);
        let epoch = self.journal.take_ownership(&target).unwrap();
        let outcome = self
            .journal
            .append(&target, head, epoch, at(NOW).at, &[draft.as_bytes()]);
        assert!(matches!(outcome, Ok(AppendOutcome::Committed(_))));
    }

    fn run_with(
        &mut self,
        files: InputFiles<'_>,
        id: &str,
        cut_off: &str,
    ) -> Result<Evaluated, ControlError> {
        let request = Request {
            files,
            evaluation_id: id,
            cut_off: instant(cut_off),
        };
        run(
            &mut self.journal,
            &mut self.store,
            &owner(),
            self.setting,
            &request,
            at(NOW),
        )
    }

    fn run(&mut self, id: &str, cut_off: &str) -> Result<Evaluated, ControlError> {
        self.run_with(files(), id, cut_off)
    }

    fn rows(&self) -> Vec<mandate_journal::StoredEvent> {
        self.journal.rows(&stream(CONTROL)).unwrap()
    }
}

fn files() -> InputFiles<'static> {
    InputFiles {
        decision: DECISION.as_bytes(),
        closes: CLOSES.as_bytes(),
        cost: COST.as_bytes(),
        baselines: BASELINES.as_bytes(),
    }
}

fn stored(store: &Store, reference: &str) -> Value {
    let reference = ArtifactRef::parse(reference).unwrap_or_else(|| panic!("{reference}"));
    parse(&get_artifact(store, &reference).unwrap_or_else(|e| panic!("{e:?}"))).unwrap()
}

/// DEC-2244 item 5: one record, appended as the `evaluator` system and admitted by the journal's
/// own draft check (§9.18, rule 145, DEC-1141). Every reference it names resolves in the store;
/// the report's inputs are the stored objects; the range is the agent stream's first and last
/// thesis record, and the head its last record's hash. A re-run finds the record.
#[test]
fn an_evaluation_is_stored_and_recorded_once_as_the_evaluator() {
    let mut world = World::new();
    let before = world.rows().len();
    let evaluated = world.run("ev_1", CUT_OFF).unwrap();
    let rows = world.rows();
    assert_eq!(rows.len(), before + 1);
    let last = rows.last().unwrap();
    let draft = Draft::parse(&last.body).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        (draft.event_type(), draft.schema_version()),
        ("EvaluationRunRecorded", 1)
    );
    let envelope = parse(&last.body).unwrap();
    let actor = envelope.get("actor").unwrap();
    assert_eq!(
        (text(actor, "kind"), text(actor, "id")),
        ("system".to_owned(), "evaluator".to_owned())
    );
    let payload = envelope.get("payload").unwrap();
    assert_eq!(text(payload, "evaluation_id"), "ev_1");
    assert_eq!(text(payload, "decision"), "DEC-122");
    assert_eq!(text(payload, "report"), evaluated.report);
    let bound = envelope.get("config_refs").unwrap();
    assert_eq!(text(bound, "rule_set"), RULE_SET);

    let report = stored(&world.store, &evaluated.report);
    assert_eq!(text(&report, "kind"), "forward_paper_evaluation");
    let inputs = report.get("inputs").unwrap();
    let mut named = BTreeSet::new();
    for member in ["theses", "closes", "cost_model"] {
        assert_eq!(text(payload, member), text(inputs, member), "{member}");
        let reference = text(payload, member);
        let object = stored(&world.store, &reference);
        assert_eq!(
            ArtifactRef::of(&to_canonical(&object)).to_string(),
            reference
        );
        named.insert(reference);
    }
    named.insert(text(payload, "code_build"));
    named.insert(text(payload, "report"));
    let build = ArtifactRef::parse(&text(payload, "code_build")).unwrap();
    assert!(
        get_artifact(&world.store, &build)
            .unwrap()
            .starts_with(b"mandate-cli/")
    );
    let refs: Vec<String> = envelope
        .get("artifact_refs")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .map(|r| r.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(refs, named.into_iter().collect::<Vec<_>>());

    let theses = world.journal.rows(&stream(&agent_stream(AGENT))).unwrap();
    let position = |seq: u64| Value::Str(format!("{}:{seq}", agent_stream(AGENT)));
    assert_eq!(
        payload.get("theses_from"),
        Some(&Value::Array(vec![position(1)]))
    );
    assert_eq!(
        payload.get("theses_to"),
        Some(&Value::Array(vec![position(2)]))
    );
    assert_eq!(
        payload.get("theses_head"),
        Some(&Value::Array(vec![Value::Str(theses[1].hash.to_hex())]))
    );
    assert_eq!(payload.get("passed"), Some(&Value::Bool(evaluated.passed)));

    assert_eq!(world.run("ev_1", CUT_OFF), Ok(evaluated));
    assert_eq!(world.rows().len(), before + 1);
}

/// An id another record holds, with another evaluation's payload, refuses; nothing is appended.
#[test]
fn a_taken_id_refuses() {
    let mut world = World::new();
    world.run("ev_1", CUT_OFF).unwrap();
    let rows = world.rows().len();
    assert_eq!(
        world.run("ev_1", "2026-09-25T00:00:00Z"),
        refused("evaluation_id_taken")
    );
    assert_eq!(world.rows().len(), rows);
    assert!(world.run("ev_2", "2026-09-25T00:00:00Z").is_ok());
    assert_eq!(world.rows().len(), rows + 1);
}

/// Outside an internal paper workspace, or with an id the API cannot name, or inputs that do not
/// decode, the run refuses having stored and appended nothing.
#[test]
fn a_refused_run_stores_and_appends_nothing() {
    let mut world = World::new();
    world.setting = false;
    let (rows, objects) = (world.rows().len(), world.store.len());
    assert_eq!(world.run("ev_1", CUT_OFF), refused("not_internal_paper"));
    world.setting = true;
    for bad in ["", "ev 1", "ev/1", &"e".repeat(65)] {
        assert_eq!(
            world.run(bad, CUT_OFF),
            refused("evaluation_id_invalid"),
            "{bad}"
        );
    }
    let unreadable = InputFiles {
        cost: b"{not json",
        ..files()
    };
    assert_eq!(
        world.run_with(unreadable, "ev_1", CUT_OFF),
        refused("input_invalid")
    );
    assert_eq!((world.rows().len(), world.store.len()), (rows, objects));
    assert!(world.run(&"e".repeat(64), CUT_OFF).is_ok());
}

/// A control stream with no registered rule set has nothing for the record to bind (journal spec
/// §9.18 requires `config_refs.rule_set`), so the run refuses having stored and appended nothing.
#[test]
fn a_stream_with_no_rule_set_refuses() {
    let mut world = World::new();
    world.journal = Journal::default();
    world.seed(
        CONTROL,
        "StreamOpened",
        r#"{"stream_type":"control","workspace_id":"ws1"}"#,
        "{}",
    );
    let deployed = format!(
        r#"{{"agent_id":"{AGENT}","mandate_version":"{}"}}"#,
        world.version
    );
    world.seed(CONTROL, "AgentDeployed", &deployed, "{}");
    world.thesis("th_1", "2026-09-23T21:00:00Z");
    let objects = world.store.len();
    assert_eq!(world.run("ev_1", CUT_OFF), refused("rule_set_missing"));
    assert_eq!((world.rows().len(), world.store.len()), (2, objects));
}

/// An evaluation the evaluator refuses as a whole is refused by its own code, with nothing stored
/// or appended: here a thesis bound to a version the store does not hold fails its record check.
#[test]
fn the_evaluators_refusal_is_the_runs() {
    let mut world = World::new();
    world.version =
        "sha256:3333333333333333333333333333333333333333333333333333333333333333".to_owned();
    world.thesis("th_3", "2026-09-23T21:00:00Z");
    let (rows, objects) = (world.rows().len(), world.store.len());
    assert_eq!(world.run("ev_1", CUT_OFF), refused("record_rejected"));
    assert_eq!((world.rows().len(), world.store.len()), (rows, objects));
}

/// A journal that cannot read one stream: every other call is the inner journal's.
struct Unreadable {
    inner: Journal,
    stream: String,
}

impl ControlJournal for Unreadable {
    fn rows(&self, stream: &StreamId) -> Result<Vec<StoredEvent>, ControlError> {
        if stream.as_str() == self.stream {
            return Err(ControlError::Journal("unreadable".to_owned()));
        }
        self.inner.rows(stream)
    }

    fn head(&self, stream: &StreamId) -> Result<Head, ControlError> {
        self.inner.head(stream)
    }

    fn take_ownership(&mut self, stream: &StreamId) -> Result<u64, ControlError> {
        self.inner.take_ownership(stream)
    }

    fn append(
        &mut self,
        stream: &StreamId,
        expected_head: u64,
        writer_epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[&[u8]],
    ) -> Result<AppendOutcome, ControlError> {
        self.inner
            .append(stream, expected_head, writer_epoch, recorded_at, drafts)
    }

    fn wait(&mut self, delay: std::time::Duration) {
        self.inner.wait(delay);
    }
}

fn unreadable(world: World, stream: &str) -> Unreadable {
    Unreadable {
        inner: world.journal,
        stream: stream.to_owned(),
    }
}

/// A control or agent stream that cannot be read refuses the reads, never admits: the gate and
/// the thesis records each answer the journal's error.
#[test]
fn an_unreadable_stream_refuses_and_never_admits() {
    let control = unreadable(World::new(), CONTROL);
    assert!(matches!(
        internal_paper(&control, &owner(), true),
        Err(ControlError::Journal(_))
    ));
    assert!(matches!(
        thesis_records(&control, &owner()),
        Err(ControlError::Journal(_))
    ));
    let agent = unreadable(World::new(), &agent_stream(AGENT));
    assert_eq!(internal_paper(&agent, &owner(), true), Ok(()));
    assert!(matches!(
        thesis_records(&agent, &owner()),
        Err(ControlError::Journal(_))
    ));
}

/// The internal paper gate runs before any agent stream is read (DEC-2244 item 4): with the
/// setting off, a run whose agent stream cannot be read refuses `not_internal_paper`, not the
/// journal's error; with it on, the same run answers the journal's error.
#[test]
#[ignore = "pending E17-8"]
fn the_gate_runs_before_any_agent_stream_is_read() {
    let mut world = World::new();
    let mut store = std::mem::take(&mut world.store);
    let mut journal = unreadable(world, &agent_stream(AGENT));
    let request = Request {
        files: files(),
        evaluation_id: "ev_1",
        cut_off: instant(CUT_OFF),
    };
    for (setting, expected) in [(false, "not_internal_paper"), (true, "")] {
        let answer = run(
            &mut journal,
            &mut store,
            &owner(),
            setting,
            &request,
            at(NOW),
        );
        if expected.is_empty() {
            assert!(
                matches!(answer, Err(ControlError::Journal(_))),
                "{answer:?}"
            );
        } else {
            assert_eq!(answer, refused(expected));
        }
    }
}
