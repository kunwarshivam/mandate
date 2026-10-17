//! The scheduler stream's five records (journal spec §9.20) as draft bytes, with the envelope of
//! §3 and deterministic event ids: a retried batch re-derives the same ids, so the journal answers
//! `AlreadyCommitted` rather than writing it twice.

use mandate_canon::{Int, Key, Object, Value, to_canonical};
use mandate_journal::Environment;
use mandate_time::{Date, UtcNanos};

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

    /// The event id of the `ordinal`th draft of the batch appended at `head`: the scheduler's id
    /// space `2`, then the epoch, the head, and the ordinal, zero-padded to 5, 14, and 6 digits
    /// (26 characters of the ULID alphabet, journal spec §3). The agent and account streams' ids
    /// start `0` and `1`, so the three never meet. A part too large for its digits makes a longer
    /// id, which the journal refuses rather than truncating it into another.
    pub fn event_id(&self, head: u64, ordinal: usize) -> String {
        format!("2{:05}{head:014}{ordinal:06}", self.epoch)
    }

    /// The drafts of `records`, appended as one batch at `head`, each with `event_time` `at`.
    /// `None` only for an integer the canonical form cannot hold, which nothing here writes.
    pub fn drafts(&self, head: u64, at: UtcNanos, records: &[Record]) -> Option<Vec<Vec<u8>>> {
        let stream = self.stream();
        records
            .iter()
            .enumerate()
            .map(|(ordinal, record)| {
                let cause = match record {
                    Record::Exceeded { .. } => ordinal
                        .checked_sub(1)
                        .map(|before| Value::Str(self.event_id(head, before))),
                    _ => None,
                };
                let (event_type, payload) = payload(&self.workspace, record)?;
                let envelope = object(vec![
                    ("actor", actor()?),
                    ("artifact_refs", Value::Array(Vec::new())),
                    ("causation_id", cause.unwrap_or(Value::Null)),
                    ("clock_source", text("scheduler")),
                    ("config_refs", Value::Object(Object::new())),
                    ("correlation_id", Value::Null),
                    ("environment", text(self.environment.as_str())),
                    ("envelope_version", int(1)?),
                    ("event_id", Value::Str(self.event_id(head, ordinal))),
                    ("event_time", Value::Str(at.to_string())),
                    ("event_type", text(event_type)),
                    ("payload", payload),
                    ("pii_refs", Value::Array(Vec::new())),
                    ("schema_version", int(1)?),
                    ("stream_id", Value::Str(stream.clone())),
                ])?;
                Some(to_canonical(&envelope))
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

/// The actor: a `system` writer named `scheduler`, whose build names the crate and its version.
/// It is not a reproducible-build digest, which is M9's (ES-01).
fn actor() -> Option<Value> {
    let digest = mandate_canon::Digest::of(
        concat!("mandate-scheduler/", env!("CARGO_PKG_VERSION")).as_bytes(),
    );
    object(vec![
        ("build", Value::Str(format!("sha256:{}", digest.to_hex()))),
        ("id", text("scheduler")),
        ("kind", text("system")),
        ("version", text(VERSION)),
    ])
}

fn decimal(nanos: Option<i64>) -> Value {
    nanos.map_or(Value::Null, |n| Value::Str(seconds(n)))
}

fn text(s: &str) -> Value {
    Value::Str(s.to_owned())
}

fn int(n: u64) -> Option<Value> {
    Int::new(n).map(Value::Int)
}

fn object(members: Vec<(&'static str, Value)>) -> Option<Value> {
    let mut map = Object::new();
    for (name, value) in members {
        map.insert(Key::new(name).ok()?, value);
    }
    Some(Value::Object(map))
}
