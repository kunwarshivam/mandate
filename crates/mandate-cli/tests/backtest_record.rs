//! `mandate backtest record` (E4-5, DEC-1144): journals the runner's stored payload as the
//! `backtest_runner` system after re-checking everything it names. The store is the one
//! `mandate-backtest-runner`'s own test leaves after a completed run of the cell's demo mandate
//! (`crates/mandate-backtest-runner/tests/fixtures/demo-store.json`), so these tests journal what
//! the runner really writes; `mandate-cli` may not link the runner (DEC-986), only read its files.

mod common;

use std::collections::BTreeMap;

use common::{CONTROL, Journal, at, owner, stream};
use mandate_canon::{Digest, Key, Value, parse, to_canonical};
use mandate_cli::backtest::{Recorded, print_recorded, record};
use mandate_cli::control::{ControlError, ControlJournal, Owner};
use mandate_journal::{AppendOutcome, ArtifactRef, ArtifactStore, Draft, Environment};

type Store = BTreeMap<Digest, Vec<u8>>;

const NOW: i64 = 1_790_000_000;
const DEMO_STORE: &str =
    include_str!("../../mandate-backtest-runner/tests/fixtures/demo-store.json");
const RULE_SET: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const SEEDED: &str = r#"{"actor":{"build":null,"id":"control_services","kind":"system","version":"0.1.0"},"artifact_refs":[],"causation_id":null,"clock_source":"local","config_refs":{},"correlation_id":null,"envelope_version":1,"environment":"paper","event_id":"@I","event_time":"2026-10-08T13:00:00.000000000Z","event_type":"@T","payload":@P,"pii_refs":[],"schema_version":1,"stream_id":"ctl:ws1"}"#;

fn text(s: &str) -> Value {
    Value::Str(s.to_owned())
}

fn member<'a>(value: &'a Value, name: &str) -> &'a Value {
    value.get(name).unwrap_or_else(|| panic!("{name}"))
}

fn member_text<'a>(value: &'a Value, name: &str) -> &'a str {
    member(value, name)
        .as_str()
        .unwrap_or_else(|| panic!("{name}"))
}

/// `value` with member `name` set to `to`, or removed when `to` is `None`.
fn edited(value: &Value, name: &str, to: Option<Value>) -> Value {
    let Value::Object(mut map) = value.clone() else {
        panic!("an object")
    };
    match to {
        Some(to) => drop(map.insert(Key::new(name).unwrap(), to)),
        None => drop(map.remove(name)),
    }
    Value::Object(map)
}

/// A workspace whose control stream registers a rule set and confirms the demo version, and a
/// store holding everything the runner stored.
struct World {
    journal: Journal,
    store: Store,
    /// The runner's payload, and its reference.
    payload: Value,
    payload_ref: String,
}

impl World {
    fn new() -> Self {
        let golden = parse(DEMO_STORE.as_bytes()).unwrap();
        let mut store = Store::new();
        for artifact in member(&golden, "artifacts").as_array().unwrap() {
            let stored = store
                .put_artifact(member_text(artifact, "bytes").as_bytes())
                .unwrap();
            assert_eq!(stored.to_string(), member_text(artifact, "ref"));
        }
        let payload_ref = member_text(&golden, "payload").to_owned();
        let reference = ArtifactRef::parse(&payload_ref).unwrap();
        let payload = parse(&store[&reference.digest()]).unwrap();
        let mut world = Self {
            journal: Journal::default(),
            store,
            payload,
            payload_ref,
        };
        world.seed(
            "StreamOpened",
            r#"{"stream_type":"control","workspace_id":"ws1"}"#,
        );
        world.rule_set(RULE_SET);
        let version = world.version().to_owned();
        world.confirmed(&version);
        world
    }

    fn version(&self) -> &str {
        member_text(&self.payload, "mandate_version")
    }

    fn rule_set(&mut self, hash: &str) {
        let payload = format!(
            r#"{{"admits_instruments":null,"content_hash":"{hash}","kind":"rule_set","model_id":null,"model_version":null,"params":[]}}"#
        );
        self.seed("ConfigSnapshotRegistered", &payload);
    }

    fn confirmed(&mut self, version: &str) {
        let payload = format!(
            r#"{{"confirmed_paths":["/name"],"mandate_version":"{version}","record_ref":"{RULE_SET}"}}"#
        );
        self.seed("MandateConfirmed", &payload);
    }

    /// Appends one event as another control-stream writer would.
    fn seed(&mut self, event_type: &str, payload: &str) {
        let control = stream(CONTROL);
        let head = self.journal.head(&control).unwrap().seq;
        let draft = SEEDED
            .replace("@I", &format!("9{head:025}"))
            .replace("@T", event_type)
            .replace("@P", payload);
        let epoch = self.journal.take_ownership(&control).unwrap();
        let outcome = self
            .journal
            .append(&control, head, epoch, at(NOW).at, &[draft.as_bytes()]);
        assert!(matches!(outcome, Ok(AppendOutcome::Committed(_))));
    }

    /// Stores `payload` and returns its reference.
    fn store_payload(&mut self, payload: &Value) -> String {
        self.store
            .put_artifact(&to_canonical(payload))
            .unwrap()
            .to_string()
    }

    /// The runner's payload of a refused run of the same inputs (journal spec §9.18 rule 143).
    fn refused_payload(&self, id: &str) -> Value {
        let payload = edited(&self.payload, "result", Some(text("refused")));
        let payload = edited(&payload, "report", Some(Value::Null));
        let payload = edited(&payload, "refusal", Some(text("corporate_action_in_range")));
        edited(&payload, "backtest_id", Some(text(id)))
    }

    fn record(&mut self, payload_ref: &str) -> Result<Recorded, ControlError> {
        self.record_as(&owner(), payload_ref)
    }

    fn record_as(&mut self, owner: &Owner, payload_ref: &str) -> Result<Recorded, ControlError> {
        record(&mut self.journal, &self.store, owner, payload_ref, at(NOW))
    }

    fn rows(&self) -> usize {
        self.journal.rows(&stream(CONTROL)).unwrap().len()
    }

    /// The last event's envelope, after `mandate-journal`'s own draft check.
    fn last(&self) -> Value {
        let rows = self.journal.rows(&stream(CONTROL)).unwrap();
        let body = &rows.last().unwrap().body;
        let draft = Draft::parse(body).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            (draft.event_type(), draft.schema_version()),
            ("BacktestRunRecorded", 1)
        );
        parse(body).unwrap()
    }
}

fn refused(reason: &'static str) -> Result<Recorded, ControlError> {
    Err(ControlError::Refused { reason })
}

/// DEC-1144 items 5 and 7: the demo run's payload is appended exactly, as the `backtest_runner`
/// system, binding the version and the stream's rule set, listing every reference it names; the
/// journal admits the draft (rules 143 and 144, DEC-1141's writer). A re-run finds it.
#[test]
fn the_runners_demo_run_is_recorded_once_as_the_system_runner() {
    let mut world = World::new();
    let payload_ref = world.payload_ref.clone();
    let recorded = world.record(&payload_ref).unwrap();
    let envelope = world.last();
    assert_eq!(member(&envelope, "payload"), &world.payload);
    let actor = member(&envelope, "actor");
    assert_eq!(member_text(actor, "kind"), "system");
    assert_eq!(member_text(actor, "id"), "backtest_runner");
    let config = member(&envelope, "config_refs");
    assert_eq!(member_text(config, "mandate_version"), world.version());
    assert_eq!(member_text(config, "rule_set"), RULE_SET);
    assert_eq!(config.as_object().unwrap().len(), 2);
    let mut expected: Vec<&str> = [
        "code_build",
        "configuration",
        "data_snapshot",
        "mandate_version",
        "model_outputs",
        "report",
        "theses",
    ]
    .iter()
    .map(|name| member_text(&world.payload, name))
    .collect();
    expected.sort_unstable();
    let listed: Vec<&str> = member(&envelope, "artifact_refs")
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_str().unwrap())
        .collect();
    assert_eq!(listed, expected);
    let report = member_text(&world.payload, "report").to_owned();
    assert_eq!(recorded.backtest_id, "bt-demo-01");
    assert_eq!(recorded.outcome, Ok(report.clone()));
    let mut printed = Vec::new();
    print_recorded(&mut printed, &recorded).unwrap();
    let Recorded { submitted, .. } = &recorded;
    assert_eq!(
        String::from_utf8(printed).unwrap(),
        format!(
            "backtest_id=bt-demo-01\nreport={report}\nrecorded as event {} at seq {}\n",
            submitted.event_id, submitted.seq
        )
    );
    let rows = world.rows();
    assert_eq!(world.record(&payload_ref).unwrap(), recorded);
    assert_eq!(world.rows(), rows, "a re-run commits nothing");
}

/// Journal spec §9.18 rule 143: a refused run is recorded `refused`, with no report, and the
/// journal admits it.
#[test]
fn a_refused_run_is_recorded_refused() {
    let mut world = World::new();
    let payload = world.refused_payload("bt-demo-02");
    let payload_ref = world.store_payload(&payload);
    let recorded = world.record(&payload_ref).unwrap();
    assert_eq!(
        recorded.outcome,
        Err("corporate_action_in_range".to_owned())
    );
    let envelope = world.last();
    let journaled = member(&envelope, "payload");
    assert_eq!(member_text(journaled, "result"), "refused");
    assert_eq!(member(journaled, "report"), &Value::Null);
    let listed = member(&envelope, "artifact_refs").as_array().unwrap().len();
    assert_eq!(listed, 6, "every reference but the report");
    let mut printed = Vec::new();
    print_recorded(&mut printed, &recorded).unwrap();
    let printed = String::from_utf8(printed).unwrap();
    assert!(
        printed.contains("refused=corporate_action_in_range\n"),
        "{printed}"
    );
}

/// DEC-1144 item 5: a payload that is not §9.18's, by its reference, its bytes, or its members,
/// refuses before anything is appended.
#[test]
fn a_payload_that_is_not_the_records_refuses() {
    let base = World::new().payload;
    let cases: Vec<(&str, Value)> = vec![
        ("extra member", edited(&base, "extra", Some(text("x")))),
        ("missing member", edited(&base, "theses", None)),
        ("marker", edited(&base, "marker", Some(text("paper")))),
        (
            "report version",
            edited(
                &base,
                "report_version",
                Some(Value::Int(mandate_canon::Int::new(1).unwrap())),
            ),
        ),
        ("result", edited(&base, "result", Some(text("running")))),
        (
            "completed with a refusal",
            edited(&base, "refusal", Some(text("no_bars"))),
        ),
        (
            "refused with a report",
            edited(&base, "result", Some(text("refused"))),
        ),
        ("id", edited(&base, "backtest_id", Some(text("bt demo")))),
        (
            "long id",
            edited(&base, "backtest_id", Some(text(&"b".repeat(65)))),
        ),
        (
            "code",
            edited(
                &World::new().refused_payload("bt-x"),
                "refusal",
                Some(text("Not A Code")),
            ),
        ),
        (
            "reference",
            edited(&base, "configuration", Some(text("sha256:abc"))),
        ),
    ];
    for (case, payload) in cases {
        let mut world = World::new();
        let payload_ref = world.store_payload(&payload);
        let rows = world.rows();
        assert_eq!(
            world.record(&payload_ref),
            refused("payload_invalid"),
            "{case}"
        );
        assert_eq!(world.rows(), rows, "{case}");
    }
    let mut world = World::new();
    let rows = world.rows();
    assert_eq!(world.record("sha256:abc"), refused("payload_ref_invalid"));
    let absent = format!("sha256:{}", "a".repeat(64));
    assert_eq!(world.record(&absent), refused("payload_missing"));
    let corrupt = ArtifactRef::parse(&world.payload_ref.clone()).unwrap();
    world.store.insert(corrupt.digest(), b"{}".to_vec());
    let payload_ref = world.payload_ref.clone();
    assert_eq!(world.record(&payload_ref), refused("payload_corrupt"));
    assert_eq!(world.rows(), rows);
}

/// DEC-1144 item 5: every artifact the payload names is re-read and re-hashed, and the report is
/// the payload's version's and inputs'; a version is the stream's latest confirmed one, stored as
/// a mandate; and a rule set is registered. Each refuses before anything is appended.
#[test]
fn what_the_payload_names_is_rechecked() {
    type Change = fn(&mut World) -> String;
    let cases: [(&str, Change); 9] = [
        ("artifact_missing", |world| {
            let digest = ArtifactRef::parse(member_text(&world.payload, "configuration"))
                .unwrap()
                .digest();
            world.store.remove(&digest);
            world.payload_ref.clone()
        }),
        ("artifact_corrupt", |world| {
            let digest = ArtifactRef::parse(member_text(&world.payload, "data_snapshot"))
                .unwrap()
                .digest();
            world.store.insert(digest, b"{}".to_vec());
            world.payload_ref.clone()
        }),
        ("report_mismatch", |world| {
            let other = member(&world.payload, "configuration").clone();
            let payload = edited(&world.payload, "report", Some(other));
            world.store_payload(&payload)
        }),
        ("report_mismatch", |world| {
            let other = member(&world.payload, "theses").clone();
            let payload = edited(&world.payload, "model_outputs", Some(other));
            world.store_payload(&payload)
        }),
        ("document_corrupt", |world| {
            let payload = world.refused_payload("bt-x");
            let other = member(&payload, "configuration").clone();
            let payload = edited(&payload, "mandate_version", Some(other.clone()));
            let version = other.as_str().unwrap().to_owned();
            world.confirmed(&version);
            world.store_payload(&payload)
        }),
        ("version_not_confirmed", |world| {
            world.confirmed(RULE_SET);
            world.payload_ref.clone()
        }),
        ("version_not_confirmed", |world| {
            world.journal = Journal::default();
            world.seed(
                "StreamOpened",
                r#"{"stream_type":"control","workspace_id":"ws1"}"#,
            );
            world.rule_set(RULE_SET);
            world.payload_ref.clone()
        }),
        ("rule_set_missing", |world| {
            world.journal = Journal::default();
            world.seed(
                "StreamOpened",
                r#"{"stream_type":"control","workspace_id":"ws1"}"#,
            );
            let version = world.version().to_owned();
            world.confirmed(&version);
            world.payload_ref.clone()
        }),
        ("backtest_id_taken", |world| {
            let payload_ref = world.payload_ref.clone();
            world.record(&payload_ref).unwrap();
            let payload = world.refused_payload("bt-demo-01");
            world.store_payload(&payload)
        }),
    ];
    for (code, change) in cases {
        let mut world = World::new();
        let payload_ref = change(&mut world);
        let rows = world.rows();
        assert_eq!(world.record(&payload_ref), refused(code), "{code}");
        assert_eq!(world.rows(), rows, "{code}");
    }
}

/// DEC-952: a record is appended only in the stream's own environment; DEC-530's backtest owner is
/// refused.
#[test]
fn a_record_in_another_environment_refuses() {
    let mut world = World::new();
    let payload_ref = world.payload_ref.clone();
    let rows = world.rows();
    let live = Owner {
        environment: Environment::Live,
        ..owner()
    };
    assert_eq!(
        world.record_as(&live, &payload_ref),
        refused("workspace_environment")
    );
    let backtest = Owner {
        environment: Environment::Backtest,
        ..owner()
    };
    assert_eq!(
        world.record_as(&backtest, &payload_ref),
        refused("paper_only")
    );
    assert_eq!(world.rows(), rows);
}

/// The read treats two records with one id as unreadable, so `backtest_id_taken` is checked for
/// that id alone: a second backtest under another id is its own record.
#[test]
fn two_backtests_are_two_records() {
    let mut world = World::new();
    let payload_ref = world.payload_ref.clone();
    world.record(&payload_ref).unwrap();
    let rows = world.rows();
    let other = world.refused_payload("bt-demo-02");
    let other_ref = world.store_payload(&other);
    let second = world.record(&other_ref).unwrap();
    assert_eq!(second.backtest_id, "bt-demo-02");
    assert_eq!(world.rows(), rows + 1);
    assert_eq!(member(&world.last(), "payload"), &other);
}

/// The record binds the stream's latest `rule_set` registration, not an earlier one and not the
/// latest registration of another kind.
#[test]
fn the_latest_rule_set_is_bound() {
    let mut world = World::new();
    let newer = format!("sha256:{}", "2".repeat(64));
    world.rule_set(&newer);
    world.seed(
        "ConfigSnapshotRegistered",
        &format!(
            r#"{{"admits_instruments":null,"content_hash":"sha256:{}","kind":"fee_config","model_id":null,"model_version":null,"params":[]}}"#,
            "3".repeat(64)
        ),
    );
    let payload_ref = world.payload_ref.clone();
    world.record(&payload_ref).unwrap();
    let envelope = world.last();
    assert_eq!(
        member_text(member(&envelope, "config_refs"), "rule_set"),
        newer
    );
}

/// A refusal code the read can serve: a lower-case letter, then lower-case letters, digits or
/// `_`, at most 64 in all; anything else refuses `payload_invalid`.
#[test]
fn only_a_servable_refusal_code_is_recorded() {
    for (code, served) in [
        ("a1_b", true),
        (&*"a".repeat(64), true),
        (&*"a".repeat(65), false),
        ("a-b", false),
        ("Abc", false),
        ("1ab", false),
        ("", false),
    ] {
        let mut world = World::new();
        let payload = edited(&world.refused_payload("bt-x"), "refusal", Some(text(code)));
        let payload_ref = world.store_payload(&payload);
        let recorded = world.record(&payload_ref);
        if served {
            assert_eq!(recorded.unwrap().outcome, Err(code.to_owned()));
        } else {
            assert_eq!(recorded, refused("payload_invalid"), "{code}");
        }
    }
}

/// #1578's shared store: `--store` wins, else a non-empty `MANDATE_ARTIFACTS`, else
/// `artifacts_unset`; the record's owner is the workspace in `paper` or `live` and no user.
#[test]
fn the_store_and_the_owner_come_from_the_flags_or_fail_closed() {
    use mandate_cli::backtest::{ARTIFACTS_VAR, RecordArgs, runner_owner, store_root};
    use std::path::{Path, PathBuf};
    let env = Some(std::ffi::OsString::from("/env"));
    let flag = Some(Path::new("/flag"));
    assert_eq!(store_root(flag, env.clone()), Ok(PathBuf::from("/flag")));
    assert_eq!(store_root(flag, None), Ok(PathBuf::from("/flag")));
    assert_eq!(store_root(None, env), Ok(PathBuf::from("/env")));
    for unset in [None, Some(std::ffi::OsString::new())] {
        assert_eq!(
            store_root(None, unset),
            Err(ControlError::Refused {
                reason: "artifacts_unset"
            })
        );
    }
    assert_eq!(ARTIFACTS_VAR, "MANDATE_ARTIFACTS");
    let args = |workspace: &str, environment: &str| RecordArgs {
        workspace: workspace.to_owned(),
        environment: environment.to_owned(),
        payload: String::new(),
        journal: "postgres://journal.invalid/j".to_owned().into(),
        store: None,
    };
    let paper = runner_owner(&args("ws1", "paper")).unwrap();
    assert_eq!(
        (paper.environment, paper.user.as_str()),
        (Environment::Paper, "")
    );
    assert_eq!(
        runner_owner(&args("ws1", "live")).unwrap().environment,
        Environment::Live
    );
    for (workspace, environment, code) in [
        ("ws1", "backtest", "environment_invalid"),
        ("ws1", "Paper", "environment_invalid"),
        ("ws 1", "paper", "owner_workspace_invalid"),
    ] {
        assert_eq!(
            runner_owner(&args(workspace, environment)),
            Err(ControlError::Refused { reason: code }),
            "{workspace} {environment}"
        );
    }
}
