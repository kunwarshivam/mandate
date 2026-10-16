//! The Robinhood connector in the shell (E7-6, the first live trade brief's C1 wiring; DEC-928):
//! [`Connector`] and [`Reconciler`] over one `mandate_robinhood::RobinhoodConnector`, with the
//! capability profile from `robinhood_profile` and never Alpaca's.
//!
//! `BrokerConnector::call` and the connector's reads are async and the stages are not, so the
//! shell owns the runtime that drives them. Unlike [`crate::adapters::AlpacaConnector`]'s, which
//! is built per call, this runtime lives as long as the connector: the MCP client's HTTP
//! connections belong to the runtime they were opened on, so the client must be connected on it
//! and every later call made on it (DEC-928 item 2). The connector and its reconciler share one
//! [`RobinhoodBridge`], so the reconciler reads the same agentic account through the same session.

use std::cell::RefCell;
use std::rc::Rc;

use mandate_domain::{CapabilityProfile, ProfileError};
use mandate_executor::{
    BrokerConnector, BrokerOutcome, BrokerRequest, BrokerSnapshot, ConnectorError, Reconciliation,
};
use mandate_robinhood::{RobinhoodConnector, Tools, robinhood_profile};
use tokio::runtime::Runtime;

use crate::error::Cause;
use crate::stages::{Connector, Reconciler};

/// The runtime and the one Robinhood connector the shell's connector and reconciler share.
pub struct RobinhoodBridge<T> {
    runtime: Runtime,
    connector: RefCell<RobinhoodConnector<T>>,
}

impl<T: Tools> RobinhoodBridge<T> {
    /// `connector` is [`RobinhoodConnector::new`] or, after a restart,
    /// [`RobinhoodConnector::restore`]; its tools, an `McpClient` in production, must have been
    /// connected on `runtime` (DEC-928 item 2).
    pub fn new(runtime: Runtime, connector: RobinhoodConnector<T>) -> Rc<Self> {
        Rc::new(Self {
            runtime,
            connector: RefCell::new(connector),
        })
    }
}

/// The shell's [`Connector`] over the bridge: each request is the Robinhood connector's own,
/// driven to its end on the bridge's runtime.
pub struct RobinhoodShellConnector<T> {
    bridge: Rc<RobinhoodBridge<T>>,
}

impl<T: Tools> RobinhoodShellConnector<T> {
    pub fn new(bridge: Rc<RobinhoodBridge<T>>) -> Self {
        Self { bridge }
    }
}

impl<T: Tools> Connector for RobinhoodShellConnector<T> {
    /// Robinhood's profile (trading spec §5.2, DEC-860 item 5), so the executor's protection reads
    /// the stop-limit Robinhood offers and never Alpaca's forms (DEC-838 item 5).
    fn profile(&self) -> Result<CapabilityProfile, ProfileError> {
        robinhood_profile()
    }

    /// One request, answered as `RobinhoodConnector::call` answers it. A request made while the
    /// reconciler holds the connector is `NotSent` (`connector_busy`) with nothing called; the
    /// stages run one at a time, so it marks a wiring fault, never a broker answer. It blocks on
    /// the bridge's runtime, so it is called from synchronous code only, never from inside another
    /// tokio runtime, as `AlpacaConnector::call` is.
    fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        let mut connector =
            self.bridge
                .connector
                .try_borrow_mut()
                .map_err(|_| ConnectorError::NotSent {
                    code: "connector_busy",
                })?;
        self.bridge.runtime.block_on(connector.call(request))
    }
}

/// The startup reconciliation's broker read for a Robinhood account (DEC-928 item 3). It reads
/// the agentic account's orders, positions and portfolio through `account_snapshot`, so every
/// read is scoped to the recorded account (CN-8), and then refuses: trading spec §7.2 and §7.3
/// take the account's status flags, equity and last equity from the broker, and Robinhood
/// publishes none of them (connections spec U-R6, U-R7). No figure is invented in their place
/// (`AGENTS.md` rule 3); the founder decides how they are read before a Robinhood account is
/// reconciled, and until then the run stops at [`crate::Stage::Reconcile`] before it opens
/// anything.
pub struct RobinhoodReconciler<T> {
    bridge: Rc<RobinhoodBridge<T>>,
}

impl<T: Tools> RobinhoodReconciler<T> {
    pub fn new(bridge: Rc<RobinhoodBridge<T>>) -> Self {
        Self { bridge }
    }
}

/// Why a read of the agentic account did not answer: a refused or unreadable read, or another
/// account in its place.
pub const ACCOUNT_UNREAD: &str = "the agentic account's snapshot";

/// Why an account that was read is not reconciled (DEC-928 item 3).
pub const STATUS_UNPUBLISHED: &str =
    "the broker's account status, which Robinhood does not publish (DEC-928)";

impl<T: Tools> Reconciler for RobinhoodReconciler<T> {
    /// The run's connector and the executor's requests are not used: the Robinhood connector
    /// offers none of the four reads they name, and the bridge's connector is the run's own.
    fn snapshot(
        &mut self,
        connector: &mut dyn Connector,
        requests: &[BrokerRequest],
    ) -> Result<BrokerSnapshot, Cause> {
        let _ = (connector, requests);
        let busy = Cause::Absent {
            what: "the Robinhood connector, held by another stage",
        };
        let robinhood = self.bridge.connector.try_borrow().map_err(|_| busy)?;
        let read = self.bridge.runtime.block_on(robinhood.account_snapshot());
        read.map_err(|_| Cause::Absent {
            what: ACCOUNT_UNREAD,
        })?;
        Err(Cause::Absent {
            what: STATUS_UNPUBLISHED,
        })
    }

    /// No Robinhood snapshot is ever handed out, so none is reconciled (DEC-928 item 3).
    fn reconcile(&mut self, snapshot: &BrokerSnapshot) -> Result<Reconciliation, Cause> {
        let _ = snapshot;
        Err(Cause::Absent {
            what: STATUS_UNPUBLISHED,
        })
    }
}
