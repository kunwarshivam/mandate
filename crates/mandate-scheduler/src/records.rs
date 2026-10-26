//! The scheduler stream's five records (journal spec §9.20) as draft bytes, with the envelope of
//! §3 and deterministic event ids: a retried batch re-derives the same ids, so the journal answers
//! `AlreadyCommitted` rather than writing it twice.

use mandate_canon::{Object, Value, stream_event_id};
use mandate_journal::Environment;
use mandate_time::{Date, UtcNanos};

use mandate_store::envelope::{Envelope, Fields, draft_bytes, int, object, text};

use crate::measure::{Measurement, seconds, tolerance_seconds};

/// The version the envelope's actor carries.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// One record the scheduler writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    Opened,
    Advanced(UtcNanos),
    DayStarted(Date),
    Measured(Measurement),
    /// The measurement just before it in the batch was out of tolerance: its `offset` and
    /// `tolerance`, with that record's event id as the cause.
    Exceeded {
        offset_ns: Option<i64>,
        tolerance_ns: u64,
    },
}

/// Where and under which epoch a batch is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Writer {
    pub workspace: String,
    pub environment: Environment,
    pub epoch: u64,
}

impl Writer {
    pub fn stream(&self) -> String {
        format!("clock:{}", self.workspace)
    }

    /// The event id of the `ordinal`th draft of the batch appended at `head`: the clock stream's
    /// [`stream_event_id`] at this writer's epoch, so two workspaces' clock streams in one journal
    /// never share an id (journal spec §3) and a retry re-derives the same one (§5.1). A batch
    /// holds a handful of drafts, so an ordinal past `u32::MAX` never occurs; it would read as
    /// `u32::MAX`.
    pub fn event_id(&self, head: u64, ordinal: usize) -> String {
        let ordinal = u32::try_from(ordinal).unwrap_or(u32::MAX);
        stream_event_id(&self.stream(), self.epoch, head, ordinal)
    }

    /// The drafts of `records`, appended as one batch at `head`, each with `event_time` `at`.
    /// `None` only for an integer the canonical form cannot hold, which nothing here writes.
    pub fn drafts(&self, head: u64, at: UtcNanos, records: &[Record]) -> Option<Vec<Vec<u8>>> {
        let stream = self.stream();
        let envelope = Envelope {
            stream: &stream,
            environment: self.environment,
            actor_kind: "system",
            actor_id: "scheduler",
            build: concat!("mandate-scheduler/", env!("CARGO_PKG_VERSION")),
            version: VERSION,
            event_time: at,
        };
        records
            .iter()
            .enumerate()
            .map(|(ordinal, record)| {
                let cause = match record {
                    Record::Exceeded { .. } => ordinal
                        .checked_sub(1)
                        .map(|before| self.event_id(head, before)),
                    _ => None,
                };
                let (event_type, payload) = payload(&self.workspace, record)?;
                draft_bytes(
                    &envelope,
                    &Fields {
                        event_id: &self.event_id(head, ordinal),
                        event_type,
                        schema_version: 1,
                        causation_id: cause.as_deref(),
                        correlation_id: None,
                        config_refs: &Object::new(),
                        payload: &payload,
                    },
                )
            })
            .collect()
    }
}

fn payload(workspace: &str, record: &Record) -> Option<(&'static str, Value)> {
    Some(match record {
        Record::Opened => (
            "StreamOpened",
            object(vec![
                ("stream_type", text("scheduler")),
                ("workspace_id", text(workspace)),
            ])?,
        ),
        Record::Advanced(clock) => (
            "ClockAdvanced",
            object(vec![("clock", Value::Str(clock.to_string()))])?,
        ),
        Record::DayStarted(date) => (
            "TradingDayStarted",
            object(vec![("date", Value::Str(date.to_string()))])?,
        ),
        Record::Measured(m) => {
            let samples = m
                .samples
                .iter()
                .map(|s| {
                    object(vec![
                        ("delay", Value::Str(seconds(s.delay_ns))),
                        ("offset", Value::Str(seconds(s.offset_ns))),
                        ("server", Value::Str(s.server.clone())),
                        ("stratum", int(u64::from(s.stratum))?),
                    ])
                })
                .collect::<Option<Vec<_>>>()?;
            (
                "ClockOffsetRecorded",
                object(vec![
                    ("offset", decimal(m.offset_ns)),
                    ("samples", Value::Array(samples)),
                    ("tolerance", Value::Str(tolerance_seconds(m.tolerance_ns))),
                    ("trigger", text(m.trigger.as_str())),
                    ("within_tolerance", Value::Bool(m.within_tolerance)),
                ])?,
            )
        }
        Record::Exceeded {
            offset_ns,
            tolerance_ns,
        } => (
            "ClockToleranceExceeded",
            object(vec![
                ("offset", decimal(*offset_ns)),
                ("tolerance", Value::Str(tolerance_seconds(*tolerance_ns))),
            ])?,
        ),
    })
}

fn decimal(nanos: Option<i64>) -> Value {
    nanos.map_or(Value::Null, |n| Value::Str(seconds(n)))
}
