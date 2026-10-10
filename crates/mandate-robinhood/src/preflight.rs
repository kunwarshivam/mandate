//! The preflight facts and the account snapshot (the first live trade brief's C4; DEC-470 item 1,
//! DEC-875 item 6, DEC-902): what the runner's preflight reads of the agentic account, mapped from
//! the answers of `get_portfolio`, `get_equity_positions`, `get_equity_orders`,
//! `get_equity_tradability` and `get_equity_quotes`. Every account read goes through
//! [`RobinhoodConnector::read`], so each one is scoped to the agentic account and checks it
//! afresh. A record of an assumed shape (DEC-875 item 4, DEC-902 items 1 to 6) with a field
//! missing, a field it does not list, a value of another type, or a number that is not canonical
//! decimal text refuses the whole read (`AGENTS.md` rule 3). Nothing here is defaulted.

use mandate_accounting::InstrumentId;
use mandate_executor::{BrokerOrder, BrokerPosition};
use mandate_num::{Price, Usd};

use crate::{RobinhoodConnector, RobinhoodError, Tools};

/// What DEC-470 item 1 checks of the agentic account, as the broker answered it: its cash, its
/// buying power, every position record (a zero quantity included), and every working order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountSnapshot {
    /// `get_portfolio`'s `cash`.
    pub cash: Usd,
    /// `get_portfolio`'s `buying_power`.
    pub buying_power: Usd,
    /// Every record of `get_equity_positions`, in the order the broker listed them.
    pub positions: Vec<BrokerPosition>,
    /// The records of `get_equity_orders` whose `state` is working (`new`, `queued`,
    /// `confirmed`, `unconfirmed`, `partially_filled`), in the order the broker listed them.
    pub open_orders: Vec<BrokerOrder>,
}

/// `get_equity_quotes`'s one record for the symbol (DEC-902 item 5): an uncrossed quote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quote {
    pub symbol: InstrumentId,
    pub bid: Price,
    pub ask: Price,
}

/// The account snapshot, and the quote of the one instrument the agentic account may trade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightFacts {
    pub account: AccountSnapshot,
    pub quote: Quote,
}

impl<T: Tools> RobinhoodConnector<T> {
    /// The agentic account's snapshot: `get_portfolio`, `get_equity_positions` and
    /// `get_equity_orders`, each through [`Self::read`] with no filter but the recorded account,
    /// on the `Ordinary` budget. A refusal of any read is the snapshot's refusal, unchanged; a
    /// record that is not exactly its assumed shape is [`RobinhoodError::Unreadable`] with the
    /// read's code (DEC-902). No order, cancel or exit waits on it (`AGENTS.md` rule 13).
    pub async fn account_snapshot(&self) -> Result<AccountSnapshot, RobinhoodError> {
        Err(RobinhoodError::Unimplemented { story: "E7-6" })
    }

    /// [`Self::account_snapshot`], then `get_equity_tradability` for `symbol` through
    /// [`Self::read`], and `get_equity_quotes` for `symbol` alone, which names no account. A
    /// symbol the account may not trade is [`RobinhoodError::NotTradable`]; a quote that is not
    /// exactly one record for `symbol`, or is crossed, is [`RobinhoodError::Unreadable`]
    /// (DEC-902 items 5 and 6).
    pub async fn preflight_facts(
        &self,
        symbol: &InstrumentId,
    ) -> Result<PreflightFacts, RobinhoodError> {
        let _ = symbol;
        Err(RobinhoodError::Unimplemented { story: "E7-6" })
    }
}
