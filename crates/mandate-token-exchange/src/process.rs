//! The process after its settings ([DEC-2330](../../../docs/project/decisions/DEC-2330.md) item
//! 3): exactly `run --vault-address <address> --vault-dir <dir>`, one Kubernetes-auth login as the
//! connection's exchange, then `run` through the vault adapter and the HTTP adapter.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use mandate_connections::ConnectError;
use mandate_connections::exchange::ExchangedGrant;
use mandate_connections::record::ConnectionId;
use mandate_time::UtcNanos;
use mandate_vault::SourceError;
use mandate_vault::openbao::OpenBaoSource;

use crate::startup::Settings;

/// The code written, alone, for arguments other than exactly the pinned ones.
pub const ARGUMENTS_REFUSED: &str = "token_exchange_arguments";

/// The code written, alone, when the vault cannot be logged in to.
pub const VAULT_REFUSED: &str = "token_exchange_vault";

/// Where the vault is and how to trust it, from the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultArgs {
    /// `https://host:port`, nothing after it.
    pub address: String,
    /// The directory holding `ca.crt`.
    pub dir: PathBuf,
}

/// The arguments after the program's name, admitted only as exactly
/// `run --vault-address <address> --vault-dir <dir>`, with an `https://` address naming only an
/// authority and an absolute directory.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn vault_args(_args: impl IntoIterator<Item = OsString>) -> Option<VaultArgs> {
    todo!()
}

/// The wall clock, read once at the process's edge for the vault session's expiry; `None` when
/// it cannot be read, which ends the run as the vault would.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn now() -> Option<UtcNanos> {
    todo!()
}

/// Why the exchange did not complete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failed {
    /// The vault could not be logged in to as the connection's exchange.
    Vault,
    /// The exchange itself refused or failed.
    Exchange(ConnectError),
}

impl Failed {
    /// The code the process writes, alone, before it exits.
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn code(&self) -> &'static str {
        todo!()
    }
}

/// Logs in as `connection`'s exchange: the CA from `args.dir` by the public-CA reader (DEC-2340),
/// the projected ServiceAccount token and namespace from `token_dir`, read at this one login.
///
/// # Errors
/// Any file's refusal, then the login's.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub async fn log_in(
    _args: &VaultArgs,
    _token_dir: &Path,
    _connection: &ConnectionId,
    _now: UtcNanos,
) -> Result<OpenBaoSource, SourceError> {
    todo!()
}

/// The whole exchange for `settings`' connection: the login at `now`, then `run` through the vault
/// adapter and the HTTP adapter of `settings`' endpoint.
///
/// # Errors
/// [`Failed::Vault`] when the login or the adapter cannot be had; [`Failed::Exchange`] with
/// `run`'s refusal.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn exchange(
    _settings: &Settings,
    _args: &VaultArgs,
    _token_dir: &Path,
    _now: UtcNanos,
) -> Result<ExchangedGrant, Failed> {
    todo!()
}
