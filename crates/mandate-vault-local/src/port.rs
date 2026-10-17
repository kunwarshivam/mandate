//! The connect core's vault port ([`mandate_connections::vault::Vault`]) over the local vault,
//! one connection at a time. Each role serves only the calls DEC-692 item 1 gives it; any other
//! call is refused as [`ConnectError::VaultUnavailable`] and touches no file.
//!
//! A code entry holds the code and the PKCE verifier; a token entry the token and its granted
//! scopes. Each field is a two-byte big-endian length and its bytes.

use mandate_connections::ConnectError;
use mandate_connections::grant::GrantedScopes;
use mandate_connections::record::{ConnectionId, Environment};
use mandate_connections::start::PkceVerifier;
use mandate_connections::vault::{AccessToken, AuthorizationCode, Vault, VaultKey, WaitingCode};

use crate::store::{ApiVault, ExecutorVault};

/// The API process's port for one connection: `put_code`, `delete_code`, `delete`.
#[derive(Debug)]
pub struct ApiPort<'a> {
    pub vault: &'a ApiVault,
    pub connection_id: ConnectionId,
    pub environment: Environment,
}

/// The executor's (and the token exchange's) port for one connection: `read_code`,
/// `put_token`, `delete_code`, `delete`.
#[derive(Debug)]
pub struct ExecutorPort<'a> {
    pub vault: &'a ExecutorVault,
    pub connection_id: ConnectionId,
    pub environment: Environment,
}

const UNIMPLEMENTED: ConnectError = ConnectError::Unimplemented {
    story: crate::entry::STORY,
};

impl Vault for ApiPort<'_> {
    fn put_code(
        &mut self,
        key: &VaultKey,
        code: AuthorizationCode,
        verifier: PkceVerifier,
    ) -> Result<(), ConnectError> {
        let _ = (key, code, verifier);
        Err(UNIMPLEMENTED)
    }

    fn read_code(&mut self, key: &VaultKey) -> Result<Option<WaitingCode>, ConnectError> {
        let _ = key;
        Err(UNIMPLEMENTED)
    }

    fn put_token(
        &mut self,
        key: &VaultKey,
        token: AccessToken,
        scopes: &GrantedScopes,
    ) -> Result<(), ConnectError> {
        let _ = (key, token, scopes);
        Err(UNIMPLEMENTED)
    }

    fn delete_code(&mut self, key: &VaultKey) -> Result<(), ConnectError> {
        let _ = key;
        Err(UNIMPLEMENTED)
    }

    fn delete(&mut self, key: &VaultKey) -> Result<(), ConnectError> {
        let _ = key;
        Err(UNIMPLEMENTED)
    }
}

impl Vault for ExecutorPort<'_> {
    fn put_code(
        &mut self,
        key: &VaultKey,
        code: AuthorizationCode,
        verifier: PkceVerifier,
    ) -> Result<(), ConnectError> {
        let _ = (key, code, verifier);
        Err(UNIMPLEMENTED)
    }

    fn read_code(&mut self, key: &VaultKey) -> Result<Option<WaitingCode>, ConnectError> {
        let _ = key;
        Err(UNIMPLEMENTED)
    }

    fn put_token(
        &mut self,
        key: &VaultKey,
        token: AccessToken,
        scopes: &GrantedScopes,
    ) -> Result<(), ConnectError> {
        let _ = (key, token, scopes);
        Err(UNIMPLEMENTED)
    }

    fn delete_code(&mut self, key: &VaultKey) -> Result<(), ConnectError> {
        let _ = key;
        Err(UNIMPLEMENTED)
    }

    fn delete(&mut self, key: &VaultKey) -> Result<(), ConnectError> {
        let _ = key;
        Err(UNIMPLEMENTED)
    }
}
