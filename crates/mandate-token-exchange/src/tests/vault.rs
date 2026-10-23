//! The exchange's vault adapter (DEC-2330 item 2) over a scripted session that records every
//! call: only the connection's own exchange, only its own key, the take read as the code entry,
//! the token written as the token entry, the client secret as one line of visible ASCII, and no
//! other call made.

use std::sync::{Arc, Mutex};

use mandate_connections::ConnectError;
use mandate_connections::code_entry;
use mandate_connections::grant::check_scope;
use mandate_connections::record::ConnectionId;
use mandate_connections::start::PkceVerifier;
use mandate_connections::token_entry;
use mandate_connections::vault::{AccessToken, AuthorizationCode, Vault, VaultKey};
use mandate_time::UtcNanos;
use mandate_vault::SourceError;
use mandate_vault::lease::Lease;
use mandate_vault::policy::{Identity, SecretPath};
use mandate_vault::source::{Grant, SecretSource};
use secrecy::{ExposeSecret, SecretSlice, SecretString};

use crate::run::ClientSecretSource;
use crate::vault::{ExchangeVault, MAX_CLIENT_SECRET};

pub(super) const CONNECTION: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W50";
const OTHER: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W51";
pub(super) const CODE: &str = "code-canary-5e1a";
pub(super) const VERIFIER: &str = "verifier-canary-0123456789abcdefghijklmnopq.~_";
pub(super) const CLIENT_SECRET: &str = "client-secret-canary-77";
const TOKEN: &str = "token-canary-31f0";

pub(super) fn connection(id: &str) -> ConnectionId {
    ConnectionId::new(id).unwrap()
}

/// One call a session saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Call {
    Take(ConnectionId),
    Read(SecretPath),
    Put(SecretPath, Vec<u8>),
    Other(&'static str),
}

/// What the scripted session holds and answers, and every call it saw.
#[derive(Debug)]
pub(super) struct Script {
    pub(super) identity: Identity,
    /// The code entry's bytes, taken once.
    pub(super) code: Mutex<Option<Vec<u8>>>,
    pub(super) client_secret: Option<Vec<u8>>,
    pub(super) fails: Option<SourceError>,
    pub(super) calls: Mutex<Vec<Call>>,
}

impl Script {
    /// The exchange of [`CONNECTION`], with format 1's entry of [`CODE`] and [`VERIFIER`] waiting
    /// and [`CLIENT_SECRET`] stored.
    pub(super) fn waiting() -> Arc<Self> {
        Arc::new(Self::with(
            Identity::TokenExchange(connection(CONNECTION)),
            Some(entry().into_bytes()),
            Some(CLIENT_SECRET.as_bytes().to_vec()),
            None,
        ))
    }

    pub(super) fn with(
        identity: Identity,
        code: Option<Vec<u8>>,
        client_secret: Option<Vec<u8>>,
        fails: Option<SourceError>,
    ) -> Self {
        Self {
            identity,
            code: Mutex::new(code),
            client_secret,
            fails,
            calls: Mutex::default(),
        }
    }

    pub(super) fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }

    fn saw(&self, call: Call) -> Result<(), SourceError> {
        self.calls.lock().unwrap().push(call);
        self.fails.clone().map_or(Ok(()), Err)
    }
}

/// Format 1's code entry of [`CODE`] and [`VERIFIER`], by the codec.
pub(super) fn entry() -> String {
    code_entry::encode(
        &AuthorizationCode(SecretString::from(CODE)),
        &PkceVerifier(SecretString::from(VERIFIER)),
    )
    .unwrap()
    .expose_secret()
    .to_owned()
}

/// A session over a shared [`Script`].
pub(super) struct Session(pub(super) Arc<Script>);

impl SecretSource for Session {
    fn identity(&self) -> &Identity {
        &self.0.identity
    }

    async fn put(&self, path: &SecretPath, secret: &SecretSlice<u8>) -> Result<(), SourceError> {
        self.0
            .saw(Call::Put(path.clone(), secret.expose_secret().to_vec()))
    }

    async fn read(&self, path: &SecretPath) -> Result<Option<SecretSlice<u8>>, SourceError> {
        self.0.saw(Call::Read(path.clone()))?;
        Ok(match path {
            SecretPath::ClientSecret => self.0.client_secret.clone().map(SecretSlice::from),
            _ => None,
        })
    }

    async fn take_code(
        &self,
        connection: &ConnectionId,
    ) -> Result<Option<SecretSlice<u8>>, SourceError> {
        self.0.saw(Call::Take(connection.clone()))?;
        Ok(self.0.code.lock().unwrap().take().map(SecretSlice::from))
    }

    async fn delete(&self, _: &SecretPath) -> Result<(), SourceError> {
        self.0.saw(Call::Other("delete"))
    }

    async fn lease(&self, _: &ConnectionId, _: UtcNanos) -> Result<Lease, SourceError> {
        self.0.saw(Call::Other("lease"))?;
        Err(SourceError::Denied)
    }

    async fn renew(&self, _: &Lease, _: UtcNanos) -> Result<Lease, SourceError> {
        self.0.saw(Call::Other("renew"))?;
        Err(SourceError::Denied)
    }

    async fn revoke(&self, _: Lease) -> Result<(), SourceError> {
        self.0.saw(Call::Other("revoke"))
    }

    async fn issue_grant(&self, _: &ConnectionId, _: UtcNanos) -> Result<Grant, SourceError> {
        self.0.saw(Call::Other("issue_grant"))?;
        Err(SourceError::Denied)
    }

    async fn revoke_grant(&self, _: &Grant) -> Result<(), SourceError> {
        self.0.saw(Call::Other("revoke_grant"))
    }
}

pub(super) fn adapter(script: &Arc<Script>) -> ExchangeVault<Session> {
    ExchangeVault::new(Session(Arc::clone(script)), connection(CONNECTION)).unwrap()
}

fn own_key() -> VaultKey {
    VaultKey(CONNECTION.to_owned())
}

fn token(text: &str) -> AccessToken {
    AccessToken(SecretString::from(text))
}

#[test]
fn only_the_connection_s_own_exchange_is_served() {
    for identity in [
        Identity::TokenExchange(connection(OTHER)),
        Identity::Executor(connection(CONNECTION)),
        Identity::Api,
        Identity::ConnectionManager,
    ] {
        let script = Arc::new(Script::with(identity.clone(), None, None, None));
        assert!(
            ExchangeVault::new(Session(Arc::clone(&script)), connection(CONNECTION)).is_err(),
            "{identity:?}"
        );
        assert!(script.calls().is_empty());
    }
}

#[test]
fn the_take_is_the_session_s_single_use_take_read_as_the_code_entry() {
    let script = Script::waiting();
    let vault = adapter(&script);
    let taken = (&vault).take_code(&own_key()).unwrap().expect("a code");
    assert_eq!(taken.code.0.expose_secret(), CODE);
    assert_eq!(taken.verifier.0.expose_secret(), VERIFIER);
    assert!(
        (&vault).take_code(&own_key()).unwrap().is_none(),
        "taken once"
    );
    assert_eq!(
        script.calls(),
        vec![
            Call::Take(connection(CONNECTION)),
            Call::Take(connection(CONNECTION))
        ]
    );
}

#[test]
fn a_taken_entry_in_another_form_is_code_entry_invalid() {
    for bytes in [
        b"not an entry".to_vec(),
        entry().replace("\"format\":1", "\"format\":2").into_bytes(),
    ] {
        let script = Arc::new(Script::with(
            Identity::TokenExchange(connection(CONNECTION)),
            Some(bytes),
            None,
            None,
        ));
        assert_eq!(
            (&adapter(&script)).take_code(&own_key()).map(|_| ()),
            Err(ConnectError::CodeEntryInvalid)
        );
    }
}

#[test]
fn the_token_is_written_as_the_token_entry_at_the_connection_s_token() {
    let script = Script::waiting();
    let scopes = check_scope("trading data").unwrap();
    (&adapter(&script))
        .put_token(&own_key(), token(TOKEN), &scopes)
        .unwrap();
    let expected = token_entry::encode(&token(TOKEN), &scopes).unwrap();
    assert_eq!(
        script.calls(),
        vec![Call::Put(
            SecretPath::Token(connection(CONNECTION)),
            expected.expose_secret().as_bytes().to_vec()
        )]
    );
}

#[test]
fn a_token_outside_the_entry_s_form_is_never_written() {
    let script = Script::waiting();
    let scopes = check_scope("trading data").unwrap();
    assert_eq!(
        (&adapter(&script)).put_token(&own_key(), token("tok en"), &scopes),
        Err(ConnectError::TokenEntryInvalid)
    );
    assert!(script.calls().is_empty());
}

#[test]
fn another_connection_s_key_is_refused_before_any_call() {
    let script = Script::waiting();
    let vault = adapter(&script);
    let other = VaultKey(OTHER.to_owned());
    let scopes = check_scope("trading data").unwrap();
    assert_eq!(
        (&vault).take_code(&other).map(|_| ()),
        Err(ConnectError::VaultUnavailable)
    );
    assert_eq!(
        (&vault).put_token(&other, token(TOKEN), &scopes),
        Err(ConnectError::VaultUnavailable)
    );
    assert!(script.calls().is_empty());
}

#[test]
fn the_client_secret_is_the_stored_line() {
    let script = Script::waiting();
    let secret = (&adapter(&script)).client_secret().unwrap();
    assert_eq!(secret.0.expose_secret(), CLIENT_SECRET);
    assert_eq!(script.calls(), vec![Call::Read(SecretPath::ClientSecret)]);
}

#[test]
fn a_client_secret_absent_or_out_of_form_is_vault_unavailable() {
    let long = vec![b's'; MAX_CLIENT_SECRET + 1];
    let at_limit = vec![b's'; MAX_CLIENT_SECRET];
    for stored in [
        None,
        Some(Vec::new()),
        Some(b"two words".to_vec()),
        Some(b"line\n".to_vec()),
        Some(b"s\xff".to_vec()),
        Some(long),
    ] {
        let script = Arc::new(Script::with(
            Identity::TokenExchange(connection(CONNECTION)),
            None,
            stored.clone(),
            None,
        ));
        assert_eq!(
            (&adapter(&script)).client_secret().map(|_| ()),
            Err(ConnectError::VaultUnavailable),
            "{stored:?}"
        );
    }
    let script = Arc::new(Script::with(
        Identity::TokenExchange(connection(CONNECTION)),
        None,
        Some(at_limit),
        None,
    ));
    assert!(
        (&adapter(&script)).client_secret().is_ok(),
        "the longest is read"
    );
}

#[test]
fn a_session_failure_is_vault_unavailable() {
    for refused in [
        SourceError::Unavailable,
        SourceError::Denied,
        SourceError::Sealed,
    ] {
        let script = Arc::new(Script::with(
            Identity::TokenExchange(connection(CONNECTION)),
            Some(entry().into_bytes()),
            Some(CLIENT_SECRET.as_bytes().to_vec()),
            Some(refused.clone()),
        ));
        let vault = adapter(&script);
        let scopes = check_scope("trading data").unwrap();
        assert_eq!(
            (&vault).take_code(&own_key()).map(|_| ()),
            Err(ConnectError::VaultUnavailable)
        );
        assert_eq!(
            (&vault).put_token(&own_key(), token(TOKEN), &scopes),
            Err(ConnectError::VaultUnavailable)
        );
        assert_eq!(
            (&vault).client_secret().map(|_| ()),
            Err(ConnectError::VaultUnavailable),
            "{refused:?}"
        );
    }
}

#[test]
fn the_exchange_never_writes_a_code_or_deletes() {
    let script = Script::waiting();
    let vault = adapter(&script);
    assert_eq!(
        (&vault).put_code(
            &own_key(),
            AuthorizationCode(SecretString::from(CODE)),
            PkceVerifier(SecretString::from(VERIFIER)),
        ),
        Err(ConnectError::VaultUnavailable)
    );
    assert_eq!(
        (&vault).delete_code(&own_key()),
        Err(ConnectError::VaultUnavailable)
    );
    assert_eq!(
        (&vault).delete(&own_key()),
        Err(ConnectError::VaultUnavailable)
    );
    assert!(script.calls().is_empty());
}
