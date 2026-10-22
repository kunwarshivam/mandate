//! The workspace's control stream read through the journal store (DEC-2272 item 1): the
//! [`Control`] the service reads each research call's deployed version from.

use mandate_journal::{StoredEvent, StreamId};
use mandate_store::Store;

use crate::service::Control;

/// `ctl:{workspace_id}` through a [`Store`].
pub struct StoreControl<S: Store> {
    store: S,
    stream: StreamId,
}

#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
impl<S: Store> StoreControl<S> {
    /// The control stream of `workspace` through `store`, or `None` when `workspace` names no
    /// stream.
    pub fn new(store: S, workspace: &str) -> Option<Self> {
        let _ = (store, workspace);
        todo!()
    }
}

#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
impl<S: Store> Control for StoreControl<S> {
    fn rows(&self) -> Option<Vec<StoredEvent>> {
        let _ = (&self.store, &self.stream);
        todo!()
    }
}
