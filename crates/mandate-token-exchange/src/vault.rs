//! The exchange's vault adapter ([DEC-2330](../../../docs/project/decisions/DEC-2330.md) item 2):
//! one session as the connection's exchange, serving `run` the client secret, the single-use take
//! of the code entry (DEC-2338 item 2) and the token entry's write (DEC-2624), and nothing else.

use mandate_connections::ConnectError;
use mandate_connections::exchange::ClientSecret;
use mandate_connections::grant::GrantedScopes;
use mandate_connections::record::ConnectionId;
use mandate_connections::start::PkceVerifier;
use mandate_connections::vault::{AccessToken, AuthorizationCode, Vault, VaultKey, WaitingCode};
use mandate_vault::source::SecretSource;

use crate::run::ClientSecretSource;

/// The longest client secret read, in bytes.
pub const MAX_CLIENT_SECRET: usize = 1024;

/// The vault, as `connection`'s exchange. Both of `run`'s ports are implemented on a shared
/// reference, so the one adapter serves as the client secret's source and as the vault.
#[expect(
    dead_code,
    reason = "the tests land before the implementation (DEC-77); the stubs read no field yet"
)]
pub struct ExchangeVault<S> {
    session: S,
    connection: ConnectionId,
    runtime: tokio::runtime::Runtime,
}

impl<S: SecretSource> ExchangeVault<S> {
    /// The adapter over `session`, which must be `connection`'s exchange.
    ///
    /// # Errors
    /// [`ConnectError::VaultUnavailable`] for any other session, or when the runtime cannot start.
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn new(_session: S, _connection: ConnectionId) -> Result<Self, ConnectError> {
        todo!()
    }
}

impl<S: SecretSource> Vault for &ExchangeVault<S> {
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    fn put_code(
        &mut self,
        _key: &VaultKey,
        _code: AuthorizationCode,
        _verifier: PkceVerifier,
    ) -> Result<(), ConnectError> {
        todo!()
    }

    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    fn take_code(&mut self, _key: &VaultKey) -> Result<Option<WaitingCode>, ConnectError> {
        todo!()
    }

    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    fn put_token(
        &mut self,
        _key: &VaultKey,
        _token: AccessToken,
        _scopes: &GrantedScopes,
    ) -> Result<(), ConnectError> {
        todo!()
    }

    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    fn delete_code(&mut self, _key: &VaultKey) -> Result<(), ConnectError> {
        todo!()
    }

    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    fn delete(&mut self, _key: &VaultKey) -> Result<(), ConnectError> {
        todo!()
    }
}

impl<S: SecretSource> ClientSecretSource for &ExchangeVault<S> {
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    fn client_secret(&mut self) -> Result<ClientSecret, ConnectError> {
        todo!()
    }
}
