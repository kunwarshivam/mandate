//! Counting the host's calls (E21-3, infrastructure design §8.1). [`Instrumented`] passes every
//! call through unchanged and records only its outcome afterwards, so what the host journals and
//! sends is the same with telemetry working, failing, or off (OPS-11).

use mandate_shell::Cause;
use mandate_shell::stages::JournalWriter;
use mandate_telemetry::catalog::{CYCLES, RECONCILIATIONS, SETTLES};
use mandate_telemetry::{Labels, Metric, Metrics, ProcessType, reason};

use crate::supervisor::Host;

/// A host whose reconciliations, cycles, and settles are counted by outcome.
#[derive(Debug)]
pub struct Instrumented<H> {
    host: H,
    metrics: Metrics,
}

impl<H> Instrumented<H> {
    pub fn new(host: H, metrics: Metrics) -> Self {
        Self { host, metrics }
    }

    fn record(&self, metric: &Metric, outcome: &Result<(), Cause>) {
        let code = if outcome.is_ok() {
            reason::COMPLETED
        } else {
            reason::FAILED
        };
        let labels = Labels::new()
            .process(ProcessType::AgentProcess)
            .reason(code);
        self.metrics.count(metric, &labels);
    }
}

impl<H: Host> Host for Instrumented<H> {
    fn reconcile(&mut self, journal: &mut dyn JournalWriter) -> Result<(), Cause> {
        let outcome = self.host.reconcile(journal);
        self.record(&RECONCILIATIONS, &outcome);
        outcome
    }

    fn cycle(&mut self, journal: &mut dyn JournalWriter) -> Result<(), Cause> {
        let outcome = self.host.cycle(journal);
        self.record(&CYCLES, &outcome);
        outcome
    }

    fn settle(&mut self, journal: &mut dyn JournalWriter) -> Result<(), Cause> {
        let outcome = self.host.settle(journal);
        self.record(&SETTLES, &outcome);
        outcome
    }
}
