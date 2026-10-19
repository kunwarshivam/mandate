//! The cell's meter store (DEC-2249 item 3): the worker's gateway reserves and settles through it,
//! and the research source reads the agent's research spend from the same records, so the cap the
//! core judges counts what the gateway has reserved, open reservations at their maximum.
//!
//! The store is in memory: a restarted process starts it empty. So while the agent stream holds a
//! reserved call on the risk day whose reservation the store does not hold, or a call record whose
//! reservation or time cannot be read, the store answers unavailable and the source has no spend.
//! The gateway then refuses every call `meter_unavailable`, and the pass gets no context, until the
//! next risk day (E21-61 journals the meter stream instead).

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use mandate_canon::{Value, parse};
use mandate_gateway::meter::{
    MeterRecord, MeterStore, Period, Scope, SharedMeter, StoreError, spend,
};
use mandate_journal::StreamId;
use mandate_num::Usd;
use mandate_spec::risk::risk_day;
use mandate_time::UtcNanos;

use crate::source::{Rows, SourceFor};

/// The meter store a cell's research worker and source share.
#[derive(Clone)]
pub struct CellMeter {
    store: Rc<RefCell<Box<dyn MeterStore>>>,
    rows: Rc<dyn Rows>,
    who: SourceFor,
    clock: Rc<dyn Fn() -> UtcNanos>,
}

impl CellMeter {
    /// The store over `store`'s records, reading `who`'s agent stream through `rows`, and judging
    /// its writes and reads at `clock()`.
    pub fn new(
        store: SharedMeter,
        rows: Rc<dyn Rows>,
        who: SourceFor,
        clock: Rc<dyn Fn() -> UtcNanos>,
    ) -> Self {
        Self::over(Box::new(store), rows, who, clock)
    }

    /// As [`CellMeter::new`], over any meter store: in a process given a journal, the workspace's
    /// journaled `meter:{workspace_id}` stream (DEC-2259 item 1), so a restart folds every
    /// reservation the gateway made before it and the hold, a cross-check now, holds nothing.
    pub fn over(
        store: Box<dyn MeterStore>,
        rows: Rc<dyn Rows>,
        who: SourceFor,
        clock: Rc<dyn Fn() -> UtcNanos>,
    ) -> Self {
        Self {
            store: Rc::new(RefCell::new(store)),
            rows,
            who,
            clock,
        }
    }

    /// Whether the store is held at `at`: the agent stream records a reserved call on `at`'s risk
    /// day that the store does not hold, or a call record whose reservation or time it cannot read,
    /// or the stream cannot be read.
    pub fn held(&self, at: UtcNanos) -> bool {
        self.unheld_call(at).unwrap_or(true)
    }

    /// Whether the agent stream records a reserved call on `at`'s risk day, or later, whose
    /// reservation the store does not hold; `None` when the day, the stream, the store or a call
    /// record's reservation or time cannot be read.
    fn unheld_call(&self, at: UtcNanos) -> Option<bool> {
        let stream = StreamId::parse(&format!("agent:{}:{}", self.who.workspace, self.who.agent))?;
        let rows = self.rows.rows(&stream)?;
        let records = self.store.borrow().read().ok()?;
        let held: BTreeSet<&str> = records
            .iter()
            .filter_map(|record| match record {
                MeterRecord::Reserved(reservation) => Some(reservation.event_id.as_str()),
                MeterRecord::Settled(_) => None,
            })
            .collect();
        for row in rows
            .iter()
            .filter(|row| row.event_type == "ModelInvocationRecorded")
        {
            let body = parse(&row.body).ok()?;
            let reservation = match body.get("payload")?.get("reservation")? {
                Value::Null => continue,
                Value::Str(id) => id.as_str(),
                _ => return None,
            };
            let recorded = body
                .get("event_time")
                .and_then(Value::as_str)
                .and_then(|text| UtcNanos::parse_rfc3339(text).ok())?;
            if !held.contains(reservation) && risk_day(at).ok()?.starts_at <= recorded {
                return Some(true);
            }
        }
        Some(false)
    }

    /// The agent's research spend on `at`'s risk day, `mandate_gateway::meter::spend` over the
    /// store's records, an open reservation at its maximum; `None` while the store is held, or
    /// when the spend cannot be read or does not fit a `Usd`.
    pub fn research_spend(&self, at: UtcNanos) -> Option<Usd> {
        if self.held(at) {
            return None;
        }
        let day = risk_day(at).ok()?.day;
        let records = self.store.borrow().read().ok()?;
        let spent = spend(
            &records,
            &Scope::Agent(self.who.agent.clone()),
            Period::Day(day),
        )
        .ok()?;
        usd(spent.micros())
    }
}

impl MeterStore for CellMeter {
    fn read(&self) -> Result<Vec<MeterRecord>, StoreError> {
        if self.held((self.clock)()) {
            return Err(StoreError::Unavailable);
        }
        self.store.borrow().read()
    }

    fn append(&mut self, record: MeterRecord) -> Result<(), StoreError> {
        if self.held((self.clock)()) {
            return Err(StoreError::Unavailable);
        }
        self.store.borrow_mut().append(record)
    }
}

/// `micros` millionths of a dollar as a `Usd`, written as `Usd::parse` reads it: no trailing zero
/// after the point, and no point for whole dollars.
fn usd(micros: u64) -> Option<Usd> {
    let (whole, fraction) = (micros / 1_000_000, micros % 1_000_000);
    if fraction == 0 {
        return Usd::parse(&whole.to_string()).ok();
    }
    let fraction = format!("{fraction:06}");
    Usd::parse(&format!("{whole}.{}", fraction.trim_end_matches('0'))).ok()
}
