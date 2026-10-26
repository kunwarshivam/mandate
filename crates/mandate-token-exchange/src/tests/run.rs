//! The exchange after the start checks, with an in-memory vault, client-secret source and token
//! endpoint: the client secret is read before the code is taken, the code is sent once with the
//! settings' client, and the token is stored under the connection, on paper only.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use mandate_connections::ConnectError;
use mandate_connections::exchange::{ClientSecret, ExchangeForm, TokenEndpoint, TokenResponse};
use mandate_connections::grant::GrantedScopes;
use mandate_connections::hosts::LiveTokenRequest;
use mandate_connections::record::ConnectionId;
use mandate_connections::start::{ClientId, Environment, OAuthClient, PkceVerifier, RedirectUri};
use mandate_connections::vault::{AccessToken, AuthorizationCode, Vault, VaultKey, WaitingCode};
use secrecy::{ExposeSecret, SecretString};

use super::{Ticking, Waitless, deadline, started};
use crate::run::{ClientSecretSource, run, vault_key};
use crate::startup::{Endpoint, Settings};

const CONNECTION: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W50";

fn settings() -> Settings {
    Settings {
        connection: ConnectionId::new(CONNECTION).unwrap(),
        endpoint: Endpoint::Live,
        client: OAuthClient {
            client_id: ClientId("PKTESTCLIENT01".to_owned()),
            redirect_uri: RedirectUri::parse(
                "https://api-staging.owlhead.ai/v1/oauth/alpaca/callback",
            )
            .unwrap(),
        },
    }
}

/// Whether the code is still waiting, shared between the vault and the secret source so the
/// source can record what it saw when it was read.
type Waiting = Rc<Cell<bool>>;

#[derive(Default)]
struct MemVault {
    codes: BTreeMap<VaultKey, WaitingCode>,
    tokens: BTreeMap<VaultKey, String>,
    waiting: Waiting,
}

impl MemVault {
    fn with_code() -> Self {
        let mut vault = Self::default();
        vault
            .put_code(
                &vault_key(&settings()),
                AuthorizationCode(SecretString::from("the-code")),
                PkceVerifier(SecretString::from("the-verifier")),
            )
            .unwrap();
        vault
    }
}

impl Vault for MemVault {
    fn put_code(
        &mut self,
        key: &VaultKey,
        code: AuthorizationCode,
        verifier: PkceVerifier,
    ) -> Result<(), ConnectError> {
        self.codes
            .insert(key.clone(), WaitingCode { code, verifier });
        self.waiting.set(true);
        Ok(())
    }

    fn take_code(&mut self, key: &VaultKey) -> Result<Option<WaitingCode>, ConnectError> {
        self.waiting.set(false);
        Ok(self.codes.remove(key))
    }

    fn put_token(
        &mut self,
        key: &VaultKey,
        token: AccessToken,
        _scopes: &GrantedScopes,
    ) -> Result<(), ConnectError> {
        self.tokens
            .insert(key.clone(), token.0.expose_secret().to_owned());
        Ok(())
    }

    fn delete_code(&mut self, key: &VaultKey) -> Result<(), ConnectError> {
        self.codes.remove(key);
        Ok(())
    }

    fn delete(&mut self, key: &VaultKey) -> Result<(), ConnectError> {
        self.codes.remove(key);
        self.tokens.remove(key);
        Ok(())
    }
}

/// Answers the client secret, or refuses, and records whether the code was still waiting.
struct Secrets {
    refuse: bool,
    waiting: Waiting,
    saw_code_waiting: Vec<bool>,
}

impl Secrets {
    fn for_vault(vault: &MemVault, refuse: bool) -> Self {
        Self {
            refuse,
            waiting: Rc::clone(&vault.waiting),
            saw_code_waiting: Vec::new(),
        }
    }
}

impl ClientSecretSource for Secrets {
    fn client_secret(&mut self) -> Result<ClientSecret, ConnectError> {
        self.saw_code_waiting.push(self.waiting.get());
        if self.refuse {
            Err(ConnectError::VaultUnavailable)
        } else {
            Ok(ClientSecret(SecretString::from("the-client-secret")))
        }
    }
}

/// One request as the endpoint saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Sent {
    code: String,
    verifier: String,
    client_id: String,
    client_secret: String,
    redirect_uri: String,
}

struct Endpoint1 {
    sent: RefCell<Vec<Sent>>,
    token_type: &'static str,
}

impl Endpoint1 {
    fn answering(token_type: &'static str) -> Self {
        Self {
            sent: RefCell::default(),
            token_type,
        }
    }
}

impl TokenEndpoint for Endpoint1 {
    fn post(
        &mut self,
        _request: LiveTokenRequest,
        form: &ExchangeForm<'_>,
    ) -> Result<TokenResponse, ConnectError> {
        self.sent.borrow_mut().push(Sent {
            code: form.code.0.expose_secret().to_owned(),
            verifier: form.code_verifier.0.expose_secret().to_owned(),
            client_id: form.client_id.0.clone(),
            client_secret: form.client_secret.0.expose_secret().to_owned(),
            redirect_uri: form.redirect_uri.to_owned(),
        });
        Ok(TokenResponse {
            access_token: AccessToken(SecretString::from("the-token")),
            token_type: self.token_type.to_owned(),
            scope: "trading data".to_owned(),
        })
    }
}

#[test]
fn an_exchange_sends_the_code_once_with_the_settings_client_and_stores_the_token() {
    let mut vault = MemVault::with_code();
    let mut secrets = Secrets::for_vault(&vault, false);
    let mut endpoint = Endpoint1::answering("bearer");
    let grant = run(
        &settings(),
        &mut secrets,
        &mut vault,
        &mut endpoint,
        &mut Waitless,
        deadline(),
    )
    .unwrap();
    assert_eq!(
        *endpoint.sent.borrow(),
        [Sent {
            code: "the-code".to_owned(),
            verifier: "the-verifier".to_owned(),
            client_id: "PKTESTCLIENT01".to_owned(),
            client_secret: "the-client-secret".to_owned(),
            redirect_uri: "https://api-staging.owlhead.ai/v1/oauth/alpaca/callback".to_owned(),
        }]
    );
    let key = VaultKey(CONNECTION.to_owned());
    assert_eq!(
        vault.tokens.get(&key).map(String::as_str),
        Some("the-token")
    );
    assert!(vault.codes.is_empty(), "the code is taken");
    assert_eq!(grant.connection_id, CONNECTION);
    assert_eq!(grant.environment, Environment::Paper);
    assert_eq!(grant.vault_key, key);
    assert_eq!(
        secrets.saw_code_waiting,
        [true],
        "the client secret is read once, before the take"
    );
}

/// A vault that cannot give the client secret takes no code and sends nothing: the code waits
/// for a run that can.
#[test]
fn a_client_secret_that_cannot_be_read_takes_no_code() {
    let mut vault = MemVault::with_code();
    let mut secrets = Secrets::for_vault(&vault, true);
    let mut endpoint = Endpoint1::answering("bearer");
    assert_eq!(
        run(
            &settings(),
            &mut secrets,
            &mut vault,
            &mut endpoint,
            &mut Waitless,
            deadline()
        )
        .map(|_| ()),
        Err(ConnectError::VaultUnavailable)
    );
    assert_eq!(vault.codes.len(), 1, "the code is still waiting");
    assert!(endpoint.sent.borrow().is_empty());
    assert!(vault.tokens.is_empty());
}

#[test]
fn without_a_waiting_code_nothing_is_sent() {
    let mut vault = MemVault::default();
    let mut secrets = Secrets::for_vault(&vault, false);
    let mut endpoint = Endpoint1::answering("bearer");
    assert_eq!(
        run(
            &settings(),
            &mut secrets,
            &mut vault,
            &mut endpoint,
            &mut Ticking(started().secs()),
            deadline()
        )
        .map(|_| ()),
        Err(ConnectError::CodeMissing)
    );
    assert!(endpoint.sent.borrow().is_empty());
    assert!(vault.tokens.is_empty());
}

/// A refused grant stores no token, and the code it spent is gone, so no rerun sends it again.
#[test]
fn a_refused_grant_stores_nothing_and_spends_the_code() {
    let mut vault = MemVault::with_code();
    let mut secrets = Secrets::for_vault(&vault, false);
    let mut endpoint = Endpoint1::answering("mac");
    assert_eq!(
        run(
            &settings(),
            &mut secrets,
            &mut vault,
            &mut endpoint,
            &mut Waitless,
            deadline()
        )
        .map(|_| ()),
        Err(ConnectError::TokenType)
    );
    assert_eq!(endpoint.sent.borrow().len(), 1);
    assert!(vault.tokens.is_empty());
    assert!(vault.codes.is_empty());
}

#[test]
fn the_vault_key_is_the_connection_id() {
    assert_eq!(vault_key(&settings()), VaultKey(CONNECTION.to_owned()));
}
