//! The label lint (design §8.1): over the catalog, every metric's name, help text, kind suffix,
//! and label keys; over a scrape, every family, sample, label key, and label value, each against
//! the catalog.

use crate::catalog::{CATALOG, Kind, Metric};
use crate::label::LabelKey;
use crate::{ReasonCode, reason};

/// Words no metric name or help text may hold (design §8.1, `AGENTS.md` "Do not"): order details,
/// positions, money, mandate content, people, and secrets.
#[rustfmt::skip]
pub const FORBIDDEN_WORDS: [&str; 34] = [
    "symbol", "instrument", "ticker", "side", "buy", "sell", "qty", "quantity", "price",
    "notional", "position", "positions", "pnl", "profit", "loss", "cash", "balance", "equity",
    "exposure", "value", "amount", "allocation", "limit", "thesis", "score", "user", "person",
    "name", "email", "phone", "secret", "token", "password", "credential",
];

/// What the lint found. A finding names the metric as the catalog or the scrape spells it and
/// the label key, never a label value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// A name that is not `mandate_` and then `[a-z0-9_]`.
    Name { metric: String },
    /// A name or help text holding one of [`FORBIDDEN_WORDS`].
    Forbidden { metric: String, word: &'static str },
    /// A name whose suffix does not match its kind.
    Suffix { metric: String },
    /// Two catalog entries with one name.
    Duplicate { metric: String },
    /// A metric declaring one label key twice, or out of [`LabelKey`] order.
    Labels { metric: String },
    /// A reason code that is not an opaque ID's shape.
    Reason { code: &'static str },
    /// A scraped family or sample that is not in the catalog.
    Unknown { metric: String },
    /// A scraped help text or type that is not the catalog's.
    Header { metric: String },
    /// A scraped label key outside the metric's declared set.
    Undeclared { metric: String, key: String },
    /// A scraped label value its key does not admit.
    Value { metric: String, key: &'static str },
    /// A scraped line that does not parse, by its 1-based number.
    Malformed { line: usize },
}

/// Every finding over `metrics`: each metric by [`metric`], and every name used twice.
pub fn catalog(metrics: &[Metric]) -> Vec<Finding> {
    let _ = UNIMPLEMENTED;
    Vec::new()
}

/// Every finding over one metric's declaration.
pub fn metric(name: &str, help: &str, kind: Kind, labels: &[LabelKey]) -> Vec<Finding> {
    let _ = UNIMPLEMENTED;
    Vec::new()
}

/// Every reason code in `codes` that is not an opaque ID's shape.
pub fn reasons(codes: &[ReasonCode]) -> Vec<Finding> {
    let _ = UNIMPLEMENTED;
    Vec::new()
}

/// The findings over the crate's own catalog and reason codes, which its tests require empty.
pub fn own() -> Vec<Finding> {
    let _ = (metric, exposition);
    let mut findings = catalog(&CATALOG);
    findings.extend(reasons(&reason::ALL));
    findings
}

/// Every finding over a Prometheus text exposition: each `# HELP` and `# TYPE` line must be a
/// catalog metric's own; each sample a catalog metric's (for a duration, its `_bucket`, `_sum`,
/// or `_count`), carrying only that metric's declared keys (and `le` on a bucket), each value
/// one its key admits.
pub fn exposition(text: &str) -> Vec<Finding> {
    let _ = UNIMPLEMENTED;
    Vec::new()
}

const UNIMPLEMENTED: &str = "E21-3";
