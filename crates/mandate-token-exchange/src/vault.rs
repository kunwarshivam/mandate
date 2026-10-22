//! The exchange's vault adapter ([DEC-2330](../../../docs/project/decisions/DEC-2330.md) item 2):
//! one session as the connection's exchange, serving `run` the client secret, the single-use take
//! of the code entry (DEC-2338 item 2) and the token entry's write (DEC-2624), and nothing else.

use mandate_connections::ConnectError;
use mandate_connections::exchange::ClientSecret;
use mandate_connections::grant::GrantedScopes;
use mandate_connections::record::ConnectionId;
use mandate_connections::start::PkceVerifier;
use mandate_connections::vault::{AccessToken, AuthorizationCode, Vault, VaultKey, WaitingCode};
use mandate_connections::{code_entry, token_entry};
use mandate_vault::policy::{Identity, SecretPath};
use mandate_vault::source::SecretSource;
use secrecy::{ExposeSecret, SecretSlice, SecretString};

use crate::run::ClientSecretSource;

/// Every refusal the adapter makes of its own: no detail reaches anyone.
const REFUSED: ConnectError = ConnectError::VaultUnavailable;

/// The longest client secret read, in bytes.
pub const MAX_CLIENT_SECRET: usize = 1024;

/// The vault, as `connection`'s exchange. Both of `run`'s ports are implemented on a shared
/// reference, so the one adapter serves as the client secret's source and as the vault.
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
    pub fn new(session: S, connection: ConnectionId) -> Result<Self, ConnectError> {
        if session.identity() != &Identity::TokenExchange(connection.clone()) {
            return Err(REFUSED);
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| REFUSED)?;
        Ok(Self {
            session,
            connection,
            runtime,
        })
    }

    /// Refuses every key but the connection's own.
    fn own(&self, key: &VaultKey) -> Result<(), ConnectError> {
        if key.0 == self.connection.as_str() {
            Ok(())
        } else {
            Err(REFUSED)
        }
    }
}

impl<S: SecretSource> Vault for &ExchangeVault<S> {
    fn put_code(
        &mut self,
        _key: &VaultKey,
        _code: AuthorizationCode,
        _verifier: PkceVerifier,
    ) -> Result<(), ConnectError> {
        Err(REFUSED)
    }

    fn take_code(&mut self, key: &VaultKey) -> Result<Option<WaitingCode>, ConnectError> {
        self.own(key)?;
        let taken = self
            .runtime
            .block_on(self.session.take_code(&self.connection))
            .map_err(|_| REFUSED)?;
        taken
            .map(|entry| code_entry::decode(entry.expose_secret()))
            .transpose()
    }

    fn put_token(
        &mut self,
        key: &VaultKey,
        token: AccessToken,
        scopes: &GrantedScopes,
    ) -> Result<(), ConnectError> {
        self.own(key)?;
        let entry = token_entry::encode(&token, scopes)?;
        let entry = SecretSlice::from(entry.expose_secret().as_bytes().to_vec());
        self.runtime
            .block_on(
                self.session
                    .put(&SecretPath::Token(self.connection.clone()), &entry),
            )
            .map_err(|_| REFUSED)
    }

    fn delete_code(&mut self, _key: &VaultKey) -> Result<(), ConnectError> {
        Err(REFUSED)
    }

    fn delete(&mut self, _key: &VaultKey) -> Result<(), ConnectError> {
        Err(REFUSED)
    }
}

impl<S: SecretSource> ClientSecretSource for &ExchangeVault<S> {
    fn client_secret(&mut self) -> Result<ClientSecret, ConnectError> {
        let read = self
            .runtime
            .block_on(self.session.read(&SecretPath::ClientSecret))
            .map_err(|_| REFUSED)?
            .ok_or(REFUSED)?;
        let text = std::str::from_utf8(read.expose_secret()).map_err(|_| REFUSED)?;
        if !client_secret_in_form(text) {
            return Err(REFUSED);
        }
        Ok(ClientSecret(SecretString::from(text.to_owned())))
    }
}

/// Whether `secret` is 1 to [`MAX_CLIENT_SECRET`] bytes from `!` to `~`.
fn client_secret_in_form(secret: &str) -> bool {
    (1..=MAX_CLIENT_SECRET).contains(&secret.len())
        && secret.bytes().all(|b| (b'!'..=b'~').contains(&b))
}
