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
    /// An empty list of streams, which would hold nothing, refused before the store is asked; or
    /// the store's refusal to hand over a stream. Nothing is held then, but a stream taken before
    /// the refusal stays fenced against its earlier writer, which only stops more (rule 3).
    pub fn acquire(mut store: S, streams: &[String]) -> Result<Self, Cause> {
        if streams.is_empty() {
            return Err(Cause::Absent {
                what: "a stream for the lease to hold",
            });
        }
        let mut epochs = BTreeMap::new();
        for stream in streams {
            let epoch = store.take_ownership(stream)?;
            epochs.insert(stream.clone(), epoch);
        }
        Ok(Self { store, epochs })
    }

    /// Whether every leased stream's current writer epoch is still the lease's own. A store that
    /// cannot answer counts as not held.
    pub fn held(&self) -> bool {
        self.epochs
            .iter()
            .all(|(stream, epoch)| self.store.writer_epoch(stream).ok() == Some(*epoch))
    }

    /// The epoch the lease holds for `stream`, while it is still the stream's current one.
    fn current(&self, stream: &str) -> Option<u64> {
        let epoch = *self.epochs.get(stream)?;
        (self.store.writer_epoch(stream).ok() == Some(epoch)).then_some(epoch)
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
        self.store.read(stream)
    }

    fn append(
        &mut self,
        stream: &str,
        expected_head: u64,
        _writer_epoch: u64,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        match self.epochs.get(stream) {
            Some(epoch) => self.store.append(stream, expected_head, *epoch, drafts),
            None => AppendOutcome::Fenced {
                current_epoch: self.store.writer_epoch(stream).unwrap_or(u64::MAX),
            },
        }
    }
}
