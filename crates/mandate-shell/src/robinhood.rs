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
    BrokerAccount, BrokerConnector, BrokerOutcome, BrokerRequest, BrokerSnapshot, ConnectorError,
    Reconciliation,
};
use mandate_num::Usd;
use mandate_robinhood::{
    AccountSnapshot, PreflightFacts, RobinhoodConnector, Tools, robinhood_profile,
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

/// The agentic account's snapshot as the executor's [`BrokerAccount`] (DEC-933 item 1, the
/// recommendation of DEC-928's note to the founder, under DEC-929):
///
/// - `cash` and `buying_power` are `get_portfolio`'s, and `non_marginable_buying_power` is the
///   lower of the two (DEC-620 item 2), so the gate never spends more than cash;
/// - `status` is `ACTIVE` and no flag is set, because a snapshot exists only when every one of
///   its reads found the recorded account listed `agentic_allowed` (DEC-875); any other answer
///   refused the read;
/// - every figure Robinhood does not publish reads as the value that adds no buying power and
///   lifts no limit: `crypto_status` `INACTIVE` (no crypto opening), `multiplier` 1 (DEC-529
///   item 11), `equity` the cash alone (never more than a long-only account's equity),
///   `last_equity` zero (below every equity threshold, so §9.2's `legacy_pdt` budget would apply
///   if the account were ever declared a margin account; as a cash account the gate's settled-cash
///   rule binds instead),
///   `maintenance_margin` the whole equity (no maintenance excess), and `accrued_fees` zero,
///   which the gate's buying power, the lower of cash and the broker's figure, never adds to.
#[must_use]
pub fn robinhood_account(snapshot: &AccountSnapshot) -> BrokerAccount {
    let cash = snapshot.cash;
    BrokerAccount {
        status: "ACTIVE".to_owned(),
        crypto_status: "INACTIVE".to_owned(),
        trading_blocked: false,
        account_blocked: false,
        trade_suspended_by_user: false,
        multiplier: 1,
        equity: cash,
        cash,
        buying_power: snapshot.buying_power,
        non_marginable_buying_power: cash.min(snapshot.buying_power),
        accrued_fees: Usd::ZERO,
        last_equity: Usd::ZERO,
        maintenance_margin: cash,
    }
}

impl<T: Tools> Reconciler for RobinhoodReconciler<T> {
    /// The run's connector and the executor's requests are not used: the Robinhood connector
    /// offers none of the four reads they name, and the bridge's connector is the run's own.
    /// Robinhood publishes no account activity, so the snapshot carries no fill and leaves the
    /// executor's cursor where it was; the live run starts only on a flat, unspent account
    /// (DEC-529 item 3), so no fill is outstanding when it does.
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
        Ok(self.executor.without_activities(
            snapshot.open_orders.clone(),
            snapshot.positions.clone(),
            robinhood_account(&snapshot),
        ))
    }

    /// The executor's own reconciliation of the snapshot, as for every broker.
    fn reconcile(&mut self, snapshot: &BrokerSnapshot) -> Result<Reconciliation, Cause> {
        self.executor.reconcile(snapshot)
    }
}
