//! Reads of the agentic account only (connections spec CN-8 and §6.2 rule 6; the first live trade
//! brief's C2; DEC-875). The connector holds one founder-typed account number, and nothing about
//! any other account of the customer leaves it, though Robinhood's session reads them all (U-R12).

use serde_json::{Map, Value};

use crate::{RobinhoodConnector, RobinhoodError, Tools};

/// A read of the agentic account's data, each one tool of the allowlist (connections spec §6.2).
/// The answers' shapes are assumed (DEC-875 item 4) until the founder's run reads real ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountRead {
    /// `get_portfolio`: its `structuredContent` is the one record.
    Portfolio,
    /// `get_equity_positions`: the records of `structuredContent.positions`.
    Positions,
    /// `get_equity_orders`: the records of `structuredContent.orders`.
    Orders,
}

impl<T: Tools> RobinhoodConnector<T> {
    /// The agentic account's own record from `get_accounts` (DEC-875 item 1), on the `Ordinary`
    /// budget. Every record must carry a text `account_number` and a boolean `agentic_allowed`,
    /// or the list is [`RobinhoodError::Unreadable`]. Exactly one record may be agentic, and it
    /// must be the only one under the recorded number: more than one agentic record, or the
    /// number listed twice, is [`RobinhoodError::AmbiguousAgenticAccount`]; otherwise a recorded
    /// account that is absent or not agentic is [`RobinhoodError::NoAgenticAccount`]. Another
    /// account is never adopted in its place, and no other record is returned.
    pub async fn agentic_account(&self) -> Result<Map<String, Value>, RobinhoodError> {
        Err(RobinhoodError::Unimplemented { story: "E7-6" })
    }

    /// One read of the agentic account, on the `Ordinary` budget (DEC-875 items 2 and 3).
    /// `filters` are the tool's other arguments; an `account_number` among them that is not the
    /// recorded number as text is [`RobinhoodError::OtherAccount`], with nothing called. Then
    /// [`Self::agentic_account`] runs, and its refusal is the read's, so a non-agentic account's
    /// data is never read. The tool is called with the recorded `account_number`, and only the
    /// answer's records naming it are returned; a record naming another account is dropped, and
    /// one naming none, or an answer of another shape, is [`RobinhoodError::Unreadable`]. No
    /// order, cancel or exit waits on a read (`AGENTS.md` rule 13).
    pub async fn read(
        &self,
        read: AccountRead,
        filters: &Map<String, Value>,
    ) -> Result<Vec<Map<String, Value>>, RobinhoodError> {
        let _ = (read, filters);
        Err(RobinhoodError::Unimplemented { story: "E7-6" })
    }
}
