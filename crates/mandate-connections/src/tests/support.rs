//! A fixture vault and a fixture token endpoint standing in for Alpaca. No network.

use std::collections::BTreeMap;

use mandate_time::UtcNanos;
use secrecy::{ExposeSecret, SecretString};

use crate::ConnectError;
use crate::exchange::{ExchangeForm, TokenEndpoint, TokenResponse};
use crate::grant::GrantedScopes;
use crate::hosts::LiveTokenRequest;
use crate::start::{ClientId, PkceVerifier};
use crate::vault::{AccessToken, AuthorizationCode, Vault, VaultKey, WaitingCode};

pub const CODE: &str = "code-canary-3f9a";
pub const TOKEN: &str = "token-canary-77c1";
pub const SECRET: &str = "secret-canary-b210";
pub const VERIFIER: &str = "verifier-canary-5d0e";
pub const CANARIES: [&str; 4] = [CODE, TOKEN, SECRET, VERIFIER];

pub fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap()
}

pub fn client() -> ClientId {
    ClientId("client-1".to_owned())
}

pub fn key() -> VaultKey {
    VaultKey("ws-1/conn-1".to_owned())
}

#[derive(Default)]
pub struct FixtureVault {
    codes: BTreeMap<VaultKey, WaitingCode>,
    tokens: BTreeMap<VaultKey, (AccessToken, GrantedScopes)>,
    pub ops: Vec<&'static str>,
}

impl FixtureVault {
    pub fn with_code() -> Self {
        let mut vault = Self::default();
        vault.codes.insert(
            key(),
            WaitingCode {
                code: AuthorizationCode(SecretString::from(CODE)),
                verifier: PkceVerifier(SecretString::from(VERIFIER)),
            },
        );
        vault
    }

    pub fn has_code(&self) -> bool {
        self.codes.contains_key(&key())
    }

    pub fn token(&self) -> Option<String> {
        self.tokens
            .get(&key())
            .map(|(t, _)| t.0.expose_secret().to_owned())
    }

    pub fn scopes(&self) -> Option<GrantedScopes> {
        self.tokens.get(&key()).map(|(_, s)| s.clone())
    }

    /// The operations that write: a delete is not one.
    pub fn writes(&self) -> Vec<&'static str> {
        self.ops
            .iter()
            .copied()
            .filter(|op| op.starts_with("put_"))
            .collect()
    }
}

impl Vault for FixtureVault {
    fn put_code(
        &mut self,
        key: &VaultKey,
        code: AuthorizationCode,
        verifier: PkceVerifier,
    ) -> Result<(), ConnectError> {
        self.ops.push("put_code");
        self.codes
            .insert(key.clone(), WaitingCode { code, verifier });
        Ok(())
    }

    fn read_code(&mut self, key: &VaultKey) -> Result<Option<WaitingCode>, ConnectError> {
        self.ops.push("read_code");
        Ok(self.codes.get(key).map(|waiting| WaitingCode {
            code: AuthorizationCode(SecretString::from(waiting.code.0.expose_secret())),
            verifier: PkceVerifier(SecretString::from(waiting.verifier.0.expose_secret())),
        }))
    }

    fn put_token(
        &mut self,
        key: &VaultKey,
        token: AccessToken,
        scopes: &GrantedScopes,
    ) -> Result<(), ConnectError> {
        self.ops.push("put_token");
        self.tokens.insert(key.clone(), (token, scopes.clone()));
        Ok(())
    }

    fn delete_code(&mut self, key: &VaultKey) -> Result<(), ConnectError> {
        self.ops.push("delete_code");
        self.codes.remove(key);
        Ok(())
    }

    fn delete(&mut self, key: &VaultKey) -> Result<(), ConnectError> {
        self.ops.push("delete");
        self.codes.remove(key);
        self.tokens.remove(key);
        Ok(())
    }
}

/// What the fixture endpoint saw: the request and the form, secrets exposed for comparison.
#[derive(Debug, PartialEq, Eq)]
pub struct Call {
    pub request: LiveTokenRequest,
    pub code: String,
    pub code_verifier: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

pub struct FixtureProvider {
    pub answer: Result<(&'static str, &'static str), ConnectError>,
    pub calls: Vec<Call>,
}

impl FixtureProvider {
    pub fn granting(token_type: &'static str, scope: &'static str) -> Self {
        Self {
            answer: Ok((token_type, scope)),
            calls: Vec::new(),
        }
    }
}

impl TokenEndpoint for FixtureProvider {
    fn post(
        &mut self,
        request: LiveTokenRequest,
        form: &ExchangeForm<'_>,
    ) -> Result<TokenResponse, ConnectError> {
        self.calls.push(Call {
            request,
            code: form.code.0.expose_secret().to_owned(),
            code_verifier: form.code_verifier.0.expose_secret().to_owned(),
            client_id: form.client_id.0.clone(),
            client_secret: form.client_secret.0.expose_secret().to_owned(),
            redirect_uri: form.redirect_uri.to_owned(),
        });
        let (token_type, scope) = self.answer.clone()?;
        Ok(TokenResponse {
            access_token: AccessToken(SecretString::from(TOKEN)),
            token_type: token_type.to_owned(),
            scope: scope.to_owned(),
        })
    }
}
