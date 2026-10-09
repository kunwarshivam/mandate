//! The start and the callback's `state` (connections spec §5.2 steps 1 to 3; DEC-821 item 3).

use secrecy::{ExposeSecret, SecretString};

use super::support::{VERIFIER, at, client};
use crate::ConnectError;
use crate::start::{
    Binding, ClientId, Environment, PendingStates, PkceVerifier, STATE_LIFETIME_SECS,
};

fn binding(user: &str, environment: Environment) -> Binding {
    Binding {
        workspace_id: "ws-1".to_owned(),
        user_id: user.to_owned(),
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
            &client(),
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
#[ignore = "pending E10-13"]
fn the_authorization_url_asks_for_paper_and_exactly_trading_and_data() {
    let mut states = PendingStates::default();
    let url = states
        .begin(
            &client(),
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
#[ignore = "pending E10-13"]
fn a_live_connection_is_refused_and_issues_no_state() {
    let mut states = PendingStates::default();
    let started = states.begin(
        &client(),
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
#[ignore = "pending E10-13"]
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
#[ignore = "pending E10-13"]
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
#[ignore = "pending E10-13"]
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
#[ignore = "pending E10-13"]
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
        environment: Environment::Paper,
    };
    let verifier = PkceVerifier(SecretString::from(verifier));
    states
        .begin(&client(), binding, state, verifier, "chal", at(secs))
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
#[ignore = "pending E10-13"]
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
#[ignore = "pending E10-13"]
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
#[ignore = "pending E10-13"]
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
#[ignore = "pending E10-13"]
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
#[ignore = "pending E10-13"]
fn reserved_characters_in_the_state_and_client_id_are_percent_encoded() {
    let mut states = PendingStates::default();
    let url = states
        .begin(
            &ClientId("id&x=y z+w".to_owned()),
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
