//! The recording handle: OpenTelemetry instruments for the catalog, read by a Prometheus
//! registry only when a scrape asks.

use std::sync::Arc;

use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Gauge, MeterProvider};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use prometheus::{Registry, TextEncoder};

use crate::catalog::{BUILD, CATALOG, Kind, Metric, REFUSED};
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
        let registry = Registry::new();
        let exporter = opentelemetry_prometheus::exporter()
            .with_registry(registry.clone())
            .without_units()
            .without_counter_suffixes()
            .without_target_info()
            .scope_info_enabled(false)
            .build()
            .map_err(|_| TelemetryError::Unavailable)?;
        let provider = SdkMeterProvider::builder()
            .with_resource(Resource::builder_empty().build())
            .with_reader(exporter)
            .build();
        let meter = provider.meter("mandate");
        let named = |kind: Kind| CATALOG.iter().filter(move |m| m.kind() == kind);
        let counters = named(Kind::Counter)
            .map(|m| {
                (
                    m.name(),
                    meter
                        .u64_counter(m.name())
                        .with_description(m.help())
                        .build(),
                )
            })
            .collect();
        let states = named(Kind::State)
            .map(|m| {
                (
                    m.name(),
                    meter.u64_gauge(m.name()).with_description(m.help()).build(),
                )
            })
            .collect();
        let metrics = Self(Some(Arc::new(Inner {
            _provider: provider,
            registry,
            counters,
            states,
        })));
        metrics.state(
            &BUILD,
            &Labels::new().process(process).release(release),
            true,
        );
        Ok(metrics)
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
        let Some(inner) = self.admitted(metric, Kind::Counter, labels) else {
            return;
        };
        if let Some((_, counter)) = inner
            .counters
            .iter()
            .find(|(name, _)| *name == metric.name())
        {
            counter.add(n, &attributes(labels));
        }
    }

    /// Sets a [`Kind::State`] to 1 or 0.
    pub fn state(&self, metric: &Metric, labels: &Labels, on: bool) {
        let Some(inner) = self.admitted(metric, Kind::State, labels) else {
            return;
        };
        if let Some((_, gauge)) = inner.states.iter().find(|(name, _)| *name == metric.name()) {
            gauge.record(u64::from(on), &attributes(labels));
        }
    }

    /// The exposition text a scrape receives, once [`lint::exposition`] finds nothing in it.
    ///
    /// # Errors
    /// [`TelemetryError::Unavailable`] for [`Metrics::off`] or a failed encoding;
    /// [`TelemetryError::Refused`] when the lint finds anything.
    pub fn encode(&self) -> Result<String, TelemetryError> {
        let inner = self.0.as_ref().ok_or(TelemetryError::Unavailable)?;
        let text = TextEncoder::new()
            .encode_to_string(&inner.registry.gather())
            .map_err(|_| TelemetryError::Unavailable)?;
        if lint::exposition(&text).is_empty() {
            Ok(text)
        } else {
            Err(TelemetryError::Refused)
        }
    }

    /// The live handle when `metric` is of `kind` and `labels` carry exactly its declared keys;
    /// otherwise the record is counted in [`REFUSED`] and `None` is returned.
    fn admitted(&self, metric: &Metric, kind: Kind, labels: &Labels) -> Option<&Inner> {
        let inner = self.0.as_deref()?;
        let refused = if metric.kind() != kind {
            Some(reason::KIND_MISMATCH)
        } else if labels.keys() != metric.labels() {
            Some(reason::LABELS_NOT_DECLARED)
        } else {
            None
        };
        match refused {
            None => Some(inner),
            Some(code) => {
                refuse(inner, code);
                None
            }
        }
    }
}

fn refuse(inner: &Inner, code: ReasonCode) {
    if let Some((_, counter)) = inner
        .counters
        .iter()
        .find(|(name, _)| *name == REFUSED.name())
    {
        counter.add(1, &attributes(&Labels::new().reason(code)));
    }
}

fn attributes(labels: &Labels) -> Vec<KeyValue> {
    labels
        .pairs()
        .map(|(key, value)| KeyValue::new(key, value.to_owned()))
        .collect()
}
