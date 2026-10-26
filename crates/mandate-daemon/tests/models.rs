//! The deployed runtime's pinned quant models (DEC-2383): which of the host's evaluations at a tick
//! are new to the agent stream, and the core inputs one evaluation makes.

use std::collections::BTreeSet;

use mandate_accounting::InstrumentId;
use mandate_canon::Digest;
use mandate_daemon::models::{Evaluated, ModelsError, fresh, inputs};
use mandate_journal::StoredEvent;
use mandate_num::{Conviction, Unit};
use mandate_runtime::{Input, ModelDirection, ModelOutput, Observation, RiskClock};
use mandate_time::UtcNanos;
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

const AGENT_STREAM: &str = "agent:ws1:demo-spy";
const SPY: &str = "b28f4066-5c6d-479b-a2af-85dc1a8f16fb";
const QQQ: &str = "c2a6f2f3-5f1e-4b2b-9f0a-1c2d3e4f5a6b";
const SOURCE: &str = "data.daily_bars";
/// 2026-10-23T20:00:00Z, a regular session's close.
const CLOSE: i64 = 1_792_785_600;
const DAY: i64 = 86_400;

fn stamp(secs: i64) -> String {
    UtcNanos::from_parts(secs, 0).unwrap().to_string()
}

fn output(model: &str, version: &str, instrument: &str, as_of: i64) -> ModelOutput {
    ModelOutput {
        model_id: model.into(),
        model_version: version.into(),
        content_hash: Digest::of(model.as_bytes()),
        instrument_id: InstrumentId::new(instrument).unwrap(),
        as_of: RiskClock::from_secs(as_of),
        expires_at: RiskClock::from_secs(as_of + DAY),
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
    }
}

fn evaluated(model: &str, version: &str, instrument: &str, as_of: i64) -> Evaluated {
    Evaluated {
        output: output(model, version, instrument, as_of),
        closes: format!("closes of {instrument} through {as_of}").into_bytes(),
        source: SOURCE.into(),
    }
}

/// An agent-stream record of `event_type` at `seq` with `payload`, as the runtime commits one.
fn row(seq: u64, event_type: &str, payload: &str) -> StoredEvent {
    let event_id = format!("01J8Z3MK{seq:018}");
    let body = format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{event_id}","stream_id":"{AGENT_STREAM}","event_type":"{event_type}","schema_version":2,"event_time":"2026-10-23T20:00:01.000000000Z","clock_source":"scheduler","causation_id":null,"correlation_id":null,"actor":{{"kind":"runtime","id":"x","version":"1","build":null}},"config_refs":{{}},"payload":{payload},"artifact_refs":[],"pii_refs":[]}}"#
    );
    StoredEvent {
        stream_id: AGENT_STREAM.into(),
        seq,
        event_id,
        event_type: event_type.into(),
        schema_version: 2,
        environment: "paper".into(),
        recorded_at: "2026-10-23T20:00:01.000000000Z".into(),
        prev_hash: Digest::of(b"prev"),
        hash: Digest::of(body.as_bytes()),
        body: body.into_bytes(),
    }
}

/// The `ModelOutputRecorded` the runtime journals for `e`, with the members `fresh` reads.
fn recorded(seq: u64, e: &Evaluated) -> StoredEvent {
    let o = &e.output;
    let payload = format!(
        r#"{{"model_id":"{}","model_version":"{}","instrument_id":"{}","as_of":"{}","expires_at":"{}","direction":"long","conviction":"1","confidence":"1","horizon_s":86400}}"#,
        o.model_id,
        o.model_version,
        o.instrument_id.as_str(),
        stamp(o.as_of.secs()),
        stamp(o.expires_at.secs()),
    );
    row(seq, "ModelOutputRecorded", &payload)
}

/// The key the oracle compares, computed from the evaluation's own fields.
fn key(e: &Evaluated) -> (String, String, String, i64) {
    let o = &e.output;
    (
        o.model_id.clone(),
        o.model_version.clone(),
        o.instrument_id.as_str().to_owned(),
        o.as_of.secs(),
    )
}

#[test]
fn nothing_journaled_keeps_every_evaluation_in_order() {
    let all = vec![
        evaluated("quant.ma_crossover", "1.0.0", SPY, CLOSE),
        evaluated("quant.ma_crossover", "1.0.0", QQQ, CLOSE),
    ];
    assert_eq!(fresh(&[], all.clone()).unwrap(), all);
    assert_eq!(fresh(&[], Vec::new()).unwrap(), Vec::new());
}

#[test]
fn an_output_already_journaled_is_not_handed_again() {
    let today = evaluated("quant.ma_crossover", "1.0.0", SPY, CLOSE);
    let agent = vec![recorded(4, &today)];
    assert_eq!(
        fresh(&agent, vec![today.clone()]).unwrap(),
        Vec::new(),
        "a tick every second, and a restart, record one daily output once"
    );
    let tomorrow = evaluated("quant.ma_crossover", "1.0.0", SPY, CLOSE + DAY);
    assert_eq!(
        fresh(&agent, vec![today, tomorrow.clone()]).unwrap(),
        vec![tomorrow],
        "the next session's close is a new output"
    );
}

#[test]
fn another_version_or_instrument_is_a_new_output() {
    let journaled = evaluated("quant.ma_crossover", "1.0.0", SPY, CLOSE);
    let agent = vec![recorded(4, &journaled)];
    let other_version = evaluated("quant.ma_crossover", "1.1.0", SPY, CLOSE);
    let other_instrument = evaluated("quant.ma_crossover", "1.0.0", QQQ, CLOSE);
    let other_model = evaluated("quant.momentum", "1.0.0", SPY, CLOSE);
    let all = vec![other_version, other_instrument, other_model];
    assert_eq!(fresh(&agent, all.clone()).unwrap(), all);
}

#[test]
fn only_a_model_output_record_counts_as_journaled() {
    let today = evaluated("quant.ma_crossover", "1.0.0", SPY, CLOSE);
    let observation = format!(
        r#"{{"source":"{SOURCE}","instrument_id":"{SPY}","as_of":"{}","data_ref":"sha256:{}"}}"#,
        stamp(CLOSE),
        Digest::of(&today.closes).to_hex()
    );
    let agent = vec![row(3, "ObservationRecorded", &observation)];
    assert_eq!(
        fresh(&agent, vec![today.clone()]).unwrap(),
        vec![today],
        "an observation alone, as a pass that stopped between the two leaves, is not the output"
    );
}

#[test]
fn an_unreadable_model_output_record_refuses_rather_than_hands_twice() {
    let today = evaluated("quant.ma_crossover", "1.0.0", SPY, CLOSE);
    let broken = row(
        4,
        "ModelOutputRecorded",
        r#"{"model_id":"quant.ma_crossover"}"#,
    );
    assert_eq!(
        fresh(&[broken], vec![today]),
        Err(ModelsError::Unreadable),
        "a record the process cannot read never lets it journal what that record may already be"
    );
}

#[test]
fn an_evaluation_is_its_observation_then_its_output() {
    let today = evaluated("quant.ma_crossover", "1.0.0", SPY, CLOSE);
    let data_ref = Digest::of(&today.closes);
    let [first, second] = inputs(&today, data_ref).unwrap();
    assert_eq!(
        first,
        Input::Observation(Observation {
            source: SOURCE.into(),
            instrument_id: Some(InstrumentId::new(SPY).unwrap()),
            as_of: RiskClock::from_secs(CLOSE),
            data_ref,
        }),
        "the closes the host read, stored, cut off at the output's as_of (DEC-503 items 5, 6)"
    );
    assert_eq!(
        second,
        Input::ModelOutput(today.output),
        "the host's output, unchanged"
    );
}

/// For any evaluations and any of them already journaled, `fresh` keeps exactly those whose key
/// no `ModelOutputRecorded` names, in the order given. A plain test over proptest's runner, so the
/// pending gate sees one function (DEC-77).
#[test]
fn fresh_keeps_exactly_the_unjournaled_keys() {
    let models = ["quant.ma_crossover", "quant.momentum"];
    let versions = ["1.0.0", "1.1.0"];
    let instruments = [SPY, QQQ];
    let cases = (
        prop::collection::vec((0usize..2, 0usize..2, 0usize..2, 0i64..3), 0..8),
        prop::collection::vec(any::<bool>(), 8),
    );
    let mut runner = TestRunner::new(ProptestConfig::with_cases(128));
    runner
        .run(&cases, |(picks, journal_mask)| {
            let mut seen = BTreeSet::new();
            let all: Vec<Evaluated> = picks
                .iter()
                .map(|&(m, v, i, d)| {
                    evaluated(models[m], versions[v], instruments[i], CLOSE + d * DAY)
                })
                .filter(|e| seen.insert(key(e)))
                .collect();
            let journaled: Vec<&Evaluated> = all
                .iter()
                .zip(&journal_mask)
                .filter(|(_, j)| **j)
                .map(|(e, _)| e)
                .collect();
            let agent: Vec<StoredEvent> = journaled
                .iter()
                .zip(1u64..)
                .map(|(e, seq)| recorded(seq, e))
                .collect();
            let keys: BTreeSet<_> = journaled.iter().map(|e| key(e)).collect();
            let expected: Vec<Evaluated> = all
                .iter()
                .filter(|e| !keys.contains(&key(e)))
                .cloned()
                .collect();
            prop_assert_eq!(fresh(&agent, all).unwrap(), expected);
            Ok(())
        })
        .unwrap();
}
