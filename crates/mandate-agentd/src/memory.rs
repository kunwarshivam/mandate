//! The in-memory store: a [`MemoryJournal`] shared by every process of one test or fixture run,
//! so two supervisors can contend for the same streams.

use std::cell::RefCell;
use std::rc::Rc;

use mandate_journal::{AppendOutcome, MemoryJournal, StoredEvent, StreamId};
use mandate_shell::Cause;
use mandate_shell::stages::JournalWriter;
use mandate_time::UtcNanos;

use crate::lease::Epochs;

/// A handle on one shared [`MemoryJournal`]. Clones share the journal; every batch is recorded at
/// the handle's fixed time, since the process reads no wall clock for the journal (ES-21).
#[derive(Debug, Clone)]
pub struct MemoryStore {
    journal: Rc<RefCell<MemoryJournal>>,
    recorded_at: UtcNanos,
}

impl MemoryStore {
    pub fn new(journal: Rc<RefCell<MemoryJournal>>, recorded_at: UtcNanos) -> Self {
        Self {
            journal,
            recorded_at,
        }
    }
}

fn stream_id(stream: &str) -> Result<StreamId, Cause> {
    StreamId::parse(stream).ok_or(Cause::Absent {
        what: "a stream id the journal can parse",
    })
}

impl Epochs for MemoryStore {
    fn writer_epoch(&self, stream: &str) -> Result<u64, Cause> {
        Ok(self.journal.borrow().head(&stream_id(stream)?).writer_epoch)
    }
}

impl JournalWriter for MemoryStore {
    fn take_ownership(&mut self, stream: &str) -> Result<u64, Cause> {
        Ok(self
            .journal
            .borrow_mut()
            .take_ownership(&stream_id(stream)?))
    }

    fn read(&self, stream: &str) -> Result<Vec<StoredEvent>, Cause> {
        Ok(self.journal.borrow().rows(&stream_id(stream)?).to_vec())
    }

    fn append(
        &mut self,
        stream: &str,
        expected_head: u64,
        writer_epoch: u64,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        let Ok(stream) = stream_id(stream) else {
            return AppendOutcome::Unavailable;
        };
        let drafts: Vec<&[u8]> = drafts.iter().map(Vec::as_slice).collect();
        self.journal.borrow_mut().append(
            &stream,
            expected_head,
            writer_epoch,
            self.recorded_at,
            &drafts,
        )
    }
}
