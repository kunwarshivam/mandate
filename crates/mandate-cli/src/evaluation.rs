//! `mandate evaluation run` ([DEC-2244](../../../docs/project/decisions/DEC-2244.md); backlog
//! E17-8, E15-3): the forward-paper evaluation of an internal paper workspace's thesis records,
//! read from the workspace's own journal and journaled as `EvaluationRunRecorded` (journal spec
//! §9.18). This part reads: whether the workspace is an internal paper one (item 4), and the
//! thesis records of its agents' streams (item 2).

use std::collections::BTreeSet;
use std::ffi::OsString;

use mandate_canon::{Value, parse};
use mandate_evaluation::ThesisRecord;
use mandate_journal::{ArtifactRef, StoredEvent};

use crate::control::{ControlError, ControlJournal, Owner, agent_stream, control_stream};
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
