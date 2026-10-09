//! The dispatcher's step over the journal, its outbox (notifications spec §5.1, NT-3, NT-8; E8-10
//! slice D2, DEC-701 item 3, DEC-704). A [`step`] reads the committed `OwnerAlertSent` events of
//! its subject streams, journals one `NoticeIssued` per cause before any send, sends each notice
//! once to each recipient's push channel, and journals each outcome as `NoticeAttempted`. Every
//! append goes through a [`NoticeWriter`], which holds only an `ntf:` stream id, so the
//! dispatcher cannot name an agent, account, or control stream. An append that does not commit
//! stops the step before its next send.

use mandate_journal::{AppendOutcome, Environment, StoredEvent, StreamId};
use mandate_notify::{Origin, Provider, PushChannel, SecureRandom};
use mandate_time::UtcNanos;

use crate::DispatchError;

/// The one handle the dispatcher appends through: a notice stream `ntf:{workspace_id}` and the
/// writer epoch the dispatcher took for it (journal spec §5.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeWriter {
    stream: StreamId,
    epoch: u64,
}

impl NoticeWriter {
    /// # Errors
    /// [`DispatchError::NotANoticeStream`] for any stream that is not a notice stream.
    pub fn new(stream: StreamId, epoch: u64) -> Result<Self, DispatchError> {
        let _ = (stream, epoch);
        Err(DispatchError::Unimplemented { story: "E8-10" })
    }

    pub fn stream(&self) -> Result<&StreamId, DispatchError> {
        let _ = (&self.stream, self.epoch);
        Err(DispatchError::Unimplemented { story: "E8-10" })
    }

    pub fn epoch(&self) -> Result<u64, DispatchError> {
        Err(DispatchError::Unimplemented { story: "E8-10" })
    }
}

/// What the dispatcher reads and writes: the committed rows of a stream in `seq` order, and one
/// all-or-nothing append to a [`NoticeWriter`]'s stream at its epoch.
pub trait Journal {
    fn committed(&self, stream: &StreamId) -> Vec<StoredEvent>;
    fn append_notices(
        &mut self,
        writer: &NoticeWriter,
        expected_head: u64,
        recorded_at: UtcNanos,
        drafts: &[&[u8]],
    ) -> AppendOutcome;
}

/// One step's inputs besides the journal, the random source, and the provider.
#[derive(Debug, Clone, Copy)]
pub struct Config<'a> {
    pub writer: &'a NoticeWriter,
    /// The workspace's agent, account, and control streams, whose alerts are causes.
    pub subjects: &'a [StreamId],
    /// The push channels, by opaque recipient id, every notice goes to until roles exist (E9-2;
    /// spec §3.3, DEC-704 item 4).
    pub audience: &'a [(&'a str, PushChannel)],
    pub origin: &'a Origin,
    pub environment: Environment,
    /// This binary's digest, `sha256:<64 hex>`, for each draft's actor.
    pub build: &'a str,
}

/// One pass of the dispatcher at `now` (spec §5.1 steps 1 to 4, DEC-704 items 2 and 3).
///
/// # Errors
/// [`DispatchError::Fenced`] when a newer dispatcher owns the notice stream and
/// [`DispatchError::NotCommitted`] for any other append that does not commit, each before the
/// next send; [`DispatchError::Notify`] when the random source fails.
pub fn step(
    config: &Config<'_>,
    journal: &mut dyn Journal,
    random: &mut dyn SecureRandom,
    provider: &mut dyn Provider,
    now: UtcNanos,
) -> Result<(), DispatchError> {
    let _ = (config, journal, random, provider, now);
    Err(DispatchError::Unimplemented { story: "E8-10" })
}

#[cfg(test)]
mod tests;
