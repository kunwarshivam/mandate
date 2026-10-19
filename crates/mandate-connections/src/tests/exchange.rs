//! The code exchange runs only for paper, sends the one live-host request, and keeps the token
//! in the vault alone (connections spec §5.2 step 4; DEC-821 items 2, 3 and 4).

use std::collections::BTreeSet;

use proptest::prelude::*;
use proptest::test_runner::TestRunner;
use secrecy::SecretString;

use super::support::{
    CANARIES, CODE, Call, FixtureProvider, FixtureVault, SECRET, TOKEN, VERIFIER, client, key,
};
use crate::ConnectError;
use crate::exchange::{ClientSecret, ExchangedGrant, PendingConnect, exchange_code};
use crate::grant::GrantedScopes;
use crate::hosts::LiveTokenRequest;
use crate::start::{ClientId, Environment, OAuthClient, RedirectUri};

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
            redirect_uri: RedirectUri::production().as_str().to_owned(),
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
        ["take_code", "put_token"],
        "take the code and verifier in one step, then store the token (DEC-2628 item 2)"
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
    assert_eq!(vault.ops.first(), Some(&"take_code"));
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
        assert_eq!(
            vault.ops,
            ["take_code"],
            "taken before the request, so nothing is left to delete: {refusal:?}"
        );
        assert_eq!(vault.writes(), Vec::<&str>::new(), "{refusal:?}");
        assert!(!vault.has_code(), "{refusal:?}");
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

#[test]
fn the_exchange_sends_the_redirect_uri_the_deployment_registered() {
    let staging = OAuthClient {
        client_id: ClientId("client-1".to_owned()),
        redirect_uri: RedirectUri::parse("https://api-staging.owlhead.ai/v1/oauth/alpaca/callback")
            .unwrap(),
    };
    let mut vault = FixtureVault::with_code();
    let mut provider = FixtureProvider::granting("bearer", "trading data");
    exchange_code(
        &pending(Environment::Paper),
        &staging,
        &secret(),
        &mut vault,
        &mut provider,
    )
    .unwrap();
    let sent: Vec<&str> = provider
        .calls
        .iter()
        .map(|call| call.redirect_uri.as_str())
        .collect();
    assert_eq!(
        sent,
        vec!["https://api-staging.owlhead.ai/v1/oauth/alpaca/callback"]
    );
}

/// A run whose take fails sends nothing: the code is sent only once it is gone from
/// the vault, so a later run is the first to send it (DEC-2628 item 2).
#[test]
fn a_code_that_cannot_be_taken_is_never_sent() {
    let mut vault = FixtureVault::with_code();
    vault.failing = Some("take_code");
    let mut provider = FixtureProvider::granting("bearer", "trading data");
    assert_eq!(
        run(Environment::Paper, &mut vault, &mut provider),
        Err(ConnectError::VaultUnavailable)
    );
    assert!(provider.calls.is_empty(), "nothing sent before the take");
    assert_eq!(vault.token(), None);
    vault.failing = None;
    assert!(run(Environment::Paper, &mut vault, &mut provider).is_ok());
    assert_eq!(
        provider.calls.len(),
        1,
        "the code is sent once, by the run that took it"
    );
}

/// A run that stops after taking the code and before its request is answered loses the
/// connect: nothing is stored, and a rerun finds no code and sends nothing. The request stays open
/// for the deadline teardown (connections spec §5.2 step 6; `manager`'s teardown tests).
#[test]
fn a_run_lost_after_the_take_leaves_nothing_for_a_rerun_to_send() {
    let mut vault = FixtureVault::with_code();
    let mut provider = FixtureProvider {
        answer: Err(ConnectError::EndpointFailed),
        calls: Vec::new(),
    };
    assert_eq!(
        run(Environment::Paper, &mut vault, &mut provider),
        Err(ConnectError::EndpointFailed)
    );
    assert!(!vault.has_code());
    provider.answer = Ok(("bearer", "trading data"));
    assert_eq!(
        run(Environment::Paper, &mut vault, &mut provider),
        Err(ConnectError::CodeMissing)
    );
    assert_eq!(provider.calls.len(), 1, "no second exchange");
    assert_eq!(vault.token(), None);
}

/// A run that stops after its request is answered and before the token is stored loses the
/// connect too: no token, no code, and a rerun sends nothing.
#[test]
fn a_run_lost_after_the_request_and_before_the_token_never_exchanges_again() {
    let mut vault = FixtureVault::with_code();
    vault.failing = Some("put_token");
    let mut provider = FixtureProvider::granting("bearer", "trading data");
    assert_eq!(
        run(Environment::Paper, &mut vault, &mut provider),
        Err(ConnectError::VaultUnavailable)
    );
    assert_eq!(provider.calls.len(), 1);
    assert!(!vault.has_code(), "taken before the request");
    vault.failing = None;
    assert_eq!(
        run(Environment::Paper, &mut vault, &mut provider),
        Err(ConnectError::CodeMissing)
    );
    assert_eq!(provider.calls.len(), 1, "no second exchange");
    assert_eq!(vault.token(), None);
}

/// One run of the exchange: the vault operation that fails, if any, and whether the endpoint
/// answers with a grant.
fn runs() -> impl Strategy<Value = Vec<(Option<&'static str>, bool)>> {
    let failing = prop_oneof![Just(None), Just(Some("take_code")), Just(Some("put_token")),];
    prop::collection::vec((failing, any::<bool>()), 1..6)
}

/// Over any sequence of runs, each stopping anywhere, the code is sent at most once and only by a
/// run that took it first, a token is stored only by a run that returned it, and no run after
/// the code is taken sends anything. The oracle follows the code by its own state, not by the
/// exchange's order (DEC-2628 item 2).
#[test]
fn over_any_sequence_of_stopped_runs_the_code_is_sent_at_most_once() {
    TestRunner::deterministic()
        .run(&runs(), |schedule| {
            let mut vault = FixtureVault::with_code();
            let mut provider = FixtureProvider::granting("bearer", "trading data");
            let mut code_waiting = true;
            let mut sent = 0_usize;
            let mut stored = false;
            for (failing, grants) in schedule {
                vault.failing = failing;
                provider.answer = if grants {
                    Ok(("bearer", "trading data"))
                } else {
                    Err(ConnectError::EndpointFailed)
                };
                let result = run(Environment::Paper, &mut vault, &mut provider);
                let consumed = code_waiting && failing != Some("take_code");
                if consumed {
                    code_waiting = false;
                    sent += 1;
                }
                let stores = consumed && grants && failing != Some("put_token");
                stored |= stores;
                prop_assert_eq!(result.is_ok(), stores);
                prop_assert_eq!(provider.calls.len(), sent);
                prop_assert!(sent <= 1);
                prop_assert_eq!(vault.has_code(), code_waiting);
                prop_assert_eq!(vault.token().is_some(), stored);
            }
            Ok(())
        })
        .unwrap();
}
