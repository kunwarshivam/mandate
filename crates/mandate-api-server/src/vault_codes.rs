//! The API's vault port over the vault ([DEC-2338](../../../docs/project/decisions/DEC-2338.md)
//! item 3): it writes the callback's code as [`Identity::Api`], and calls nothing else. The vault's
//! policy gives that identity no read of any token (item 4).

use std::path::PathBuf;
use std::sync::Arc;

use mandate_connections::ConnectError;
use mandate_connections::code_entry::encode;
use mandate_connections::start::{Binding, Environment, PkceVerifier};
use mandate_connections::vault::AuthorizationCode;
use mandate_time::UtcNanos;
use mandate_vault::SourceError;
use mandate_vault::mounted::read_mounted_credentials;
use mandate_vault::openbao::{OpenBaoConfig, OpenBaoSource};
use mandate_vault::policy::{Identity, SecretPath};
use mandate_vault::source::SecretSource;
use secrecy::{ExposeSecret, SecretSlice};

use crate::clock::Clock;
use crate::connect::CodeVault;

/// How the port opens its session: a fresh login as [`Identity::Api`] for each write.
pub trait ApiLogin: Send + Sync {
    /// The session a login opens.
    type Session: SecretSource;

    /// Logs in at `now`.
    fn login(&self, now: UtcNanos) -> impl Future<Output = Result<Self::Session, SourceError>>;
}

/// The deployment's login: the AppRole role id and secret id read from the mounted files
/// `api.role-id` and `api.secret-id` in `dir` at each login, never from the environment or argv.
#[derive(Debug, Clone)]
pub struct OpenBaoLogin {
    pub config: OpenBaoConfig,
    pub dir: PathBuf,
}

impl ApiLogin for OpenBaoLogin {
    type Session = OpenBaoSource;

    async fn login(&self, now: UtcNanos) -> Result<OpenBaoSource, SourceError> {
        let (role_id, secret_id) = read_mounted_credentials(&self.dir, &Identity::Api)?;
        OpenBaoSource::login(&self.config, Identity::Api, &role_id, &secret_id, now).await
    }
}

/// The [`CodeVault`] the callback writes through.
pub struct VaultCodes<L> {
    pub login: L,
    pub clock: Arc<dyn Clock>,
}

impl<L: ApiLogin> VaultCodes<L> {
    async fn write(&self, binding: &Binding, entry: &SecretSlice<u8>) -> Result<(), SourceError> {
        let now = self.clock.now().ok_or(SourceError::Unavailable)?;
        let session = self.login.login(now).await?;
        if session.identity() != &Identity::Api {
            return Err(SourceError::Denied);
        }
        session
            .put(&SecretPath::Code(binding.connection_id.clone()), entry)
            .await
    }
}

impl<L: ApiLogin> CodeVault for VaultCodes<L> {
    /// Refuses a live binding before anything is read, encodes the entry before any login, then
    /// writes it on a current-thread runtime of its own, so the caller must be off the server's
    /// runtime (`spawn_blocking`).
    fn put_code(
        &self,
        binding: &Binding,
        code: AuthorizationCode,
        verifier: PkceVerifier,
    ) -> Result<(), ConnectError> {
        if binding.environment != Environment::Paper {
            return Err(ConnectError::EnvironmentRefused);
        }
        let entry = encode(&code, &verifier)?;
        let entry = SecretSlice::from(entry.expose_secret().as_bytes().to_vec());
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| ConnectError::VaultUnavailable)?
            .block_on(self.write(binding, &entry))
            .map_err(|_| ConnectError::VaultUnavailable)
    }
}
