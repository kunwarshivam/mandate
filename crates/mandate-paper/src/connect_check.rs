//! The connect check (R0 blocker R1, [DEC-946](../../../docs/project/decisions/DEC-946.md);
//! connections spec §8.1 checks 1, 2, 3 and 7; journal spec §9.8): the connect sequence's
//! executor part for a Robinhood connection, which `mandate connection record` then binds
//! (`--checked`). The runner is the one binary that may dial the broker (DEC-529 item 3), and the
//! CLI, the control services, never calls one (connections spec §5.2), so the check is the
//! runner's `--connect-check` mode.
//!
//! In order: the arguments and the process environment, the `--confirm-live` acknowledgement, the
//! founder-typed account number ([`ACCOUNT_NUMBER_VAR`]), core dumps off, then the login and the
//! reads ([`CheckPorts::observe`]); then [`mandate_connections::checks::run`] over what was read,
//! and `StreamOpened` (a new stream only) and `ConnectionChecked` (occasion `connect`) appended on
//! `acct:{workspace}:{account_ref}` in one batch. A failed check is journaled too, and the mode
//! still refuses ([`PaperError::ConnectCheck`]). The account number is never printed, journaled
//! or kept: the record holds an opaque `account_pii_ref` only.

use mandate_domain::Environment;
use mandate_journal::{AppendOutcome, StreamId};
use mandate_mcp::Listed;
use mandate_robinhood::{RobinhoodConnector, Tools};
use mandate_time::UtcNanos;

use crate::PaperError;

/// The variable the founder types the agentic account's number into (DEC-875 item 1: the
/// number comes from the user, never from `get_accounts`). Read once; never printed or stored.
pub const ACCOUNT_NUMBER_VAR: &str = "MANDATE_RH_ACCOUNT_NUMBER";

/// What `--connect-check` was asked for: opaque ids only (TI-8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckArgs {
    pub workspace: String,
    pub account_ref: String,
    pub connection: String,
    /// The journal's DSN, required: the check is nothing until it is journaled.
    pub journal: String,
    pub acknowledged: Environment,
}

/// The contract the first connect listed: its names and whether every allowlisted tool was
/// among them ([`Listed`]), and its hash as lower-case hex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeenContract {
    pub listed: Listed,
    pub hash: String,
}

/// What the connect read through the broker's own host: the contract, `None` when no session
/// was made or its listing could not be read, and whether `get_accounts` showed the recorded
/// account as the one agentic account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observed {
    pub contract: Option<SeenContract>,
    pub agentic_account_read: bool,
}

/// A passing check: the `ConnectionChecked` event id `connection record --checked` names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub event_id: String,
}

/// The connect check's effects, the only things the binary supplies.
pub trait CheckPorts {
    /// Turns the process's core dumps off (LT-9), before any credential is read.
    fn harden(&mut self) -> Result<(), PaperError>;

    /// The founder's login at the build's endpoint, then the first connect and the account read
    /// over its session. `Err` only when no login was made, and then nothing is journaled.
    fn observe(&mut self, account_number: &str) -> Result<Observed, PaperError>;

    /// The clock the check is recorded at.
    fn now(&self) -> UtcNanos;

    /// Takes the account stream's ownership: its new writer epoch and its head.
    fn own(&mut self, stream: &StreamId) -> Result<(u64, u64), PaperError>;

    /// Appends `drafts` at `head` under `epoch`, in one batch.
    fn append(
        &mut self,
        stream: &StreamId,
        head: u64,
        epoch: u64,
        at: UtcNanos,
        drafts: &[Vec<u8>],
    ) -> Result<AppendOutcome, PaperError>;
}

/// Parses the arguments after `--connect-check`.
///
/// # Errors
/// [`PaperError::Usage`].
pub fn parse_check<I: IntoIterator<Item = String>>(args: I) -> Result<CheckArgs, PaperError> {
    let _ = args.into_iter();
    Err(PaperError::Unimplemented { story: "R1" })
}

/// The connect check, in the module documentation's order.
///
/// # Errors
/// Every [`PaperError`] is a stop; [`PaperError::ConnectCheck`] after a failed check was
/// journaled.
pub fn connect_check<P: CheckPorts>(
    args: &CheckArgs,
    vars: &[(String, String)],
    ports: &mut P,
) -> Result<Checked, PaperError> {
    let _ = (args, vars, ports);
    Err(PaperError::Unimplemented { story: "R1" })
}

/// What the connect read: with a contract, the recorded account through `connector`'s own
/// account check (DEC-875 item 1); with none, nothing is called.
pub async fn observe<T: Tools>(
    contract: Option<SeenContract>,
    connector: &RobinhoodConnector<T>,
) -> Observed {
    let _ = connector;
    Observed {
        contract,
        agentic_account_read: false,
    }
}
