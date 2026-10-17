//! The fencing lease: the writer epoch of each of a deployment's streams, taken once at start
//! (journal spec §5.1, infrastructure design §3.4).

use std::collections::BTreeMap;

use mandate_journal::{AppendOutcome, StoredEvent};
use mandate_shell::Cause;
use mandate_shell::stages::JournalWriter;

/// The journal's answer when the lease no longer holds a stream: a newer writer took it.
pub const FENCED: Cause = Cause::Append { outcome: "Fenced" };

/// What a store answers about a stream's current writer epoch, which the lease compares with its
/// own before it lets anything act.
pub trait Epochs {
    /// The stream's current writer epoch, 0 for a stream nobody has owned.
    ///
    /// # Errors
    /// A store that cannot answer, which the lease reads as not held (`AGENTS.md` rule 3).
    fn writer_epoch(&self, stream: &str) -> Result<u64, Cause>;
}

/// One process's hold on a deployment's streams. It is a [`JournalWriter`] whose
/// `take_ownership` hands back the epoch the lease took, while it is still the stream's current
/// one, instead of taking a newer epoch: a cycle run through it can never fence a sibling, and is
/// itself fenced once a sibling has taken the stream.
#[derive(Debug)]
pub struct Lease<S> {
    store: S,
    epochs: BTreeMap<String, u64>,
}

impl<S: JournalWriter + Epochs> Lease<S> {
    /// Takes the writer epoch of every stream, in the order given, fencing out any earlier writer.
    ///
    /// # Errors
    /// The store's refusal to hand over a stream; nothing is held then.
    pub fn acquire(mut store: S, streams: &[String]) -> Result<Self, Cause> {
        let _ = (&mut store, streams);
        Err(Cause::Unimplemented { story: "E21-2" })
    }

    /// Whether every leased stream's current writer epoch is still the lease's own. A store that
    /// cannot answer counts as not held.
    pub fn held(&self) -> bool {
        false
    }

    /// The epoch the lease holds for `stream`, while it is still the stream's current one.
    fn current(&self, stream: &str) -> Option<u64> {
        let _ = (&self.epochs, stream);
        None
    }

    /// Gives the lease up. The journal has no unlock: the epochs stay where they are, and the next
    /// process fences them by taking newer ones. Nothing is appended.
    pub fn release(self) -> S {
        self.store
    }
}

impl<S: JournalWriter + Epochs> JournalWriter for Lease<S> {
    fn take_ownership(&mut self, stream: &str) -> Result<u64, Cause> {
        self.current(stream).ok_or(FENCED)
    }

    fn read(&self, stream: &str) -> Result<Vec<StoredEvent>, Cause> {
        let _ = stream;
        Err(Cause::Unimplemented { story: "E21-2" })
    }

    fn append(
        &mut self,
        stream: &str,
        expected_head: u64,
        _writer_epoch: u64,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        let _ = (stream, expected_head, drafts);
        AppendOutcome::Unavailable
    }
}
