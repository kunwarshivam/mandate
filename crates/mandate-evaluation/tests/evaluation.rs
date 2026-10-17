//! Hand-calculated cases for the forward-paper harness (E17-8, E15-3, DEC-1002).

mod common;

use common::*;
use mandate_canon::{Digest, Value};
use mandate_evaluation::{EvaluationRecord, SignalModelKey};

/// The thesis of the common closes: as of Monday 2026-09-21 14:00Z, its horizon Wednesday
/// 2026-09-23 21:00Z, after the Wednesday close.
const AS_OF: &str = "2026-09-21T14:00:00Z";
const HORIZON: &str = "2026-09-23T21:00:00Z";
const AFTER: &str = "2026-09-24T00:00:00Z";

/// DEC-1002 items 3 to 5, hand-calculated. Entry is the first close after `as_of`, 100; exit the
/// last close at or before the horizon, 110: a gross return of 0.1. The cost of a 10,000 USD
/// round trip (100 shares) is 3 bps a leg, 3 + 3 = 6 USD, plus the fold's fees: on the buy CAT
/// 100 × 0.00001 = 0.001; on the sell SEC 11,000 × 0.00003 = 0.33, TAF 100 × 0.0002 = 0.02 and
/// CAT 0.001; 0.352 accrued, 0.36 rounded up to the cent. 6.36 ÷ 10,000 = 0.000636. Net 0.099364;
/// the basket returned 0.05 and the index 0.02 over the same window.
#[test]
fn a_closed_thesis_is_scored_net_of_its_round_trip() {
    let stored = stored_research();
    let records = records(stored.0, &[thesis("th_1", AS_OF, HORIZON)]);
    let report = evaluate(&records, &stored, &decision(0), AFTER)
        .unwrap_or_else(|e| panic!("the evaluation completes: {}", e.code()));
    assert_eq!(report.groups.len(), 1);
    let group = &report.groups[0];
    assert_eq!((group.closed, group.open, group.outside_window), (1, 0, 0));
    let scorecard = group
        .outcome
        .as_ref()
        .unwrap_or_else(|code| panic!("{code}"));
    let row = &scorecard.theses[0];
    assert_eq!(row.entry, price("100"));
    assert_eq!(row.exit, price("110"));
    assert_eq!(row.round_trip_cost, ratio("0.000636"));
    assert_eq!(row.net_return, ratio("0.099364"));
    assert_eq!(row.excess_over_basket, ratio("0.049364"));
    assert_eq!(row.excess_over_index, ratio("0.079364"));
}

/// DEC-1002 item 3: a horizon after the cut-off is open, counted and not scored; one outside the
/// registered window is counted `outside_window`, never passed to the scorer.
#[test]
fn an_open_thesis_is_counted_and_never_scored() {
    let stored = stored_research();
    let records = records(
        stored.0,
        &[
            thesis("th_1", AS_OF, HORIZON),
            thesis("th_2", AS_OF, "2026-09-30T21:00:00Z"),
            thesis("th_3", "2026-08-20T14:00:00Z", "2026-08-25T21:00:00Z"),
        ],
    );
    let report =
        evaluate(&records, &stored, &decision(0), AFTER).unwrap_or_else(|e| panic!("{}", e.code()));
    let group = &report.groups[0];
    assert_eq!((group.closed, group.open, group.outside_window), (1, 1, 1));
    let scorecard = group
        .outcome
        .as_ref()
        .unwrap_or_else(|code| panic!("{code}"));
    assert_eq!(scorecard.scoreable_count, 1);
}

/// DEC-1002 item 2 and DEC-99: a refused thesis is scored like an admitted one.
#[test]
fn a_refused_thesis_is_scored_like_an_admitted_one() {
    let stored = stored_research();
    let mut refused = thesis("th_1", AS_OF, HORIZON);
    refused.admitted = false;
    let admitted_report = evaluate(
        &records(stored.0, &[thesis("th_1", AS_OF, HORIZON)]),
        &stored,
        &decision(0),
        AFTER,
    )
    .unwrap_or_else(|e| panic!("{}", e.code()));
    let refused_report = evaluate(&records(stored.0, &[refused]), &stored, &decision(0), AFTER)
        .unwrap_or_else(|e| panic!("{}", e.code()));
    assert_eq!(
        admitted_report.groups[0].outcome,
        refused_report.groups[0].outcome
    );
}

/// DEC-1002 item 4: a new research-agent version starts its own scorecard.
#[test]
fn each_signal_model_version_has_its_own_scorecard() {
    let stored = stored_research();
    let mut next = thesis("th_2", AS_OF, HORIZON);
    next.model_version = "0.2.0";
    next.content_hash = NEXT_HASH;
    let records = records(stored.0, &[thesis("th_1", AS_OF, HORIZON), next]);
    let report =
        evaluate(&records, &stored, &decision(0), AFTER).unwrap_or_else(|e| panic!("{}", e.code()));
    let keys: Vec<&SignalModelKey> = report.groups.iter().map(|group| &group.key).collect();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0].model_version, "0.1.0");
    assert_eq!(keys[1].model_version, "0.2.0");
    for group in &report.groups {
        assert_eq!(group.closed, 1);
    }
}

/// DEC-1002 item 1: a record whose verdict the stored mandate contradicts, or that names a mandate
/// the store does not hold, refuses the evaluation and names the record.
#[test]
fn a_record_that_fails_its_check_refuses_the_evaluation() {
    let stored = stored_research();
    let mut records = records(
        stored.0,
        &[
            thesis("th_1", AS_OF, HORIZON),
            thesis("th_2", AS_OF, HORIZON),
        ],
    );
    records[1].mandate_version = Digest::of(b"another mandate");
    match evaluate(&records, &stored, &decision(0), AFTER) {
        Err(mandate_evaluation::EvaluationError::RecordRejected { index, .. }) => {
            assert_eq!(index, 1)
        }
        other => panic!("expected a rejected record, got {other:?}"),
    }
}

/// DEC-1002 items 4 and 7: a group with nothing closed is the scorer's refusal, never a figure,
/// and an evaluation with no scored group does not pass.
#[test]
fn a_group_with_nothing_closed_is_a_refusal_and_does_not_pass() {
    let stored = stored_research();
    let records = records(stored.0, &[thesis("th_2", AS_OF, "2026-09-30T21:00:00Z")]);
    let report =
        evaluate(&records, &stored, &decision(0), AFTER).unwrap_or_else(|e| panic!("{}", e.code()));
    assert_eq!(report.groups[0].outcome, Err("empty_scoreable_set"));
    assert!(!report.passed());
    let canonical = report
        .canonical()
        .unwrap_or_else(|e| panic!("{}", e.code()));
    assert_eq!(canonical.get("passed"), Some(&Value::Bool(false)));
}

/// The report is reproducible: the same records give the same bytes, and the record names it.
#[test]
fn the_report_is_reproducible_and_its_record_names_it() {
    let stored = stored_research();
    let records = records(
        stored.0,
        &[
            thesis("th_1", AS_OF, HORIZON),
            thesis("th_2", AS_OF, "2026-09-30T21:00:00Z"),
        ],
    );
    let first =
        evaluate(&records, &stored, &decision(0), AFTER).unwrap_or_else(|e| panic!("{}", e.code()));
    let second =
        evaluate(&records, &stored, &decision(0), AFTER).unwrap_or_else(|e| panic!("{}", e.code()));
    let bytes = first
        .canonical_bytes()
        .unwrap_or_else(|e| panic!("{}", e.code()));
    assert_eq!(
        bytes,
        second
            .canonical_bytes()
            .unwrap_or_else(|e| panic!("{}", e.code()))
    );
    let payload = mandate_evaluation::evaluation_run_recorded(&EvaluationRecord {
        evaluation_id: "ev_01",
        report: &first,
        records: &records,
        code_build: Digest::of(b"build"),
    })
    .unwrap_or_else(|e| panic!("{}", e.code()));
    let report_ref = format!(
        "sha256:{}",
        first
            .digest()
            .unwrap_or_else(|e| panic!("{}", e.code()))
            .to_hex()
    );
    assert_eq!(payload.get("report"), Some(&Value::Str(report_ref)));
    assert_eq!(
        payload.get("theses_from"),
        Some(&Value::Array(vec![Value::Str(
            "agent:ws_internal:agt_research:10".to_owned()
        )]))
    );
    assert_eq!(
        payload.get("theses_to"),
        Some(&Value::Array(vec![Value::Str(
            "agent:ws_internal:agt_research:11".to_owned()
        )]))
    );
    assert_eq!(payload.get("passed"), Some(&Value::Bool(first.passed())));
}

/// DEC-1002 item 3 at its edges: a horizon exactly at the cut-off has closed; one exactly at either
/// end of the registered window is inside it, and one a second past its end is outside.
#[test]
fn the_cut_off_and_the_window_are_inclusive() {
    let stored = stored_research();
    let at_cut_off = records(stored.0, &[thesis("th_1", AS_OF, HORIZON)]);
    let report = evaluate(&at_cut_off, &stored, &decision(0), HORIZON)
        .unwrap_or_else(|e| panic!("{}", e.code()));
    assert_eq!((report.groups[0].closed, report.groups[0].open), (1, 0));
    let mut window = decision(0);
    window.window.from = at(HORIZON);
    window.window.to = at(HORIZON);
    let report =
        evaluate(&at_cut_off, &stored, &window, AFTER).unwrap_or_else(|e| panic!("{}", e.code()));
    assert_eq!(
        (report.groups[0].closed, report.groups[0].outside_window),
        (1, 0)
    );
    window.window.to = at("2026-09-23T20:59:59Z");
    let report =
        evaluate(&at_cut_off, &stored, &window, AFTER).unwrap_or_else(|e| panic!("{}", e.code()));
    assert_eq!(
        (report.groups[0].closed, report.groups[0].outside_window),
        (0, 1)
    );
    window.window.to = at(HORIZON);
    window.window.from = at("2026-09-23T21:00:01Z");
    let report =
        evaluate(&at_cut_off, &stored, &window, AFTER).unwrap_or_else(|e| panic!("{}", e.code()));
    assert_eq!(
        (report.groups[0].closed, report.groups[0].outside_window),
        (0, 1)
    );
}

/// DEC-1002 item 3 (review, major 1): a horizon the closes have not reached yet is still open, so
/// its exit is never read from an earlier, stale close; nor is it scored while a baseline's closes
/// stop short of it.
#[test]
fn a_horizon_the_closes_have_not_reached_is_open() {
    let stored = stored_research();
    let late = thesis("th_1", AS_OF, "2026-09-24T21:00:00Z");
    let report = evaluate(
        &records(stored.0, &[late]),
        &stored,
        &decision(0),
        "2026-09-25T00:00:00Z",
    )
    .unwrap_or_else(|e| panic!("{}", e.code()));
    assert_eq!((report.groups[0].closed, report.groups[0].open), (0, 1));
}

/// The record's stream ranges come from the records' own positions, whatever order they arrive in.
#[test]
fn the_record_ranges_each_stream_by_its_lowest_and_highest_seq() {
    let stored = stored_research();
    let mut records = records(
        stored.0,
        &[
            thesis("th_1", AS_OF, HORIZON),
            thesis("th_2", AS_OF, HORIZON),
            thesis("th_3", AS_OF, HORIZON),
        ],
    );
    records[0].seq = 12;
    records[1].seq = 10;
    records[2].seq = 11;
    let report =
        evaluate(&records, &stored, &decision(0), AFTER).unwrap_or_else(|e| panic!("{}", e.code()));
    let payload = mandate_evaluation::evaluation_run_recorded(&EvaluationRecord {
        evaluation_id: "ev_01",
        report: &report,
        records: &records,
        code_build: Digest::of(b"build"),
    })
    .unwrap_or_else(|e| panic!("{}", e.code()));
    let one = |text: &str| Some(Value::Array(vec![Value::Str(text.to_owned())]));
    assert_eq!(
        payload.get("theses_from").cloned(),
        one("agent:ws_internal:agt_research:10")
    );
    assert_eq!(
        payload.get("theses_to").cloned(),
        one("agent:ws_internal:agt_research:12")
    );
    assert_eq!(
        payload.get("theses_head").cloned(),
        one(&format!("sha256:{}", records[0].hash.to_hex()))
    );
}

/// The same journal record read twice refuses the evaluation, naming the repeat.
#[test]
fn a_record_read_twice_refuses_the_evaluation() {
    let stored = stored_research();
    let mut records = records(
        stored.0,
        &[
            thesis("th_1", AS_OF, HORIZON),
            thesis("th_2", AS_OF, HORIZON),
        ],
    );
    records[1].seq = records[0].seq;
    match evaluate(&records, &stored, &decision(0), AFTER) {
        Err(error @ mandate_evaluation::EvaluationError::DuplicateRecord { index: 1 }) => {
            assert_eq!(error.code(), "duplicate_record");
        }
        other => panic!("expected a duplicate record, got {other:?}"),
    }
}

/// Every refusal's stable code (ES-09).
#[test]
fn each_refusal_has_its_stable_code() {
    use mandate_evaluation::EvaluationError;
    let cases = [
        (
            EvaluationError::RecordRejected {
                index: 0,
                code: "invalid_input",
            },
            "record_rejected",
        ),
        (
            EvaluationError::RecordMalformed {
                index: 0,
                member: "as_of",
            },
            "record_malformed",
        ),
        (
            EvaluationError::DuplicateRecord { index: 0 },
            "duplicate_record",
        ),
        (EvaluationError::Accounting("negative"), "negative"),
        (
            EvaluationError::BaselineWithoutCloses,
            "baseline_without_closes",
        ),
        (EvaluationError::Research("empty_closes"), "empty_closes"),
        (
            EvaluationError::Num(mandate_num::NumError::Overflow),
            "overflow",
        ),
    ];
    for (error, code) in cases {
        assert_eq!(error.code(), code, "{error}");
    }
}

/// The artifact's bytes are the canonical form of the report object, and its reference their hash.
#[test]
fn the_artifact_bytes_are_the_canonical_report() {
    let stored = stored_research();
    let report = evaluate(
        &records(stored.0, &[thesis("th_1", AS_OF, HORIZON)]),
        &stored,
        &decision(0),
        AFTER,
    )
    .unwrap_or_else(|e| panic!("{}", e.code()));
    let bytes = report
        .canonical_bytes()
        .unwrap_or_else(|e| panic!("{}", e.code()));
    let canonical = report
        .canonical()
        .unwrap_or_else(|e| panic!("{}", e.code()));
    assert_eq!(bytes, mandate_canon::to_canonical(&canonical));
    assert_eq!(
        report.digest().unwrap_or_else(|e| panic!("{}", e.code())),
        Digest::of(&bytes)
    );
}
