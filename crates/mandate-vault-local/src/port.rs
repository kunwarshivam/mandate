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

use secrecy::{ExposeSecret, SecretSlice, SecretString};

use crate::entry::{Entry, KeyRef};
use crate::store::{ApiVault, ExecutorVault};

/// The API process's port for one connection: `put_code`, `delete_code`, `delete`.
#[derive(Debug)]
pub struct ApiPort<'a> {
    pub vault: &'a ApiVault,
    pub connection_id: ConnectionId,
    pub environment: Environment,
}

/// The executor's (and the token exchange's) port for one connection: `take_code`,
/// `put_token`, `delete_code`, `delete`.
#[derive(Debug)]
pub struct ExecutorPort<'a> {
    pub vault: &'a ExecutorVault,
    pub connection_id: ConnectionId,
    pub environment: Environment,
}

/// Every refusal of the port: the connect core needs no more than that the vault did not serve
/// the call, and no detail reaches an API response (API-11).
const REFUSED: ConnectError = ConnectError::VaultUnavailable;

fn key_ref(key: &VaultKey) -> Result<KeyRef, ConnectError> {
    KeyRef::new(&key.0).map_err(|_| REFUSED)
}

fn code_entry(
    key: &VaultKey,
    connection_id: &ConnectionId,
    environment: Environment,
) -> Result<Entry, ConnectError> {
    Ok(Entry::code(
        key_ref(key)?,
        connection_id.clone(),
        environment,
    ))
}

fn token_entry(
    key: &VaultKey,
    connection_id: &ConnectionId,
    environment: Environment,
) -> Result<Entry, ConnectError> {
    Ok(Entry::token(
        key_ref(key)?,
        connection_id.clone(),
        environment,
    ))
}

/// Each field as a two-byte big-endian length and its bytes, into a buffer of exactly that size
/// that the secret box then owns.
fn encode(fields: &[&[u8]]) -> Result<SecretSlice<u8>, ConnectError> {
    let len = fields
        .iter()
        .try_fold(0_usize, |sum, f| sum.checked_add(f.len())?.checked_add(2))
        .ok_or(REFUSED)?;
    let mut out = Vec::with_capacity(len);
    for field in fields {
        let field_len = u16::try_from(field.len()).map_err(|_| REFUSED)?;
        out.extend_from_slice(&field_len.to_be_bytes());
        out.extend_from_slice(field);
    }
    Ok(SecretSlice::from(out))
}

/// The two non-empty UTF-8 fields of a code entry, and nothing after them.
fn decode_code(bytes: &[u8]) -> Result<WaitingCode, ConnectError> {
    let (code, rest) = take_field(bytes)?;
    let (verifier, rest) = take_field(rest)?;
    if !rest.is_empty() || code.is_empty() || verifier.is_empty() {
        return Err(REFUSED);
    }
    let text = |field: &[u8]| -> Result<SecretString, ConnectError> {
        std::str::from_utf8(field)
            .map(|s| SecretString::from(s.to_owned()))
            .map_err(|_| REFUSED)
    };
    Ok(WaitingCode {
        code: AuthorizationCode(text(code)?),
        verifier: PkceVerifier(text(verifier)?),
    })
}

fn take_field(bytes: &[u8]) -> Result<(&[u8], &[u8]), ConnectError> {
    let (len, rest) = bytes.split_first_chunk::<2>().ok_or(REFUSED)?;
    let len = usize::from(u16::from_be_bytes(*len));
    if rest.len() < len {
        return Err(REFUSED);
    }
    Ok(rest.split_at(len))
}

impl Vault for ApiPort<'_> {
    fn put_code(
        &mut self,
        key: &VaultKey,
        code: AuthorizationCode,
        verifier: PkceVerifier,
    ) -> Result<(), ConnectError> {
        let entry = code_entry(key, &self.connection_id, self.environment)?;
        let secret = encode(&[
            code.0.expose_secret().as_bytes(),
            verifier.0.expose_secret().as_bytes(),
        ])?;
        self.vault.put_code(&entry, &secret).map_err(|_| REFUSED)
    }

    fn take_code(&mut self, _: &VaultKey) -> Result<Option<WaitingCode>, ConnectError> {
        Err(REFUSED)
    }

    fn put_token(
        &mut self,
        _: &VaultKey,
        _: AccessToken,
        _: &GrantedScopes,
    ) -> Result<(), ConnectError> {
        Err(REFUSED)
    }

    fn delete_code(&mut self, key: &VaultKey) -> Result<(), ConnectError> {
        let entry = code_entry(key, &self.connection_id, self.environment)?;
        self.vault.delete(&entry).map_err(|_| REFUSED)
    }

    fn delete(&mut self, key: &VaultKey) -> Result<(), ConnectError> {
        let code = code_entry(key, &self.connection_id, self.environment)?;
        let token = token_entry(key, &self.connection_id, self.environment)?;
        self.vault.delete(&code).map_err(|_| REFUSED)?;
        self.vault.delete(&token).map_err(|_| REFUSED)
    }
}

impl Vault for ExecutorPort<'_> {
    fn put_code(
        &mut self,
        _: &VaultKey,
        _: AuthorizationCode,
        _: PkceVerifier,
    ) -> Result<(), ConnectError> {
        Err(REFUSED)
    }

    fn take_code(&mut self, key: &VaultKey) -> Result<Option<WaitingCode>, ConnectError> {
        let entry = code_entry(key, &self.connection_id, self.environment)?;
        match self.vault.take_code(&entry).map_err(|_| REFUSED)? {
            Some(secret) => decode_code(secret.expose_secret()).map(Some),
            None => Ok(None),
        }
    }

    fn put_token(
        &mut self,
        key: &VaultKey,
        token: AccessToken,
        scopes: &GrantedScopes,
    ) -> Result<(), ConnectError> {
        let entry = token_entry(key, &self.connection_id, self.environment)?;
        let scopes = scopes.iter().collect::<Vec<_>>().join(" ");
        let secret = encode(&[token.0.expose_secret().as_bytes(), scopes.as_bytes()])?;
        self.vault.put(&entry, &secret).map_err(|_| REFUSED)
    }

    fn delete_code(&mut self, key: &VaultKey) -> Result<(), ConnectError> {
        let entry = code_entry(key, &self.connection_id, self.environment)?;
        self.vault.delete(&entry).map_err(|_| REFUSED)
    }

    fn delete(&mut self, key: &VaultKey) -> Result<(), ConnectError> {
        let code = code_entry(key, &self.connection_id, self.environment)?;
        let token = token_entry(key, &self.connection_id, self.environment)?;
        self.vault.delete(&code).map_err(|_| REFUSED)?;
        self.vault.delete(&token).map_err(|_| REFUSED)
    }
}
