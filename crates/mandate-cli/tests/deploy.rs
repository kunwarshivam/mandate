//! D2b (E10-16, DEC-530 item 9): `agent deploy` stores its record and commits one `AgentDeployed`
//! for the workspace's latest confirmed version. The control stream is seeded here, record by
//! record, in journal spec §9.2's shapes, so these tests read no other command's output. Payloads,
//! records and codes are written out from the vectors and DEC-530; the round trip folds the stream
//! with the functions DEC-505 item 1 names, since `mandate-cli` may not depend on `mandate-shell`.
//! Journal spec v0.54 §9.24 (DEC-1171): the deployment is version 2, naming the document's
//! connection, exactly when rule 201 holds on the control stream, and version 1 otherwise; and an
//! agent id is deployed once (DEC-1173 item 1).

mod common;

use std::collections::{BTreeMap, BTreeSet};

use Do::{As, Confirmed, Corrupt, Created, Deploy, Lose, Seed, SeedV2};
use common::{AGENT, CONTROL, FixedIds, Journal, OTHER_AGENT, OWNER, at, owner, stream};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_cli::control::{ControlError, ControlJournal, Owner, Submitted};
use mandate_cli::deploy::deploy;
use mandate_journal::{AppendOutcome, ArtifactError, ArtifactRef, ArtifactSource};
use mandate_journal::{ArtifactStore, Draft, Environment};
use mandate_spec::context::{AgentId, ContextArgs, JournaledFact, Membership};
use mandate_spec::{Mandate, ValidationContext, Violation, validate::validate};
use mandate_time::Date;

type Store = BTreeMap<Digest, Vec<u8>>;

const NOW: i64 = 1_790_000_000;
const MANDATE: &str = include_str!("fixtures/spy_mandate.json");
const SPY: &str = r#"{"asset_class":"us_equity","etp":"plain","etp_classified_at":"2026-10-05T13:30:00Z","etp_source":"nasdaq_trader_symbol_directory","exchange":"arca","increment":"whole","instrument_id":"b28f4066-5c6d-479b-a2af-85dc1a8f16fb","symbol":"SPY"}"#;
const MODEL: &str = r#"{"admits_instruments":false,"content_hash":"sha256:4f3559229f89b27c04b43ec773b0ff0b884895362622964b78dfd791d67e18fc","kind":"model_version","model_id":"quant.ma_crossover","model_version":"1.0.0","params":["fast_periods","slow_periods"]}"#;
const SNAPSHOT: &str = r#"{"admits_instruments":null,"content_hash":"@H","kind":"instrument_snapshot","model_id":null,"model_version":null,"params":[]}"#;
const SEEDED: &str = r#"{"actor":{"build":null,"id":"control_services","kind":"system","version":"0.1.0"},"artifact_refs":[],"causation_id":null,"clock_source":"local","config_refs":{},"correlation_id":null,"envelope_version":1,"environment":"paper","event_id":"@I","event_time":"2026-10-08T13:00:00.000000000Z","event_type":"@T","payload":@P,"pii_refs":[],"schema_version":1,"stream_id":"ctl:ws1"}"#;
const DEPLOYED: &str = r#"{"agent_id":"agent-a","mandate_version":"@V","record_ref":"@R"}"#;
const RECORD: &str = r#"{"agent_id":"agent-a","kind":"deployment_record","mandate_version":"@V","step_up":{"assertion_id":"cli-assertion-1","authenticated_at":1790000000,"method":"cli_confirm"},"user":"user-owner"}"#;
const STOPPED: &str = r#"{"agent_id":"agent-a","connection_id":"conn_alpaca_paper_01","loss_added":"0","reason":"owner_stop","retired_on":"2026-09-20"}"#;
/// Another agent's stop, which leaves `agent-a`'s deployment active.
const OTHER_STOPPED: &str = r#"{"agent_id":"agent-b","connection_id":"conn_alpaca_paper_01","loss_added":"0","reason":"owner_stop","retired_on":"2026-09-20"}"#;
const REVOKED: &str = r#"{"connection_id":"conn_alpaca_paper_01"}"#;
/// The fixture's paper connection established at version 2 with its account (journal spec §9.8).
const ESTABLISHED: &str = r#"{"account_ref":"01J0ACC0VNT000000000000000","broker":"alpaca","connection_id":"conn_alpaca_paper_01","environment":"paper","margin_attestation":null,"scopes":["data","trading"],"step_up":{"assertion_id":"cli-assertion-0","authenticated_at":"2026-10-08T12:59:00.000000000Z","method":"cli_confirm"},"user":"user-owner"}"#;
/// The same connection at version 1, which names no account.
const ESTABLISHED_V1: &str = r#"{"broker":"alpaca","connection_id":"conn_alpaca_paper_01","environment":"paper","scopes":["data","trading"]}"#;
/// The fixture's top-level members but the two system fields (mandate spec §4.1 V-020).
const PATHS: &str = "autonomy behavior capital connection_id environment goal name notifications \
                     protection risk universe";
/// DEC-530 item 9's codes for `agent deploy`, with DEC-2809's `second_instrument` and
/// `second_agent_on_account`.
const CODES: &str = "agent_active agent_invalid agent_stopped code_mismatch \
                     control_stream_invalid document_corrupt document_missing \
                     instrument_unregistered paper_only second_agent_on_account second_instrument \
                     version_invalid version_superseded version_unconfirmed version_unknown";

fn json(text: &str) -> Value {
    parse(text.as_bytes()).unwrap()
}

fn canonical(text: &str) -> Vec<u8> {
    to_canonical(&json(text))
}

/// `sha256:` and the digest of `text`'s canonical form: a version, or a record's reference.
fn reference(text: &str) -> String {
    format!("sha256:{}", Digest::of(&canonical(text)).to_hex())
}

/// The fixture by name, or the fixture with one member's value changed.
fn document(name: &str) -> String {
    let (from, to) = match name {
        "v1" => return MANDATE.to_owned(),
        "v2" => (r#"max_order_usd":"300"#, r#"max_order_usd":"200"#),
        "m101" => (r#""version":"1.0.0""#, r#""version":"1.0.1""#),
        "qqq" => (r#""symbol":"SPY""#, r#""symbol":"QQQ""#),
        "wide" => (r#""max_instruments":1"#, r#""max_instruments":2"#),
        _ => ("conn_alpaca_paper_01", "conn_alpaca_paper_02"),
    };
    assert!(MANDATE.contains(from), "{from}");
    MANDATE.replace(from, to)
}

/// DEC-530 item 4: the first eight hex digits of the SHA-256 of the gesture's canonical object.
fn code(gesture: &str) -> String {
    Digest::of(&canonical(gesture)).to_hex()[..8].to_owned()
}

fn deploy_code(agent: &str, name: &str) -> String {
    let version = reference(&document(name));
    let gesture = r#""gesture":"agent_deploy""#;
    code(&format!(
        r#"{{"agent_id":"{agent}",{gesture},"mandate_version":"{version}"}}"#
    ))
}

/// One step of a case, on a document [`document`] names; a code of `None` is the right one.
#[derive(Debug, Clone, Copy)]
enum Do<'a> {
    /// `MandateVersionCreated` for the document, which is stored.
    Created(&'a str),
    Confirmed(&'a str),
    Deploy(&'a str, &'a str, Option<&'a str>),
    Seed(&'a str, &'a str),
    /// A version-2 record, as `Seed`, naming a cause as an establishment must.
    SeedV2(&'a str, &'a str),
    As(Environment),
    /// Remove the stored fixture, or overwrite it with another mandate, `v2`.
    Lose,
    Corrupt,
}

/// A workspace whose control stream registers the fixture's model and SPY's snapshot; its owner is
/// in paper unless `environment` says otherwise.
#[derive(Default)]
struct World {
    journal: Journal,
    store: Store,
    ids: FixedIds,
    environment: Option<Environment>,
    /// The store's writes fail once this many more have succeeded.
    fails_after: Option<usize>,
}

impl World {
    fn new() -> Self {
        let mut world = Self::default();
        world.seed("ConfigSnapshotRegistered", MODEL);
        world.store.put_artifact(SPY.as_bytes()).unwrap();
        let snapshot = SNAPSHOT.replace("@H", &reference(SPY));
        world.seed("ConfigSnapshotRegistered", &snapshot);
        world
    }

    /// `v1` created and confirmed.
    fn confirmed() -> Self {
        let mut world = Self::new();
        for step in [Created("v1"), Confirmed("v1")] {
            world.run(step).unwrap();
        }
        world
    }

    /// Appends one event as another control-stream writer would.
    fn seed(&mut self, event_type: &str, payload: &str) {
        self.seed_at(event_type, payload, 1);
    }

    fn seed_at(&mut self, event_type: &str, payload: &str, schema_version: u64) {
        let control = stream(CONTROL);
        let head = self.journal.head(&control).unwrap().seq;
        let draft = self.drafted(event_type, payload, schema_version);
        let (journal, at) = (&mut self.journal, at(NOW).at);
        let epoch = journal.take_ownership(&control).unwrap();
        let outcome = journal.append(&control, head, epoch, at, &[draft.as_slice()]);
        assert!(matches!(outcome, Ok(AppendOutcome::Committed(_))));
    }

    /// Another control-stream writer's draft of `event_type`, its event id from the head now.
    fn drafted(&self, event_type: &str, payload: &str, schema_version: u64) -> Vec<u8> {
        let control = stream(CONTROL);
        let head = self.journal.head(&control).unwrap().seq;
        let draft = SEEDED.replace("@I", &format!("9{head:025}"));
        let draft = match schema_version {
            1 => draft,
            _ => draft
                .replace(
                    r#""schema_version":1"#,
                    &format!(r#""schema_version":{schema_version}"#),
                )
                .replace(
                    r#""causation_id":null"#,
                    r#""causation_id":"8000000000000000000000000K""#,
                ),
        };
        let draft = draft.replace("@T", event_type);
        draft.replace("@P", payload).into_bytes()
    }

    /// A §9.2 version record of `event_type` for the named document, every envelope path entered
    /// by the user, or confirmed.
    fn version(&mut self, event_type: &str, name: &str) {
        let version = reference(&document(name));
        let (mut entered, mut paths) = (Vec::new(), Vec::new());
        for p in PATHS.split_whitespace() {
            entered.push(format!(r#"{{"path":"/{p}","source":"user_entered"}}"#));
            paths.push(format!(r#""/{p}""#));
        }
        let (entered, paths) = (entered.join(","), paths.join(","));
        let record = format!(r#""record_ref":"sha256:{}""#, "7".repeat(64));
        let payload = match event_type {
            "MandateVersionCreated" => format!(r#""provenance":[{entered}]"#),
            _ => format!(r#""confirmed_paths":[{paths}]"#),
        };
        let payload = format!(r#"{{{payload},"mandate_version":"{version}",{record}}}"#);
        self.seed(event_type, &payload);
    }

    fn run(&mut self, step: Do<'_>) -> Result<Submitted, ControlError> {
        let environment = self.environment.unwrap_or(Environment::Paper);
        let owner = Owner {
            environment,
            ..owner()
        };
        let fixture = Digest::of(&canonical(MANDATE));
        match step {
            Created(name) => {
                self.store
                    .put_artifact(&canonical(&document(name)))
                    .unwrap();
                self.version("MandateVersionCreated", name);
            }
            Confirmed(name) => self.version("MandateConfirmed", name),
            Deploy(agent, name, code) => {
                let version = reference(&document(name));
                let code = code.map_or_else(|| deploy_code(agent, name), str::to_owned);
                let (after, store) = (self.fails_after, self.store.clone());
                let mut failing = Failing(store, after.unwrap_or_default());
                let store: &mut dyn ArtifactStore = match after {
                    Some(_) => &mut failing,
                    None => &mut self.store,
                };
                let (journal, ids, now) = (&mut self.journal, &mut self.ids, at(NOW));
                return deploy(journal, store, ids, &owner, agent, &version, &code, now);
            }
            Seed(event_type, payload) => self.seed(event_type, payload),
            SeedV2(event_type, payload) => self.seed_at(event_type, payload, 2),
            As(environment) => self.environment = Some(environment),
            Lose => drop(self.store.remove(&fixture)),
            Corrupt => drop(self.store.insert(fixture, canonical(&document("v2")))),
        }
        let (event_id, seq) = (String::new(), 0);
        Ok(Submitted { event_id, seq })
    }

    fn appends(&self) -> usize {
        self.journal.attempts.len()
    }

    fn rows(&self) -> Vec<mandate_journal::StoredEvent> {
        self.journal.rows(&stream(CONTROL)).unwrap()
    }
}

fn refused(result: Result<Submitted, ControlError>) -> &'static str {
    match result {
        Err(ControlError::Refused { reason }) => reason,
        other => panic!("not refused: {other:?}"),
    }
}

/// One `AgentDeployed` with exactly §9.2's payload, `config_refs.mandate_version` (rule 22), and
/// the document and the stored record as its `artifact_refs`; a re-run spends nothing.
#[test]
fn a_deployment_is_one_agent_deployed_naming_the_version_and_its_record() {
    let mut world = World::confirmed();
    let before = world.appends();
    let first = world.run(Deploy(AGENT, "v1", None)).unwrap();
    let rows = world.rows();
    let body = &rows.last().unwrap().body;
    let draft = Draft::parse(body).unwrap();
    let schema = (draft.event_type(), draft.schema_version());
    assert_eq!(schema, ("AgentDeployed", 1));
    let version = reference(MANDATE);
    let record = canonical(&RECORD.replace("@V", &version));
    let record_ref = format!("sha256:{}", Digest::of(&record).to_hex());
    let payload = DEPLOYED.replace("@V", &version).replace("@R", &record_ref);
    let mut listed = [version.clone(), record_ref];
    listed.sort();
    let listed = format!(r#"["{}","{}"]"#, listed[0], listed[1]);
    let refs = format!(r#"{{"mandate_version":"{version}"}}"#);
    let envelope = parse(body).unwrap();
    let members = ["payload", "artifact_refs", "config_refs"];
    let got = members.map(|m| envelope.get(m).cloned());
    let expected = [json(&payload), json(&listed), json(&refs)].map(Some);
    assert_eq!(got, expected);
    let actor = envelope.get("actor").and_then(|a| a.get("id"));
    assert_eq!(actor.and_then(Value::as_str), Some(OWNER));
    let stored = world.store.get(&Digest::of(&record));
    assert_eq!(stored, Some(&record), "the record, stored");
    world.seed("ConfigSnapshotRegistered", MODEL);
    let again = world.run(Deploy(AGENT, "v1", None));
    assert_eq!(again, Ok(first), "a re-run finds it, after any event");
    let spent = (world.appends(), world.ids.assertions);
    assert_eq!(spent, (before + 2, 1), "one event, one assertion");
}

/// What D2b writes, the spec's fold reads as the agent's version in force, and only V-001 and
/// V-002 remain: the two facts the run supplies (DEC-505 item 3).
#[test]
fn the_spec_fold_reads_the_deployment_as_the_agents_version() {
    let mut world = World::confirmed();
    world.run(Deploy(AGENT, "v1", None)).unwrap();
    let documents = |d: &Digest| world.store.get(d).map(|bytes| parse(bytes).unwrap());
    let mut facts = Vec::new();
    for row in &world.rows() {
        let payload = parse(&row.body).unwrap().get("payload").unwrap().clone();
        let fact = JournaledFact::from_record(&row.event_type, &payload, &documents, None);
        facts.extend(fact.unwrap());
    }
    let version = Digest::of(&canonical(MANDATE));
    let active = facts.iter().any(|f| {
        matches!(f, JournaledFact::AgentVersionActive { agent, version: v, .. }
            if agent.as_str() == AGENT && v.digest() == version)
    });
    assert!(active, "{facts:?}");
    let mandate = Mandate::parse(&json(MANDATE)).unwrap();
    let membership = Some(Membership {
        workspace_users: 1,
        approver_users: 1,
    });
    let args = ContextArgs {
        agent: AgentId::new(AGENT),
        connection_id: mandate.connection_id.clone(),
        validation_date: Date::parse("2026-10-08").unwrap(),
        membership,
        independent_approval_required: false,
        instrument_groups: BTreeMap::new(),
        eligibility_failures: BTreeSet::new(),
    };
    let context = ValidationContext::from_journal(&mandate, args, &facts).unwrap();
    let violations = validate(&mandate, &context).unwrap().violations;
    let supplied = BTreeSet::from([Violation::V001, Violation::V002, Violation::V008]);
    assert_eq!(
        violations, supplied,
        "the run's three facts: the bare fold knows no connection, so neither its environment \
         (V-001) nor its profile (V-008, which fails closed, DEC-1152 item 1)"
    );
}

/// A version confirmed again after another's confirmation is the latest, so it deploys; and once
/// that agent is stopped, its id is never deployed again (DEC-1173 item 1).
#[test]
fn a_reconfirmed_version_is_the_latest_and_a_stopped_agent_is_never_deployed_again() {
    let mut world = World::confirmed();
    for step in [Created("v2"), Confirmed("v2")] {
        world.run(step).unwrap();
    }
    let superseded = world.run(Deploy(AGENT, "v1", None));
    assert_eq!(refused(superseded), "version_superseded");
    world.run(Confirmed("v1")).unwrap();
    let before = world.appends();
    let deployed = world.run(Deploy(AGENT, "v1", None)).unwrap();
    let rows = world.rows();
    assert_eq!(rows.last().map(|r| &r.event_id), Some(&deployed.event_id));
    assert_eq!(world.appends(), before + 1, "one new deployment");
    world.run(Seed("AgentStopped", STOPPED)).unwrap();
    let before = world.appends();
    let again = world.run(Deploy(AGENT, "v1", None));
    assert_eq!(refused(again), "agent_stopped");
    assert_eq!(world.appends(), before, "a refusal writes nothing");
}

/// The deployment's version and, at version 2, its connection: what rule 201 decides.
type DeployedAs = (u64, Option<String>);

fn deployed_as(world: &World) -> DeployedAs {
    let rows = world.rows();
    let row = rows.last().unwrap();
    assert_eq!(row.event_type, "AgentDeployed");
    let body = parse(&row.body).unwrap();
    let connection = body
        .get("payload")
        .and_then(|p| p.get("connection_id"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    assert!(Draft::parse(&row.body).is_ok(), "the journal admits it");
    (row.schema_version, connection)
}

/// Journal spec v0.54 §9.24: version 2, naming the document's connection, exactly when the control
/// stream holds a version-2 establishment of it whose latest establishment or revocation is an
/// establishment (rule 201); otherwise version 1, as before, and never refused for it.
#[test]
fn a_deployment_names_its_connection_exactly_when_rule_201_holds() {
    let connection = Some("conn_alpaca_paper_01".to_owned());
    let reconnected = [
        SeedV2("ConnectionEstablished", ESTABLISHED),
        Seed("ConnectionRevoked", REVOKED),
        SeedV2("ConnectionEstablished", ESTABLISHED),
    ];
    let another = ESTABLISHED
        .replace("conn_alpaca_paper_01", "conn_alpaca_paper_02")
        .replace("01J0ACC0VNT000000000000000", "01J0ACC0VNT000000000000001");
    let cases: [(&str, &[Do<'_>], DeployedAs); 5] = [
        (
            "another_connection_established",
            &[SeedV2("ConnectionEstablished", &another)],
            (1, None),
        ),
        ("unrecorded", &[], (1, None)),
        (
            "established",
            &[SeedV2("ConnectionEstablished", ESTABLISHED)],
            (2, connection.clone()),
        ),
        ("reconnected", &reconnected, (2, connection)),
        (
            "established_v1",
            &[Seed("ConnectionEstablished", ESTABLISHED_V1)],
            (1, None),
        ),
    ];
    for (name, setup, expected) in cases {
        let mut world = World::new();
        for step in setup.iter().chain(&[Created("v1"), Confirmed("v1")]) {
            world.run(*step).unwrap();
        }
        world.run(Deploy(AGENT, "v1", None)).unwrap();
        assert_eq!(deployed_as(&world), expected, "{name}");
    }
}

/// A revocation another writer commits after `agent deploy` read the rows, and before it appends,
/// never lets it append version 2 after the revocation: the version-2 draft is pinned to the head
/// it was decided at, so it reads the rows again, where V-001 now refuses the revoked connection,
/// and nothing is appended (#1748 review). A connection revoked before the command ran is the
/// refusal table's `version_invalid`.
#[test]
fn a_revocation_that_lands_before_the_append_is_read_and_the_deployment_refused() {
    let mut world = World::new();
    for step in [
        SeedV2("ConnectionEstablished", ESTABLISHED),
        Created("v1"),
        Confirmed("v1"),
    ] {
        world.run(step).unwrap();
    }
    let revocation = world.drafted("ConnectionRevoked", REVOKED, 1);
    world.journal.land_before_next.push(revocation);
    let before = world.appends();
    let result = world.run(Deploy(AGENT, "v1", None));
    assert_eq!(refused(result), "version_invalid", "decided again after it");
    let rows = world.rows();
    let last = rows.last().map(|r| r.event_type.as_str());
    assert_eq!(last, Some("ConnectionRevoked"));
    let deployed = rows
        .iter()
        .filter(|r| r.event_type == "AgentDeployed")
        .count();
    assert_eq!(deployed, 0, "no deployment after the revocation");
    assert_eq!(
        world.appends(),
        before + 1,
        "the pinned append, refused as behind, and no other"
    );
}

/// Every refusal carries one of DEC-530 item 9's codes, each case its own, and writes nothing: no
/// append, no stored object, no assertion. Where two checks both fail, the earlier in item 9's
/// order refuses: a superseded version before a wrong code, and a wrong code before an active
/// deployment.
#[test]
fn every_refusal_has_its_own_code_and_writes_nothing() {
    let unreadable = MODEL.replace(r#"["fast_periods","slow_periods"]"#, r#""x""#);
    let model = Seed("ConfigSnapshotRegistered", &unreadable);
    let (v1, c1, d1) = (Created("v1"), Confirmed("v1"), Deploy(AGENT, "v1", None));
    let (v2, c2) = (Created("v2"), Confirmed("v2"));
    let (stop, revoke) = (
        Seed("AgentStopped", STOPPED),
        Seed("ConnectionRevoked", REVOKED),
    );
    let other = Deploy(OTHER_AGENT, "v1", None);
    let other_stop = Seed("AgentStopped", OTHER_STOPPED);
    let codes = [deploy_code(OTHER_AGENT, "v1"), deploy_code(AGENT, "v2")];
    let established = SeedV2("ConnectionEstablished", ESTABLISHED);
    let cases: [(&str, &[Do<'_>], Do<'_>); 23] = [
        ("paper_only", &[v1, c1, As(Environment::Live)], d1),
        ("paper_only", &[v1, c1, As(Environment::Backtest)], d1),
        ("agent_invalid", &[v1, c1], Deploy("agent a", "v1", None)),
        ("version_unknown", &[v1, c1], Deploy(AGENT, "v2", None)),
        ("document_missing", &[v1, c1, Lose], d1),
        ("document_corrupt", &[v1, c1, Corrupt], d1),
        ("version_unconfirmed", &[v1], d1),
        ("version_superseded", &[v1, c1, v2, c2], d1),
        (
            "version_superseded",
            &[v1, c1, v2, c2],
            Deploy(AGENT, "v1", Some("00000000")),
        ),
        (
            "code_mismatch",
            &[v1, c1, d1, v2, c2],
            Deploy(AGENT, "v2", Some("00000000")),
        ),
        (
            "code_mismatch",
            &[v1, c1],
            Deploy(AGENT, "v1", Some("00000000")),
        ),
        (
            "code_mismatch",
            &[v1, c1],
            Deploy(AGENT, "v1", Some(&codes[0])),
        ),
        (
            "code_mismatch",
            &[v1, c1, v2],
            Deploy(AGENT, "v1", Some(&codes[1])),
        ),
        (
            "agent_active",
            &[v1, c1, d1, v2, c2],
            Deploy(AGENT, "v2", None),
        ),
        (
            "agent_active",
            &[v1, c1, d1, other_stop, v2, c2],
            Deploy(AGENT, "v2", None),
        ),
        ("control_stream_invalid", &[model, v1, c1], d1),
        ("version_invalid", &[v1, c1, other], d1),
        ("version_invalid", &[revoke, v1, c1], d1),
        (
            "version_invalid",
            &[Created("m101"), Confirmed("m101")],
            Deploy(AGENT, "m101", None),
        ),
        (
            "agent_stopped",
            &[v1, c1, d1, stop, Created("conn2"), Confirmed("conn2")],
            Deploy(AGENT, "conn2", None),
        ),
        (
            "instrument_unregistered",
            &[Created("qqq"), Confirmed("qqq")],
            Deploy(AGENT, "qqq", None),
        ),
        (
            "second_instrument",
            &[Created("wide"), Confirmed("wide")],
            Deploy(AGENT, "wide", None),
        ),
        ("second_agent_on_account", &[established, v1, c1, d1], other),
    ];
    let mut seen = BTreeSet::new();
    for (code, setup, step) in cases {
        let mut world = World::new();
        for earlier in setup {
            world.run(*earlier).unwrap();
        }
        let state = |w: &World| (w.appends(), w.store.clone(), w.ids.assertions);
        let before = state(&world);
        assert_eq!(refused(world.run(step)), code, "{setup:?} {step:?}");
        assert_eq!(state(&world), before, "{code} writes nothing");
        seen.insert(code);
    }
    let codes: BTreeSet<&str> = CODES.split_whitespace().collect();
    assert_eq!(seen, codes, "the closed set, each code its own");
}

/// A store that reads the objects it holds and fails every write once `1` more have succeeded.
struct Failing(Store, usize);

impl ArtifactSource for Failing {
    fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
        self.0.read_artifact(reference)
    }
}

impl ArtifactStore for Failing {
    fn put_artifact(&mut self, bytes: &[u8]) -> Result<ArtifactRef, ArtifactError> {
        self.1 = self.1.checked_sub(1).ok_or(ArtifactError::Unavailable)?;
        self.0.put_artifact(bytes)
    }
}

/// The record is stored before the deployment is committed, so a store that fails commits nothing.
#[test]
fn a_store_that_fails_commits_nothing() {
    let mut world = World::confirmed();
    let before = world.appends();
    world.fails_after = Some(0);
    let result = world.run(Deploy(AGENT, "v1", None));
    assert!(
        matches!(result, Err(ControlError::Journal(_))),
        "{result:?}"
    );
    assert_eq!(world.appends(), before, "nothing appended");
}

/// The `step_up` member a version 3 `AgentDeployed` or `MandateConfirmed` carries (journal spec
/// v0.84 §9.9 and §9.24).
const STEP_UP: &str = r#""step_up":{"assertion_id":"01JAA3P0A000000000000000H1","authenticated_at":"2026-10-08T12:59:57.000000000Z","method":"passkey"}"#;

/// Journal spec v0.84, DEC-2131 item 4: a version 3 `MandateConfirmed` confirms its version as its
/// version 2 does, so the deployment commits the same event; and a version 3 `AgentDeployed` is
/// the agent's active deployment as its version 2 is, so a re-run finds it and commits nothing.
#[test]
fn a_version_3_deployment_reads_as_its_version_2() {
    let version = reference(MANDATE);
    let paths: Vec<String> = PATHS
        .split_whitespace()
        .map(|p| format!(r#""/{p}""#))
        .collect();
    let confirmed = format!(
        r#"{{"agent_id":null,"base_version":null,"confirmed_paths":[{}],"mandate_version":"{version}","record_ref":"sha256:{}""#,
        paths.join(","),
        "7".repeat(64)
    );
    let deployed = format!(
        r#"{{"agent_id":"{OTHER_AGENT}","mandate_version":"{version}","record_ref":"sha256:{}","connection_id":"conn_alpaca_paper_01""#,
        "6".repeat(64)
    );
    let read = |schema_version: u64, trailing: &str| {
        let mut world = World::new();
        world.run(Created("v1")).unwrap();
        world.seed_at(
            "MandateConfirmed",
            &format!("{confirmed}{trailing}}}"),
            schema_version,
        );
        let first = world.run(Deploy(AGENT, "v1", None));
        world.seed_at(
            "AgentDeployed",
            &format!("{deployed}{trailing}}}"),
            schema_version,
        );
        let seeded = world.rows().last().map(|r| (r.event_id.clone(), r.seq));
        let before = world.appends();
        let again = world.run(Deploy(OTHER_AGENT, "v1", None));
        let found = again.map(|s| (s.event_id, s.seq));
        assert_eq!(found, Ok(seeded.unwrap()), "the seeded deployment, found");
        assert_eq!(world.appends(), before, "a re-run commits nothing");
        let rows = world.rows();
        let versions: Vec<u64> = rows.iter().map(|r| r.schema_version).collect();
        (first, versions)
    };
    let (version_2, versions_2) = read(2, "");
    let (version_3, versions_3) = read(3, &format!(",{STEP_UP}"));
    assert!(version_2.is_ok(), "{version_2:?}");
    assert_eq!(version_3, version_2);
    assert_eq!(versions_3.iter().filter(|v| **v == 3).count(), 2);
    assert_eq!(versions_2.iter().filter(|v| **v == 2).count(), 2);
}

/// DEC-2809 item 1: a version whose universe may hold a second instrument is refused at deploy,
/// after the checks on the agent, so an agent already deployed on it answers with its deployment,
/// and before the V-rules; it was confirmed, which DEC-2809 leaves to the envelope.
#[test]
fn a_version_that_may_hold_a_second_instrument_is_refused_at_deploy() {
    let mut world = World::new();
    for step in [Created("wide"), Confirmed("wide")] {
        world.run(step).unwrap();
    }
    let before = world.appends();
    assert_eq!(
        refused(world.run(Deploy(AGENT, "wide", None))),
        "second_instrument"
    );
    assert_eq!(world.appends(), before, "a refusal writes nothing");
}

/// DEC-2809 item 1's place in DEC-530 item 9's order: after the checks on the agent, so a stopped
/// agent meeting a wide version is still `agent_stopped`; before the V-rules, so a wide version
/// that also fails one (a revoked connection, `version_invalid` on its own) is `second_instrument`.
#[test]
fn second_instrument_is_judged_after_the_agent_and_before_the_rules() {
    let mut world = World::confirmed();
    for step in [
        Deploy(AGENT, "v1", None),
        Seed("AgentStopped", STOPPED),
        Created("wide"),
        Confirmed("wide"),
    ] {
        world.run(step).unwrap();
    }
    assert_eq!(
        refused(world.run(Deploy(AGENT, "wide", None))),
        "agent_stopped"
    );

    let mut world = World::new();
    for step in [
        Seed("ConnectionRevoked", REVOKED),
        Created("v1"),
        Confirmed("v1"),
    ] {
        world.run(step).unwrap();
    }
    assert_eq!(
        refused(world.run(Deploy(AGENT, "v1", None))),
        "version_invalid",
        "a revoked connection fails a rule on its own"
    );
    for step in [Created("wide"), Confirmed("wide")] {
        world.run(step).unwrap();
    }
    assert_eq!(
        refused(world.run(Deploy(AGENT, "wide", None))),
        "second_instrument"
    );
}

/// DEC-2809 item 2: while an agent that has not stopped is deployed on an account, through this
/// connection or another naming the same account, a second agent's deployment there is refused
/// `second_agent_on_account` and writes nothing; a connection to another account is not; the
/// first agent's stop frees the account.
#[test]
fn a_second_running_agent_on_the_account_is_refused_at_deploy() {
    let sibling = ESTABLISHED.replace("conn_alpaca_paper_01", "conn_alpaca_paper_02");
    let elsewhere = sibling.replace("01J0ACC0VNT000000000000000", "01J0ACC0VNT000000000000001");
    let running = |connection: &str| {
        let mut world = World::new();
        for step in [
            SeedV2("ConnectionEstablished", ESTABLISHED),
            SeedV2("ConnectionEstablished", connection),
            Created("v1"),
            Confirmed("v1"),
            Deploy(AGENT, "v1", None),
            Created("conn2"),
            Confirmed("conn2"),
        ] {
            world.run(step).unwrap();
        }
        world
    };

    let mut world = running(&sibling);
    let before = world.appends();
    assert_eq!(
        refused(world.run(Deploy(OTHER_AGENT, "conn2", None))),
        "second_agent_on_account",
        "a sibling connection to the same account"
    );
    assert_eq!(world.appends(), before, "a refusal writes nothing");
    world.run(Seed("AgentStopped", STOPPED)).unwrap();
    assert!(
        world.run(Deploy(OTHER_AGENT, "conn2", None)).is_ok(),
        "the first agent's stop frees the account"
    );

    let mut world = running(&elsewhere);
    assert!(
        world.run(Deploy(OTHER_AGENT, "conn2", None)).is_ok(),
        "another account is free"
    );
}
