//! The closed metric set. A [`Metric`] has private fields and is built only here, so every metric
//! any process records is in [`CATALOG`], and [`crate::lint::catalog`] checks each one.

use crate::label::LabelKey;

/// What an instrument records. Every value is a count or a 0-or-1 state: none can carry a
/// quantity, a price, or a position. A duration kind joins with the first metric that needs one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A monotonic count of events; its name ends `_total`.
    Counter,
    /// A state that is 0 or 1 (ready, a build's info); its name does not end `_total`.
    State,
}

/// One metric: its exported name, its fixed help text, its kind, and the exact label keys every
/// record of it carries, in [`LabelKey`] order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metric {
    name: &'static str,
    help: &'static str,
    kind: Kind,
    labels: &'static [LabelKey],
}

impl Metric {
    pub const fn name(&self) -> &'static str {
        self.name
    }

    pub const fn help(&self) -> &'static str {
        self.help
    }

    pub const fn kind(&self) -> Kind {
        self.kind
    }

    pub const fn labels(&self) -> &'static [LabelKey] {
        self.labels
    }
}

/// Decision cycles a process ran, by outcome (`completed`, `failed`).
pub const CYCLES: Metric = Metric {
    name: "mandate_process_cycles_total",
    help: "Decision cycles run, by outcome.",
    kind: Kind::Counter,
    labels: &[LabelKey::Process, LabelKey::Reason],
};

/// Start-up reconciliations a process ran, by outcome (`completed`, `failed`).
pub const RECONCILIATIONS: Metric = Metric {
    name: "mandate_process_reconciliations_total",
    help: "Start-up reconciliations run, by outcome.",
    kind: Kind::Counter,
    labels: &[LabelKey::Process, LabelKey::Reason],
};

/// Drain settles a process ran, by outcome (`completed`, `failed`).
pub const SETTLES: Metric = Metric {
    name: "mandate_process_settles_total",
    help: "Drain settles run, by outcome.",
    kind: Kind::Counter,
    labels: &[LabelKey::Process, LabelKey::Reason],
};

/// 1 for the running build, by process type and release.
pub const BUILD: Metric = Metric {
    name: "mandate_process_build_info",
    help: "The running build.",
    kind: Kind::State,
    labels: &[LabelKey::Process, LabelKey::Release],
};

/// Records refused because their labels were not the metric's declared keys or their metric was
/// of another kind, by reason; the refused record itself is never exported.
pub const REFUSED: Metric = Metric {
    name: "mandate_telemetry_refused_total",
    help: "Records refused, by reason.",
    kind: Kind::Counter,
    labels: &[LabelKey::Reason],
};

/// Every metric there is.
pub static CATALOG: [Metric; 5] = [CYCLES, RECONCILIATIONS, SETTLES, BUILD, REFUSED];

/// The catalog entry named `name`, if any.
pub fn find(name: &str) -> Option<&'static Metric> {
    CATALOG.iter().find(|metric| metric.name == name)
}
