//! The process's [`Broker`] over `mandate-robinhood`'s connector (DEC-2386 items 3 and 4): the
//! connector the shell drives, over whatever MCP seam it is given. This crate names no host and
//! holds no token; a test hands it a scripted seam, and the founder's live build, DEC-2382's, is
//! not this one.
//!
//! Robinhood answers `Submit`, `Cancel` and `ListOrders`, and publishes no account activity, so a
//! reconciliation reads the account once, through [`RobinhoodBroker::account_reads`], as the
//! shell's reconciler does (DEC-933 item 1, DEC-2386 item 2).

use mandate_domain::{CapabilityProfile, ProfileError};
use mandate_executor::BrokerConnector;
use mandate_executor::{
    BrokerAccount, BrokerOrder, BrokerOutcome, BrokerPosition, BrokerRequest, ConnectorError,
    FoldedEvent,
};
use mandate_robinhood::{RobinhoodConnector, Tickers, Tools, broker_account};
use tokio::runtime::Runtime;

use crate::Broker;

/// The runtime that drives the connector: one current-thread runtime, built once, that holds
/// nothing a restart would lose, as `AlpacaBroker`'s does.
fn runtime() -> Result<Runtime, ConnectorError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .map_err(|_| ConnectorError::NotSent {
            code: "runtime_unavailable",
        })
}

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
        Ok(Self {
            connector: RobinhoodConnector::new(tools, account_number, tickers),
            runtime: runtime()?,
        })
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
        let connector = RobinhoodConnector::restore(tools, account_number, tickers, records)
            .map_err(|_| ConnectorError::NotSent { code: "restore" })?;
        Ok(Self {
            connector,
            runtime: runtime()?,
        })
    }

    /// The agentic account in one read: `account_snapshot`, mapped by `broker_account`.
    ///
    /// # Errors
    /// `Unreadable` (`account_snapshot`) when the read is refused or cannot be read.
    pub fn account_reads(&mut self) -> Result<AccountReads, ConnectorError> {
        let snapshot = self
            .runtime
            .block_on(self.connector.account_snapshot())
            .map_err(|_| ConnectorError::Unreadable {
                code: "account_snapshot",
            })?;
        Ok(AccountReads {
            account: broker_account(&snapshot),
            open_orders: snapshot.open_orders,
            positions: snapshot.positions,
        })
    }
}

impl<T: Tools> Broker for RobinhoodBroker<T> {
    fn profile(&self) -> Result<CapabilityProfile, ProfileError> {
        self.connector.profile()
    }

    fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        self.runtime.block_on(self.connector.call(request))
    }

    fn account_reads(&mut self) -> Option<Result<AccountReads, ConnectorError>> {
        Some(RobinhoodBroker::account_reads(self))
    }
}
