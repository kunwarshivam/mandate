//! The permission checks (connections spec §8.1 checks 1, 2, 3, and 7; CN-2, CN-3, CN-10; E7-12).
//! **The connection's executor process** runs them at a connect, a reconnect, a credential
//! replacement, each executor start, and daily, and journals the [`CheckReport`] as
//! `ConnectionChecked` (journal spec §9.8). Check 4 (uniqueness) is the connection manager's
//! ([`crate::record::Registry::admit`]); checks 5 and 6 come from `AccountStateObserved`.
//!
//! Every check here is pure: it reads what the executor already learned through its own
//! environment's host and a table of what each broker documents, and sends nothing. The other
//! environment's host is therefore never probed (CN-3, DEC-441 item 21).

use std::collections::BTreeSet;

use mandate_domain::fund_movement::is_fund_movement_name;

use crate::ConnectError;
use crate::grant::{GrantedScopes, REQUESTED_SCOPES_SORTED};
use crate::record::{AccountPiiRef, AuthKind, Broker, ConnectionId, ConnectionState, Environment};

/// Whether an Alpaca OAuth grant requested with `env=paper` counts as reaching paper only
/// (check 2). The founder accepted this residual risk for paper connections in DEC-821 item 1;
/// to revert it, this is the one line to change. Flipping it to `false` refuses every Alpaca
/// OAuth grant as `reaches_both`, paper ones included (DEC-441 item 21), and can never admit a
/// live one: a live Alpaca OAuth grant is refused either way.
pub const ALPACA_PAPER_OAUTH_REACHES_PAPER_ONLY: bool = true;

/// When the checks run (journal spec §9.8 `ConnectionChecked.occasion`).
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

/// The checks, named as journal spec §9.8 names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Check {
    Account,
    Contract,
    Environment,
    Scope,
}

/// Why a check failed (journal spec §9.8's reasons table).
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

/// `ConnectionChecked`'s members (journal spec §9.8): every check run, in the record's order
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
/// 1. scope: the grant is the credential's kind; OAuth scopes exactly `trading` and `data`; no
///    permission, scope, or tool that can move funds out (DEC-676); a live key whose permissions
///    cannot be read or are empty is refused, a paper one recorded (DEC-685);
/// 2. environment: from the broker's documented reach only, never a request; a credential that
///    may reach both environments is refused, except Alpaca paper OAuth while
///    [`ALPACA_PAPER_OAUTH_REACHES_PAPER_ONLY`] holds (DEC-821);
/// 3. account: readable, and for Robinhood the dedicated agentic account (`dedicated` must be
///    `Some(true)`);
/// 7. contract (MCP only): the allowlisted tools are present and the hash equals the pinned one,
///    which only the first connect may lack.
pub fn run(input: &CheckInput) -> Result<CheckReport, ConnectError> {
    let (account, account_pii_ref) = account_check(input);
    let mut results = vec![(Check::Account, account)];
    if input.broker == Broker::Robinhood {
        results.push((Check::Contract, contract_check(input)));
    }
    results.push((Check::Environment, environment_check(input)));
    results.push((Check::Scope, scope_check(input)));
    Ok(CheckReport {
        connection_id: input.connection_id.clone(),
        occasion: input.occasion,
        results,
        account_pii_ref,
    })
}

/// Whether any name can move funds out (CN-2), by the one rule the MCP client also applies
/// (DEC-839 item 3; DEC-676 items 1 and 4).
fn any_moves_funds(names: &BTreeSet<String>) -> bool {
    names.iter().any(|name| is_fund_movement_name(name))
}

/// Check 1. A grant of another kind than the credential is refused as a mismatch before any
/// rule reads it (DEC-676 item 3). A fund-movement name is reported as one before any other
/// mismatch (CN-2). A live key whose permissions cannot be read, or were read and are empty, is
/// refused, since it cannot show fund movement absent (DEC-685); a paper or demo key is recorded
/// and disclosed (DEC-441 item 4). The MCP allowlist is check 7's.
fn scope_check(input: &CheckInput) -> Outcome {
    match (input.auth_kind, &input.granted) {
        (AuthKind::Oauth, Granted::OAuth(GrantedScopes(scopes))) => {
            if any_moves_funds(scopes) {
                Outcome::Failed(Reason::FundMovement)
            } else if !scopes
                .iter()
                .map(String::as_str)
                .eq(REQUESTED_SCOPES_SORTED)
            {
                Outcome::Failed(Reason::ScopeMismatch)
            } else {
                Outcome::Passed
            }
        }
        (AuthKind::McpOauth, Granted::Tools(tools)) => {
            if any_moves_funds(tools) {
                Outcome::Failed(Reason::FundMovement)
            } else {
                Outcome::Passed
            }
        }
        (AuthKind::ApiKey, Granted::KeyPermissions(Some(permissions)))
            if any_moves_funds(permissions) =>
        {
            Outcome::Failed(Reason::FundMovement)
        }
        (AuthKind::ApiKey, Granted::KeyPermissions(permissions)) => {
            let shown = permissions.as_ref().is_some_and(|names| !names.is_empty());
            match input.environment {
                Environment::Live if !shown => Outcome::Failed(Reason::PermissionsUnreadable),
                Environment::Live | Environment::Paper => Outcome::Passed,
            }
        }
        _ => Outcome::Failed(Reason::ScopeMismatch),
    }
}

/// Check 2, from what each broker documents and never from a request (CN-3): a key is issued
/// for one environment's host; Robinhood has only live (DEC-124); an Alpaca OAuth grant reaches
/// paper only when requested with `env=paper` while DEC-821 holds. Any other pairing is not
/// documented to stay in its environment, so it may reach both (DEC-441 item 21).
fn environment_check(input: &CheckInput) -> Outcome {
    match (input.broker, input.auth_kind) {
        (Broker::Alpaca | Broker::KrakenDerivativesUs, AuthKind::ApiKey) => Outcome::Passed,
        (Broker::Alpaca, AuthKind::Oauth) => {
            if input.environment == Environment::Paper && ALPACA_PAPER_OAUTH_REACHES_PAPER_ONLY {
                Outcome::Passed
            } else {
                Outcome::Failed(Reason::ReachesBoth)
            }
        }
        (Broker::Robinhood, AuthKind::McpOauth) => match input.environment {
            Environment::Live => Outcome::Passed,
            Environment::Paper => Outcome::Failed(Reason::WrongEnvironment),
        },
        _ => Outcome::Failed(Reason::ReachesBoth),
    }
}

/// Check 3, and the reference journal rule 62 records: present exactly when the account was read.
/// The fingerprint comparison is the control services' (rule 58), never the executor's.
fn account_check(input: &CheckInput) -> (Outcome, Option<AccountPiiRef>) {
    match &input.account {
        AccountRead::Unreadable => (Outcome::Failed(Reason::AccountUnreadable), None),
        AccountRead::Read { pii_ref, dedicated } => {
            let outcome = if input.broker == Broker::Robinhood && *dedicated != Some(true) {
                Outcome::Failed(Reason::NotDedicated)
            } else {
                Outcome::Passed
            };
            (outcome, Some(pii_ref.clone()))
        }
    }
}

/// Check 7 (connections spec §6.2 rule 3): the allowlisted tools must be present, which is
/// reported over drift, and the hash must equal the pinned one. Only the first connect has no pin
/// and pins the hash it sees; at any other occasion a missing pin fails closed as drift (DEC-676
/// item 2).
fn contract_check(input: &CheckInput) -> Outcome {
    match (&input.contract, &input.pinned_contract) {
        (Some(seen), _) if !seen.allowlisted_tools_present => Outcome::Failed(Reason::ToolsMissing),
        (None, _) => Outcome::Failed(Reason::ToolsMissing),
        (Some(seen), Some(pinned)) if *pinned == seen.hash => Outcome::Passed,
        (Some(_), None) if input.occasion == Occasion::Connect => Outcome::Passed,
        (Some(_), _) => Outcome::Failed(Reason::ContractDrift),
    }
}

/// §8.1's order: checks 1, 2, 3, and 7.
const SECTION_8_1_ORDER: [Check; 4] = [
    Check::Scope,
    Check::Environment,
    Check::Account,
    Check::Contract,
];

impl CheckReport {
    /// The first failed check in §8.1's order (1, 2, 3, 7), for `ConnectionRefused`; `None` when
    /// every check passed. It reads the report only, so it cannot fail.
    pub fn refusal(&self) -> Option<(Check, Reason)> {
        SECTION_8_1_ORDER
            .iter()
            .find_map(|check| self.failed(*check).map(|reason| (*check, reason)))
    }

    /// Runs `store` (the vault write of a new credential) only when nothing was refused, so a
    /// refused credential is never stored (`CheckRefused`); `store`'s own error is returned as
    /// it is.
    pub fn then_store(
        &self,
        store: impl FnOnce() -> Result<(), ConnectError>,
    ) -> Result<(), ConnectError> {
        if self.refusal().is_some() {
            return Err(ConnectError::CheckRefused);
        }
        store()
    }

    /// A later run's consequence (connections spec §8.1, "failure later"; §9.1; journal spec §9.8
    /// rule 60): a failed scope, environment, or account check suspends the connection, whatever
    /// else failed with it, and so does a missing allowlisted tool, which §8.1 check 1 names
    /// (DEC-674); contract drift alone degrades it (§8.2); and a connect-time occasion changes no
    /// state, since its failure is a refusal.
    pub fn later_state(&self) -> Option<ConnectionState> {
        if !matches!(self.occasion, Occasion::ExecutorStart | Occasion::Daily) {
            return None;
        }
        let suspending = [Check::Scope, Check::Environment, Check::Account];
        if suspending.iter().any(|check| self.failed(*check).is_some())
            || self.failed(Check::Contract) == Some(Reason::ToolsMissing)
        {
            return Some(ConnectionState::Suspended);
        }
        self.failed(Check::Contract)
            .map(|_| ConnectionState::Degraded)
    }

    fn failed(&self, check: Check) -> Option<Reason> {
        self.results.iter().find_map(|(c, outcome)| match outcome {
            Outcome::Failed(reason) if *c == check => Some(*reason),
            _ => None,
        })
    }
}

impl Check {
    /// The check as journal spec §9.8 writes it.
    pub fn code(self) -> &'static str {
        match self {
            Self::Account => "account",
            Self::Contract => "contract",
            Self::Environment => "environment",
            Self::Scope => "scope",
        }
    }
}

impl Reason {
    /// The reason as journal spec §9.8 writes it.
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
