//! D2a (E10-16, DEC-530): `version create` and `version confirm`. Payloads, records and codes are
//! written out from journal spec §9.2's vectors and DEC-530. `mandate-cli` may not depend on
//! `mandate-shell`, so the round trip folds the stream with the functions DEC-505 item 1 names.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use Do::{As, Confirm, Corrupt, Create, Lose, Seed};
use common::{AGENT, CONTROL, FixedIds, Journal, OWNER, at, owner, stream};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_cli::control::{ControlError, ControlJournal, Owner, Submitted};
use mandate_cli::version::{confirm, create};
use mandate_journal::{AppendOutcome, ArtifactError, ArtifactRef, ArtifactSource};
use mandate_journal::{ArtifactStore, Draft, Environment};
use mandate_spec::context::{AgentId, ContextArgs, JournaledFact, Membership};
use mandate_spec::{Mandate, ValidationContext, Violation, validate::validate};
use mandate_time::Date;

type Store = BTreeMap<Digest, Vec<u8>>;

const NOW: i64 = 1_790_000_000;
const MANDATE: &str = include_str!("fixtures/spy_mandate.json");
const SPY: &str = r#"{"asset_class":"us_equity","etp":"plain","etp_classified_at":"2026-10-05T13:30:00Z","etp_source":"nasdaq_trader_symbol_directory","exchange":"arca","increment":"whole","instrument_id":"b0b6dd9d-8b9b-48a9-ba46-b9d54906e415","symbol":"SPY"}"#;
const MODEL: &str = r#"{"admits_instruments":false,"content_hash":"sha256:4f3559229f89b27c04b43ec773b0ff0b884895362622964b78dfd791d67e18fc","kind":"model_version","model_id":"quant.ma_crossover","model_version":"1.0.0","params":["fast_periods","slow_periods"]}"#;
const SNAPSHOT: &str = r#"{"admits_instruments":null,"content_hash":"@H","kind":"instrument_snapshot","model_id":null,"model_version":null,"params":[]}"#;
const SEEDED: &str = r#"{"actor":{"build":null,"id":"control_services","kind":"system","version":"0.1.0"},"artifact_refs":[],"causation_id":null,"clock_source":"local","config_refs":{},"correlation_id":null,"envelope_version":1,"environment":"paper","event_id":"@I","event_time":"2026-10-08T13:00:00.000000000Z","event_type":"@T","payload":@P,"pii_refs":[],"schema_version":1,"stream_id":"ctl:ws1"}"#;
const CREATED: &str = r#"{"mandate_version":"@V","provenance":[@P],"record_ref":"@R"}"#;
const VERSION_RECORD: &str =
    r#"{"kind":"mandate_version_record","mandate_version":"@V","user":"user-owner"}"#;
const CONFIRMED: &str = r#"{"confirmed_paths":[@C],"mandate_version":"@V","record_ref":"@R"}"#;
const CONFIRMATION_RECORD: &str = r#"{"kind":"mandate_confirmation_record","mandate_version":"@V","step_up":{"assertion_id":"cli-assertion-1","authenticated_at":1790000000,"method":"cli_confirm"},"user":"user-owner","warnings":["W-002"]}"#;
/// A retired agent's loss on the fixture's connection, above its floor of 0.1 x 1000 (V-032).
const STOPPED: &str = r#"{"agent_id":"agent-b","connection_id":"conn_alpaca_paper_01","loss_added":"500","reason":"owner_stop","retired_on":"2026-09-20"}"#;
const REVOKED: &str = r#"{"connection_id":"conn_alpaca_paper_01"}"#;
/// The fixture's top-level members but the two system fields (mandate spec §4.1 V-020).
const PATHS: &str = "autonomy behavior capital connection_id environment goal name notifications \
                     protection risk universe";
/// DEC-530 item 10's codes for the two commands.
const CODES: &str = "code_mismatch control_stream_invalid document_corrupt document_missing \
                     instrument_unregistered mandate_invalid mandate_not_paper paper_only \
                     version_invalid version_unknown";

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
        "spaced" => return format!("\n {MANDATE} "),
        "junk" => return "not json".to_owned(),
        "nameless" => (r#""name":"paper-spy","#, ""),
        "v2" => (r#"max_order_usd":"300"#, r#"max_order_usd":"200"#),
        "live" => (r#"environment":"paper"#, r#"environment":"live"#),
        "m101" => (r#""version":"1.0.0""#, r#""version":"1.0.1""#),
        "qqq" => (r#""symbol":"SPY""#, r#""symbol":"QQQ""#),
        "big_order" => (r#""max_order_usd":"300""#, r#""max_order_usd":"2000""#),
        "short_floor" => (r#"allocation":"0.1""#, r#"allocation":"0.05""#),
        "wide_hysteresis" => (r#""hysteresis":"0.01""#, r#""hysteresis":"0.05""#),
        "unpinned" => (r#""pinned":true"#, r#""pinned":false"#),
        "unprotected" => (r#""enabled":true"#, r#""enabled":false"#),
        "ended" => (r#""end_date":null"#, r#""end_date":"2026-01-02""#),
        "quiet" => (r#""start":"23:00""#, r#""start":"07:00""#),
        _ => ("b0b6dd9d-", "a0b6dd9d-"),
    };
    assert!(MANDATE.contains(from), "{from}");
    MANDATE.replace(from, to)
}

/// DEC-530 item 4: the first eight hex digits of the SHA-256 of the gesture's canonical object.
fn confirm_code(name: &str) -> String {
    let version = reference(&document(name));
    let gesture = format!(r#"{{"gesture":"mandate_confirm","mandate_version":"{version}"}}"#);
    Digest::of(&canonical(&gesture)).to_hex()[..8].to_owned()
}

/// One step of a case, on a document [`document`] names; a code of `None` is the right one.
#[derive(Debug, Clone, Copy)]
enum Do<'a> {
    Create(&'a str),
    Confirm(&'a str, Option<&'a str>),
    Seed(&'a str, &'a str),
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

    /// Appends one event as another control-stream writer would.
    fn seed(&mut self, event_type: &str, payload: &str) {
        let control = stream(CONTROL);
        let head = self.journal.head(&control).unwrap().seq;
        let draft = SEEDED.replace("@I", &format!("9{head:025}"));
        let draft = draft.replace("@T", event_type);
        let draft = [draft.replace("@P", payload)];
        let (journal, at) = (&mut self.journal, at(NOW).at);
        let epoch = journal.take_ownership(&control).unwrap();
        let outcome = journal.append(&control, head, epoch, at, &[draft[0].as_bytes()]);
        assert!(matches!(outcome, Ok(AppendOutcome::Committed(_))));
    }

    fn run(&mut self, step: Do<'_>) -> Result<Submitted, ControlError> {
        let environment = self.environment.unwrap_or(Environment::Paper);
        let owner = Owner {
            environment,
            ..owner()
        };
        let now = at(NOW);
        let (after, store) = (self.fails_after, self.store.clone());
        let mut failing = Failing(store, after.unwrap_or_default());
        let store: &mut dyn ArtifactStore = match after {
            Some(_) => &mut failing,
            None => &mut self.store,
        };
        let journal = &mut self.journal;
        let fixture = Digest::of(&canonical(MANDATE));
        match step {
            Do::Create(name) => {
                let bytes = document(name).into_bytes();
                return create(journal, store, &owner, &bytes, now);
            }
            Do::Confirm(name, code) => {
                let version = reference(&document(name));
                let code = code.map_or_else(|| confirm_code(name), str::to_owned);
                let ids = &mut self.ids;
                return confirm(journal, store, ids, &owner, &version, &code, now);
            }
            Do::Seed(event_type, payload) => self.seed(event_type, payload),
            Do::As(environment) => self.environment = Some(environment),
            Do::Lose => drop(self.store.remove(&fixture)),
            Do::Corrupt => drop(self.store.insert(fixture, canonical(&document("v2")))),
        }
        let (event_id, seq) = (String::new(), 0);
        Ok(Submitted { event_id, seq })
    }

    fn appends(&self) -> usize {
        self.journal.attempts.len()
    }

    /// The stream's last event is `event_type` with exactly `payload`, `@V` the fixture's version,
    /// `@P` and `@C` the envelope paths entered and confirmed, and `@R` the reference of `record`,
    /// which is stored. Its `artifact_refs` are those two, its `config_refs` none, and its draft
    /// passes the journal's schema check.
    fn committed(&self, event_type: &str, payload: &str, record: &str) {
        let rows = self.journal.rows(&stream(CONTROL)).unwrap();
        let body = &rows.last().unwrap().body;
        let draft = Draft::parse(body).unwrap();
        let schema = (draft.event_type(), draft.schema_version());
        assert_eq!(schema, (event_type, 1));
        let version = reference(MANDATE);
        let record = canonical(&record.replace("@V", &version));
        let record_ref = format!("sha256:{}", Digest::of(&record).to_hex());
        let (mut entered, mut paths) = (Vec::new(), Vec::new());
        for p in PATHS.split_whitespace() {
            entered.push(format!(r#"{{"path":"/{p}","source":"user_entered"}}"#));
            paths.push(format!(r#""/{p}""#));
        }
        let payload = payload.replace("@P", &entered.join(","));
        let payload = payload
            .replace("@C", &paths.join(","))
            .replace("@V", &version);
        let payload = json(&payload.replace("@R", &record_ref));
        let mut listed = [version, record_ref];
        listed.sort();
        let listed = format!(r#"["{}","{}"]"#, listed[0], listed[1]);
        let envelope = parse(body).unwrap();
        let members = ["payload", "artifact_refs", "config_refs"];
        let got = members.map(|m| envelope.get(m).cloned());
        let expected = [payload, json(&listed), json("{}")].map(Some);
        assert_eq!(got, expected, "{event_type}");
        let actor = envelope.get("actor").and_then(|a| a.get("id"));
        assert_eq!(actor.and_then(Value::as_str), Some(OWNER));
        let stored = self.store.get(&Digest::of(&record));
        assert_eq!(stored, Some(&record), "the record, stored");
    }
}

fn refused(result: Result<Submitted, ControlError>) -> &'static str {
    match result {
        Err(ControlError::Refused { reason }) => reason,
        other => panic!("not refused: {other:?}"),
    }
}

#[test]
fn a_version_is_its_stored_document_created_with_every_path_user_entered() {
    let mut world = World::new();
    let before = world.appends();
    let first = world.run(Do::Create("spaced")).unwrap();
    let document = canonical(MANDATE);
    let stored = world.store.get(&Digest::of(&document));
    assert_eq!(stored, Some(&document), "canonical, under its version");
    world.committed("MandateVersionCreated", CREATED, VERSION_RECORD);
    world.seed("ConfigSnapshotRegistered", MODEL);
    let again = world.run(Do::Create("v1"));
    assert_eq!(again, Ok(first), "a re-run finds it, after any event");
    assert_eq!(world.appends(), before + 2, "it committed once");
}

#[test]
fn a_confirmation_is_one_event_bound_to_the_version_shown_that_the_spec_fold_reads() {
    let mut world = World::new();
    world.run(Do::Create("v1")).unwrap();
    world.seed("ConnectionRevoked", &REVOKED.replace("_01", "_02"));
    let before = world.appends();
    let first = world.run(Do::Confirm("v1", None)).unwrap();
    world.committed("MandateConfirmed", CONFIRMED, CONFIRMATION_RECORD);
    let again = world.run(Do::Confirm("v1", None));
    assert_eq!(again, Ok(first.clone()), "a re-run finds it");
    let spent = (world.appends(), world.ids.assertions);
    assert_eq!(spent, (before + 1, 1), "one event, one assertion");
    for step in [Do::Create("v2"), Do::Confirm("v2", None)] {
        world.run(step).unwrap();
    }
    let before = world.appends();
    let again = world.run(Do::Confirm("v1", None)).unwrap();
    assert_ne!(
        again, first,
        "after v2's, v1 is confirmed anew (DEC-530 item 7)"
    );
    let rows = world.journal.rows(&stream(CONTROL)).unwrap();
    let latest = rows
        .iter()
        .rev()
        .find(|r| r.event_type == "MandateConfirmed");
    assert_eq!(
        latest.map(|r| &r.event_id),
        Some(&again.event_id),
        "the latest"
    );
    assert_eq!(world.appends(), before + 1, "one event");

    let documents = |d: &Digest| world.store.get(d).map(|bytes| parse(bytes).unwrap());
    let rows = world.journal.rows(&stream(CONTROL)).unwrap();
    let mut facts = Vec::new();
    for row in &rows {
        let payload = parse(&row.body).unwrap().get("payload").unwrap().clone();
        let fact = JournaledFact::from_record(&row.event_type, &payload, &documents, None);
        facts.extend(fact.unwrap());
    }
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
    let supplied = BTreeSet::from([Violation::V001, Violation::V002]);
    assert_eq!(
        violations, supplied,
        "the run's two facts; V-002 alone did not refuse"
    );
}

/// Every refusal carries one of DEC-530 item 10's codes, each case its own, and writes nothing:
/// no append, no stored object, no assertion.
#[test]
fn every_refusal_has_its_own_code_and_writes_nothing() {
    let unreadable = MODEL.replace(r#"["fast_periods","slow_periods"]"#, r#""x""#);
    let (v1, model) = (Create("v1"), Seed("ConfigSnapshotRegistered", &unreadable));
    let revoke = Seed("ConnectionRevoked", REVOKED);
    let deployed = (reference(MANDATE), "6".repeat(64));
    let deployed = format!(
        r#"{{"agent_id":"agent-b","mandate_version":"{}","record_ref":"sha256:{}"}}"#,
        deployed.0, deployed.1
    );
    let claim = Seed("AgentDeployed", &deployed);
    let (live, backtest) = (As(Environment::Live), As(Environment::Backtest));
    let (confirm, other) = (|name| Confirm(name, None), confirm_code("v2"));
    let cases: [(&str, &[Do<'_>], Do<'_>); 24] = [
        ("paper_only", &[live], v1),
        ("paper_only", &[v1, backtest], confirm("v1")),
        ("mandate_invalid", &[], Create("junk")),
        ("mandate_invalid", &[], Create("nameless")),
        ("mandate_not_paper", &[], Create("live")),
        ("version_unknown", &[v1], confirm("v2")),
        ("document_missing", &[v1, Lose], confirm("v1")),
        ("document_corrupt", &[v1, Corrupt], confirm("v1")),
        ("code_mismatch", &[v1], Confirm("v1", Some("00000000"))),
        (
            "code_mismatch",
            &[v1, Create("v2")],
            Confirm("v1", Some(&other)),
        ),
        ("control_stream_invalid", &[model, v1], confirm("v1")),
        ("version_invalid", &[revoke, v1], confirm("v1")),
        ("version_invalid", &[Create("m101")], confirm("m101")),
        ("version_invalid", &[v1, claim, Create("v2")], confirm("v2")),
        (
            "version_invalid",
            &[Seed("AgentStopped", STOPPED), v1],
            confirm("v1"),
        ),
        (
            "version_invalid",
            &[Create("big_order")],
            confirm("big_order"),
        ),
        (
            "version_invalid",
            &[Create("short_floor")],
            confirm("short_floor"),
        ),
        (
            "version_invalid",
            &[Create("wide_hysteresis")],
            confirm("wide_hysteresis"),
        ),
        (
            "version_invalid",
            &[Create("unpinned")],
            confirm("unpinned"),
        ),
        (
            "version_invalid",
            &[Create("unprotected")],
            confirm("unprotected"),
        ),
        ("version_invalid", &[Create("ended")], confirm("ended")),
        ("version_invalid", &[Create("quiet")], confirm("quiet")),
        ("instrument_unregistered", &[Create("qqq")], confirm("qqq")),
        ("instrument_unregistered", &[Create("aid")], confirm("aid")),
    ];
    let mut seen = BTreeSet::new();
    for (code, setup, step) in cases {
        let mut world = World::new();
        for earlier in setup {
            world.run(*earlier).unwrap();
        }
        let state = |w: &World| (w.appends(), w.store.clone(), w.ids.assertions);
        let before = state(&world);
        assert_eq!(refused(world.run(step)), code, "{step:?}");
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

/// Each command stores every object, the document and then the record, before it commits, so a
/// store that fails at any of those writes commits nothing.
#[test]
fn a_store_that_fails_commits_nothing() {
    let (create, confirm) = (Do::Create("v1"), Do::Confirm("v1", None));
    for (setup, step, after) in [
        (None, create, 0),
        (None, create, 1),
        (Some(create), confirm, 0),
    ] {
        let mut world = World::new();
        setup.map(|e| world.run(e).unwrap());
        let before = world.appends();
        world.fails_after = Some(after);
        let result = world.run(step);
        let failed = matches!(result, Err(ControlError::Journal(_)));
        assert!(failed, "{step:?} after {after}: {result:?}");
        assert_eq!(world.appends(), before, "{step:?} after {after}");
    }
}
