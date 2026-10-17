#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! Metrics through the OpenTelemetry API with a Prometheus pull exporter (backlog E21-3,
//! [infrastructure design](../../../docs/design/infrastructure.md) §8.1, ADR-0001 ES-18, DEC-73).
//!
//! **Telemetry is never the record and never an input** (OPS-11). [`Metrics`] only takes records
//! in: nothing reads a value back except [`Metrics::encode`], which renders the exposition text a
//! scrape receives. Every record is infallible and never blocks on the exporter, so a process
//! whose exporter cannot bind, whose scraper stalls, or whose scrape fails runs the same decision
//! cycle as one with no telemetry at all. The release profile aborts on panic, so the exporter's
//! thread is written to the same no-panic lints as the trading paths.
//!
//! **Only opaque labels leave** (design §8.1, `AGENTS.md` "Do not"). The metric set is closed:
//! [`catalog::CATALOG`] is every metric, a [`Metric`]'s fields are private, and its label keys are
//! [`LabelKey`]'s six, each taking only its own value type ([`OpaqueId`], [`ProcessType`],
//! [`ReasonCode`], [`Release`]). A record whose labels are not the metric's declared keys is
//! refused and counted. [`lint::catalog`] checks every metric's name and help text, and
//! [`lint::exposition`] checks every scrape before it is served: a scrape it finds anything in is
//! answered `503` with fixed text, never sent.
//!
//! **Air-gapped.** The exporter is a pull endpoint on the address the process names, with no
//! collector, no push, and no outbound connection (design §8.1: push export only from M8, under a
//! data policy). Where it may be reached from is the cell's network policy (E21-36, E21-39).
//!
//! No core crate depends on this one: its `allowed_dependents` in `xtask/layers.toml` names only
//! process binaries (OPS-11's layering check).

pub mod catalog;
mod exporter;
pub mod label;
pub mod lint;
mod metrics;
pub mod reason;

pub use catalog::{Kind, Metric};
pub use exporter::{Exporter, answer, serve};
pub use label::{LabelKey, Labels, OpaqueId, ProcessType, ReasonCode, Release};
pub use metrics::Metrics;

/// Every way this crate can refuse. None of them is ever a reason for a process to stop or to
/// change what it does.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TelemetryError {
    /// The body of every stub before its implementation (DEC-77, DEC-921).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    /// Text that is not an opaque ID, reason code, or release was offered as one.
    #[error("not an opaque label value")]
    NotOpaque,
    /// The registry could not be built or gathered.
    #[error("the metrics registry is unavailable")]
    Unavailable,
    /// The exposition lint found something outside the allowed set, so nothing is served.
    #[error("the scrape failed the label lint")]
    Refused,
    /// The exporter could not bind or start its thread.
    #[error("the exporter could not start: {0:?}")]
    Start(std::io::ErrorKind),
}
