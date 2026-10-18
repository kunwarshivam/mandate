//! `mandate evaluation run` ([DEC-2244](../../../docs/project/decisions/DEC-2244.md); backlog
//! E17-8, E15-3): the forward-paper evaluation of an internal paper workspace's thesis records,
//! read from the workspace's own journal and journaled as `EvaluationRunRecorded` (journal spec
//! §9.18). This part reads: whether the workspace is an internal paper one (item 4), and the
//! thesis records of its agents' streams (item 2).

use std::ffi::OsString;

use mandate_evaluation::ThesisRecord;

use crate::control::{ControlError, ControlJournal, Owner};

/// The operator's setting that marks the workspace an internal paper one (DEC-2244 item 4, read as
/// DEC-1143 item 1 reads the API's).
pub const INTERNAL_PAPER_VAR: &str = "MANDATE_INTERNAL_PAPER_WORKSPACE";

/// The setting's value: on only for exactly `true`; off when absent, empty, or exactly `false`.
///
/// # Errors
/// [`ControlError::Refused`] with `internal_paper_setting` for any other value.
pub fn internal_paper_setting(value: Option<OsString>) -> Result<bool, ControlError> {
    let _ = value;
    Err(ControlError::Unimplemented { story: "E17-8" })
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
    let _ = (journal, owner, setting);
    Err(ControlError::Unimplemented { story: "E17-8" })
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
    let _ = (journal, owner);
    Err(ControlError::Unimplemented { story: "E17-8" })
}
