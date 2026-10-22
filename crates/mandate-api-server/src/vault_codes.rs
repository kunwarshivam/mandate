//! The API's vault port over the vault ([DEC-2338](../../../docs/project/decisions/DEC-2338.md)
//! item 3): it writes the callback's code as [`mandate_vault::policy::Identity::Api`], and calls nothing else. The vault's
//! policy gives that identity no read of any token (item 4).

use std::path::PathBuf;
use std::sync::Arc;

use mandate_connections::ConnectError;
use mandate_connections::start::{Binding, PkceVerifier};
use mandate_connections::vault::AuthorizationCode;
use mandate_time::UtcNanos;
use mandate_vault::SourceError;
use mandate_vault::openbao::{OpenBaoConfig, OpenBaoSource};
use mandate_vault::source::SecretSource;

use crate::clock::Clock;
use crate::connect::CodeVault;

/// How the port opens its session: a fresh login as [`mandate_vault::policy::Identity::Api`] for each write.
pub trait ApiLogin: Send + Sync {
    /// The session a login opens.
    type Session: SecretSource;

    /// Logs in at `now`.
    fn login(&self, now: UtcNanos) -> impl Future<Output = Result<Self::Session, SourceError>>;
}

/// The deployment's login: the AppRole role id and secret id read from the mounted files
/// `api.role-id` and `api.secret-id` in `dir` at each login, never from the environment or argv.
#[derive(Debug)]
pub struct OpenBaoLogin {
    pub config: OpenBaoConfig,
    pub dir: PathBuf,
}

impl ApiLogin for OpenBaoLogin {
    type Session = OpenBaoSource;

    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    async fn login(&self, _now: UtcNanos) -> Result<OpenBaoSource, SourceError> {
        todo!()
    }
}

/// The [`CodeVault`] the callback writes through.
pub struct VaultCodes<L> {
    pub login: L,
    pub clock: Arc<dyn Clock>,
}

impl<L: ApiLogin> CodeVault for VaultCodes<L> {
    /// Refuses a live binding before anything is read, encodes the entry before any login, then
    /// writes it on a current-thread runtime of its own, so the caller must be off the server's
    /// runtime (`spawn_blocking`).
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    fn put_code(
        &self,
        _binding: &Binding,
        _code: AuthorizationCode,
        _verifier: PkceVerifier,
    ) -> Result<(), ConnectError> {
        todo!()
    }
}
