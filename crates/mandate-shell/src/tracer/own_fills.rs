//! The own-fill reconciliation a run performs before its preflight judges the account
//! ([DEC-955](../../../../docs/project/decisions/DEC-955.md)): the account stream folded, the
//! broker asked only about the orders the stream owns, by their own ids, and about its fills, and
//! the fill records that word calls for appended. Read-only at the broker and journal-only: no
//! request here can place, cancel or replace an order.

use crate::error::ShellError;
use crate::stages::Stage;

use super::ProductionCycle;

/// What one own-fill pass booked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Booked {
    /// The fill records appended, `FillApplied` and `LateFillApplied` together.
    pub fills: usize,
}

impl ProductionCycle {
    /// Books the deployment's own unbooked fills (DEC-955 items 1 to 5).
    ///
    /// # Errors
    /// Every [`ShellError`] is a stop after which nothing further is read, journaled or sent.
    pub fn book_own_fills(&mut self) -> Result<Booked, ShellError> {
        let _ = &self.stages;
        Err(crate::error::refused(Stage::Reconcile)(
            crate::error::Cause::Unimplemented { story: "E2" },
        ))
    }
}
