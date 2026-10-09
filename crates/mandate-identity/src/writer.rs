//! The one writer of `Member*` records on a workspace's control stream `ctl:{workspace_id}`
//! (E9-7, DEC-659 items 2 and 8, DEC-646).
//!
//! Pure: the stream is a [`ControlStream`] port, a compare-and-append on journal spec §5.1's
//! `expected_head` that the workspace store implements, and the clock is handed in. One attempt
//! reads the head and the last membership record, stamps `event_time`, runs [`crate::check_order`],
//! and appends at the head it read; a moved head re-runs it, and a refusal is returned for the
//! caller to retry, never clamped (rule 106).

use mandate_time::UtcNanos;

use mandate_identity_seal::Seal;

use crate::{MembershipEvent, MembershipRecord, RecordRefusal, check_order};

/// How many times [`write_membership`] re-runs an attempt whose head moved before it reports
/// [`WriteError::Contended`].
pub const ATTEMPTS: u32 = 8;

/// One record of the control stream as the writer reads it: a membership record of the seven
/// §9.12 types, or any other record, which says nothing about their order (DEC-659 item 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlEntry {
    /// A `Member*` record, as the store maps it.
    Membership(MembershipRecord),
    /// Any other control-stream record, by its `seq` and envelope `event_time`.
    Other {
        /// Its `seq`.
        seq: u64,
        /// Its envelope's `event_time`.
        event_time: UtcNanos,
    },
}

/// The control stream read in one step: its head `seq` (0 when empty) and its records in `seq`
/// order, at least from its last membership record on (all of them when it holds none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlView {
    /// The head `seq` the append must still find.
    pub head: u64,
    /// The records, in `seq` order, up to the head.
    pub tail: Vec<ControlEntry>,
}

/// What a compare-and-append did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appended {
    /// The record is committed at `expected_head + 1`.
    Committed,
    /// The head is no longer `expected_head`, and nothing was written.
    HeadMoved,
}

/// The workspace control stream the writer appends to, implemented by the workspace store.
pub trait ControlStream {
    /// A storage failure, passed through.
    type Error;
    /// Reads the head and the records [`ControlView`] names, together.
    fn read(&mut self) -> Result<ControlView, Self::Error>;
    /// Commits `record` at `expected_head + 1` only if the head is still `expected_head`.
    fn append(
        &mut self,
        expected_head: u64,
        record: &MembershipRecord,
    ) -> Result<Appended, Self::Error>;
}

/// Why [`write_membership`] committed nothing. Every variant is retried by the caller later, and
/// nothing is granted while it lasts (DEC-659 item 8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteError<E> {
    /// The record is refused against the stream's last membership record.
    Refused(RecordRefusal),
    /// The head moved on every one of [`ATTEMPTS`] attempts.
    Contended,
    /// The store failed.
    Store(E),
    /// The head is already `u64::MAX`, so no record has a `seq` after it, and nothing was written.
    StreamFull,
}

/// Commits `event` as the next membership record of `stream`, at `seq` head + 1 and `event_time`
/// `now()` read inside the attempt, only if [`crate::check_order`] passes it against the stream's
/// last membership record; returns the committed record.
pub fn write_membership<S: ControlStream>(
    stream: &mut S,
    mut now: impl FnMut() -> UtcNanos,
    event: MembershipEvent,
) -> Result<MembershipRecord, WriteError<S::Error>> {
    for _ in 0..ATTEMPTS {
        let view = stream.read().map_err(WriteError::Store)?;
        let last = view.tail.iter().rev().find_map(|entry| match entry {
            ControlEntry::Membership(record) => Some(record),
            ControlEntry::Other { .. } => None,
        });
        let seq = view.head.checked_add(1).ok_or(WriteError::StreamFull)?;
        let record = MembershipRecord::new(Seal::grant(), seq, now(), event.clone());
        check_order(last, &record).map_err(WriteError::Refused)?;
        let appended = stream.append(view.head, &record);
        match appended.map_err(WriteError::Store)? {
            Appended::Committed => return Ok(record),
            Appended::HeadMoved => continue,
        }
    }
    Err(WriteError::Contended)
}
