//! `EvaluationRunRecorded`'s payload (journal spec §9.18, DEC-1002 item 7): the report by artifact
//! reference, the digests that reproduce it, and the range of each agent stream the evaluation read.
//! The caller stores the report's bytes before it appends the record (`AGENTS.md` rule 5).

use std::collections::BTreeMap;

use mandate_canon::{Digest, Value};

use crate::report::{
    EVALUATION_REPORT_VERSION, EvaluationReport, decimal, int, object, reference, text,
};
use crate::{EvaluationError, ThesisRecord};

/// What one `EvaluationRunRecorded` says.
#[derive(Debug, Clone, Copy)]
pub struct EvaluationRecord<'a> {
    pub evaluation_id: &'a str,
    pub report: &'a EvaluationReport,
    pub records: &'a [ThesisRecord],
    pub code_build: Digest,
}

/// The payload at `schema_version` 1. `theses_from`, `theses_to` and `theses_head` hold one entry
/// per agent stream read, in stream order (rule 145): the first and the last record's
/// `stream_id:seq`, and the last record's hash.
pub fn evaluation_run_recorded(record: &EvaluationRecord<'_>) -> Result<Value, EvaluationError> {
    let mut streams: BTreeMap<&str, Vec<&ThesisRecord>> = BTreeMap::new();
    for thesis in record.records {
        streams
            .entry(thesis.stream_id.as_str())
            .or_default()
            .push(thesis);
    }
    let ranges: Vec<(&ThesisRecord, &ThesisRecord)> = streams
        .values()
        .filter_map(|read| {
            let first = read.iter().copied().min_by_key(|thesis| thesis.seq)?;
            let last = read.iter().copied().max_by_key(|thesis| thesis.seq)?;
            Some((first, last))
        })
        .collect();
    let position = |thesis: &ThesisRecord| text(&format!("{}:{}", thesis.stream_id, thesis.seq));
    let report = record.report;
    object(vec![
        ("evaluation_id", text(record.evaluation_id)),
        ("decision", text(&report.decision_id)),
        ("cut_off", decimal(report.cut_off)),
        ("theses", reference(&report.inputs.theses)),
        (
            "theses_from",
            Value::Array(ranges.iter().map(|(first, _)| position(first)).collect()),
        ),
        (
            "theses_to",
            Value::Array(ranges.iter().map(|(_, last)| position(last)).collect()),
        ),
        (
            "theses_head",
            Value::Array(
                ranges
                    .iter()
                    .map(|(_, last)| reference(&last.hash))
                    .collect(),
            ),
        ),
        ("closes", reference(&report.inputs.closes)),
        ("cost_model", reference(&report.inputs.cost_model)),
        ("code_build", reference(&record.code_build)),
        ("report", reference(&report.digest()?)),
        ("report_version", int(u64::from(EVALUATION_REPORT_VERSION))?),
        ("passed", Value::Bool(report.passed())),
    ])
}
