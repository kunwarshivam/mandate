//! The start and the callback's `state` (connections spec §5.2 steps 1 to 3; DEC-821 item 3).

use secrecy::{ExposeSecret, SecretString};

use super::support::{VERIFIER, at, client};

/// Alpaca's page with the fixture's client.
fn authorizer() -> Authorizer {
    Authorizer {
        page: AuthorizePage::alpaca(),
        client: client(),
    }
}
use crate::ConnectError;
use crate::record::ConnectionId;
use crate::start::{
    AUTHORIZE_URL, AuthorizePage, Authorizer, Binding, ClientId, Environment, OAuthClient,
    PendingStates, PkceVerifier, RedirectUri, STATE_LIFETIME_SECS,
};

fn connection(id: &str) -> ConnectionId {
    ConnectionId::new(id).unwrap()
}

fn binding(user: &str, environment: Environment) -> Binding {
    Binding {
        workspace_id: "ws-1".to_owned(),
        user_id: user.to_owned(),
        connection_id: connection("conn_1"),
        environment,
    }
}

fn verifier() -> PkceVerifier {
    PkceVerifier(SecretString::from(VERIFIER))
}

fn issued_at_1000() -> PendingStates {
    let mut states = PendingStates::default();
    states
        .begin(
            &authorizer(),
            binding("u1", Environment::Paper),
            "st-1",
            verifier(),
            "chal",
            at(1000),
        )
        .unwrap();
    states
}

fn redeem(states: &mut PendingStates, user: &str, secs: i64) -> Result<Binding, ConnectError> {
    states.redeem("st-1", user, at(secs)).map(|r| r.binding)
}

#[test]
fn the_authorization_url_asks_for_paper_and_exactly_trading_and_data() {
    let mut states = PendingStates::default();
    let url = states
        .begin(
            &authorizer(),
            binding("u1", Environment::Paper),
            "st-1",
            verifier(),
            "chal",
            at(1000),
        )
        .unwrap();
    let (base, query) = url.0.split_once('?').unwrap();
    assert_eq!(base, "https://app.alpaca.markets/oauth/authorize");
    let mut pairs: Vec<&str> = query.split('&').collect();
    pairs.sort_unstable();
    let mut expected = vec![
        "response_type=code",
        "client_id=client-1",
        "redirect_uri=https%3A%2F%2Fapi.owlhead.ai%2Fv1%2Foauth%2Falpaca%2Fcallback",
        "state=st-1",
        "code_challenge=chal",
        "code_challenge_method=S256",
        "scope=trading%20data",
        "env=paper",
    ];
    expected.sort_unstable();
    assert_eq!(pairs, expected);
    assert!(!url.0.contains(VERIFIER), "the verifier never leaves");
}

#[test]
fn a_live_connection_is_refused_and_issues_no_state() {
    let mut states = PendingStates::default();
    let started = states.begin(
        &authorizer(),
        binding("u1", Environment::Live),
        "st-1",
        verifier(),
        "chal",
        at(1000),
    );
    assert_eq!(started, Err(ConnectError::EnvironmentRefused));
    assert_eq!(
        redeem(&mut states, "u1", 1001),
        Err(ConnectError::StateUnknown)
    );
}

#[test]
fn a_state_is_redeemed_once() {
    let mut states = issued_at_1000();
    assert_eq!(
        redeem(&mut states, "u1", 1010),
        Ok(binding("u1", Environment::Paper))
    );
    assert_eq!(
        redeem(&mut states, "u1", 1011),
        Err(ConnectError::StateUnknown)
    );
}

#[test]
fn a_state_expires_and_is_spent() {
    let last_good = 1000 + STATE_LIFETIME_SECS - 1;
    let mut fresh = issued_at_1000();
    assert_eq!(
        redeem(&mut fresh, "u1", last_good),
        Ok(binding("u1", Environment::Paper))
    );
    let mut stale = issued_at_1000();
    assert_eq!(
        redeem(&mut stale, "u1", last_good + 1),
        Err(ConnectError::StateExpired)
    );
    assert_eq!(
        redeem(&mut stale, "u1", last_good),
        Err(ConnectError::StateUnknown)
    );
}

#[test]
fn another_users_redeem_is_refused_and_spends_the_state() {
    let mut states = issued_at_1000();
    assert_eq!(
        redeem(&mut states, "u2", 1010),
        Err(ConnectError::StateUserMismatch)
    );
    assert_eq!(
        redeem(&mut states, "u1", 1011),
        Err(ConnectError::StateUnknown)
    );
}

#[test]
fn an_unknown_state_is_refused() {
    let mut states = issued_at_1000();
    assert_eq!(
        states.redeem("st-2", "u1", at(1010)).map(|r| r.binding),
        Err(ConnectError::StateUnknown)
    );
    assert_eq!(
        redeem(&mut states, "u1", 1011),
        Ok(binding("u1", Environment::Paper))
    );
}

fn begin_with(
    states: &mut PendingStates,
    state: &str,
    (workspace, user): (&str, &str),
    verifier: &str,
    secs: i64,
) -> Result<(), ConnectError> {
    let binding = Binding {
        workspace_id: workspace.to_owned(),
        user_id: user.to_owned(),
        connection_id: connection(&format!("conn_{workspace}")),
        environment: Environment::Paper,
    };
    let verifier = PkceVerifier(SecretString::from(verifier));
    states
        .begin(&authorizer(), binding, state, verifier, "chal", at(secs))
        .map(|_| ())
}

fn redeemed(
    states: &mut PendingStates,
    state: &str,
    user: &str,
    secs: i64,
) -> Result<(String, String, String), ConnectError> {
    states.redeem(state, user, at(secs)).map(|r| {
        let verifier = r.verifier.0.expose_secret().to_owned();
        (r.binding.workspace_id, r.binding.user_id, verifier)
    })
}

fn owned(workspace: &str, user: &str, verifier: &str) -> (String, String, String) {
    (workspace.to_owned(), user.to_owned(), verifier.to_owned())
}

#[test]
fn each_state_returns_the_workspace_and_verifier_bound_to_it_at_begin() {
    let mut states = PendingStates::default();
    begin_with(&mut states, "st-a", ("ws-a", "u1"), "verifier-a", 1000).unwrap();
    begin_with(&mut states, "st-b", ("ws-b", "u2"), "verifier-b", 1000).unwrap();
    assert_eq!(
        redeemed(&mut states, "st-b", "u2", 1010),
        Ok(owned("ws-b", "u2", "verifier-b"))
    );
    assert_eq!(
        redeemed(&mut states, "st-a", "u1", 1010),
        Ok(owned("ws-a", "u1", "verifier-a"))
    );
}

#[test]
fn a_state_already_issued_is_refused_and_the_first_binding_is_kept() {
    let mut states = PendingStates::default();
    begin_with(&mut states, "st-1", ("ws-1", "u1"), "verifier-a", 1000).unwrap();
    assert_eq!(
        begin_with(&mut states, "st-1", ("ws-2", "u2"), "verifier-b", 1010),
        Err(ConnectError::StateReused)
    );
    assert_eq!(
        redeemed(&mut states, "st-1", "u1", 1010),
        Ok(owned("ws-1", "u1", "verifier-a"))
    );
}

#[test]
fn an_expired_unredeemed_state_is_still_never_reissued() {
    let mut states = PendingStates::default();
    begin_with(&mut states, "st-1", ("ws-1", "u1"), "verifier-a", 1000).unwrap();
    let later = 1000 + STATE_LIFETIME_SECS + 60;
    assert_eq!(
        begin_with(&mut states, "st-1", ("ws-1", "u1"), "verifier-b", later),
        Err(ConnectError::StateReused)
    );
}

#[test]
fn an_expired_state_redeemed_by_another_user_is_refused_and_spent() {
    let mut states = PendingStates::default();
    begin_with(&mut states, "st-1", ("ws-1", "u1"), "verifier-a", 1000).unwrap();
    let later = 1000 + STATE_LIFETIME_SECS;
    let refused = redeemed(&mut states, "st-1", "u2", later);
    assert!(
        matches!(
            refused,
            Err(ConnectError::StateExpired | ConnectError::StateUserMismatch)
        ),
        "{refused:?}"
    );
    assert_eq!(
        redeemed(&mut states, "st-1", "u1", 1010),
        Err(ConnectError::StateUnknown)
    );
}

#[test]
fn reserved_characters_in_the_state_and_client_id_are_percent_encoded() {
    let mut states = PendingStates::default();
    let url = states
        .begin(
            &Authorizer {
                page: AuthorizePage::alpaca(),
                client: OAuthClient {
                    client_id: ClientId("id&x=y z+w".to_owned()),
                    redirect_uri: RedirectUri::production(),
                },
            },
            binding("u1", Environment::Paper),
            "a&b=c d+e",
            verifier(),
            "chal",
            at(1000),
        )
        .unwrap();
    let (_, query) = url.0.split_once('?').unwrap();
    let mut pairs: Vec<&str> = query
        .split('&')
        .filter(|pair| pair.starts_with("state=") || pair.starts_with("client_id="))
        .collect();
    pairs.sort_unstable();
    assert_eq!(
        pairs,
        ["client_id=id%26x%3Dy%20z%2Bw", "state=a%26b%3Dc%20d%2Be"]
    );
}

#[test]
fn the_verifier_prints_no_secret() {
    assert_eq!(format!("{:?}", verifier()), "PkceVerifier(redacted)");
}

#[test]
fn the_state_returns_the_connection_it_was_issued_for() {
    let mut states = PendingStates::default();
    begin_with(&mut states, "st-a", ("ws-a", "u1"), "verifier-a", 1000).unwrap();
    begin_with(&mut states, "st-b", ("ws-b", "u1"), "verifier-b", 1000).unwrap();
    let redeemed = states.redeem("st-b", "u1", at(1010)).unwrap();
    assert_eq!(redeemed.binding.connection_id, connection("conn_ws-b"));
    assert_eq!(states.len(), 1);
    assert!(!states.is_empty());
}

const ACCEPTED_REDIRECTS: [&str; 7] = [
    "https://api.owlhead.ai/v1/oauth/alpaca/callback",
    "https://api-staging.owlhead.ai/v1/oauth/alpaca/callback",
    "https://api.example.test:8443/v1/oauth/alpaca/callback",
    "http://localhost:8080/v1/oauth/alpaca/callback",
    "http://127.0.0.1/v1/oauth/alpaca/callback",
    "http://[::1]:9000/v1/oauth/alpaca/callback",
    "http://[::1]/v1/oauth/alpaca/callback",
];

const REFUSED_REDIRECTS: [&str; 23] = [
    "",
    "http://api.owlhead.ai/v1/oauth/alpaca/callback",
    "http://localhost.evil.test/v1/oauth/alpaca/callback",
    "http://127.0.0.2.evil.test/v1/oauth/alpaca/callback",
    "HTTPS://api.owlhead.ai/v1/oauth/alpaca/callback",
    "ftp://api.owlhead.ai/v1/oauth/alpaca/callback",
    "https:///v1/oauth/alpaca/callback",
    "https://api.owlhead.ai/v1/oauth/alpaca/callback/",
    "https://api.owlhead.ai/v1/oauth/alpaca/callbacks",
    "https://api.owlhead.ai/v1/workspaces/ws-1/oauth/alpaca/callback",
    "https://api.owlhead.ai/v1/oauth/alpaca/callback?ws=1",
    "https://api.owlhead.ai/v1/oauth/alpaca/callback#f",
    "https://user@api.owlhead.ai/v1/oauth/alpaca/callback",
    "https://api.owlhead.ai/v1/oauth/alpaca/../alpaca/callback",
    "https://api owlhead.ai/v1/oauth/alpaca/callback",
    "https://api.owlhead.ai:/v1/oauth/alpaca/callback",
    "https://api.owlhead.ai:99999/v1/oauth/alpaca/callback",
    " https://api.owlhead.ai/v1/oauth/alpaca/callback",
    "https://api.owlhead.ai:0/v1/oauth/alpaca/callback",
    "https://api.owlhead.ai:+443/v1/oauth/alpaca/callback",
    "https://[::1]/v1/oauth/alpaca/callback",
    "http://[::2]/v1/oauth/alpaca/callback",
    "http://[::1/v1/oauth/alpaca/callback",
];

#[test]
fn a_redirect_uri_is_an_https_origin_or_a_loopback_http_one_with_the_callback_path() {
    for text in ACCEPTED_REDIRECTS {
        assert_eq!(
            RedirectUri::parse(text).map(|uri| uri.as_str().to_owned()),
            Ok(text.to_owned()),
            "{text}"
        );
    }
    for text in REFUSED_REDIRECTS {
        assert_eq!(
            RedirectUri::parse(text),
            Err(ConnectError::RedirectUriInvalid),
            "{text:?}"
        );
    }
    assert_eq!(
        RedirectUri::parse(ACCEPTED_REDIRECTS[0]),
        Ok(RedirectUri::production())
    );
}

#[test]
fn the_authorization_url_carries_the_deployments_own_redirect_uri() {
    let staging = OAuthClient {
        client_id: ClientId("client-1".to_owned()),
        redirect_uri: RedirectUri::parse(ACCEPTED_REDIRECTS[1]).unwrap(),
    };
    let mut states = PendingStates::default();
    let url = states
        .begin(
            &Authorizer {
                page: AuthorizePage::alpaca(),
                client: staging,
            },
            binding("u1", Environment::Paper),
            "st-1",
            verifier(),
            "chal",
            at(1000),
        )
        .unwrap();
    assert!(
        url.0.contains(
            "&redirect_uri=https%3A%2F%2Fapi-staging.owlhead.ai%2Fv1%2Foauth%2Falpaca%2Fcallback&"
        ),
        "{}",
        url.0
    );
    assert!(!url.0.contains("api.owlhead.ai"), "{}", url.0);
}

#[test]
fn pruning_drops_only_the_expired_states() {
    let mut states = PendingStates::default();
    begin_with(&mut states, "st-old", ("ws-a", "u1"), "verifier-a", 1000).unwrap();
    begin_with(&mut states, "st-new", ("ws-b", "u1"), "verifier-b", 1300).unwrap();
    states.prune(at(1000 + STATE_LIFETIME_SECS - 1));
    assert_eq!(states.len(), 2, "nothing has expired yet");
    states.prune(at(1000 + STATE_LIFETIME_SECS));
    assert_eq!(states.len(), 1, "the first expires at its lifetime");
    assert_eq!(
        redeemed(&mut states, "st-old", "u1", 1001),
        Err(ConnectError::StateUnknown)
    );
    assert_eq!(
        redeemed(&mut states, "st-new", "u1", 1301),
        Ok(owned("ws-b", "u1", "verifier-b"))
    );
    states.prune(at(1300 + STATE_LIFETIME_SECS));
    assert!(states.is_empty());
}

#[test]
fn holding_a_state_neither_spends_it_nor_outlives_it() {
    let mut states = PendingStates::default();
    begin_with(&mut states, "st-a", ("ws-a", "u1"), "verifier-a", 1000).unwrap();
    assert!(states.holds("st-a", at(1000)), "held when issued");
    assert!(
        states.holds("st-a", at(1000 + STATE_LIFETIME_SECS - 1)),
        "held to its last second"
    );
    assert!(
        !states.holds("st-a", at(1000 + STATE_LIFETIME_SECS)),
        "not held once expired"
    );
    assert!(
        !states.holds("st-b", at(1000)),
        "an unknown state is not held"
    );
    assert_eq!(
        redeemed(&mut states, "st-a", "u1", 1001),
        Ok(owned("ws-a", "u1", "verifier-a")),
        "holding spends nothing"
    );
    assert!(
        !states.holds("st-a", at(1001)),
        "a redeemed state is not held"
    );
}

/// Alpaca's page is the constant, and the authorization URL is on the authorizer's page, its query
/// after one `?` (DEC-2633 item 3).
#[test]
#[cfg(debug_assertions)]
fn the_authorization_url_is_on_the_authorizer_s_page() {
    assert_eq!(AuthorizePage::alpaca().as_str(), AUTHORIZE_URL);
    let page = AuthorizePage::for_cell("http://oauth-fixture:8091/alpaca/oauth/authorize").unwrap();
    let mut states = PendingStates::default();
    let url = states
        .begin(
            &Authorizer {
                page,
                client: client(),
            },
            binding("u1", Environment::Paper),
            "st-cell",
            verifier(),
            "chal",
            at(1000),
        )
        .unwrap();
    let (on, query) = url.0.split_once('?').unwrap();
    assert_eq!(on, "http://oauth-fixture:8091/alpaca/oauth/authorize");
    assert!(query.contains("state=st-cell"), "{query}");
}

/// A cell's page is a loopback host, or exactly the fixture pod over plain `http`, with a path and
/// nothing else, and never an Alpaca host (DEC-2633 items 1 and 3), kept exactly as given.
#[test]
#[cfg(debug_assertions)]
fn a_cell_page_is_loopback_or_exactly_the_fixture_pod() {
    for admitted in [
        "http://127.0.0.1:8091/alpaca/oauth/authorize",
        "https://localhost/oauth/authorize",
        "http://[::1]:9000/a",
        "http://oauth-fixture:8091/alpaca/oauth/authorize",
    ] {
        assert_eq!(
            AuthorizePage::for_cell(admitted).map(|page| page.as_str().to_owned()),
            Some(admitted.to_owned()),
            "{admitted}"
        );
    }
    for refused in [
        "",
        "https://app.alpaca.markets/oauth/authorize",
        "http://evil.example/oauth/authorize",
        "https://oauth-fixture:8091/alpaca/oauth/authorize",
        "http://oauth-fixture/alpaca/oauth/authorize",
        "http://oauth-fixture:8092/alpaca/oauth/authorize",
        "http://OAUTH-FIXTURE:8091/alpaca/oauth/authorize",
        "http://oauth-fixture.:8091/alpaca/oauth/authorize",
        "http://127.0.0.1:0/a",
        "http://127.0.0.1:080/a",
        "http://127.0.0.1:65536/a",
        "http://127.0.0.1",
        "http://127.0.0.1/",
        "http://127.0.0.1/a?x=1",
        "http://127.0.0.1/a#f",
        "http://user@127.0.0.1/a",
        "ftp://127.0.0.1/a",
    ] {
        assert_eq!(AuthorizePage::for_cell(refused), None, "{refused}");
    }
}
