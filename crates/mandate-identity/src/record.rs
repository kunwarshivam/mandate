//! A stored control-stream record as the membership fold and the writer read it (journal spec
//! §9.12, DEC-991 item 4): each of the seven `Member*` types maps to its [`MembershipRecord`], and
//! every other type is a [`ControlEntry::Other`] with its `seq` and `event_time`. The workspace
//! store's adapters and the founding bootstrap share this one mapping, so the seal stays inside
//! this crate and no adapter writes its own reading of a payload.
//!
//! A membership record whose members do not map is never skipped: it is an [`UnreadableRecord`],
//! and the caller commits nothing on it and grants nothing from it (identity spec ID-7, `AGENTS.md`
//! rule 3).

use mandate_canon::Value;

use crate::writer::ControlEntry;

/// The seven `Member*` event types of journal spec §9.12, the only records of a membership.
pub const MEMBERSHIP_TYPES: [&str; 7] = [
    "MemberInvited",
    "MemberInvitationRevoked",
    "MemberActivated",
    "MemberRoleChanged",
    "MemberDeactivated",
    "MemberReactivated",
    "MemberRemoved",
];

/// A stored record that does not map: its `seq` and the envelope or payload member at fault.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the control-stream record at seq {seq} does not map at {member}")]
pub struct UnreadableRecord {
    /// The record's `seq`.
    pub seq: u64,
    /// The member at fault, dotted from the envelope.
    pub member: &'static str,
}

/// The record at `seq` whose envelope is `envelope`, as a [`ControlEntry`].
///
/// # Errors
/// [`UnreadableRecord`] for an envelope with no `event_time` or `event_type`, or a `Member*`
/// record whose payload does not map.
pub fn control_entry(seq: u64, envelope: &Value) -> Result<ControlEntry, UnreadableRecord> {
    let _ = envelope;
    Err(UnreadableRecord {
        seq,
        member: "unimplemented",
    })
}
