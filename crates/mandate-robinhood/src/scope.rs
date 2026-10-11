//! Reads of the agentic account only (connections spec CN-8 and §6.2 rule 6; the first live trade
//! brief's C2; DEC-875). The connector holds one founder-typed account number, and nothing about
//! any other account of the customer leaves it, though Robinhood's session reads them all (U-R12).

use mandate_mcp::CallClass;
use serde_json::{Map, Value};

use crate::connector::{ToolResult, tool_result};
use crate::{RobinhoodConnector, RobinhoodError, Tools};

const ACCOUNTS: &str = "get_accounts";
const NUMBER: &str = "account_number";

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
    /// `get_equity_tradability`: its `structuredContent` is the one record. Its tool lists the
    /// `symbol` filter (DEC-902 item 6, DEC-879 item 5).
    Tradability,
}

impl<T: Tools> RobinhoodConnector<T> {
    /// The agentic account's own record from `get_accounts` (DEC-875 item 1), on the `Ordinary`
    /// budget. Every record must carry a text `account_number` and a boolean `agentic_allowed`,
    /// or the list is [`RobinhoodError::Unreadable`]. Exactly one record may be agentic, and it
    /// must be the only one under the recorded number: more than one agentic record, or the
    /// number listed twice, is [`RobinhoodError::AmbiguousAgenticAccount`]; otherwise a recorded
    /// account that is absent or not agentic is [`RobinhoodError::NoAgenticAccount`]. Numbers
    /// match byte for byte, with no normalization. Another account, a near miss of the recorded
    /// number included, is never adopted in its place, and no other record is returned.
    pub async fn agentic_account(&self) -> Result<Map<String, Value>, RobinhoodError> {
        let unreadable = RobinhoodError::Unreadable { code: "accounts" };
        let arguments = Value::Object(Map::new());
        let answer = self
            .tools
            .call_tool(CallClass::Ordinary, ACCOUNTS, &arguments)
            .await;
        let mut content = structured(answer).ok_or(unreadable.clone())?;
        let Some(Value::Array(list)) = content.remove("accounts") else {
            return Err(unreadable);
        };
        let mut records = Vec::with_capacity(list.len());
        for record in list {
            let Value::Object(record) = record else {
                return Err(unreadable);
            };
            let number = record.get(NUMBER).and_then(Value::as_str);
            let allowed = record.get("agentic_allowed").and_then(Value::as_bool);
            let (Some(number), Some(allowed)) = (number, allowed) else {
                return Err(unreadable);
            };
            let ours = number == self.account_number;
            records.push((ours, allowed, record));
        }
        let agentic = records.iter().filter(|(_, allowed, _)| *allowed).count();
        let listed = records.iter().filter(|(ours, _, _)| *ours).count();
        if agentic > 1 || listed > 1 {
            return Err(RobinhoodError::AmbiguousAgenticAccount);
        }
        records
            .into_iter()
            .find(|(ours, allowed, _)| *ours && *allowed)
            .map(|(_, _, record)| record)
            .ok_or(RobinhoodError::NoAgenticAccount)
    }

    /// One read of the agentic account, on the `Ordinary` budget (DEC-875 items 2 and 3).
    /// `filters` are the tool's other arguments; an `account_number` among them that is not the
    /// recorded number as text is [`RobinhoodError::OtherAccount`], with nothing called. Any other
    /// key the tool does not list is [`RobinhoodError::UnlistedFilter`], with nothing called
    /// (DEC-879); a listed key's value is forwarded as given. Then
    /// [`Self::agentic_account`] runs, and its refusal is the read's, so a non-agentic account's
    /// data is never read. The tool is called with the recorded `account_number`, and only the
    /// answer's records naming it byte for byte are returned. A record naming another account is
    /// dropped, but one naming a near miss of the recorded number (equal to it, or one a prefix of
    /// the other, once both are trimmed and ASCII lower-cased), one naming none, or an answer of
    /// another shape is [`RobinhoodError::Unreadable`]. No
    /// order, cancel or exit waits on a read (`AGENTS.md` rule 13).
    pub async fn read(
        &self,
        read: AccountRead,
        filters: &Map<String, Value>,
    ) -> Result<Vec<Map<String, Value>>, RobinhoodError> {
        self.scoped_read(read, filters, false).await
    }

    /// One read of the agentic account whose answer holds its records' member and nothing else
    /// (DEC-902 item 5): beside [`Self::read`], a next-page cursor, or any other member of the
    /// `structuredContent`, is [`RobinhoodError::Unreadable`] with the read's code, because
    /// reading only the first page could hide an open order of the agentic account. The
    /// preflight reads this way; following pages is C3's list request, so the check lives here
    /// with the preflight callers and not inside [`Self::read`] (#1264's minor 5). A one-record
    /// answer has no member to check, so its closed shape is the caller's.
    pub async fn read_sole_member(
        &self,
        read: AccountRead,
        filters: &Map<String, Value>,
    ) -> Result<Vec<Map<String, Value>>, RobinhoodError> {
        self.scoped_read(read, filters, true).await
    }

    /// [`Self::read`] and [`Self::read_sole_member`] differ only in `sole_member`.
    async fn scoped_read(
        &self,
        read: AccountRead,
        filters: &Map<String, Value>,
        sole_member: bool,
    ) -> Result<Vec<Map<String, Value>>, RobinhoodError> {
        let recorded = self.account_number.as_str();
        match filters.get(NUMBER) {
            None => {}
            Some(Value::String(named)) if named == recorded => {}
            Some(_) => return Err(RobinhoodError::OtherAccount),
        }
        let listed = read.listed_filters();
        let unlisted = filters
            .keys()
            .any(|key| key != NUMBER && !listed.contains(&key.as_str()));
        if unlisted {
            return Err(RobinhoodError::UnlistedFilter);
        }
        self.agentic_account().await?;
        let (tool, member, code) = read.wire();
        let unreadable = RobinhoodError::Unreadable { code };
        let mut arguments = filters.clone();
        arguments.insert(NUMBER.to_owned(), Value::String(recorded.to_owned()));
        let answer = self
            .tools
            .call_tool(CallClass::Ordinary, tool, &Value::Object(arguments))
            .await;
        let mut content = structured(answer).ok_or(unreadable.clone())?;
        if sole_member && member.is_some() && content.len() != 1 {
            return Err(unreadable);
        }
        let records = match member {
            None => vec![Value::Object(content)],
            Some(member) => match content.remove(member) {
                Some(Value::Array(records)) => records,
                _ => return Err(unreadable),
            },
        };
        let mut kept = Vec::new();
        for record in records {
            let Value::Object(record) = record else {
                return Err(unreadable);
            };
            match record.get(NUMBER) {
                Some(Value::String(named)) if named == recorded => kept.push(record),
                Some(Value::String(named)) if near_miss(named, recorded) => {
                    return Err(unreadable);
                }
                Some(Value::String(_)) => {}
                _ => return Err(unreadable),
            }
        }
        Ok(kept)
    }
}

impl AccountRead {
    /// The filter keys the read's tool lists besides `account_number` (DEC-879 item 1): the
    /// contract's `get_equity_orders` parameters, and none for the two tools whose parameters the
    /// contract does not name, the narrowest set.
    fn listed_filters(self) -> &'static [&'static str] {
        match self {
            Self::Portfolio | Self::Positions => &[],
            Self::Orders => &[
                "order_id",
                "state",
                "symbol",
                "created_at_gte",
                "placed_agent",
                "cursor",
            ],
            Self::Tradability => &["symbol"],
        }
    }

    /// The tool, the member of its `structuredContent` that holds the records (`None` when the
    /// content is the one record), and the code a refusal of it carries.
    fn wire(self) -> (&'static str, Option<&'static str>, &'static str) {
        match self {
            Self::Portfolio => ("get_portfolio", None, "portfolio"),
            Self::Positions => ("get_equity_positions", Some("positions"), "positions"),
            Self::Orders => ("get_equity_orders", Some("orders"), "orders"),
            Self::Tradability => ("get_equity_tradability", None, "tradability"),
        }
    }
}

/// The `structuredContent` of an answer that arrived and is not `isError`; `None` otherwise.
pub(crate) fn structured<E>(answer: Result<String, E>) -> Option<Map<String, Value>> {
    match tool_result(&answer.ok()?)? {
        ToolResult::Content(content) => Some(content),
        ToolResult::Refused => None,
    }
}

/// Whether `named`, already known not to be byte-equal to `recorded`, is a near miss of it
/// (DEC-875 item 3): once both are trimmed and ASCII lower-cased, it equals `recorded` or is a
/// prefix of it, or has it as a prefix. Equal text is a prefix of itself, so the first test
/// covers both "equals" and "is a prefix of it".
fn near_miss(named: &str, recorded: &str) -> bool {
    let named = named.trim().to_ascii_lowercase();
    let recorded = recorded.trim().to_ascii_lowercase();
    recorded.starts_with(&named) || named.starts_with(&recorded)
}
