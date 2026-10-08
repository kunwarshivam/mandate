//! Where the authorization code and the token live (CN-1; infrastructure §5). The local vault
//! (V1) and the full vault (infrastructure §5) implement [`Vault`]; nothing else holds a secret.

use std::fmt;

use secrecy::SecretString;

use crate::ConnectError;

/// The vault path of one pending or established connection; derived from the workspace and
/// the `connection_id`, never sent to a client.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct VaultKey(pub String);

/// The authorization code from the callback. It lives about ten minutes (DEC-690 item 5).
pub struct AuthorizationCode(pub SecretString);

/// The OAuth access token. It exists only in the vault and, as a `SecretString`, in the memory
/// of the connection's executor (DEC-821 item 3).
pub struct AccessToken(pub SecretString);

impl fmt::Debug for AuthorizationCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AuthorizationCode(redacted)")
    }
}

impl fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccessToken(redacted)")
    }
}

/// Write-only storage for one connection's secrets. No method returns a token: the executor's
/// lease reads it through the vault product, not through this crate.
pub trait Vault {
    /// The API process stores the callback's code here (connections spec §5.2 step 3).
    fn put_code(&mut self, key: &VaultKey, code: AuthorizationCode) -> Result<(), ConnectError>;
    /// Removes and returns the code, so it is used at most once.
    fn take_code(&mut self, key: &VaultKey) -> Result<Option<AuthorizationCode>, ConnectError>;
    fn put_token(&mut self, key: &VaultKey, token: AccessToken) -> Result<(), ConnectError>;
    /// Deletes everything stored under `key`.
    fn delete(&mut self, key: &VaultKey) -> Result<(), ConnectError>;
}
