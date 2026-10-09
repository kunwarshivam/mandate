//! The code exchange runs only for paper, sends the one live-host request, and keeps the token
//! in the vault alone (connections spec §5.2 step 4; DEC-821 items 2, 3 and 4).

use std::collections::BTreeSet;

use secrecy::SecretString;

use super::support::{
    CANARIES, CODE, Call, FixtureProvider, FixtureVault, SECRET, TOKEN, VERIFIER, client, key,
};
use crate::ConnectError;
use crate::exchange::{ClientSecret, ExchangedGrant, PendingConnect, exchange_code};
use crate::grant::GrantedScopes;
use crate::hosts::LiveTokenRequest;
use crate::start::{Environment, REDIRECT_URI};

fn pending(environment: Environment) -> PendingConnect {
    PendingConnect {
        connection_id: "conn-1".to_owned(),
        environment,
        vault_key: key(),
    }
}

fn secret() -> ClientSecret {
    ClientSecret(SecretString::from(SECRET))
}

fn run(
    environment: Environment,
    vault: &mut FixtureVault,
    provider: &mut FixtureProvider,
) -> Result<ExchangedGrant, ConnectError> {
    exchange_code(&pending(environment), &client(), &secret(), vault, provider)
}

#[test]
fn a_live_connection_never_reaches_the_vault_or_the_token_endpoint() {
    let mut vault = FixtureVault::with_code();
    let mut provider = FixtureProvider::granting("bearer", "trading data");
    assert_eq!(
        run(Environment::Live, &mut vault, &mut provider),
        Err(ConnectError::EnvironmentRefused)
    );
    assert!(provider.calls.is_empty());
    assert!(vault.ops.is_empty());
}

#[test]
fn a_paper_exchange_sends_the_one_live_token_request_and_stores_only_the_token() {
    let mut vault = FixtureVault::with_code();
    let mut provider = FixtureProvider::granting("bearer", "trading data");
    let grant = run(Environment::Paper, &mut vault, &mut provider).unwrap();
    assert_eq!(
        provider.calls,
        vec![Call {
            request: LiveTokenRequest::token_endpoint(),
            code: CODE.to_owned(),
            code_verifier: VERIFIER.to_owned(),
            client_id: "client-1".to_owned(),
            client_secret: SECRET.to_owned(),
            redirect_uri: REDIRECT_URI.to_owned(),
        }]
    );
    assert_eq!(
        grant,
        ExchangedGrant {
            connection_id: "conn-1".to_owned(),
            environment: Environment::Paper,
            scopes: GrantedScopes(BTreeSet::from(["data".to_owned(), "trading".to_owned()])),
            vault_key: key(),
        }
    );
    assert_eq!(vault.token(), Some(TOKEN.to_owned()));
    assert_eq!(
        vault.scopes(),
        Some(GrantedScopes(BTreeSet::from([
            "data".to_owned(),
            "trading".to_owned()
        ]))),
        "the granted scope is stored with the token"
    );
    assert!(!vault.has_code(), "the code and verifier are deleted");
    assert_eq!(
        vault.ops,
        ["read_code", "put_token", "delete_code"],
        "read once, store the token, then delete the code and verifier"
    );
    assert_eq!(
        run(Environment::Paper, &mut vault, &mut provider),
        Err(ConnectError::CodeMissing)
    );
    assert_eq!(provider.calls.len(), 1, "a spent code is never sent again");
}

#[test]
fn with_no_code_waiting_nothing_is_sent_or_written() {
    let mut vault = FixtureVault::default();
    let mut provider = FixtureProvider::granting("bearer", "trading data");
    assert_eq!(
        run(Environment::Paper, &mut vault, &mut provider),
        Err(ConnectError::CodeMissing)
    );
    assert!(provider.calls.is_empty());
    assert_eq!(vault.ops.first(), Some(&"read_code"));
    assert_eq!(vault.writes(), Vec::<&str>::new());
    assert_eq!(vault.token(), None);
    assert!(!vault.has_code());
}

#[test]
fn a_refused_grant_stores_no_token_and_deletes_the_entry() {
    let answers = [
        (
            Ok(("bearer", "trading data account:write")),
            ConnectError::ScopeMismatch,
        ),
        (Ok(("bearer", "trading")), ConnectError::ScopeMismatch),
        (Ok(("mac", "trading data")), ConnectError::TokenType),
        (
            Err(ConnectError::EndpointFailed),
            ConnectError::EndpointFailed,
        ),
    ];
    for (answer, refusal) in answers {
        let mut vault = FixtureVault::with_code();
        let mut provider = FixtureProvider {
            answer,
            calls: Vec::new(),
        };
        assert_eq!(
            run(Environment::Paper, &mut vault, &mut provider),
            Err(refusal.clone())
        );
        assert_eq!(vault.token(), None, "{refusal:?}");
        assert_eq!(provider.calls.len(), 1, "never retried: {refusal:?}");
        assert_eq!(vault.ops, ["read_code", "delete"], "{refusal:?}");
        assert_eq!(vault.writes(), Vec::<&str>::new(), "{refusal:?}");
        assert!(!vault.has_code(), "{refusal:?}");
        assert_eq!(vault.ops.last(), Some(&"delete"), "{refusal:?}");
    }
}

#[test]
fn no_secret_reaches_the_grant_or_an_error() {
    let mut vault = FixtureVault::with_code();
    let mut provider = FixtureProvider::granting("bearer", "trading data");
    let grant = run(Environment::Paper, &mut vault, &mut provider).unwrap();
    let mut printed = format!("{grant:?}");
    for answer in [
        Ok(("bearer", "trading data account:write")),
        Ok(("mac", "trading data")),
        Err(ConnectError::EndpointFailed),
    ] {
        let mut vault = FixtureVault::with_code();
        let mut provider = FixtureProvider {
            answer,
            calls: Vec::new(),
        };
        let error = run(Environment::Paper, &mut vault, &mut provider).unwrap_err();
        printed.push_str(&format!("{error:?} {error}"));
    }
    for canary in CANARIES {
        assert!(!printed.contains(canary), "{printed}");
    }
}
