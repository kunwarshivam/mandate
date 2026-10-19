//! A fixture vault and a fixture token endpoint standing in for Alpaca. No network.

use std::collections::BTreeMap;
use std::fmt;

use mandate_time::UtcNanos;
use secrecy::{ExposeSecret, SecretString};

use crate::ConnectError;
use crate::exchange::{ExchangeForm, TokenEndpoint, TokenResponse};
use crate::grant::GrantedScopes;
use crate::hosts::LiveTokenRequest;
use crate::start::{ClientId, OAuthClient, PkceVerifier, RedirectUri};
use crate::vault::{AccessToken, AuthorizationCode, Vault, VaultKey, WaitingCode};

pub const CODE: &str = "code-canary-3f9a";
pub const TOKEN: &str = "token-canary-77c1";
pub const SECRET: &str = "secret-canary-b210";
pub const VERIFIER: &str = "verifier-canary-5d0e";
pub const CANARIES: [&str; 4] = [CODE, TOKEN, SECRET, VERIFIER];

pub fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap()
}

pub fn client() -> OAuthClient {
    OAuthClient {
        client_id: ClientId("client-1".to_owned()),
        redirect_uri: RedirectUri::production(),
    }
}

pub fn key() -> VaultKey {
    VaultKey("ws-1/conn-1".to_owned())
}

#[derive(Default)]
pub struct FixtureVault {
    codes: BTreeMap<VaultKey, WaitingCode>,
    tokens: BTreeMap<VaultKey, (AccessToken, GrantedScopes)>,
    pub ops: Vec<&'static str>,
    /// The operation that fails with `vault_unavailable`, as a run that stops there would: it is
    /// recorded in `ops`, and changes nothing.
    pub failing: Option<&'static str>,
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

    /// Records `op`, failing it when it is the `failing` one.
    fn record(&mut self, op: &'static str) -> Result<(), ConnectError> {
        self.ops.push(op);
        if self.failing == Some(op) {
            Err(ConnectError::VaultUnavailable)
        } else {
            Ok(())
        }
    }
}

impl Vault for FixtureVault {
    fn put_code(
        &mut self,
        key: &VaultKey,
        code: AuthorizationCode,
        verifier: PkceVerifier,
    ) -> Result<(), ConnectError> {
        self.record("put_code")?;
        self.codes
            .insert(key.clone(), WaitingCode { code, verifier });
        Ok(())
    }

    fn take_code(&mut self, key: &VaultKey) -> Result<Option<WaitingCode>, ConnectError> {
        self.record("take_code")?;
        Ok(self.codes.remove(key))
    }

    fn put_token(
        &mut self,
        key: &VaultKey,
        token: AccessToken,
        scopes: &GrantedScopes,
    ) -> Result<(), ConnectError> {
        self.record("put_token")?;
        self.tokens.insert(key.clone(), (token, scopes.clone()));
        Ok(())
    }

    fn delete_code(&mut self, key: &VaultKey) -> Result<(), ConnectError> {
        self.record("delete_code")?;
        self.codes.remove(key);
        Ok(())
    }

    fn delete(&mut self, key: &VaultKey) -> Result<(), ConnectError> {
        self.record("delete")?;
        self.codes.remove(key);
        self.tokens.remove(key);
        Ok(())
    }
}

/// What the fixture endpoint saw: the request and the form, secrets exposed for comparison
/// but never printed.
#[derive(PartialEq, Eq)]
pub struct Call {
    pub request: LiveTokenRequest,
    pub code: String,
    pub code_verifier: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

impl fmt::Debug for Call {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Call({:?}, {})", self.request, self.redirect_uri)
    }
}

/// Alpaca stand-in: a code is single-use, so a second exchange of one fails (connections spec §5.2 step 4).
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
        let code = form.code.0.expose_secret().to_owned();
        let reused = self.calls.iter().any(|call| call.code == code);
        self.calls.push(Call {
            request,
            code,
            code_verifier: form.code_verifier.0.expose_secret().to_owned(),
            client_id: form.client_id.0.clone(),
            client_secret: form.client_secret.0.expose_secret().to_owned(),
            redirect_uri: form.redirect_uri.to_owned(),
        });
        if reused {
            return Err(ConnectError::EndpointFailed);
        }
        let (token_type, scope) = self.answer.clone()?;
        Ok(TokenResponse {
            access_token: AccessToken(SecretString::from(TOKEN)),
            token_type: token_type.to_owned(),
            scope: scope.to_owned(),
        })
    }
}
