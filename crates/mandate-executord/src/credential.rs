//! The broker an executor runs over and its paper credential (DEC-2306 items 1 to 3).
//!
//! The run configuration chooses the broker: [`BrokerChoice::Fixture`], the default, or
//! [`BrokerChoice::AlpacaPaper`], Alpaca's paper trading host through `mandate-alpaca`'s
//! transport. The paper credential is the OAuth access token the vault leases to this account's
//! connection, in lane X's token entry (DEC-2624), and only that: the connection is the one the
//! account stream's latest connect check passed for, and a lease the vault refused or an entry
//! that does not decode refuses the start, before the process takes the account stream
//! (`AGENTS.md` rules 3 and 7). The token reaches the paper host only as a bearer header.

use mandate_alpaca::{AlpacaPaperHttp, Credentials};
use mandate_connections::record::ConnectionId;
use mandate_connections::vault::AccessToken;
use mandate_executor::AccountScope;
use mandate_store::Store;
use mandate_vault::SourceError;
use mandate_vault::lease::{Lease, LeaseKeeper};

/// The code every stub of this story's tests PR returns, so every pending test fails at it
/// (DEC-77); the implementation PR replaces the stubs and removes it.
#[allow(
    non_upper_case_globals,
    reason = "the pending gate recognises a stub by the name `Unimplemented` (DEC-137)"
)]
pub const Unimplemented: &str = "unimplemented";

/// Which broker the process runs over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokerChoice {
    /// The in-process fixture paper broker (DEC-2007 item 1).
    Fixture,
    /// Alpaca's paper trading host (DEC-2306 item 1).
    AlpacaPaper,
}

/// The broker `flag`, the value of `--broker`, names: none is the fixture, and anything but
/// `fixture` or `alpaca-paper` is `broker_unknown`.
///
/// # Errors
/// `broker_unknown`.
pub fn broker_choice(flag: Option<&str>) -> Result<BrokerChoice, &'static str> {
    let _ = flag;
    Err(Unimplemented)
}

/// The connection the account stream's latest connect check passed for: the
/// `ConnectionChecked` record last appended there, every check of which passed.
///
/// # Errors
/// `connection_unchecked` when there is none, or the latest failed; a journal error's code when
/// the stream cannot be read.
pub fn checked_connection<S: Store>(
    store: &S,
    scope: &AccountScope,
) -> Result<ConnectionId, &'static str> {
    let _ = (store, scope);
    Err(Unimplemented)
}

/// The access token a leased secret holds: DEC-2624's token entry, decoded by its one reader.
///
/// # Errors
/// `credential_malformed` for any other bytes; it names nothing of them.
pub fn read_token(secret: &[u8]) -> Result<AccessToken, &'static str> {
    let _ = secret;
    Err(Unimplemented)
}

/// The access token `lease` holds, with the keeper that renews it.
///
/// # Errors
/// `credential_unavailable` when the vault refused the lease, and [`read_token`]'s refusal when
/// its secret is not a token entry. Neither names the secret.
pub fn paper_token(
    lease: Result<Lease, SourceError>,
) -> Result<(AccessToken, LeaseKeeper), &'static str> {
    let _ = lease;
    Err(Unimplemented)
}

/// The transport to Alpaca's paper trading host that sends `token` as its bearer credential.
///
/// # Errors
/// `transport_unavailable` when the HTTPS client cannot be built.
pub fn paper_transport(token: AccessToken) -> Result<AlpacaPaperHttp, &'static str> {
    let _ = (token, Credentials::bearer);
    Err(Unimplemented)
}
