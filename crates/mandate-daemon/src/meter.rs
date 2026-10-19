//! The cell's meter store (DEC-2249 item 3): the worker's gateway reserves and settles through it,
//! and the research source reads the agent's research spend from the same records, so the cap the
//! core judges counts what the gateway has reserved, open reservations at their maximum.
//!
//! The store is in memory: a restarted process starts it empty. So while the agent stream holds a
//! reserved call on the risk day whose reservation the store does not hold, or a call record whose
//! reservation or time cannot be read, the store answers unavailable and the source has no spend.
//! The gateway then refuses every call `meter_unavailable`, and the pass gets no context, until the
//! next risk day (E21-61 journals the meter stream instead).

use std::rc::Rc;

use mandate_gateway::meter::{MeterRecord, MeterStore, SharedMeter, StoreError};
use mandate_num::Usd;
use mandate_time::UtcNanos;

use crate::source::{Rows, SourceFor};

/// The meter store a cell's research worker and source share.
#[derive(Clone)]
pub struct CellMeter {
    store: SharedMeter,
    rows: Rc<dyn Rows>,
    who: SourceFor,
    clock: Rc<dyn Fn() -> UtcNanos>,
}

impl CellMeter {
    /// The store over `store`'s records, reading `who`'s agent stream through `rows`, and judging
    /// its writes and reads at `clock()`.
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn new(
        store: SharedMeter,
        rows: Rc<dyn Rows>,
        who: SourceFor,
        clock: Rc<dyn Fn() -> UtcNanos>,
    ) -> Self {
        let _ = (store, rows, who, clock);
        todo!()
    }

    /// Whether the store is held at `at`: the agent stream records a reserved call on `at`'s risk
    /// day that the store does not hold, or a call record whose reservation or time it cannot read,
    /// or the stream cannot be read.
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn held(&self, at: UtcNanos) -> bool {
        let _ = (&self.store, &self.rows, &self.who, &self.clock, at);
        todo!()
    }

    /// The agent's research spend on `at`'s risk day, `mandate_gateway::meter::spend` over the
    /// store's records, an open reservation at its maximum; `None` while the store is held, or
    /// when the spend cannot be read or does not fit a `Usd`.
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn research_spend(&self, at: UtcNanos) -> Option<Usd> {
        let _ = at;
        todo!()
    }
}

impl MeterStore for CellMeter {
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    fn read(&self) -> Result<Vec<MeterRecord>, StoreError> {
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
