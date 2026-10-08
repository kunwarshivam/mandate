//! `ObservationRecorded` as journal spec §9.1 closes it: `source`, `instrument_id`, `as_of` and
//! `data_ref`, and nothing else (DEC-177; DEC-503 item 7, the coordinator's ruling on the first
//! paper trade brief's X-2, slice R0).
//!
//! The hand cases type every expected payload from the spec, with the timestamp written out by hand
//! rather than printed by `mandate_time`. The registered-schema cases hand the draft, in an envelope
//! these tests build themselves, to `mandate_journal::Draft::parse`, which shares no code with the
//! runtime's writer, so a draft the journal would refuse at append fails here.

mod common;

use std::collections::BTreeSet;

use common::{
    ACCOUNT_STREAM, AGENT_STREAM, AllowGate, FixedPlan, Shell, TestIds, event, instrument, object,
    ports, text, universe, with_clock,
};
use mandate_canon::{Digest, Value, to_canonical};
use mandate_journal::{Draft, Invalid, InvalidReason};
use mandate_runtime::{EventDraft, FoldedEvent, Input, Observation, RiskClock, RuntimeError};
use mandate_time::UtcNanos;
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

/// The journal vectors' observed data digest (`journal.yaml`, `agent_stream`, seq 2).
const DATA_HEX: &str = "8b7c2f893ea54b83cbbad125b9d8efe09e8c3c5aedcb62df0f1ddb75fbd101bc";

/// 2026-09-21T13:59:58Z, the vectors' observation cut-off, in seconds since the epoch.
const AS_OF_SECS: i64 = 1_789_999_198;

/// The same instant as journal spec §4.7 writes it, typed by hand.
const AS_OF_TEXT: &str = "2026-09-21T13:59:58.000000000Z";

/// The last second a §4.7 timestamp can hold, 9999-12-31T23:59:59Z.
const LAST_TIMESTAMP_SECS: i64 = 253_402_300_799;

fn reconciliation(seq: u64, at: i64) -> FoldedEvent {
    event(
        ACCOUNT_STREAM,
        seq,
        "ReconciliationRun",
        with_clock(&[("result", text("clean"))], at),
    )
}

fn digest(hex: &str) -> Digest {
    Digest::from_hex(hex).unwrap_or_else(|| panic!("{hex} is 64 hex characters"))
}

fn observation(instrument_id: Option<&str>, as_of: i64) -> Observation {
    Observation {
        source: "alpaca.iex.quotes".to_owned(),
        instrument_id: instrument_id.map(instrument),
        as_of: RiskClock::from_secs(as_of),
        data_ref: digest(DATA_HEX),
    }
}

/// A started shell whose startup hold a clean reconciliation has lifted, so the step under test is
/// the observation's alone.
fn started() -> Shell {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::silent();
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell
        .fold_one(&reconciliation(1, 100))
        .unwrap_or_else(|error| panic!("the reconciliation folds: {error}"));
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::Tick(RiskClock::from_secs(100)), &ports);
    shell
}

fn step(shell: &mut Shell, observation: Observation) -> Result<Vec<EventDraft>, RuntimeError> {
    let ids = TestIds;
    let gate = AllowGate;
    let plan = FixedPlan::silent();
    let view = universe(&["AAPL"]);
    let ports = ports(&ids, &gate, &plan, &view);
    shell
        .step(Input::Observation(observation), &ports)
        .map(|ran| {
            assert!(ran.handed.is_empty(), "an observation hands no intent");
            assert!(ran.timers.is_empty(), "an observation arms no timer");
            ran.drafts
        })
}

/// The one draft an observation step must produce.
fn only_draft(shell: &mut Shell, observation: Observation) -> EventDraft {
    let drafts = step(shell, observation)
        .unwrap_or_else(|error| panic!("step refused with {}: {error}", error.code()));
    let types: Vec<&str> = drafts.iter().map(|d| d.event_type.as_str()).collect();
    assert_eq!(
        types,
        vec!["ObservationRecorded"],
        "an observation is journaled as one ObservationRecorded and nothing else"
    );
    drafts
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("the draft whose type was just read"))
}

/// The draft in an agent-stream envelope of this file's own making, with `artifact_refs` written
/// from `listed` rather than read from the payload, parsed by the journal's registered schemas.
fn parse_as_journal_draft(draft: &EventDraft, listed: &[String]) -> Result<Draft, Invalid> {
    let causation = draft
        .causation_id
        .as_ref()
        .map_or_else(|| "null".to_owned(), |id| format!("\"{}\"", id.0));
    let artifacts = listed
        .iter()
        .map(|reference| format!("\"{reference}\""))
        .collect::<Vec<_>>()
        .join(",");
    let body = format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{}",
        "stream_id":"{AGENT_STREAM}","event_type":"{}","schema_version":1,
        "event_time":"2026-09-21T13:59:58.000000000Z","clock_source":"scheduler",
        "causation_id":{causation},"correlation_id":null,
        "actor":{{"kind":"agent","id":"agent-a","version":"0.1.0","build":"sha256:{}"}},
        "config_refs":{{}},
        "payload":{},"artifact_refs":[{artifacts}],"pii_refs":[]}}"#,
        draft.event_id.0,
        draft.event_type,
        "c".repeat(64),
        String::from_utf8_lossy(&to_canonical(&draft.payload))
    );
    Draft::parse(body.as_bytes())
}

#[test]
#[ignore = "pending E15-13"]
fn an_observation_is_drafted_as_the_closed_section_9_1_payload() {
    let mut shell = started();
    let draft = only_draft(&mut shell, observation(Some("AAPL"), AS_OF_SECS));
    assert_eq!(
        draft.payload,
        object(&[
            ("source", text("alpaca.iex.quotes")),
            ("instrument_id", text("AAPL")),
            ("as_of", text(AS_OF_TEXT)),
            ("data_ref", text(&format!("sha256:{DATA_HEX}"))),
        ]),
        "ObservationRecorded is exactly journal spec §9.1's four members: no `instrument`, `at` or \
         inline `data`, and `as_of` a §4.7 timestamp rather than risk-clock seconds"
    );
    assert_eq!(
        draft.causation_id, None,
        "an observation is caused by nothing on the stream"
    );
}

#[test]
#[ignore = "pending E15-13"]
fn an_observation_about_no_single_instrument_drafts_a_null_instrument_id() {
    let mut shell = started();
    let draft = only_draft(&mut shell, observation(None, AS_OF_SECS));
    assert_eq!(
        draft.payload,
        object(&[
            ("source", text("alpaca.iex.quotes")),
            ("instrument_id", Value::Null),
            ("as_of", text(AS_OF_TEXT)),
            ("data_ref", text(&format!("sha256:{DATA_HEX}"))),
        ]),
        "`instrument_id` is present and `null` for data about no single instrument (journal spec \
         §9.1; §4.2: a member is never absent)"
    );
}

#[test]
#[ignore = "pending E15-13"]
fn the_observation_draft_passes_the_registered_schema_with_its_data_listed_as_an_artifact() {
    let mut shell = started();
    for instrument_id in [Some("AAPL"), None] {
        let draft = only_draft(&mut shell, observation(instrument_id, AS_OF_SECS));
        let parsed = parse_as_journal_draft(&draft, &[format!("sha256:{DATA_HEX}")])
            .unwrap_or_else(|error| {
                panic!("the journal refuses the runtime's ObservationRecorded: {error:?}")
            });
        assert_eq!(parsed.event_type(), "ObservationRecorded");
        let unlisted = parse_as_journal_draft(&draft, &[]).map(|_| ());
        assert!(
            matches!(
                &unlisted,
                Err(Invalid { reason: InvalidReason::ArtifactRefs, path }) if path == "artifact_refs"
            ),
            "the payload's `data_ref` is a stored artifact the envelope must list (journal spec \
             §3), so the same draft with no `artifact_refs` is refused there: {unlisted:?}"
        );
    }
}

#[test]
#[ignore = "pending E15-13"]
fn the_first_and_last_seconds_a_timestamp_holds_are_drafted_as_timestamps() {
    let mut shell = started();
    for (secs, written) in [
        (0, "1970-01-01T00:00:00.000000000Z"),
        (LAST_TIMESTAMP_SECS, "9999-12-31T23:59:59.000000000Z"),
    ] {
        let draft = only_draft(&mut shell, observation(Some("AAPL"), secs));
        assert_eq!(
            draft.payload.get("as_of"),
            Some(&text(written)),
            "the cut-off {secs} is the §4.7 timestamp {written}"
        );
    }
}

#[test]
#[ignore = "pending E15-13"]
fn an_as_of_no_timestamp_can_hold_is_refused_and_journals_nothing() {
    for secs in [-1, LAST_TIMESTAMP_SECS + 1, i64::MIN, i64::MAX] {
        let mut shell = started();
        let before = shell.agent_journal.len();
        let refused = step(&mut shell, observation(Some("AAPL"), secs));
        match refused {
            Err(RuntimeError::NonCanonicalPayload { field }) => assert_eq!(
                field, "as_of",
                "the refusal names the member the cut-off {secs} cannot be written to"
            ),
            other => panic!(
                "the cut-off {secs} lies outside journal spec §4.7's years 1970 to 9999, so the \
                 step must refuse it as a non-canonical `as_of` rather than draft it (AGENTS.md \
                 rule 3); it answered {other:?}"
            ),
        }
        assert_eq!(
            shell.agent_journal.len(),
            before,
            "a refused observation leaves nothing on the agent stream"
        );
    }
}

fn check<S>(strategy: S, body: impl Fn(S::Value) -> Result<(), TestCaseError>)
where
    S: Strategy,
    S::Value: std::fmt::Debug,
{
    let config = ProptestConfig {
        cases: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    if let Err(failure) = proptest::test_runner::TestRunner::new(config).run(&strategy, body) {
        panic!("{failure}");
    }
}

/// Every observation the runtime accepts is drafted as the registered schema reads it, and the
/// draft gives back exactly the observation: the source, the instrument or `null`, the cut-off to
/// the second with no fraction, and the digest the shell stored. The oracle reads the payload as
/// plain canonical values, parses the timestamp with `mandate_time`'s reader, and compares the
/// digest with the hex it generated, so it shares nothing with the runtime's writer.
#[test]
#[ignore = "pending E15-13"]
fn every_observation_draft_is_the_registered_schema_and_reads_back_to_its_observation() {
    let source = "[a-z][a-z0-9._-]{0,24}";
    let instrument_id = proptest::option::of("[A-Z][A-Z0-9.]{0,7}");
    let as_of = 0_i64..=LAST_TIMESTAMP_SECS;
    let hex = "[0-9a-f]{64}";
    check(
        (source, instrument_id, as_of, hex),
        |(source, instrument_id, as_of, hex)| {
            let mut shell = started();
            let given = Observation {
                source: source.clone(),
                instrument_id: instrument_id.as_deref().map(instrument),
                as_of: RiskClock::from_secs(as_of),
                data_ref: digest(&hex),
            };
            let drafts = step(&mut shell, given).map_err(|error| {
                TestCaseError::fail(format!("step refused with {}: {error}", error.code()))
            })?;
            prop_assert_eq!(drafts.len(), 1, "one draft per observation");
            let draft = drafts
                .first()
                .ok_or_else(|| TestCaseError::fail("the draft just counted"))?;
            prop_assert_eq!(draft.event_type.as_str(), "ObservationRecorded");
            let members: BTreeSet<&str> = draft
                .payload
                .as_object()
                .map(|members| members.keys().map(|key| key.as_str()).collect())
                .unwrap_or_default();
            prop_assert_eq!(
                members,
                BTreeSet::from(["as_of", "data_ref", "instrument_id", "source"]),
                "exactly §9.1's members"
            );
            prop_assert_eq!(draft.payload.get("source"), Some(&text(&source)));
            let named = instrument_id.as_deref().map_or(Value::Null, text);
            prop_assert_eq!(draft.payload.get("instrument_id"), Some(&named));
            let reference = format!("sha256:{hex}");
            prop_assert_eq!(draft.payload.get("data_ref"), Some(&text(&reference)));
            let written = draft
                .payload
                .get("as_of")
                .and_then(Value::as_str)
                .ok_or_else(|| TestCaseError::fail("`as_of` is a string"))?;
            let read = UtcNanos::parse(written).map_err(|error| {
                TestCaseError::fail(format!("`as_of` {written} is no timestamp: {error:?}"))
            })?;
            prop_assert_eq!((read.secs(), read.nanos()), (as_of, 0));
            parse_as_journal_draft(draft, &[reference]).map_err(|error| {
                TestCaseError::fail(format!("the journal refuses the draft: {error:?}"))
            })?;
            Ok(())
        },
    );
}
