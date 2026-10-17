//! The backtest runner (DEC-1144) on the cell's demo mandate (`deploy/cell/demo/mandate.json`) and
//! the demo inputs under `fixtures/demo/`: a completed run, a refused one, determinism, and every
//! refusal of the inputs, each storing no payload.
//!
//! `fixtures/demo-store.json` is the whole store a completed demo run leaves, which
//! `mandate-cli`'s `backtest record` tests journal: the contract between the two steps. A changed
//! run fails here first; `MANDATE_BLESS_RUNNER=1 cargo nextest run -p mandate-backtest-runner`
//! rewrites the file, which a reviewer then reads as the run's change.

use std::collections::BTreeMap;

use mandate_backtest_runner::{InputFiles, Inputs, Ran, RunnerError, decode, run};
use mandate_canon::{Digest, Key, Object, Value, parse, to_canonical};
use mandate_journal::{ArtifactRef, ArtifactStore, Draft, get_artifact};

type Store = BTreeMap<Digest, Vec<u8>>;

const MANDATE: &str = include_str!("../../../deploy/cell/demo/mandate.json");
const BARS: &str = include_str!("fixtures/demo/bars.json");
const OUTPUTS: &str = include_str!("fixtures/demo/outputs.json");
const THESES: &str = include_str!("fixtures/demo/theses.json");
const INSTRUMENT: &str = include_str!("fixtures/demo/instrument.json");
const SETTINGS: &str = include_str!("fixtures/demo/settings.json");
const GOLDEN: &str = "tests/fixtures/demo-store.json";
const RULE_SET: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";

/// The five demo files, with `edit` applied to the one it names: `(file, from, to)`.
fn files(edit: Option<(&str, &str, &str)>) -> [String; 5] {
    let mut texts = [BARS, OUTPUTS, THESES, INSTRUMENT, SETTINGS].map(str::to_owned);
    if let Some((name, from, to)) = edit {
        let index = mandate_backtest_runner::FILE_NAMES
            .iter()
            .position(|n| *n == name)
            .unwrap();
        assert!(texts[index].contains(from), "{name} holds {from}");
        texts[index] = texts[index].replacen(from, to, 1);
    }
    texts
}

fn decoded(edit: Option<(&str, &str, &str)>) -> Result<Inputs, RunnerError> {
    let [bars, outputs, theses, instrument, settings] = files(edit);
    decode(&InputFiles {
        bars: bars.as_bytes(),
        outputs: outputs.as_bytes(),
        theses: theses.as_bytes(),
        instrument: instrument.as_bytes(),
        settings: settings.as_bytes(),
    })
}

/// A store holding the demo mandate in canonical form, and its version.
fn store() -> (Store, String) {
    let mut store = Store::new();
    let canonical = to_canonical(&parse(MANDATE.as_bytes()).unwrap());
    let version = store.put_artifact(&canonical).unwrap().to_string();
    (store, version)
}

fn ran(edit: Option<(&str, &str, &str)>) -> (Store, Ran) {
    let (mut store, version) = store();
    let ran = run(&mut store, "bt-demo-01", &version, &decoded(edit).unwrap()).unwrap();
    (store, ran)
}

fn payload(store: &Store, ran: &Ran) -> Value {
    parse(&get_artifact(store, &ran.payload).unwrap()).unwrap()
}

fn member<'a>(value: &'a Value, name: &str) -> &'a Value {
    value.get(name).unwrap_or_else(|| panic!("{name}"))
}

fn object(pairs: Vec<(&str, Value)>) -> Value {
    let mut map = Object::new();
    for (k, v) in pairs {
        map.insert(Key::new(k).unwrap(), v);
    }
    Value::Object(map)
}

/// `payload` as the CLI's `backtest_runner` system writer appends it: `config_refs` binding the
/// version and a rule set, and every digest reference the payload holds in `artifact_refs`,
/// sorted; then `mandate-journal`'s own draft check (journal spec §9.18, rules 143 and 144).
fn journaled(payload: &Value) -> Result<Draft, mandate_journal::Invalid> {
    let version = member(payload, "mandate_version").clone();
    let mut refs: Vec<String> = payload
        .as_object()
        .unwrap()
        .values()
        .filter_map(Value::as_str)
        .filter(|s| ArtifactRef::parse(s).is_some())
        .map(str::to_owned)
        .collect();
    refs.sort();
    refs.dedup();
    let text = |s: &str| Value::Str(s.to_owned());
    let envelope = object(vec![
        (
            "actor",
            object(vec![
                ("build", text(RULE_SET)),
                ("id", text("backtest_runner")),
                ("kind", text("system")),
                ("version", text("0.0.0")),
            ]),
        ),
        (
            "artifact_refs",
            Value::Array(refs.iter().map(|r| text(r)).collect()),
        ),
        ("causation_id", Value::Null),
        ("clock_source", text("local")),
        (
            "config_refs",
            object(vec![
                ("mandate_version", version),
                ("rule_set", text(RULE_SET)),
            ]),
        ),
        ("correlation_id", Value::Null),
        ("environment", text("paper")),
        (
            "envelope_version",
            Value::Int(mandate_canon::Int::new(1).unwrap()),
        ),
        ("event_id", text("01J8ZA00000000000000000001")),
        ("event_time", text("2026-10-08T13:00:00.000000000Z")),
        ("event_type", text("BacktestRunRecorded")),
        ("payload", payload.clone()),
        ("pii_refs", Value::Array(Vec::new())),
        (
            "schema_version",
            Value::Int(mandate_canon::Int::new(1).unwrap()),
        ),
        ("stream_id", text("ctl:ws1")),
    ]);
    Draft::parse(&to_canonical(&envelope))
}

/// DEC-1144 items 2 and 3: the demo mandate over the demo inputs completes. Every reference the
/// payload names is stored and re-hashes, the report is the version's, and the payload passes the
/// journal's own check as the system runner would append it.
#[test]
fn the_demo_completes_and_every_reference_resolves() {
    let (store, ran) = ran(None);
    let payload = payload(&store, &ran);
    let report_ref = ran.outcome.unwrap();
    assert_eq!(member(&payload, "result"), &Value::Str("completed".into()));
    assert_eq!(member(&payload, "refusal"), &Value::Null);
    assert_eq!(
        member(&payload, "report"),
        &Value::Str(report_ref.to_string())
    );
    assert_eq!(
        member(&payload, "backtest_id"),
        &Value::Str("bt-demo-01".into())
    );
    for name in [
        "mandate_version",
        "data_snapshot",
        "model_outputs",
        "theses",
        "configuration",
        "code_build",
        "report",
    ] {
        let reference = ArtifactRef::parse(member(&payload, name).as_str().unwrap()).unwrap();
        get_artifact(&store, &reference).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
    let report = parse(&get_artifact(&store, &report_ref).unwrap()).unwrap();
    assert_eq!(
        member(&report, "kind"),
        &Value::Str("backtest_report".into())
    );
    assert_eq!(member(&report, "report_version").as_int(), Some(2));
    assert_eq!(
        member(&report, "mandate_version"),
        member(&payload, "mandate_version")
    );
    let inputs = member(&report, "inputs");
    for (input, named) in [
        ("bars", "data_snapshot"),
        ("model_outputs", "model_outputs"),
        ("theses", "theses"),
        ("config", "configuration"),
    ] {
        assert_eq!(member(inputs, input), member(&payload, named), "{input}");
    }
    let decisions = member(&report, "decisions");
    assert_eq!(
        member(decisions, "count").as_int(),
        Some(24),
        "one decision at each of the 25 daily bars but the first, which no close precedes"
    );
    let outcomes = member(decisions, "outcomes");
    let asked = member(outcomes, "ask_unanswered").as_int().unwrap();
    assert!(
        asked > 0,
        "the demo asks every opening, and nobody answers: {outcomes:?}"
    );
    assert_eq!(
        member(outcomes, "submitted").as_int(),
        Some(0),
        "{outcomes:?}"
    );
    journaled(&payload).unwrap();
    let printed = ran.to_string();
    assert_eq!(
        printed,
        format!(
            "backtest_id=bt-demo-01\npayload={}\nreport={report_ref}\n",
            ran.payload
        )
    );
}

/// DEC-1001 item 4, journal spec §9.18 rule 143: a run `run_version` refuses still stores its
/// payload, `refused` with the stable code and no report, and the journal admits it.
#[test]
fn a_refused_run_stores_a_refused_payload() {
    let (store, ran) = ran(Some((
        "bars.json",
        r#""corporate_actions": []"#,
        r#""corporate_actions": ["2026-09-15"]"#,
    )));
    assert_eq!(ran.outcome, Err("corporate_action_in_range"));
    let payload = payload(&store, &ran);
    assert_eq!(member(&payload, "result"), &Value::Str("refused".into()));
    assert_eq!(member(&payload, "report"), &Value::Null);
    assert_eq!(
        member(&payload, "refusal"),
        &Value::Str("corporate_action_in_range".into())
    );
    journaled(&payload).unwrap();
    assert!(
        ran.to_string()
            .ends_with("refused=corporate_action_in_range\n")
    );
}

/// FR-4.5: the same inputs give the same payload, and the store is the committed snapshot that
/// `mandate-cli`'s record tests read.
#[test]
fn the_run_is_deterministic_and_matches_the_committed_store() {
    let (first_store, first) = ran(None);
    let (second_store, second) = ran(None);
    assert_eq!(first, second);
    assert_eq!(first_store, second_store);
    let snapshot = format!(
        "{}\n",
        String::from_utf8(to_canonical(&snapshot(&first_store, &first))).unwrap()
    );
    if std::env::var_os("MANDATE_BLESS_RUNNER").is_some() {
        std::fs::write(GOLDEN, &snapshot).unwrap();
    }
    let committed = std::fs::read_to_string(GOLDEN)
        .unwrap_or_else(|_| panic!("{GOLDEN} is missing; bless it with MANDATE_BLESS_RUNNER=1"));
    assert_eq!(committed, snapshot, "bless with MANDATE_BLESS_RUNNER=1");
}

/// The store as the golden file holds it: the payload's reference, and every artifact's bytes as
/// text, by reference. A reference is not a canonical key, so the artifacts are a list of pairs.
fn snapshot(store: &Store, ran: &Ran) -> Value {
    let text = |s: String| Value::Str(s);
    let artifacts = store
        .iter()
        .map(|(digest, bytes)| {
            object(vec![
                ("bytes", text(String::from_utf8(bytes.clone()).unwrap())),
                ("ref", text(format!("sha256:{}", digest.to_hex()))),
            ])
        })
        .collect();
    object(vec![
        ("artifacts", Value::Array(artifacts)),
        ("payload", text(ran.payload.to_string())),
    ])
}

/// DEC-1144 item 3: a missing, unknown, or malformed member refuses the run before anything is
/// stored, naming the file and member.
#[test]
fn a_malformed_input_refuses_naming_its_member() {
    let cases = [
        (
            "settings.json",
            r#""min_order_usd": "1","#,
            "",
            "settings.json/min_order_usd",
        ),
        (
            "settings.json",
            r#""account_type": "margin""#,
            r#""account_type": "margin", "extra": 1"#,
            "settings.json/extra",
        ),
        (
            "bars.json",
            r#""open": "99""#,
            r#""open": 99"#,
            "bars.json/bars/0/open",
        ),
        (
            "outputs.json",
            r#""direction": "long""#,
            r#""direction": "short""#,
            "outputs.json/outputs/0/direction",
        ),
        (
            "instrument.json",
            r#""exchange": "arca""#,
            r#""exchange": "lse""#,
            "instrument.json/exchange",
        ),
        (
            "settings.json",
            r#""model": "fixed""#,
            r#""model": "linear""#,
            "settings.json/sim/slippage/model",
        ),
        (
            "theses.json",
            r#""theses": []"#,
            r#""theses": {}"#,
            "theses.json/theses",
        ),
    ];
    for (file, from, to, path) in cases {
        let error = decoded(Some((file, from, to))).unwrap_err();
        assert_eq!(error, RunnerError::Input(path.to_owned()), "{file}: {to}");
        assert_eq!(error.code(), "input_invalid");
    }
}

/// DEC-1144 item 4: the id, the version, and the stored mandate are checked before anything is
/// stored; a refusal leaves the store as it was.
#[test]
fn a_bad_id_or_mandate_stores_nothing() {
    let inputs = decoded(None).unwrap();
    let (store, version) = store();
    let missing = format!("sha256:{}", "a".repeat(64));
    let spaced = store_spaced();
    let cases: [(&str, &str, Store, RunnerError); 5] = [
        (
            "bt demo",
            &version,
            store.clone(),
            RunnerError::BacktestIdInvalid,
        ),
        ("", &version, store.clone(), RunnerError::BacktestIdInvalid),
        (
            "bt-1",
            "sha256:abc",
            store.clone(),
            RunnerError::MandateVersionInvalid,
        ),
        ("bt-1", &missing, store.clone(), RunnerError::MandateMissing),
        (
            "bt-1",
            &spaced.1,
            spaced.0.clone(),
            RunnerError::MandateInvalid,
        ),
    ];
    for (id, version, before, expected) in cases {
        let mut store = before.clone();
        let error = run(&mut store, id, version, &inputs).unwrap_err();
        assert_eq!(error, expected, "{id} {version}");
        assert_eq!(store, before, "{}", error.code());
    }
    let long = "b".repeat(65);
    let mut store = store;
    assert_eq!(
        run(&mut store, &long, &version, &inputs).unwrap_err(),
        RunnerError::BacktestIdInvalid
    );
    let longest = "b".repeat(64);
    assert!(run(&mut store, &longest, &version, &inputs).is_ok());
}

/// The demo mandate stored with a space before it: bytes that re-hash to their reference but are
/// not the version's canonical document.
fn store_spaced() -> (Store, String) {
    let mut store = Store::new();
    let version = store
        .put_artifact(format!(" {MANDATE}").as_bytes())
        .unwrap()
        .to_string();
    (store, version)
}

/// DEC-1144 item 4: a mandate the backtest's context rejects refuses the run, storing nothing:
/// here one whose order cap is above its own allocation.
#[test]
fn a_mandate_the_context_rejects_refuses() {
    let document = MANDATE.replace(r#""max_order_usd":"300""#, r#""max_order_usd":"2000""#);
    assert_ne!(document, MANDATE);
    let mut store = Store::new();
    let canonical = to_canonical(&parse(document.as_bytes()).unwrap());
    let version = store.put_artifact(&canonical).unwrap().to_string();
    let before = store.clone();
    let error = run(&mut store, "bt-1", &version, &decoded(None).unwrap()).unwrap_err();
    assert_eq!(error, RunnerError::MandateRejected);
    assert_eq!(store, before);
}

/// The binary over the fixture directory and an empty store holding the demo mandate prints the
/// same three lines the library's run does; a directory missing a file refuses, naming it, and
/// prints nothing on stdout.
#[test]
fn the_binary_reads_the_directory_and_prints_the_references() {
    let root = std::env::temp_dir().join(format!("mandate-backtest-runner-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut fs_store = mandate_artifacts_fs::FsArtifactStore::open(&root).unwrap();
    let canonical = to_canonical(&parse(MANDATE.as_bytes()).unwrap());
    let version = fs_store.put_artifact(&canonical).unwrap().to_string();
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/demo");
    let invoke = |inputs: &str| {
        std::process::Command::new(env!("CARGO_BIN_EXE_mandate-backtest-runner"))
            .args(["run", "--inputs", inputs, "--store"])
            .arg(&root)
            .args(["--mandate-version", &version, "--backtest-id", "bt-demo-01"])
            .output()
            .unwrap()
    };
    let output = invoke(fixtures);
    assert!(output.status.success(), "{output:?}");
    let (_, expected) = ran(None);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        expected.to_string()
    );
    let missing = invoke(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"));
    assert!(!missing.status.success());
    assert!(missing.stdout.is_empty());
    let stderr = String::from_utf8(missing.stderr).unwrap();
    assert!(
        stderr.contains("input_invalid") && stderr.contains("bars.json"),
        "{stderr}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// `read_dir` reads the five files by their names: over the fixture directory it decodes what the
/// files' own bytes decode to, so the run is the same.
#[test]
fn reading_the_directory_decodes_the_files() {
    let dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/demo"));
    let read = mandate_backtest_runner::read_dir(dir).unwrap();
    let (mut store, version) = store();
    let from_dir = run(&mut store, "bt-demo-01", &version, &read).unwrap();
    assert_eq!(from_dir, ran(None).1);
}
