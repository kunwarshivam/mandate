//! One broker account's executor as a long-running process (E21-30, DEC-2003).

use std::time::Duration;

use mandate_domain::{CapabilityProfile, ProfileError};
use mandate_executor::{
    AccountScope, BindingGateSource, BrokerOutcome, BrokerRequest, ConnectorError, ExecutorConfig,
    ExecutorState, IdGen, InstrumentSnapshot, MandateView, Ports,
};
use mandate_journal::Environment;
use mandate_process::Lifecycle;
use mandate_store::Store;
use mandate_time::UtcNanos;

/// The version the envelope's actor carries.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub trait Broker {
    /// The broker's capability profile, set on the state before `Input::Started` (DEC-838 item 5).
    fn profile(&self) -> Result<CapabilityProfile, ProfileError>;
    fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError>;
}

/// The executor core's ports for one account.
pub struct Wiring {
    pub ids: Box<dyn IdGen>,
    pub mandates: Box<dyn MandateView>,
    pub instruments: Box<dyn InstrumentSnapshot>,
    pub config: ExecutorConfig,
    pub fees: mandate_accounting::Config,
    pub gate: Box<dyn BindingGateSource>,
}

impl Wiring {
    fn ports(&self) -> Ports<'_> {
        Ports {
            ids: self.ids.as_ref(),
            mandates: self.mandates.as_ref(),
            instruments: self.instruments.as_ref(),
            config: &self.config,
            fees: &self.fees,
        }
    }
}

/// What an executor process runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub scope: AccountScope,
    pub environment: Environment,
    pub liveness: Duration,
    /// How many broker answers deep one input may lead before the process stops, so a broker that
    /// keeps answering with work for the core can never hold the loop (DEC-2003 item 7).
    pub max_chain: usize,
}

/// An owner alert the core asked for, after the draft it names committed: a fixed key and an opaque
/// event id, nothing else (`AGENTS.md` rule 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alert {
    pub key: &'static str,
    pub subject: Option<String>,
}

/// Why a reconciliation read gave no snapshot.

/// One broker account's executor process.
pub struct ExecutorProcess<S, B> {
    store: S,
    broker: B,
    state: ExecutorState,
    lifecycle: Lifecycle,
}

impl<S: Store, B: Broker> ExecutorProcess<S, B> {
    /// Starts at `now`: refuses any environment but paper before taking anything, takes a newer
    /// writer epoch on the account stream, folds it, and opens it if it is empty.
    pub fn start(store: S, broker: B, wiring: Wiring, config: &Config, now: UtcNanos) -> Self {
        let _ = (store, broker, wiring, config, now);
        todo!()
    }

    /// One pass of the loop at the injected time `now`.
    pub fn step(&mut self, now: UtcNanos) -> Vec<Alert> {
        let _ = now;
        todo!()
    }

    /// Drains at `now`.
    pub fn drain(&mut self, now: UtcNanos) {
        let _ = now;
        todo!()
    }

    pub fn lifecycle(&self) -> &Lifecycle {
        &self.lifecycle
    }

    pub fn state(&self) -> &ExecutorState {
        &self.state
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    pub fn broker(&self) -> &B {
        &self.broker
    }

    /// Whether the process may take an opening (design §3.4).
    pub fn ready(&self) -> bool {
        todo!()
    }
}
