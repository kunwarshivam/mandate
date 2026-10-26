//! The backtest runner (DEC-1144) on the cell's demo mandate (`deploy/cell/demo/mandate.json`) and
//! the demo inputs under `fixtures/demo/`: a completed run, a refused one, determinism, and every
//! refusal of the inputs, each storing no payload.
//!
//! `fixtures/demo-store.json` is the whole store a completed demo run leaves, which
//! `mandate-cli`'s `backtest record` tests journal: the contract between the two steps. A changed
//! run fails here first; `MANDATE_BLESS_RUNNER=1 cargo nextest run -p mandate-backtest-runner`
//! rewrites the file, which a reviewer then reads as the run's change.

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::AccountType;
use mandate_num::{Bps, Price, Qty, TickRule, Usd};
use mandate_risk::DayTradeRegime;
use mandate_sim::{FirstBarVolumes, Session, Slippage};
use mandate_time::UtcNanos;

use mandate_backtest_runner::{InputFiles, Inputs, Ran, RunnerError, decode, run};
use mandate_canon::{Digest, Key, Object, Value, parse, to_canonical};
use mandate_journal::{ArtifactRef, ArtifactStore, Draft, get_artifact};
use mandate_num::SplitRatio;
use mandate_time::Date;
use mandate_version_backtest::RecordedAction;

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
/// payload, `refused` with the stable code and no report, and the journal admits it. Two splits out
/// of ex-date order refuse the run (DEC-2245 item 7).
#[test]
fn a_refused_run_stores_a_refused_payload() {
    let (store, ran) = ran(Some((
        "bars.json",
        r#""corporate_actions": []"#,
        r#""corporate_actions": [{"kind": "split", "ex_date": "2026-09-15", "ratio": {"new": 2, "old": 1}, "cash_in_lieu_price": null}, {"kind": "split", "ex_date": "2026-09-10", "ratio": {"new": 2, "old": 1}, "cash_in_lieu_price": null}]"#,
    )));
    assert_eq!(ran.outcome, Err("corporate_actions_unordered"));
    let payload = payload(&store, &ran);
    assert_eq!(member(&payload, "result"), &Value::Str("refused".into()));
    assert_eq!(member(&payload, "report"), &Value::Null);
    assert_eq!(
        member(&payload, "refusal"),
        &Value::Str("corporate_actions_unordered".into())
    );
    journaled(&payload).unwrap();
    assert!(
        ran.to_string()
            .ends_with("refused=corporate_actions_unordered\n")
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

/// DEC-1144 items 2 and 4: the id, the version, and the stored mandate are checked before anything is
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
    let document = MANDATE.replace(r#""max_order_usd":"1000""#, r#""max_order_usd":"2000""#);
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
    let runner = || {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_mandate-backtest-runner"));
        command.env_remove("MANDATE_ARTIFACTS");
        command
    };
    let invoke = |inputs: &str| {
        runner()
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

/// DEC-1144 item 3: each spelling the format admits decodes to its own variant, read back from the
/// decoded inputs rather than through a run, so a reader that dropped an arm or swapped two
/// variants fails here.
#[test]
fn each_spelling_decodes_to_its_variant() {
    let edited = |file, from, to| decoded(Some((file, from, to))).unwrap();
    let base = decoded(None).unwrap();
    assert_eq!(base.instrument.tick, TickRule::RegNmsEquity);
    let tick = edited(
        "instrument.json",
        "\"rule\": \"reg_nms_equity\"",
        "\"rule\": \"increment\", \"increment\": \"0.05\"",
    );
    assert_eq!(
        tick.instrument.tick,
        TickRule::Increment(Price::parse("0.05").unwrap())
    );
    assert_eq!(
        base.settings.sim.slippage,
        Slippage::Fixed {
            half_spread_bps: Bps::parse("1").unwrap(),
            impact_bps: Bps::parse("2").unwrap(),
        }
    );
    let sqrt = edited(
        "settings.json",
        "\"model\": \"fixed\",\n   \"half_spread_bps\": \"1\",\n   \"impact_bps\": \"2\"",
        "\"model\": \"sqrt\", \"half_spread_bps\": \"1\", \"coefficient_bps\": \"3\"",
    );
    assert_eq!(
        sqrt.settings.sim.slippage,
        Slippage::Sqrt {
            half_spread_bps: Bps::parse("1").unwrap(),
            coefficient_bps: Bps::parse("3").unwrap(),
        }
    );
    assert_eq!(
        base.settings.regime,
        DayTradeRegime::IntradayMargin {
            maintenance_excess: Usd::parse("100000").unwrap()
        }
    );
    let legacy = edited(
        "settings.json",
        "\"kind\": \"intraday_margin\",\n  \"maintenance_excess\": \"100000\"",
        "\"kind\": \"legacy_pdt\"",
    );
    assert_eq!(legacy.settings.regime, DayTradeRegime::LegacyPdt);
    assert_eq!(base.settings.account_type, AccountType::Margin);
    let cash = edited(
        "settings.json",
        "\"account_type\": \"margin\"",
        "\"account_type\": \"cash\"",
    );
    assert_eq!(cash.settings.account_type, AccountType::Cash);
    assert!(base.stop_limit_classes.is_empty());
    let stop_limit = edited(
        "settings.json",
        "\"order_type\": \"stop_limit\",\n      \"quantity_form\": \"whole\",\n      \"times_in_force\": [\n       \"day\",\n       \"gtc\"\n      ],\n      \"protection_forms\": []",
        "\"order_type\": \"stop_limit\", \"quantity_form\": \"whole\", \"times_in_force\": [\"gtc\"], \"protection_forms\": [\"stop_limit\"]",
    );
    assert_eq!(
        stop_limit.stop_limit_classes,
        BTreeSet::from([mandate_accounting::AssetClass::UsEquity])
    );
    let first = base.bars[0].start;
    assert_eq!(
        base.medians.median_at(first),
        Some(Qty::parse("1500000").unwrap())
    );
    let unknown = edited(
        "bars.json",
        "\"first_bar_median\": \"1500000\"",
        "\"first_bar_median\": null",
    );
    assert_eq!(unknown.medians.median_at(first), None);
    assert_eq!(
        unknown.medians.median_at(base.bars[1].start),
        base.medians.median_at(base.bars[1].start)
    );
    assert_eq!(base.coverage_start, first);
    assert!(base.corporate_actions.is_empty() && base.theses.is_empty());
    assert_eq!(base.outputs.len(), 25);
    assert!(
        base.bars
            .iter()
            .all(|bar| bar.auction && bar.session == Session::Regular)
    );
}

/// DEC-1144 item 3: a member the format types as one thing refuses another, and a variant that
/// takes no member refuses one.
#[test]
fn a_member_of_another_type_refuses() {
    let cases = [
        (
            "instrument.json",
            "\"rule\": \"reg_nms_equity\"",
            "\"rule\": \"reg_nms_equity\", \"increment\": \"0.01\"",
            "instrument.json/tick/increment",
        ),
        (
            "settings.json",
            "\"kind\": \"intraday_margin\",\n  \"maintenance_excess\": \"100000\"",
            "\"kind\": \"legacy_pdt\", \"maintenance_excess\": \"1\"",
            "settings.json/regime/maintenance_excess",
        ),
        (
            "settings.json",
            "\"mark_max_age_s\": 3600",
            "\"mark_max_age_s\": 4294967296",
            "settings.json/mark_max_age_s",
        ),
        (
            "settings.json",
            "\"mark_max_age_s\": 3600",
            "\"mark_max_age_s\": \"3600\"",
            "settings.json/mark_max_age_s",
        ),
        (
            "bars.json",
            "\"auction\": true",
            "\"auction\": \"true\"",
            "bars.json/bars/0/auction",
        ),
        (
            "bars.json",
            "\"session\": \"regular\"",
            "\"session\": \"Regular\"",
            "bars.json/bars/0/session",
        ),
        (
            "bars.json",
            "\"corporate_actions\": []",
            "\"corporate_actions\": {}",
            "bars.json/corporate_actions",
        ),
        (
            "outputs.json",
            "\"content_hash\": \"sha256:",
            "\"content_hash\": \"sha512:",
            "outputs.json/outputs/0/content_hash",
        ),
        (
            "settings.json",
            "\"decision_latency_ms\": 0",
            "\"decision_latency_ms\": \"0\"",
            "settings.json/sim/decision_latency_ms",
        ),
        (
            "settings.json",
            "\"valid_from\": \"2026-09-01\"",
            "\"valid_from\": \"2027-09-01\"",
            "settings.json/fees/calendar",
        ),
        (
            "settings.json",
            "\"profile_version\": 1",
            "\"profile_version\": 0",
            "settings.json/profile",
        ),
    ];
    for (file, from, to, path) in cases {
        let error = decoded(Some((file, from, to))).unwrap_err();
        assert_eq!(error, RunnerError::Input(path.to_owned()), "{file}: {to}");
    }
    let thesis = r#""theses": [{"thesis_id": "", "received_at": "2026-09-01T13:00:00Z", "instrument": "b28f4066-5c6d-479b-a2af-85dc1a8f16fb", "as_of": "2026-09-01T13:00:00Z", "expires_at": "2026-12-01T00:00:00Z", "admitted": true, "confidence": "0.8", "invalidated_at": null}]"#;
    let error = decoded(Some(("theses.json", "\"theses\": []", thesis))).unwrap_err();
    assert_eq!(
        error,
        RunnerError::Input("theses.json/theses/0/thesis_id".to_owned())
    );
    let named = thesis
        .replace("\"thesis_id\": \"\"", "\"thesis_id\": \"th-1\"")
        .replace(
            "\"invalidated_at\": null",
            "\"invalidated_at\": \"2026-09-02T13:00:00Z\"",
        );
    let read = decoded(Some(("theses.json", "\"theses\": []", &named))).unwrap();
    let [thesis] = read.theses.as_slice() else {
        panic!("one thesis")
    };
    assert_eq!(thesis.thesis_id, "th-1");
    assert!(thesis.admitted);
    assert_eq!(
        thesis.invalidated_at,
        Some(UtcNanos::parse_rfc3339("2026-09-02T13:00:00Z").unwrap())
    );
}

/// #1578's shared store: the flag wins, else a non-empty `MANDATE_ARTIFACTS`, else the run refuses
/// `artifacts_unset` (fail closed).
#[test]
fn the_store_is_the_flag_or_the_cells_variable() {
    use mandate_backtest_runner::store_root;
    use std::path::PathBuf;
    let flag = Some(PathBuf::from("/flag"));
    let env = Some(std::ffi::OsString::from("/env"));
    assert_eq!(
        store_root(flag.clone(), env.clone()),
        Ok(PathBuf::from("/flag"))
    );
    assert_eq!(store_root(flag, None), Ok(PathBuf::from("/flag")));
    assert_eq!(store_root(None, env), Ok(PathBuf::from("/env")));
    assert_eq!(
        store_root(None, Some("".into())),
        Err(RunnerError::StoreUnset)
    );
    assert_eq!(store_root(None, None), Err(RunnerError::StoreUnset));
    assert_eq!(RunnerError::StoreUnset.code(), "artifacts_unset");
    assert_eq!(mandate_backtest_runner::ARTIFACTS_VAR, "MANDATE_ARTIFACTS");
}

/// `bars.json` holding exactly these corporate-action records.
fn with_actions(records: &str) -> Result<Inputs, RunnerError> {
    let replaced = format!(r#""corporate_actions": [{records}]"#);
    decoded(Some(("bars.json", r#""corporate_actions": []"#, &replaced)))
}

fn date(text: &str) -> Date {
    Date::parse(text).unwrap_or_else(|e| panic!("{e}"))
}

const SPLIT: &str = r#"{"kind": "split", "ex_date": "2026-08-14", "ratio": {"new": 1, "old": 10}, "cash_in_lieu_price": "40.5"}"#;
const DIVIDEND: &str = r#"{"kind": "cash_dividend", "ex_date": "2026-08-18", "pay_date": "2026-08-28", "per_share": "0.25"}"#;

/// DEC-2245 item 1: each kind decodes to its record, in the file's order; a dividend may be paid
/// on its ex-date.
#[test]
fn each_action_kind_decodes_to_its_record() {
    let inputs =
        with_actions(&format!("{SPLIT}, {DIVIDEND}")).unwrap_or_else(|e| panic!("decodes: {e:?}"));
    assert_eq!(
        inputs.corporate_actions,
        [
            RecordedAction::Split {
                ex_date: date("2026-08-14"),
                ratio: SplitRatio::new(1, 10).unwrap(),
                cash_in_lieu_price: Some(Price::parse("40.5").unwrap()),
            },
            RecordedAction::CashDividend {
                ex_date: date("2026-08-18"),
                pay_date: date("2026-08-28"),
                per_share: Price::parse("0.25").unwrap(),
            },
        ]
    );
    let same_day = DIVIDEND.replace(r#""2026-08-28""#, r#""2026-08-18""#);
    let inputs = with_actions(&same_day).unwrap_or_else(|e| panic!("decodes: {e:?}"));
    assert!(matches!(
        inputs.corporate_actions.as_slice(),
        [RecordedAction::CashDividend { pay_date, .. }] if *pay_date == date("2026-08-18")
    ));
    let no_price = SPLIT.replace(r#""40.5""#, "null");
    let inputs = with_actions(&no_price).unwrap_or_else(|e| panic!("decodes: {e:?}"));
    assert!(matches!(
        inputs.corporate_actions.as_slice(),
        [RecordedAction::Split {
            cash_in_lieu_price: None,
            ..
        }]
    ));
}

/// DEC-2245 item 1: an action of another kind, or a record missing, adding, or mistyping a member,
/// refuses, naming it; a dividend paid before its ex-date and a split of zero shares refuse too.
#[test]
fn a_malformed_action_refuses_naming_its_member() {
    let at = "bars.json/corporate_actions/0";
    let cases = [
        (
            SPLIT.replace(r#""split""#, r#""spin_off""#),
            format!("{at}/kind"),
        ),
        (
            SPLIT.replace(r#""kind": "split", "#, ""),
            format!("{at}/kind"),
        ),
        (
            SPLIT.replace(r#", "cash_in_lieu_price": "40.5""#, ""),
            format!("{at}/cash_in_lieu_price"),
        ),
        (
            SPLIT.replace(r#""new": 1"#, r#""new": 0"#),
            format!("{at}/ratio"),
        ),
        (
            SPLIT.replace(r#""new": 1"#, r#""new": "1""#),
            format!("{at}/ratio/new"),
        ),
        (
            SPLIT.replace(r#""old": 10}"#, r#""old": 10, "extra": 1}"#),
            format!("{at}/ratio/extra"),
        ),
        (
            SPLIT.replace(r#""40.5""#, r#""-1""#),
            format!("{at}/cash_in_lieu_price"),
        ),
        (
            SPLIT.replace(r#""2026-08-14""#, r#""2026-08-14T00:00:00Z""#),
            format!("{at}/ex_date"),
        ),
        (
            DIVIDEND.replace(r#""2026-08-28""#, r#""2026-08-17""#),
            format!("{at}/pay_date"),
        ),
        (
            DIVIDEND.replace(r#""0.25""#, r#""0""#),
            format!("{at}/per_share"),
        ),
        (
            DIVIDEND.replace(r#""per_share""#, r#""amount""#),
            format!("{at}/amount"),
        ),
        (
            DIVIDEND.replace(r#""0.25"}"#, r#""0.25", "cash_in_lieu_price": null}"#),
            format!("{at}/cash_in_lieu_price"),
        ),
        (r#""2026-08-14""#.to_owned(), at.to_owned()),
    ];
    for (record, path) in cases {
        assert_eq!(
            with_actions(&record).map(|_| ()),
            Err(RunnerError::Input(path.clone())),
            "{record}"
        );
    }
}
