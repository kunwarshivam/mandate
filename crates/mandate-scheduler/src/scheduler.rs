//! The scheduler: one writer epoch on the clock stream, a tick each second, a trading day at its
//! first tick, and a measurement when one is due (infrastructure design §3.4, journal spec §9.18).

use std::time::Duration;

use mandate_canon::{Value, parse};
use mandate_journal::{AppendOutcome, Environment, StoredEvent, StreamId};
use mandate_process::{Exit, Input, Kind, Lifecycle, State};
use mandate_time::{Date, ExchangeCalendar, UtcNanos, new_york_date_and_hour};

use crate::measure::{self, Measurement, Sample, Schedule, Trigger};
use crate::records::{Record, Writer};
use crate::store::ClockStore;
use crate::tick;

/// The workspace's time servers, asked once per measurement.
pub trait TimeServers {
    /// Every answer received; a server that did not answer is left out.
    fn ask(&mut self) -> Vec<Sample>;
}

/// What a scheduler runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub workspace: String,
    pub environment: Environment,
    pub tolerance_ns: u64,
    /// The liveness bound of [`mandate_process`]: the loop handles an input at least this often.
    pub liveness: Duration,
}

/// A batch handed to the journal and not yet known to be committed: retried as the same bytes,
/// so the journal answers `AlreadyCommitted` if it was (journal spec §5.1).
struct Pending {
    head: u64,
    records: Vec<Record>,
    drafts: Vec<Vec<u8>>,
}

/// One workspace's scheduler.
pub struct Scheduler<S> {
    store: S,
    writer: Writer,
    stream: Option<StreamId>,
    calendar: ExchangeCalendar,
    tolerance_ns: u64,
    head: u64,
    last_tick: Option<UtcNanos>,
    last_day: Option<Date>,
    schedule: Schedule,
    pending: Option<Pending>,
    lifecycle: Lifecycle,
}

impl<S: ClockStore> Scheduler<S> {
    /// Starts at `now`: refuses any environment but paper before taking anything, takes a newer
    /// writer epoch (fencing every earlier scheduler), replays the stream, and opens it if it is
    /// empty. A store that cannot answer leaves the scheduler exited `Refused`.
    pub fn start(store: S, config: &Config, calendar: ExchangeCalendar, now: UtcNanos) -> Self {
        todo!()
    }

    /// One second of the loop at the host clock's reading `host`: a retried batch first, then the
    /// tick (with its trading day), then a measurement if one is due. Nothing is written once the
    /// scheduler has exited.
    pub fn step(&mut self, host: UtcNanos, servers: &mut dyn TimeServers) {
        todo!()
    }

    /// Drains at `now`: no new tick or measurement; a batch in flight is tried once more, then
    /// the scheduler exits `Drained`, or `DrainIncomplete` if that batch is still unknown.
    pub fn drain(&mut self, now: UtcNanos) {
        todo!()
    }

    pub fn lifecycle(&self) -> &Lifecycle {

        todo!()

    }

    pub fn store(&self) -> &S {

        todo!()

    }

    /// The last tick written, the risk clock (mandate spec §5.2).
    pub fn last_tick(&self) -> Option<UtcNanos> {
        todo!()
    }

    fn exited(&self) -> bool {

        todo!()

    }

    /// Retries the pending batch; whether nothing is left pending.
    fn flush(&mut self, now: UtcNanos) -> bool {
        todo!()
    }

    /// Writes `records` as one batch at the head; whether it is known to be committed.
    fn write(&mut self, now: UtcNanos, records: Vec<Record>) -> bool {
        todo!()
    }

    fn append(&mut self, now: UtcNanos, pending: Pending) -> bool {

        todo!()

    }

    /// What a committed record changes: the last tick, the last trading day, and the schedule,
    /// which dates a measurement by the tick before it.
    fn apply(&mut self, record: &Record) {
        todo!()
    }

    fn measured(&mut self, before_session: bool) {

        todo!()

    }

    /// The state the stream's records leave: its head, last tick, last trading day, and last
    /// measurements. A record this scheduler cannot read changes nothing but the head; the next
    /// tick is still later than every tick written.
    fn replay(&mut self, rows: &[StoredEvent]) {
        todo!()
    }
}

/// The exit a scheduler stopped with, if it has.
pub fn exit<S>(scheduler: &Scheduler<S>) -> Option<Exit> {
    todo!()
}
