//! The recording handle: OpenTelemetry instruments for the catalog, read by a Prometheus
//! registry only when a scrape asks.

use std::sync::Arc;
use std::time::Duration;

use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Gauge, Histogram, MeterProvider};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use prometheus::{Registry, TextEncoder};

use crate::catalog::{BUILD, CATALOG, DURATION_BOUNDS_S, Kind, Metric, REFUSED};
use crate::label::{Labels, ProcessType, Release};
use crate::{ReasonCode, TelemetryError, lint, reason};

/// A process's metrics. Cloning shares them. Every record returns nothing and never fails: a
/// refused record is counted in [`REFUSED`] and otherwise dropped (OPS-11).
#[derive(Clone)]
pub struct Metrics(Option<Arc<Inner>>);

struct Inner {
    _provider: SdkMeterProvider,
    registry: Registry,
    counters: Vec<(&'static str, Counter<u64>)>,
    states: Vec<(&'static str, Gauge<u64>)>,
    durations: Vec<(&'static str, Histogram<f64>)>,
}

impl std::fmt::Debug for Metrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() {
            "Metrics"
        } else {
            "Metrics(off)"
        })
    }
}

impl Metrics {
    /// The catalog's instruments over a registry of their own, with [`BUILD`] set for `process`
    /// and `release`. No unit or `_total` suffix is added, no scope labels and no `target_info`
    /// are exported: each exported name is the catalog's own.
    ///
    /// # Errors
    /// [`TelemetryError::Unavailable`] if the exporter cannot be built; the process then runs
    /// with [`Metrics::off`].
    pub fn new(process: ProcessType, release: &Release) -> Result<Self, TelemetryError> {
        let _ = (
            process,
            release,
            BUILD,
            DURATION_BOUNDS_S,
            CATALOG,
            REFUSED,
            reason::FAILED,
        );
        Err(TelemetryError::Unimplemented { story: "E21-3" })
    }

    /// Metrics that record nothing and serve nothing: what a process runs with when
    /// [`Metrics::new`] fails.
    pub fn off() -> Self {
        Self(None)
    }

    /// Adds 1 to a [`Kind::Counter`].
    pub fn count(&self, metric: &Metric, labels: &Labels) {
        self.add(metric, labels, 1);
    }

    /// Adds `n` to a [`Kind::Counter`].
    pub fn add(&self, metric: &Metric, labels: &Labels, n: u64) {
        let _ = (metric, labels);
    }

    /// Sets a [`Kind::State`] to 1 or 0.
    pub fn state(&self, metric: &Metric, labels: &Labels, on: bool) {
        let _ = (metric, labels);
    }

    /// Records one [`Kind::Duration`].
    pub fn duration(&self, metric: &Metric, labels: &Labels, elapsed: Duration) {
        let _ = (metric, labels);
    }

    /// The exposition text a scrape receives, once [`lint::exposition`] finds nothing in it.
    ///
    /// # Errors
    /// [`TelemetryError::Unavailable`] for [`Metrics::off`] or a failed encoding;
    /// [`TelemetryError::Refused`] when the lint finds anything.
    pub fn encode(&self) -> Result<String, TelemetryError> {
        let _ = (lint::own, TextEncoder::new);
        Err(TelemetryError::Unimplemented { story: "E21-3" })
    }
}
