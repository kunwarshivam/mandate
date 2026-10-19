//! The model gateway's meter stream, journaled (DEC-2258, E21-61): `meter:{workspace_id}` through
//! a [`Store`], so a restarted gateway folds every reservation it made, an open one at its
//! maximum, and its caps hold across a crash (DEC-998 items 5 and 8). One writer per workspace: the
//! store's writer epoch, taken at open, fences an older writer.

use std::rc::Rc;

use mandate_gateway::meter::{MeterRecord, MeterStore, StoreError};
use mandate_gateway::provider::IdSource;
use mandate_journal::{Environment, StreamId};
use mandate_time::UtcNanos;

use crate::Store;

/// A meter store over a workspace's journaled meter stream.
pub struct JournalMeter<S: Store> {
    store: S,
    stream: Option<StreamId>,
    workspace: String,
    environment: Environment,
    epoch: Option<u64>,
    now: Rc<dyn Fn() -> UtcNanos>,
    ids: Box<dyn IdSource>,
}

impl<S: Store> JournalMeter<S> {
    /// The meter stream of `workspace` in `environment` over `store`, its writer epoch taken now,
    /// each record written at `now()`, and the stream's `StreamOpened`, when this writer opens it,
    /// at the next of `ids`. A workspace that names no stream, or a store that grants no
    /// epoch, gives a meter whose every read and append fails `Unavailable` (DEC-2258 item 4).
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn open(
        store: S,
        workspace: &str,
        environment: Environment,
        now: Rc<dyn Fn() -> UtcNanos>,
        ids: Box<dyn IdSource>,
    ) -> Self {
        let _ = (store, workspace, environment, now, ids);
        todo!()
    }
}

impl<S: Store> MeterStore for JournalMeter<S> {
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    fn read(&self) -> Result<Vec<MeterRecord>, StoreError> {
        let _ = (
            &self.store,
            &self.stream,
            &self.workspace,
            self.environment,
            self.epoch,
            &self.now,
            &self.ids,
        );
        todo!()
    }

    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    fn append(&mut self, record: MeterRecord) -> Result<(), StoreError> {
        let _ = record;
        todo!()
    }
}
