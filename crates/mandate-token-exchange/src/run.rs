//! The exchange itself, after the start checks (connections spec §5.2 step 4; DEC-821 item 2;
//! DEC-2628; DEC-2626 item 3; DEC-2363): the client secret is read first, so a vault that cannot
//! give it takes no code, and then `mandate_connections::exchange::exchange_waiting_code` waits
//! for the code until the request's deadline, takes it, sends the one request, and stores the
//! token.

use mandate_connections::ConnectError;
use mandate_connections::exchange::{
    ClientSecret, CodeWait, ExchangedGrant, PendingConnect, TokenEndpoint, exchange_waiting_code,
};
use mandate_connections::start::Environment;
use mandate_connections::vault::{Vault, VaultKey};
use mandate_time::UtcNanos;

use crate::startup::Settings;

/// Where the process reads the platform's OAuth client secret: the vault, as the connection's
/// token-exchange identity, the only one allowed to read it (DEC-2636 item 2).
pub trait ClientSecretSource {
    /// The client secret, once.
    ///
    /// # Errors
    /// Any refusal or outage, which the exchange reports without taking the code.
    fn client_secret(&mut self) -> Result<ClientSecret, ConnectError>;
}

/// The vault key of `settings`' connection: its id, which the vault maps to its own paths.
pub fn vault_key(settings: &Settings) -> VaultKey {
    VaultKey(settings.connection.as_str().to_owned())
}

/// Runs the one exchange for `settings`' connection, always on paper (DEC-821 item 1): the client
/// secret from `secrets`, then the wait for the code until `deadline` on `wait` (DEC-2363), the
/// take, the request to `endpoint` and the token write in `vault`.
///
/// # Errors
/// The client secret's refusal, before anything is taken; then `exchange_waiting_code`'s.
pub fn run(
    settings: &Settings,
    secrets: &mut impl ClientSecretSource,
    vault: &mut impl Vault,
    endpoint: &mut impl TokenEndpoint,
    wait: &mut impl CodeWait,
    deadline: UtcNanos,
) -> Result<ExchangedGrant, ConnectError> {
    let client_secret = secrets.client_secret()?;
    let pending = PendingConnect {
        connection_id: settings.connection.as_str().to_owned(),
        environment: Environment::Paper,
        vault_key: vault_key(settings),
    };
    exchange_waiting_code(
        &pending,
        &settings.client,
        &client_secret,
        vault,
        endpoint,
        wait,
        deadline,
    )
}
