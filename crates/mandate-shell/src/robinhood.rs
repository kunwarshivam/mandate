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
//!
//! [`robinhood_account`] reads the agentic account's snapshot into the executor's
//! [`BrokerAccount`] (DEC-933 item 1): what Robinhood reports is taken as reported, the status
//! flags are the agentic check every read passes, and every figure Robinhood does not publish
//! reads as the value that adds no buying power and lifts no limit.

use std::cell::RefCell;
use std::rc::Rc;

use mandate_accounting::{AccountType, InstrumentId};
use mandate_alpaca::{AccountRules, DeclaredRegime};
use mandate_domain::{CapabilityProfile, ProfileError};
use mandate_executor::{
    BrokerConnector, BrokerOutcome, BrokerRequest, BrokerSnapshot, ConnectorError, Reconciliation,
};
use mandate_robinhood::{
    AccountSnapshot, PreflightFacts, Quote, RobinhoodConnector, Tickers, Tools, robinhood_profile,
};
use tokio::runtime::Runtime;

use crate::adapters::ExecutorReconciler;
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

    /// The preflight's read of the agentic account and of `symbol`'s quote
    /// (`RobinhoodConnector::preflight_facts`), driven on the bridge's runtime (DEC-933 item 2).
    /// Every read is the connector's own, scoped to the recorded account; nothing is sent.
    ///
    /// # Errors
    /// [`Cause::Absent`] naming [`ACCOUNT_UNREAD`] when any read is refused or unreadable, the
    /// symbol is not tradable, or the connector is held by another stage.
    pub fn preflight_facts(&self, symbol: &InstrumentId) -> Result<PreflightFacts, Cause> {
        let unread = || Cause::Absent {
            what: ACCOUNT_UNREAD,
        };
        let robinhood = self.connector.try_borrow().map_err(|_| unread())?;
        self.runtime
            .block_on(robinhood.preflight_facts(symbol))
            .map_err(|_| unread())
    }

    /// The preflight's reads on the account snapshot `account` the same run's lapse read
    /// (`RobinhoodConnector::preflight_facts_on`, DEC-959 item 1): the tradability and the
    /// quote, driven on the bridge's runtime, and no account figure read again. The caller has
    /// judged `account` fresh; nothing is sent.
    ///
    /// # Errors
    /// As [`Self::preflight_facts`].
    pub fn preflight_facts_on(
        &self,
        account: AccountSnapshot,
        symbol: &InstrumentId,
    ) -> Result<PreflightFacts, Cause> {
        let unread = || Cause::Absent {
            what: ACCOUNT_UNREAD,
        };
        let robinhood = self.connector.try_borrow().map_err(|_| unread())?;
        self.runtime
            .block_on(robinhood.preflight_facts_on(account, symbol))
            .map_err(|_| unread())
    }

    /// Registers the run's tickers with the connector (`RobinhoodConnector::register`, DEC-956
    /// item 1): the session is opened before the run reads its confirmed mandate, so the pinned
    /// instrument's `(asset id, symbol)` is handed over here, before the preflight.
    ///
    /// # Errors
    /// [`Cause::Absent`] when the connector is held by another stage; nothing is registered.
    pub fn register(&self, tickers: Tickers) -> Result<(), Cause> {
        let mut robinhood = self.connector.try_borrow_mut().map_err(|_| Cause::Absent {
            what: "the Robinhood connector, held by another stage",
        })?;
        robinhood.register(tickers);
        Ok(())
    }

    /// The agentic account's snapshot (`RobinhoodConnector::account_snapshot`), driven on the
    /// bridge's runtime: the evidence the live door's lapse counts (DEC-933 item 3, DEC-947
    /// item 1). The account reads alone, scoped to the recorded account; no quote is read and
    /// nothing is sent.
    ///
    /// # Errors
    /// [`Cause::Absent`] naming [`ACCOUNT_UNREAD`] when any read is refused or unreadable, or
    /// the connector is held by another stage.
    pub fn account_snapshot(&self) -> Result<AccountSnapshot, Cause> {
        let unread = || Cause::Absent {
            what: ACCOUNT_UNREAD,
        };
        let robinhood = self.connector.try_borrow().map_err(|_| unread())?;
        self.runtime
            .block_on(robinhood.account_snapshot())
            .map_err(|_| unread())
    }

    /// `symbol`'s quote as `get_equity_quotes` answers it now
    /// (`RobinhoodConnector::equity_quote`), driven on the bridge's runtime: the broker's own ask
    /// the runner's opening cross-check reads at the check (DEC-945). It names no account and
    /// sends nothing.
    ///
    /// # Errors
    /// [`Cause::Absent`] naming [`QUOTE_UNREAD`] when the read is refused or unreadable, or the
    /// connector is held by another stage.
    pub fn equity_quote(&self, symbol: &InstrumentId) -> Result<Quote, Cause> {
        let unread = || Cause::Absent { what: QUOTE_UNREAD };
        let robinhood = self.connector.try_borrow().map_err(|_| unread())?;
        self.runtime
            .block_on(robinhood.equity_quote(symbol))
            .map_err(|_| unread())
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

/// The startup reconciliation's broker read for a Robinhood account (DEC-928 item 3, DEC-933
/// item 1). It reads the agentic account's portfolio, positions and orders through
/// `account_snapshot`, so every read is scoped to the recorded account (CN-8), maps the account
/// by [`robinhood_account`], and hands the snapshot to the executor's own reconciliation, which
/// reconciles it exactly as it reconciles Alpaca's.
pub struct RobinhoodReconciler<T> {
    bridge: Rc<RobinhoodBridge<T>>,
    executor: ExecutorReconciler,
}

impl<T: Tools> RobinhoodReconciler<T> {
    /// `executor` is the run's own reconciler, the one `CoreExecutor::pair` made beside the
    /// run's executor, so the snapshot is taken at that executor's head and cursor.
    pub fn new(bridge: Rc<RobinhoodBridge<T>>, executor: ExecutorReconciler) -> Self {
        Self { bridge, executor }
    }
}

/// Why a read of the agentic account did not answer: a refused or unreadable read, or another
/// account in its place.
pub const ACCOUNT_UNREAD: &str = "the agentic account's snapshot";

/// Why a read of the instrument's Robinhood quote did not answer: a refused or unreadable read,
/// a record of another instrument, or a crossed quote (DEC-902 item 7).
pub const QUOTE_UNREAD: &str = "the instrument's Robinhood quote";

/// The account type and day-trading regime the Robinhood connector declares (trading spec §7.2):
/// a cash account, on the founder's attestation for DEC-529's order (DEC-529 item 11, DEC-620
/// item 2), under `legacy_pdt`, since the contract does not show Robinhood has moved to the
/// intraday margin standard. No broker call answers it.
#[must_use]
pub fn robinhood_account_rules() -> AccountRules {
    AccountRules {
        account_type: AccountType::Cash,
        regime: DeclaredRegime::LegacyPdt,
    }
}

/// The agentic account's snapshot as the executor's [`BrokerAccount`] (DEC-933 item 1): the one
/// mapping the shell and the executor process share, in `mandate-robinhood` (DEC-2386 item 2).
pub use mandate_robinhood::broker_account as robinhood_account;

impl<T: Tools> Reconciler for RobinhoodReconciler<T> {
    /// The run's connector and the executor's requests are not used: the Robinhood connector
    /// offers none of the four reads they name, and the bridge's connector is the run's own.
    /// Robinhood publishes no account activity, so the snapshot leaves the executor's cursor
    /// where it was, and its only fills are the ones an order record and the position imply: an
    /// entry's on a ledger flat in the instrument (DEC-1151, connections spec U-R8), and a
    /// risk-reducing sell's at its limit (DEC-2300, DEC-2395); the live run starts only
    /// on a flat, unspent account (DEC-529 item 3), so none is outstanding when it does.
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
        let snapshot = read.map_err(|_| Cause::Absent {
            what: ACCOUNT_UNREAD,
        })?;
        self.executor.without_activities(
            snapshot.open_orders.clone(),
            snapshot.positions.clone(),
            robinhood_account(&snapshot),
        )
    }

    /// The executor's own reconciliation of the snapshot, as for every broker.
    fn reconcile(&mut self, snapshot: &BrokerSnapshot) -> Result<Reconciliation, Cause> {
        self.executor.reconcile(snapshot)
    }
}
