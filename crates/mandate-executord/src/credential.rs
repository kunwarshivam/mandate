//! The broker an executor runs over and its paper credential (DEC-2306 items 1 to 3).
//!
//! The run configuration chooses the broker: [`BrokerChoice::Fixture`], the default, or
//! [`BrokerChoice::AlpacaPaper`], Alpaca's paper trading host through `mandate-alpaca`'s
//! transport. The paper credential is the secret the vault leases to this account's connection,
//! and only that: the connection is the one the account stream's latest connect check passed for,
//! and a lease the vault refused or a secret that does not read refuses the start, before the
//! process takes the account stream (`AGENTS.md` rules 3 and 7).

use mandate_alpaca::Credentials;
use mandate_canon::{Value, parse};
use mandate_connections::record::ConnectionId;
use mandate_executor::AccountScope;
use mandate_journal::StreamId;
use mandate_store::Store;
use mandate_vault::SourceError;
use mandate_vault::lease::{Lease, LeaseKeeper};
use secrecy::ExposeSecret;

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
    match flag {
        None | Some("fixture") => Ok(BrokerChoice::Fixture),
        Some("alpaca-paper") => Ok(BrokerChoice::AlpacaPaper),
        Some(_) => Err("broker_unknown"),
    }
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
    let stream = StreamId::parse(&format!("acct:{}:{}", scope.workspace.0, scope.account.0))
        .ok_or("stream_unavailable")?;
    let rows = store.rows(&stream).ok_or("stream_unavailable")?;
    let latest = rows
        .iter()
        .rev()
        .find(|row| row.event_type == "ConnectionChecked")
        .ok_or("connection_unchecked")?;
    let payload = parse(&latest.body)
        .ok()
        .and_then(|body| body.get("payload").cloned())
        .ok_or("connection_record_unreadable")?;
    let passed = payload
        .get("results")
        .and_then(Value::as_array)
        .is_some_and(|results| {
            !results.is_empty()
                && results
                    .iter()
                    .all(|result| result.get("result").and_then(Value::as_str) == Some("passed"))
        });
    if !passed {
        return Err("connection_unchecked");
    }
    payload
        .get("connection_id")
        .and_then(Value::as_str)
        .and_then(|id| ConnectionId::new(id).ok())
        .ok_or("connection_record_unreadable")
}

/// The paper credentials `lease` holds, with the keeper that renews it.
///
/// # Errors
/// `credential_unavailable` when the vault refused the lease, and `credential_malformed` when its
/// secret is not the canonical `{key_id, secret}` object. Neither names the secret.
pub fn paper_credentials(
    lease: Result<Lease, SourceError>,
) -> Result<(Credentials, LeaseKeeper), &'static str> {
    let lease = lease.map_err(|_| "credential_unavailable")?;
    let credentials = Credentials::from_vault_secret(lease.secret.expose_secret())
        .map_err(|_| "credential_malformed")?;
    Ok((credentials, LeaseKeeper::new(lease)))
}
