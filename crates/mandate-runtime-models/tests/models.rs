//! The runtime's model composer (DEC-2383 item 7): the confirmed mandate's pinned quant model,
//! evaluated at the tick over the closes the trusted reader admits, against the registry folded
//! from the control stream. Each answer is checked against an oracle the test computes its own way:
//! the crossover from integer cents, the session's close from the calendar's published hours, and
//! the closes object as hand-written canonical JSON. Every refusal is no output (MI-10).

mod common;

use std::fs;
use std::path::Path;

use common::*;
use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::Value;
use mandate_daemon::ArtifactSink;
use mandate_daemon::models::{Evaluated, Models, Streams};
use mandate_journal::{ArtifactStore, Environment, StoredEvent};
use mandate_runtime::ModelDirection;
use mandate_runtime_models::{
    Held, IEX_SOURCE, PinnedModels, RECORDED_SOURCE, Registry, registry, source, wiring,
};
use mandate_time::Date;
use proptest::prelude::*;

/// Whether the crossover is long on `series`: the mean of the last five closes above the mean of
/// the last twenty, compared exactly in cents as 20 × fast sum against 5 × slow sum.
fn long(series: &[(Date, u32)]) -> bool {
    let sum = |n: usize| -> u64 {
        series
            .iter()
            .rev()
            .take(n)
            .map(|(_, c)| u64::from(*c))
            .sum()
    };
    series.len() >= 20 && sum(5) * 20 > sum(20) * 5
}

/// The one output the oracle expects for `series`, checked member by member.
fn assert_expected(answer: &[Evaluated], series: &[(Date, u32)]) {
    let [evaluated] = answer else {
        panic!("one output: {answer:?}");
    };
    let output = &evaluated.output;
    assert_eq!(output.model_id, MODEL);
    assert_eq!(output.model_version, "1.0.0");
    assert_eq!(output.content_hash.to_hex(), PINNED_HASH);
    assert_eq!(output.instrument_id.as_str(), QQQ);
    assert_eq!(output.direction, ModelDirection::Long);
    assert_eq!(
        output.as_of,
        tick(LAST_CLOSE),
        "the close it read, not the tick"
    );
    assert_eq!(output.expires_at.secs(), at(LAST_CLOSE).secs() + 86_400);
    assert_eq!(output.horizon_s, 86_400);
    assert_eq!(evaluated.source, RECORDED_SOURCE);
    assert_eq!(
        String::from_utf8(evaluated.closes.clone()).unwrap(),
        String::from_utf8(closes_json(series)).unwrap()
    );
}

/// A pinned mandate reads no stream.
const NO_STREAMS: Streams<'static> = Streams {
    control: None,
    agent: &[],
};

fn composer(root: &Root, control: &[StoredEvent], document: &Value) -> Option<PinnedModels> {
    PinnedModels::new(
        document,
        registry(control, Environment::Paper)?,
        &root.bars(),
    )
}

#[test]
fn the_cell_demo_mandate_pins_the_content_the_host_runs() {
    assert_eq!(
        mandate_modelhost::content(MODEL, "1.0.0")
            .unwrap()
            .hash
            .to_hex(),
        PINNED_HASH
    );
}

#[test]
fn a_rising_series_gives_the_pinned_model_s_long_output_at_the_close_it_read() {
    let root = Root::new("rising");
    let series = rising(30);
    store_series(&root.bars(), "QQQ", &series, None);
    let models = composer(&root, &registered(), &document()).unwrap();
    assert_expected(&models.evaluate(tick(NOW), NO_STREAMS), &series);
    assert_eq!(
        models.evaluate(tick(NOW), NO_STREAMS),
        models.evaluate(tick(NOW), NO_STREAMS),
        "the same tick over the same partitions answers the same"
    );
}

/// DEC-2283 item 6: over an `alpaca-iex` root the composer reads the daily generation its tick
/// names, so the same series stored there gives the same output, named by the IEX source; a series
/// stored only in the generation of the session before gives none.
#[test]
fn a_generation_root_gives_the_output_of_the_generation_its_tick_names() {
    let root = Root::new("generations");
    let series = rising(30);
    let generations = root.generations();
    store_series(
        &mandate_bars::generations::daily_generation(&generations, last()),
        "QQQ",
        &series,
        None,
    );
    let models = PinnedModels::new(
        &document(),
        registry(&registered(), Environment::Paper).unwrap(),
        &generations,
    )
    .unwrap();
    let answer = models.evaluate(tick(NOW), NO_STREAMS);
    let [evaluated] = answer.as_slice() else {
        panic!("one output: {answer:?}");
    };
    assert_eq!(evaluated.source, IEX_SOURCE);
    assert_eq!(evaluated.output.direction, ModelDirection::Long);
    assert_eq!(evaluated.output.as_of, tick(LAST_CLOSE));
    assert_eq!(
        String::from_utf8(evaluated.closes.clone()).unwrap(),
        String::from_utf8(closes_json(&series)).unwrap()
    );

    let earlier = Root::new("earlier-generation");
    let before = sessions(31, last());
    let shifted: Vec<(Date, u32)> = before[..30]
        .iter()
        .zip(series.iter())
        .map(|(day, (_, cents))| (*day, *cents))
        .collect();
    let generations = earlier.generations();
    store_series(
        &mandate_bars::generations::daily_generation(&generations, before[29]),
        "QQQ",
        &shifted,
        None,
    );
    let models = PinnedModels::new(
        &document(),
        registry(&registered(), Environment::Paper).unwrap(),
        &generations,
    )
    .unwrap();
    assert!(
        models.evaluate(tick(NOW), NO_STREAMS).is_empty(),
        "the generation of the session before is never read in its place"
    );
}

#[test]
fn a_falling_or_flat_series_gives_no_output() {
    for (name, step) in [("falling", -10i64), ("flat", 0)] {
        let root = Root::new(name);
        let series: Vec<(Date, u32)> = sessions(30, last())
            .into_iter()
            .zip((0i64..).map(|n| u32::try_from(20_000 + step * n).unwrap()))
            .collect();
        store_series(&root.bars(), "QQQ", &series, None);
        let models = composer(&root, &registered(), &document()).unwrap();
        assert_eq!(models.evaluate(tick(NOW), NO_STREAMS), Vec::new(), "{name}");
    }
}

/// A mandate pinning `quant.ma_crossover` 1.1.0 (DEC-2390) gets its exit output on a falling
/// series, conviction −1, handed on like a `Long`; 1.0.0 gives none on the same closes.
#[test]
fn a_pinned_1_1_0_s_exit_output_is_handed_on_on_a_falling_series() {
    let root = Root::new("exit");
    let series: Vec<(Date, u32)> = sessions(30, last())
        .into_iter()
        .zip((0u32..).map(|n| 20_000 - 10 * n))
        .collect();
    store_series(&root.bars(), "QQQ", &series, None);
    let exit_hash = mandate_modelhost::content(MODEL, "1.1.0")
        .unwrap()
        .hash
        .to_hex();
    let pinned = edited(|text| {
        text.replace(PINNED_HASH, &exit_hash)
            .replace(r#""version":"1.0.0""#, r#""version":"1.1.0""#)
    });
    let control = [registration_of(1, "paper", ("1.1.0", &exit_hash), PARAMS)];
    let answer = composer(&root, &control, &pinned)
        .unwrap()
        .evaluate(tick(NOW), NO_STREAMS);
    let [evaluated] = answer.as_slice() else {
        panic!("one output: {answer:?}");
    };
    assert_eq!(evaluated.output.model_version, "1.1.0");
    assert_eq!(evaluated.output.conviction.to_string(), "-1");
    assert_eq!(evaluated.output.as_of, tick(LAST_CLOSE));
    assert_eq!(
        String::from_utf8(evaluated.closes.clone()).unwrap(),
        String::from_utf8(closes_json(&series)).unwrap()
    );
    assert_eq!(
        composer(&root, &registered(), &document())
            .unwrap()
            .evaluate(tick(NOW), NO_STREAMS),
        Vec::new()
    );
}

#[test]
fn the_registry_is_the_latest_registration_of_each_model_in_the_environment() {
    let other = "b".repeat(64);
    let latest = registry(
        &[
            registration(1, "paper", &other, PARAMS),
            rule_set(2),
            registration(3, "live", &other, PARAMS),
            registration(4, "paper", PINNED_HASH, PARAMS),
            registration(5, "live", &other, PARAMS),
            control_row(6, "AgentDeployed", "paper", r#"{"agent_id":"x"}"#),
        ],
        Environment::Paper,
    )
    .unwrap();
    let entries: Vec<(String, String, String)> = latest
        .iter()
        .map(|(id, model)| {
            (
                id.as_str().to_owned(),
                model.version.clone(),
                model.content_hash.to_hex(),
            )
        })
        .collect();
    assert_eq!(
        entries,
        vec![(MODEL.to_owned(), "1.0.0".to_owned(), PINNED_HASH.to_owned())]
    );
    assert_eq!(
        registry(&[rule_set(1)], Environment::Paper),
        Some(Registry::new())
    );
}

#[test]
fn a_registration_that_does_not_read_gives_no_registry() {
    let unreadable = [
        control_row(
            2,
            "ConfigSnapshotRegistered",
            "paper",
            r#"{"kind":"model_version","model_id":"quant.ma_crossover"}"#,
        ),
        control_row(
            2,
            "ConfigSnapshotRegistered",
            "paper",
            r#"{"model_id":"x"}"#,
        ),
    ];
    for row in unreadable {
        let mut control = registered();
        control.push(row);
        assert_eq!(registry(&control, Environment::Paper), None);
    }
    let mut torn = registration(3, "paper", PINNED_HASH, PARAMS);
    torn.body.truncate(10);
    assert_eq!(registry(&[torn], Environment::Paper), None);
    let mut bare = control_row(3, "ConfigSnapshotRegistered", "paper", "1");
    assert_eq!(
        registry(&[bare.clone()], Environment::Paper),
        None,
        "a payload that is no object"
    );
    let body = String::from_utf8(bare.body).unwrap();
    bare.body = body.replace(r#","payload":1"#, "").into_bytes();
    assert_eq!(registry(&[bare], Environment::Paper), None, "no payload");
    assert_eq!(
        registry(&registered(), Environment::Live),
        Some(Registry::new()),
        "another environment's rows are not read at all"
    );
}

#[test]
fn the_recording_and_the_iex_generations_name_a_source_and_nothing_else_does() {
    for (root, named) in [
        ("/srv/artifacts/datasets/alpaca-iex", Some(IEX_SOURCE)),
        ("alpaca-iex/", Some(IEX_SOURCE)),
        ("/srv/artifacts/datasets/alpaca-iexx", None),
        ("/srv/artifacts/datasets/alpaca-iex/daily", None),
        (
            "/srv/artifacts/datasets/cell-recorded",
            Some(RECORDED_SOURCE),
        ),
        (
            "/srv/artifacts/datasets/cell-recorded/",
            Some(RECORDED_SOURCE),
        ),
        ("cell-recorded", Some(RECORDED_SOURCE)),
        ("/srv/artifacts/datasets/alpaca", None),
        ("/srv/artifacts/datasets/cell-recorded-x", None),
        ("/", None),
        ("", None),
    ] {
        assert_eq!(source(Path::new(root)), named, "{root}");
    }
}

#[test]
fn the_wiring_needs_a_recorded_bars_root_a_readable_registry_and_a_pinned_quant_model() {
    let root = Root::new("wiring");
    let store = || FsArtifactStore::open(root.artifacts()).unwrap();
    let bars = root.bars();
    let bars = bars.to_str();
    let wired = |control: &[StoredEvent], document: &Value, bars: Option<&str>| {
        wiring(control, Environment::Paper, document, bars, store()).is_some()
    };
    assert!(wired(&registered(), &document(), bars));
    assert!(!wired(&registered(), &document(), None), "no root");
    assert!(
        !wired(&registered(), &document(), Some("")),
        "an empty root"
    );
    assert!(
        !wired(&registered(), &document(), Some("/srv/datasets/alpaca")),
        "a root no source names"
    );
    let mut torn = registered();
    torn[1].body.truncate(10);
    assert!(!wired(&torn, &document(), bars), "an unreadable registry");
    let not_quant = edited(|text| {
        text.replace(
            r#""id":"quant.ma_crossover""#,
            r#""id":"fast.ma_crossover""#,
        )
    });
    assert!(
        !wired(&registered(), &not_quant, bars),
        "no quant model, nothing to evaluate"
    );
    assert!(
        !wired(&registered(), &Value::Null, bars),
        "a document that does not parse"
    );
}

#[test]
fn held_confirms_the_stored_closes_and_never_writes() {
    let root = Root::new("held");
    let series = rising(30);
    let held_bytes = closes_json(&series);
    FsArtifactStore::open(root.artifacts())
        .unwrap()
        .put_artifact(&held_bytes)
        .unwrap();
    let listing = || {
        let mut files = Vec::new();
        let mut dirs = vec![root.artifacts()];
        while let Some(dir) = dirs.pop() {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    dirs.push(path);
                } else {
                    files.push(path);
                }
            }
        }
        files.sort();
        files
    };
    let before = listing();
    let mut held = Held(FsArtifactStore::open_existing(root.artifacts()).unwrap());
    assert!(held.put(&held_bytes));
    assert!(
        !held.put(&closes_json(&rising(29))),
        "bytes the store does not hold"
    );
    assert!(!held.put(b""), "nothing is not held");
    assert_eq!(listing(), before, "the read-only runtime wrote nothing");
}

/// A way the evaluation's inputs can be wrong; each is no output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fault {
    None,
    Unregistered,
    RegisteredInAnotherEnvironment,
    RegisteredWithAnotherHash,
    RegisteredWithOtherParams,
    ReRegisteredWithAnotherHash,
    PinnedHashDiffers,
    SessionMissing,
    AnotherSymbolStored,
    StaleClock,
    FewerThanSlow,
}

fn faults() -> impl Strategy<Value = Fault> {
    prop_oneof![
        4 => Just(Fault::None),
        1 => Just(Fault::Unregistered),
        1 => Just(Fault::RegisteredInAnotherEnvironment),
        1 => Just(Fault::RegisteredWithAnotherHash),
        1 => Just(Fault::RegisteredWithOtherParams),
        1 => Just(Fault::ReRegisteredWithAnotherHash),
        1 => Just(Fault::PinnedHashDiffers),
        1 => Just(Fault::SessionMissing),
        1 => Just(Fault::AnotherSymbolStored),
        1 => Just(Fault::StaleClock),
        1 => Just(Fault::FewerThanSlow),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, failure_persistence: None, ..ProptestConfig::default() })]

    /// For any closes and any one fault, the composer answers exactly what the oracle derives
    /// from the stored series, the registrations and the clock on its own: the pinned model's
    /// `Long` output with the closes it read when nothing is wrong and the crossover is long, and
    /// nothing otherwise.
    #[test]
    fn the_answer_is_the_oracle_s(
        cents in prop::collection::vec(10_000u32..20_000, 20..=30),
        fault in faults(),
        missing in 1usize..19,
    ) {
        let root = Root::new("prop");
        let count = if fault == Fault::FewerThanSlow { 19 } else { cents.len() };
        let series: Vec<(Date, u32)> = sessions(count, last()).into_iter().zip(cents.iter().copied()).collect();
        let symbol = if fault == Fault::AnotherSymbolStored { "SPY" } else { "QQQ" };
        let skip = (fault == Fault::SessionMissing).then_some(missing);
        store_series(&root.bars(), symbol, &series, skip);
        let other = "c".repeat(64);
        let control = match fault {
            Fault::Unregistered => vec![rule_set(1)],
            Fault::RegisteredInAnotherEnvironment => vec![registration(1, "live", PINNED_HASH, PARAMS)],
            Fault::RegisteredWithAnotherHash => vec![registration(1, "paper", &other, PARAMS)],
            Fault::RegisteredWithOtherParams => vec![registration(1, "paper", PINNED_HASH, r#"["fast_periods"]"#)],
            Fault::ReRegisteredWithAnotherHash => vec![
                registration(1, "paper", PINNED_HASH, PARAMS),
                registration(2, "paper", &other, PARAMS),
            ],
            _ => registered(),
        };
        let document = if fault == Fault::PinnedHashDiffers {
            edited(|text| text.replace(PINNED_HASH, &other))
        } else {
            document()
        };
        let clock = if fault == Fault::StaleClock { STALE } else { NOW };
        let answer = composer(&root, &control, &document).unwrap().evaluate(tick(clock), NO_STREAMS);
        if fault == Fault::None && long(&series) {
            assert_expected(&answer, &series);
        } else {
            prop_assert_eq!(answer, Vec::new(), "{:?}", fault);
        }
    }
}
