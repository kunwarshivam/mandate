//! The permission checks (connections spec §8.1 checks 1, 2, 3, and 7; CN-2, CN-3, CN-10; E7-12).
//! **The connection's executor process** runs them at a connect, a reconnect, a credential
//! replacement, each executor start, and daily, and journals the [`CheckReport`] as
//! `ConnectionChecked` (journal spec §9.12). Check 4 (uniqueness) is the connection manager's
//! ([`crate::record::Registry::admit`]); checks 5 and 6 come from `AccountStateObserved`.
//!
//! Every check here is pure: it reads what the executor already learned through its own
//! environment's host and a table of what each broker documents, and sends nothing. The other
//! environment's host is therefore never probed (CN-3, DEC-441 item 21).

use std::collections::BTreeSet;

use crate::ConnectError;
use crate::grant::GrantedScopes;
use crate::record::{AccountPiiRef, AuthKind, Broker, ConnectionId, ConnectionState, Environment};

/// Whether an Alpaca OAuth grant requested with `env=paper` counts as reaching paper only
/// (check 2). The founder accepted this residual risk for paper connections in DEC-821 item 1
/// (PR #762, with #769); until DEC-821 is in force, or to revert it, this is the one line to
/// change, and every Alpaca OAuth grant is then refused as `reaches_both` (DEC-441 item 21).
pub const ALPACA_PAPER_OAUTH_REACHES_PAPER_ONLY: bool = true;

/// When the checks run (journal spec §9.12 `ConnectionChecked.occasion`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Occasion {
    Connect,
    Reconnect,
    Reauthorize,
    ExecutorStart,
    Daily,
}

/// What the credential is allowed to do, as the broker reported it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Granted {
    /// OAuth scopes, which only [`crate::grant::check_scope`] builds; [`run`] still re-checks
    /// them, so a set that was never checked cannot pass.
    OAuth(GrantedScopes),
    /// An API key's permissions, or `None` when the venue cannot report them (Alpaca keys).
    KeyPermissions(Option<BTreeSet<String>>),
    /// The MCP server's tool names.
    Tools(BTreeSet<String>),
}

/// What reading the account through the connection's own host showed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountRead {
    Unreadable,
    /// The broker account id went to the personal-data vault under `pii_ref`; `dedicated` is
    /// whether it is a dedicated agentic account, where the broker has them (Robinhood), and
    /// `None` elsewhere.
    Read {
        pii_ref: AccountPiiRef,
        dedicated: Option<bool>,
    },
}

/// What the MCP client saw of the server's contract (connections spec §6.2 rule 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractSeen {
    pub allowlisted_tools_present: bool,
    pub hash: String,
}

/// Everything the checks read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckInput {
    pub connection_id: ConnectionId,
    pub broker: Broker,
    pub environment: Environment,
    pub auth_kind: AuthKind,
    pub occasion: Occasion,
    pub granted: Granted,
    pub account: AccountRead,
    /// MCP connections only.
    pub contract: Option<ContractSeen>,
    /// The contract hash pinned in the connection's record; `None` at the first connect.
    pub pinned_contract: Option<String>,
}

/// The checks, named as journal spec §9.12 names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Check {
    Account,
    Contract,
    Environment,
    Scope,
}

/// Why a check failed (journal spec §9.12's reasons table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    ScopeMismatch,
    FundMovement,
    PermissionsUnreadable,
    WrongEnvironment,
    ReachesBoth,
    AccountUnreadable,
    NotDedicated,
    ToolsMissing,
    ContractDrift,
}

/// One check's result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Passed,
    Failed(Reason),
}

/// `ConnectionChecked`'s members (journal spec §9.12): every check run, in the record's order
/// (account, contract, environment, scope; contract for MCP only), and the account's
/// personal-data reference when it was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    pub connection_id: ConnectionId,
    pub occasion: Occasion,
    pub results: Vec<(Check, Outcome)>,
    pub account_pii_ref: Option<AccountPiiRef>,
}

/// Runs checks 1, 2, 3, and 7 (connections spec §8.1):
/// 1. scope: OAuth scopes exactly `trading` and `data`; no permission or tool that can move
///    funds out; a live key whose permissions cannot be read is refused, a paper one recorded;
/// 2. environment: from the broker's documented reach only, never a request; a credential that
///    may reach both environments is refused, except Alpaca paper OAuth while
///    [`ALPACA_PAPER_OAUTH_REACHES_PAPER_ONLY`] holds (DEC-821);
/// 3. account: readable, and for Robinhood the dedicated agentic account (`dedicated` must be
///    `Some(true)`);
/// 7. contract (MCP only): the allowlisted tools are present and the hash equals the pinned one.
pub fn run(input: &CheckInput) -> Result<CheckReport, ConnectError> {
    let _ = input;
    Err(ConnectError::Unimplemented { story: "E7-12" })
}

impl CheckReport {
    /// The first failed check in §8.1's order (1, 2, 3, 7), for `ConnectionRefused`; `None` when
    /// every check passed.
    pub fn refusal(&self) -> Result<Option<(Check, Reason)>, ConnectError> {
        Err(ConnectError::Unimplemented { story: "E7-12" })
    }

    /// Runs `store` (the vault write of a new credential) only when nothing was refused, so a
    /// refused credential is never stored.
    pub fn then_store(
        &self,
        store: impl FnOnce() -> Result<(), ConnectError>,
    ) -> Result<(), ConnectError> {
        let _ = store;
        Err(ConnectError::Unimplemented { story: "E7-12" })
    }

    /// A later run's consequence (connections spec §8.1, "failure later"; §9.1): a failed scope,
    /// environment, or account check suspends the connection, contract drift degrades it, and a
    /// connect-time occasion changes no state (its failure is a refusal).
    pub fn later_state(&self) -> Result<Option<ConnectionState>, ConnectError> {
        Err(ConnectError::Unimplemented { story: "E7-12" })
    }
}

impl Reason {
    /// The reason as journal spec §9.12 writes it.
    pub fn code(self) -> &'static str {
        match self {
            Self::ScopeMismatch => "scope_mismatch",
            Self::FundMovement => "fund_movement",
            Self::PermissionsUnreadable => "permissions_unreadable",
            Self::WrongEnvironment => "wrong_environment",
            Self::ReachesBoth => "reaches_both",
            Self::AccountUnreadable => "account_unreadable",
            Self::NotDedicated => "not_dedicated",
            Self::ToolsMissing => "tools_missing",
            Self::ContractDrift => "contract_drift",
        }
    }
}
