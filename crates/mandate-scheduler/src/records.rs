//! The scheduler stream's five records (journal spec §9.18) as draft bytes, with the envelope of
//! §3 and deterministic event ids: a retried batch re-derives the same ids, so the journal answers
//! `AlreadyCommitted` rather than writing it twice.

use mandate_canon::{Int, Key, Object, Value, to_canonical};
use mandate_journal::Environment;
use mandate_time::{Date, UtcNanos};

use crate::measure::{Measurement, seconds, tolerance_seconds};

/// The version the envelope's actor carries.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// One record the scheduler writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    Opened,
    Advanced(UtcNanos),
    DayStarted(Date),
    Measured(Measurement),
    /// The measurement just before it in the batch was out of tolerance: its `offset` and
    /// `tolerance`, with that record's event id as the cause.
    Exceeded {
        offset_ns: Option<i64>,
        tolerance_ns: u64,
    },
}

/// Where and under which epoch a batch is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Writer {
    pub workspace: String,
    pub environment: Environment,
    pub epoch: u64,
}

impl Writer {
    pub fn stream(&self) -> String {
        todo!()
    }

    /// The event id of the `ordinal`th draft of the batch appended at `head`: the scheduler's id
    /// space `2`, then the epoch, the head, and the ordinal, zero-padded to 5, 14, and 6 digits
    /// (26 characters of the ULID alphabet, journal spec §3). The agent and account streams' ids
    /// start `0` and `1`, so the three never meet. A part too large for its digits makes a longer
    /// id, which the journal refuses rather than truncating it into another.
    pub fn event_id(&self, head: u64, ordinal: usize) -> String {
        todo!()
    }

    /// The drafts of `records`, appended as one batch at `head`, each with `event_time` `at`.
    /// `None` only for an integer the canonical form cannot hold, which nothing here writes.
    pub fn drafts(&self, head: u64, at: UtcNanos, records: &[Record]) -> Option<Vec<Vec<u8>>> {
        todo!()
    }
}

fn payload(workspace: &str, record: &Record) -> Option<(&'static str, Value)> {

    todo!()

}

/// The actor: a `system` writer named `scheduler`, whose build names the crate and its version.
/// It is not a reproducible-build digest, which is M9's (ES-01).
fn actor() -> Option<Value> {
    todo!()
}

fn decimal(nanos: Option<i64>) -> Value {

    todo!()

}

fn text(s: &str) -> Value {

    todo!()

}

fn int(n: u64) -> Option<Value> {

    todo!()

}

fn object(members: Vec<(&'static str, Value)>) -> Option<Value> {

    todo!()

}
