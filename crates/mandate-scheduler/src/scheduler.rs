//! The scheduler: one writer epoch on the clock stream, a tick each second, a trading day at its
//! first tick, and a measurement when one is due (infrastructure design §3.4, journal spec §9.20).

use std::time::Duration;

use mandate_canon::{Value, parse};
use mandate_journal::{AppendOutcome, Environment, StoredEvent, StreamId};
use mandate_process::{Exit, Input, Kind, Lifecycle, State};
use mandate_time::{Date, ExchangeCalendar, UtcNanos, new_york_date_and_hour};

use crate::measure::{self, Measurement, Sample, Schedule, Trigger};
use crate::records::{Record, Writer};
use crate::tick;
use mandate_store::Store;

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
    /// The monotonic second of the last tick this process wrote: at most one tick per second.
    ticked_at: Option<u64>,
}

impl<S: Store> Scheduler<S> {
    /// Starts at `now`: refuses any environment but paper before taking anything, takes a newer
    /// writer epoch (fencing every earlier scheduler), replays the stream, and opens it if it is
    /// empty. A store that cannot answer leaves the scheduler exited `Refused`.
    pub fn start(store: S, config: &Config, calendar: ExchangeCalendar, now: UtcNanos) -> Self {
        let writer = Writer {
            workspace: config.workspace.clone(),
            environment: config.environment,
            epoch: 0,
        };
        let stream = StreamId::parse(&writer.stream());
        let mut this = Self {
            store,
            writer,
            stream,
            calendar,
            tolerance_ns: config.tolerance_ns,
            head: 0,
            last_tick: None,
            last_day: None,
            schedule: Schedule::default(),
            pending: None,
            lifecycle: Lifecycle::new(Kind::Writer, config.liveness, now),
            ticked_at: None,
        };
        if config.environment != Environment::Paper {
            this.lifecycle.observe(now, Input::Failed);
            return this;
        }
        this.lifecycle.observe(now, Input::StartupPassed);
        let Some(stream) = this.stream.clone() else {
            this.lifecycle.observe(now, Input::Failed);
            return this;
        };
        let Some(epoch) = this.store.take_ownership(&stream) else {
            this.lifecycle.observe(now, Input::Failed);
            return this;
        };
        this.writer.epoch = epoch;
        this.lifecycle.observe(now, Input::EpochTaken);
        let Some(rows) = this.store.rows(&stream) else {
            this.lifecycle.observe(now, Input::Failed);
            return this;
        };
        this.replay(&rows);
        this.lifecycle.observe(
            now,
            Input::Replayed {
                position: this.head,
                head: this.head,
            },
        );
        if this.head == 0 {
            this.write(now, vec![Record::Opened]);
        }
        this
    }

    /// One pass of the loop at the host clock's reading `host` and the process's monotonic second
    /// `second`: a retried batch first, then, once per monotonic second, the tick (with its trading
    /// day) and a measurement if one is due. A pass in a second that already ticked writes no
    /// tick, so a loop that falls behind and runs passes back to back never moves the risk clock
    /// ahead of the host clock. Nothing is written once the scheduler has exited.
    pub fn step(&mut self, host: UtcNanos, second: u64, servers: &mut dyn TimeServers) {
        if self.exited() {
            return;
        }
        self.lifecycle.observe(host, Input::Heartbeat);
        if !self.flush(host) || self.ticked_at.is_some_and(|last| second <= last) {
            return;
        }
        self.ticked_at = Some(second);
        let Some(tick) = tick::next(self.last_tick, host) else {
            return;
        };
        let mut records = vec![Record::Advanced(tick)];
        records
            .extend(tick::trading_day(&self.calendar, tick, self.last_day).map(Record::DayStarted));
        if !self.write(host, records) {
            return;
        }
        if let Some(trigger) = measure::due(&self.schedule, &self.calendar, tick) {
            let measurement = Measurement::judge(trigger, servers.ask(), self.tolerance_ns);
            let exceeded = (!measurement.within_tolerance).then_some(Record::Exceeded {
                offset_ns: measurement.offset_ns,
                tolerance_ns: measurement.tolerance_ns,
            });
            let mut records = vec![Record::Measured(measurement)];
            records.extend(exceeded);
            self.write(host, records);
        }
    }

    /// Drains at `now`: no new tick or measurement; a batch in flight is tried once more, then
    /// the scheduler exits `Drained`, or `DrainIncomplete` if that batch is still unknown.
    pub fn drain(&mut self, now: UtcNanos) {
        if self.exited() {
            return;
        }
        self.lifecycle.observe(now, Input::DrainRequested);
        let settled = self.flush(now);
        let last = if settled {
            Input::Settled
        } else {
            Input::Failed
        };
        self.lifecycle.observe(now, last);
    }

    pub fn lifecycle(&self) -> &Lifecycle {
        &self.lifecycle
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    /// The last tick written, the risk clock (mandate spec §5.2).
    pub fn last_tick(&self) -> Option<UtcNanos> {
        self.last_tick
    }

    fn exited(&self) -> bool {
        matches!(self.lifecycle.state(), State::Exited(_))
    }

    /// Retries the pending batch; whether nothing is left pending.
    fn flush(&mut self, now: UtcNanos) -> bool {
        match self.pending.take() {
            Some(pending) => self.append(now, pending),
            None => true,
        }
    }

    /// Writes `records` as one batch at the head; whether it is known to be committed.
    fn write(&mut self, now: UtcNanos, records: Vec<Record>) -> bool {
        let Some(drafts) = self.writer.drafts(self.head, now, &records) else {
            self.lifecycle.observe(now, Input::Failed);
            return false;
        };
        self.append(
            now,
            Pending {
                head: self.head,
                records,
                drafts,
            },
        )
    }

    fn append(&mut self, now: UtcNanos, pending: Pending) -> bool {
        let Some(stream) = self.stream.clone() else {
            return false;
        };
        let outcome = self.store.append(
            &stream,
            pending.head,
            self.writer.epoch,
            now,
            &pending.drafts,
        );
        match outcome {
            AppendOutcome::Committed(stored) | AppendOutcome::AlreadyCommitted(stored) => {
                self.head = stored.last().map_or(self.head, |e| e.seq);
                for record in &pending.records {
                    self.apply(record);
                }
                true
            }
            AppendOutcome::Unavailable | AppendOutcome::Ambiguous => {
                self.pending = Some(pending);
                false
            }
            AppendOutcome::Fenced { .. } => {
                self.lifecycle.observe(now, Input::Fenced);
                false
            }
            AppendOutcome::HeadMismatch { .. }
            | AppendOutcome::IdempotencyConflict { .. }
            | AppendOutcome::Invalid { .. } => {
                self.lifecycle.observe(now, Input::Failed);
                false
            }
        }
    }

    /// What a committed record changes: the last tick, the last trading day, and the schedule,
    /// which dates a measurement by the tick before it.
    fn apply(&mut self, record: &Record) {
        match record {
            Record::Advanced(clock) => self.last_tick = Some(*clock),
            Record::DayStarted(date) => self.last_day = Some(*date),
            Record::Measured(m) => {
                self.schedule.measured_since_start = true;
                self.measured(m.trigger == Trigger::BeforeSession);
            }
            Record::Opened | Record::Exceeded { .. } => {}
        }
    }

    fn measured(&mut self, before_session: bool) {
        self.schedule.last_measured = self.last_tick;
        if before_session {
            self.schedule.before_session_on = self
                .last_tick
                .and_then(|t| new_york_date_and_hour(t).ok())
                .map(|(date, _)| date);
        }
    }

    /// The state the stream's records leave: its head, last tick, last trading day, and last
    /// measurements. A record this scheduler cannot read changes nothing but the head; the next
    /// tick is still later than every tick written.
    fn replay(&mut self, rows: &[StoredEvent]) {
        for row in rows {
            self.head = row.seq;
            let body = parse(&row.body).ok();
            let payload = body.as_ref().and_then(|b| b.get("payload"));
            let member = |name: &str| payload.and_then(|p| p.get(name)).and_then(Value::as_str);
            match row.event_type.as_str() {
                "ClockAdvanced" => {
                    let clock = member("clock").and_then(|c| UtcNanos::parse(c).ok());
                    self.last_tick = clock.max(self.last_tick);
                }
                "TradingDayStarted" => {
                    let date = member("date").and_then(|d| Date::parse(d).ok());
                    self.last_day = date.max(self.last_day);
                }
                "ClockOffsetRecorded" => self.measured(member("trigger") == Some("before_session")),
                _ => {}
            }
        }
    }
}

/// The exit a scheduler stopped with, if it has.
pub fn exit<S>(scheduler: &Scheduler<S>) -> Option<Exit> {
    match scheduler.lifecycle.state() {
        State::Exited(exit) => Some(exit),
        _ => None,
    }
}
