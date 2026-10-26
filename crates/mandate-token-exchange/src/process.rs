//! The process after its settings ([DEC-2330](../../../docs/project/decisions/DEC-2330.md) item
//! 3): exactly `run --vault-address <address> --vault-dir <dir>`, one Kubernetes-auth login as the
//! connection's exchange, then `run` through the vault adapter and the HTTP adapter.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mandate_connections::ConnectError;
use mandate_connections::exchange::{CodeWait, ExchangedGrant};
use mandate_connections::record::ConnectionId;
use mandate_time::UtcNanos;
use mandate_vault::SourceError;
use mandate_vault::mounted::{
    read_mounted_ca, read_service_account_namespace, read_service_account_token,
};
use mandate_vault::openbao::{OpenBaoConfig, OpenBaoSource};
use mandate_vault::policy::{Identity, Mounts};

use crate::http::HttpTokenEndpoint;
use crate::run::run;
use crate::startup::{Endpoint, Settings};
use crate::vault::ExchangeVault;

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
pub fn vault_args(args: impl IntoIterator<Item = OsString>) -> Option<VaultArgs> {
    let args: Vec<String> = args
        .into_iter()
        .map(OsString::into_string)
        .collect::<Result<_, _>>()
        .ok()?;
    let [run, address_flag, address, dir_flag, dir] = args.as_slice() else {
        return None;
    };
    let dir = PathBuf::from(dir);
    let admitted = run == "run"
        && address_flag == "--vault-address"
        && dir_flag == "--vault-dir"
        && address_in_form(address)
        && dir.is_absolute();
    admitted.then(|| VaultArgs {
        address: address.clone(),
        dir,
    })
}

/// Whether `address` is `https://` and an authority with nothing after it.
fn address_in_form(address: &str) -> bool {
    address.strip_prefix("https://").is_some_and(|authority| {
        !authority.is_empty()
            && authority
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':' | b'[' | b']'))
    })
}

/// The wall clock, read at the process's edge: for the vault session's expiry and the request's
/// deadline at the start, and by [`HostWait`] before each take; `None` when it cannot be read,
/// which ends the run as the vault would.
#[allow(
    clippy::disallowed_methods,
    reason = "the process's edge reads the wall clock, for the vault session's expiry and the wait's deadline; the exchange core reads none"
)]
pub fn now() -> Option<UtcNanos> {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    let secs = i64::try_from(since.as_secs()).ok()?;
    UtcNanos::from_parts(secs, since.subsec_nanos()).ok()
}

/// The wait's reading of the host clock (DEC-2363 item 3): the clock's `read` while it can be read,
/// and `deadline` when it cannot, so an unreadable clock reads as the deadline reached and the wait
/// ends `code_missing` rather than runs on.
pub fn read_or(deadline: UtcNanos, read: Option<UtcNanos>) -> UtcNanos {
    read.unwrap_or(deadline)
}

/// The wait's clock and sleep, the host's (DEC-2363 item 3), the clock read through [`read_or`].
pub struct HostWait {
    pub deadline: UtcNanos,
}

impl CodeWait for HostWait {
    fn now(&mut self) -> UtcNanos {
        read_or(self.deadline, now())
    }

    fn pause(&mut self, pause: Duration) {
        std::thread::sleep(pause);
    }
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
    pub fn code(&self) -> &'static str {
        match self {
            Self::Vault => VAULT_REFUSED,
            Self::Exchange(refused) => refused.code(),
        }
    }
}

/// Logs in as `connection`'s exchange: the CA from `args.dir` by the public-CA reader (DEC-2340),
/// the projected ServiceAccount token and namespace from `token_dir`, read at this one login.
///
/// # Errors
/// Any file's refusal, then the login's.
pub async fn log_in(
    args: &VaultArgs,
    token_dir: &Path,
    connection: &ConnectionId,
    now: UtcNanos,
) -> Result<OpenBaoSource, SourceError> {
    let ca = read_mounted_ca(&args.dir)?;
    let namespace = read_service_account_namespace(token_dir)?;
    let jwt = read_service_account_token(token_dir)?;
    let config = OpenBaoConfig {
        address: args.address.clone(),
        ca_pem: ca.into_bytes(),
        mounts: Mounts::default(),
    };
    OpenBaoSource::login_kubernetes(
        &config,
        Identity::TokenExchange(connection.clone()),
        &jwt,
        &namespace,
        now,
    )
    .await
}

/// The whole exchange for `settings`' connection: the login at `now`, then `run` through the vault
/// adapter and the HTTP adapter of `settings`' endpoint, waiting for the code until `deadline`.
///
/// # Errors
/// [`Failed::Vault`] when the login or the adapter cannot be had; [`Failed::Exchange`] with
/// `run`'s refusal.
pub fn exchange(
    settings: &Settings,
    args: &VaultArgs,
    token_dir: &Path,
    now: UtcNanos,
    deadline: UtcNanos,
) -> Result<ExchangedGrant, Failed> {
    let session = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| Failed::Vault)?
        .block_on(log_in(args, token_dir, &settings.connection, now))
        .map_err(|_| Failed::Vault)?;
    let vault =
        ExchangeVault::new(session, settings.connection.clone()).map_err(|_| Failed::Vault)?;
    let mut endpoint = match &settings.endpoint {
        Endpoint::Live => HttpTokenEndpoint::live(),
        #[cfg(debug_assertions)]
        Endpoint::Cell(url) => HttpTokenEndpoint::for_cell(url.clone()),
    }
    .map_err(Failed::Exchange)?;
    run(
        settings,
        &mut &vault,
        &mut &vault,
        &mut endpoint,
        &mut HostWait { deadline },
        deadline,
    )
    .map_err(Failed::Exchange)
}
