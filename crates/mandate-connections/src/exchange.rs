//! The code exchange (connections spec §5.2 step 4). **The token-exchange process only**
//! (DEC-821 item 2): it is the one process with the client secret and the live-host token
//! endpoint. It is not the executor: it holds no order client and no token lease, and the
//! executor's egress contains no live host (DEC-821 item 4).

use std::fmt;

use secrecy::SecretString;

use crate::ConnectError;
use crate::grant::GrantedScopes;
use crate::hosts::LiveTokenRequest;
use crate::start::{ClientId, Environment, PkceVerifier};
use crate::vault::{AccessToken, AuthorizationCode, Vault, VaultKey};

/// The platform's OAuth client secret (infrastructure §5.1).
pub struct ClientSecret(pub SecretString);

impl fmt::Debug for ClientSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ClientSecret(redacted)")
    }
}

/// The form body of the exchange: `grant_type=authorization_code`, the code, the client id and
/// secret, the redirect URI of the authorization request, and the PKCE `code_verifier`, which is
/// sent but never relied on (connections spec §5.2 step 4; DEC-690 item 9).
pub struct ExchangeForm<'a> {
    pub code: &'a AuthorizationCode,
    pub code_verifier: &'a PkceVerifier,
    pub client_id: &'a ClientId,
    pub client_secret: &'a ClientSecret,
    pub redirect_uri: &'a str,
}

/// The token response's members (Alpaca: `access_token`, `token_type`, `scope`).
#[derive(Debug)]
pub struct TokenResponse {
    pub access_token: AccessToken,
    pub token_type: String,
    pub scope: String,
}

/// Sends the one live-host request. The adapter implementing it receives a
/// [`LiveTokenRequest`], so it has no URL of its own to choose.
pub trait TokenEndpoint {
    fn post(
        &mut self,
        request: LiveTokenRequest,
        form: &ExchangeForm<'_>,
    ) -> Result<TokenResponse, ConnectError>;
}

/// A connection the callback accepted and whose code waits in the vault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingConnect {
    pub connection_id: String,
    pub environment: Environment,
    pub vault_key: VaultKey,
}

/// What the permission checks (E7-12) receive. It carries no secret: the token stays in the
/// vault under `vault_key`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangedGrant {
    pub connection_id: String,
    pub environment: Environment,
    pub scopes: GrantedScopes,
    pub vault_key: VaultKey,
}

/// Exchanges the waiting code for a token, in this order (connections spec §5.2 step 4):
///
/// 1. A connection that is not paper is refused before the vault or the endpoint is touched
///    (DEC-821 items 3 and 4).
/// 2. The code and PKCE verifier are read once from the vault; with none waiting, the exchange
///    is refused and nothing is written.
/// 3. The endpoint receives the [`LiveTokenRequest`] and the form, verifier included.
/// 4. The token type must be `bearer` and the scope must pass [`crate::grant::check_scope`].
///    On any refusal the token is dropped before any vault write, and the vault entry is
///    deleted, since a read code is never tried again.
/// 5. The token is stored with the granted scopes, then the code and verifier are deleted, and
///    the grant, without the token, is returned.
pub fn exchange_code(
    pending: &PendingConnect,
    client_id: &ClientId,
    client_secret: &ClientSecret,
    vault: &mut impl Vault,
    endpoint: &mut impl TokenEndpoint,
) -> Result<ExchangedGrant, ConnectError> {
    let _ = (pending, client_id, client_secret, vault, endpoint);
    Err(ConnectError::Unimplemented { story: "E10-13" })
}
