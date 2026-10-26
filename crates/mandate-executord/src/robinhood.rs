//! The process's [`Broker`] over `mandate-robinhood`'s connector (DEC-2386 items 3 and 4): the
//! connector the shell drives, over whatever MCP seam it is given. This crate names no host and
//! holds no token; a test hands it a scripted seam, and the founder's live build, DEC-2382's, is
//! not this one.
//!
//! Robinhood answers `Submit`, `Cancel` and `ListOrders`, and publishes no account activity, so a
//! reconciliation reads the account once, through [`RobinhoodBroker::account_reads`], as the
//! shell's reconciler does (DEC-933 item 1, DEC-2386 item 2).

use mandate_domain::{CapabilityProfile, ProfileError};
use mandate_executor::{
    BrokerAccount, BrokerOrder, BrokerOutcome, BrokerPosition, BrokerRequest, ConnectorError,
    FoldedEvent,
};
use mandate_robinhood::{RobinhoodConnector, Tickers, Tools};
use tokio::runtime::Runtime;

use crate::Broker;

/// The stub's answer until DEC-2386 slice C's implementation lands (DEC-77).
const UNIMPLEMENTED: ConnectorError = ConnectorError::NotSent {
    code: "unimplemented",
};

/// One read of the agentic account, as a reconciliation takes it: its working orders, its
/// positions, and the account as DEC-933 item 1 maps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountReads {
    pub open_orders: Vec<BrokerOrder>,
    pub positions: Vec<BrokerPosition>,
    pub account: BrokerAccount,
}

/// Robinhood's agentic account over `T`, the MCP seam.
pub struct RobinhoodBroker<T> {
    connector: RobinhoodConnector<T>,
    runtime: Runtime,
}

impl<T: Tools> RobinhoodBroker<T> {
    /// The broker on `account_number`, naming each instrument by its ticker in `tickers`.
    ///
    /// # Errors
    /// `NotSent` (`runtime_unavailable`) when the runtime that drives the connector cannot be
    /// built.
    pub fn new(tools: T, account_number: String, tickers: Tickers) -> Result<Self, ConnectorError> {
        let _ = (tools, account_number, tickers);
        Err(UNIMPLEMENTED)
    }

    /// The broker after a restart, its connector restored from the account stream's `records`
    /// (DEC-860 item 7), so a cancel goes by the broker's order id and no key is placed again.
    ///
    /// # Errors
    /// `NotSent` (`runtime_unavailable`, `restore`) when the runtime cannot be built or the
    /// records cannot be read.
    pub fn restore(
        tools: T,
        account_number: String,
        tickers: Tickers,
        records: &[FoldedEvent],
    ) -> Result<Self, ConnectorError> {
        let _ = (tools, account_number, tickers, records);
        Err(UNIMPLEMENTED)
    }

    /// The agentic account in one read: `account_snapshot`, mapped by `broker_account`.
    ///
    /// # Errors
    /// `Unreadable` (`account_snapshot`) when the read is refused or cannot be read.
    pub fn account_reads(&mut self) -> Result<AccountReads, ConnectorError> {
        let _ = (&self.connector, &self.runtime);
        Err(UNIMPLEMENTED)
    }
}

impl<T: Tools> Broker for RobinhoodBroker<T> {
    fn profile(&self) -> Result<CapabilityProfile, ProfileError> {
        let _ = (&self.connector, &self.runtime);
        Err(ProfileError::Unimplemented)
    }

    fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        let _ = (&self.connector, &self.runtime, request);
        Err(UNIMPLEMENTED)
    }
}
