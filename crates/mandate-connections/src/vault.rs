//! Where the authorization code and the token live (CN-1; infrastructure §5). The local vault
//! (V1) and the full vault (infrastructure §5) implement [`Vault`]; nothing else holds a secret.

use std::fmt;

use secrecy::SecretString;

use crate::ConnectError;
use crate::grant::GrantedScopes;
use crate::start::PkceVerifier;

/// The vault path of one pending or established connection; derived from the workspace and
/// the `connection_id`, never sent to a client.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct VaultKey(pub String);

/// The authorization code from the callback. It lives about ten minutes (DEC-690 item 5).
pub struct AuthorizationCode(pub SecretString);

/// The OAuth access token. It exists only in the vault and, as a `SecretString`, in the memory
/// of the token-exchange process until its vault write, and of the connection's executor
/// through its lease (DEC-821 items 2 and 4).
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

/// What the callback stored for the exchange: the code and the PKCE verifier, together
/// (connections spec §5.2 step 3).
#[derive(Debug)]
pub struct WaitingCode {
    pub code: AuthorizationCode,
    pub verifier: PkceVerifier,
}

/// Write-only storage for one connection's secrets. No method returns a token: the executor's
/// lease reads it through the vault product, not through this crate.
pub trait Vault {
    /// The API process stores the callback's code and PKCE verifier here, in one write
    /// (connections spec §5.2 step 3).
    fn put_code(
        &mut self,
        key: &VaultKey,
        code: AuthorizationCode,
        verifier: PkceVerifier,
    ) -> Result<(), ConnectError>;
    /// Reads the code and verifier. The token-exchange process reads them once (§5.2 step 4).
    fn read_code(&mut self, key: &VaultKey) -> Result<Option<WaitingCode>, ConnectError>;
    /// Stores the token with the granted scopes, which the executor reads through its lease.
    fn put_token(
        &mut self,
        key: &VaultKey,
        token: AccessToken,
        scopes: &GrantedScopes,
    ) -> Result<(), ConnectError>;
    /// Deletes the code and verifier, once the token is stored.
    fn delete_code(&mut self, key: &VaultKey) -> Result<(), ConnectError>;
    /// Deletes everything stored under `key`.
    fn delete(&mut self, key: &VaultKey) -> Result<(), ConnectError>;
}
