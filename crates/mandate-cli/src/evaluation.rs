//! `mandate evaluation run` ([DEC-2244](../../../docs/project/decisions/DEC-2244.md); backlog
//! E17-8, E15-3): the forward-paper evaluation of an internal paper workspace's thesis records,
//! read from the workspace's own journal and journaled as `EvaluationRunRecorded` (journal spec
//! §9.18): whether the workspace is an internal paper one (item 4), the thesis records of its
//! agents' streams (item 2), and the run that evaluates, stores and appends (items 1 and 5).

use std::collections::BTreeSet;
use std::ffi::OsString;

use mandate_canon::{Value, parse};
use mandate_evaluation::{InputFiles, ThesisRecord};
use mandate_journal::{ArtifactRef, ArtifactStore, StoredEvent};
use mandate_time::UtcNanos;

use crate::control::{
    ControlError, ControlJournal, Now, Owner, Submitted, agent_stream, control_stream,
};
use crate::version::{refused, rows};

/// The agent-stream records the evaluation reads (journal spec §9.4).
const THESIS_TYPES: [&str; 2] = ["ThesisProposed", "ThesisRevised"];

/// The operator's setting that marks the workspace an internal paper one (DEC-2244 item 4, read as
/// DEC-1143 item 1 reads the API's).
pub const INTERNAL_PAPER_VAR: &str = "MANDATE_INTERNAL_PAPER_WORKSPACE";

/// The setting's value: on only for exactly `true`; off when absent, empty, or exactly `false`.
///
/// # Errors
/// [`ControlError::Refused`] with `internal_paper_setting` for any other value.
pub fn internal_paper_setting(value: Option<OsString>) -> Result<bool, ControlError> {
    match value.as_ref().map(|v| v.to_str()) {
        None => Ok(false),
        Some(Some("" | "false")) => Ok(false),
        Some(Some("true")) => Ok(true),
        Some(_) => Err(refused("internal_paper_setting")),
    }
}

/// Refuses unless `setting` is on and every row of the workspace's control stream, which must
/// hold one or more, is `paper` (DEC-2244 item 4). It reads no agent stream.
///
/// # Errors
/// [`ControlError::Refused`] with `not_internal_paper`; [`ControlError::Journal`] when the stream
/// cannot be read.
pub fn internal_paper(
    journal: &dyn ControlJournal,
    owner: &Owner,
    setting: bool,
) -> Result<(), ControlError> {
    let stored = journal.rows(&control_stream(owner)?)?;
    let paper = !stored.is_empty() && stored.iter().all(|row| row.environment == "paper");
    if setting && paper {
        Ok(())
    } else {
        Err(refused("not_internal_paper"))
    }
}

/// Every `ThesisProposed` and `ThesisRevised` of the workspace's agents (DEC-2244 item 2): the
/// agents are the `agent_id`s of the control stream's `AgentDeployed` records, each read once, in
/// id order, and each stream's records in stream order.
///
/// # Errors
/// [`ControlError::Refused`] with `record_unbound` for a record with no
/// `config_refs.mandate_version`, or `no_theses` when no agent stream holds one;
/// [`ControlError::Journal`] when a stream cannot be read or a stored event is not canonical.
pub fn thesis_records(
    journal: &dyn ControlJournal,
    owner: &Owner,
) -> Result<Vec<ThesisRecord>, ControlError> {
    let agents: BTreeSet<String> = rows(journal, owner)?
        .iter()
        .filter(|row| row.event_type == "AgentDeployed")
        .filter_map(|row| row.member("agent_id"))
        .map(str::to_owned)
        .collect();
    let mut records = Vec::new();
    for agent in &agents {
        for stored in journal.rows(&agent_stream(owner, agent)?)? {
            if THESIS_TYPES.contains(&stored.event_type.as_str()) {
                records.push(thesis_record(stored)?);
            }
        }
    }
    if records.is_empty() {
        return Err(refused("no_theses"));
    }
    Ok(records)
}

/// One stored thesis record, with the version its envelope binds.
fn thesis_record(stored: StoredEvent) -> Result<ThesisRecord, ControlError> {
    let envelope =
        parse(&stored.body).map_err(|e| ControlError::Journal(format!("a stored event: {e:?}")))?;
    let mandate_version = envelope
        .get("config_refs")
        .and_then(|refs| refs.get("mandate_version"))
        .and_then(Value::as_str)
        .and_then(ArtifactRef::parse)
        .ok_or_else(|| refused("record_unbound"))?
        .digest();
    Ok(ThesisRecord {
        stream_id: stored.stream_id,
        seq: stored.seq,
        hash: stored.hash,
        mandate_version,
        payload: envelope.get("payload").cloned().unwrap_or(Value::Null),
    })
}

/// What `run` committed or found: the evaluation, its report, whether it passed, and the event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluated {
    pub evaluation_id: String,
    pub report: String,
    pub passed: bool,
    pub submitted: Submitted,
}

/// One evaluation as `run` is asked for it: the four input files' bytes, the id the API will name
/// it by, and the instant it reads up to.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    pub files: InputFiles<'a>,
    pub evaluation_id: &'a str,
    pub cut_off: UtcNanos,
}

/// Runs the forward-paper evaluation of the workspace's thesis records and journals it
/// (DEC-2244 items 1, 2, 4 and 5): the internal paper gate, then the records, the inputs and the
/// evaluation; then it stores the build text, the three input objects, the report and the payload,
/// re-reads each by its digest, and appends `EvaluationRunRecorded` as the `system` `evaluator`.
/// A re-run of the same evaluation finds the record it committed (DEC-290).
///
/// # Errors
/// [`ControlError::Refused`], having appended nothing: `internal_paper` refusals,
/// `evaluation_id_invalid`, `input_invalid`, [`thesis_records`]' refusals, the evaluator's own
/// code when it refuses the evaluation (having stored nothing), `artifact_corrupt` for a stored
/// object that does not re-hash, and `evaluation_id_taken` for an id another record holds.
/// [`ControlError::Journal`] as `commit` reports.
pub fn run(
    journal: &mut dyn ControlJournal,
    store: &mut dyn ArtifactStore,
    owner: &Owner,
    setting: bool,
    request: &Request<'_>,
    now: Now,
) -> Result<Evaluated, ControlError> {
    let _ = (journal, store, owner, setting, request, now);
    Err(ControlError::Unimplemented { story: "E17-8" })
}
