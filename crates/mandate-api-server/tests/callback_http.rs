//! The OAuth callback over HTTP (§4.5, §4.10, DEC-2626): the session's user redeems the `state`
//! once, the code and verifier go to the vault for a request still open and nowhere else, and
//! every answer is a redirect that carries neither the secret nor the outcome.

mod common;
mod support_connect;
mod support_vault;

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use mandate_api::problem::ProblemCode;
use mandate_api_server::clock::Clock;
use mandate_api_server::connect::CodeVault;
use mandate_api_server::session::COOKIE_NAME;
use mandate_api_server::vault_codes::VaultCodes;
use mandate_canon::to_canonical;
use mandate_connections::code_entry;
use mandate_connections::record::ConnectionId;
use mandate_connections::start::{AuthorizePage, Authorizer, Binding, Environment, PkceVerifier};
use mandate_vault::policy::{Identity, SecretPath};
use secrecy::{ExposeSecret, SecretString};

use common::{ManualClock, Reply, WORKSPACE, send};
use support_connect::{
    ALICE, APP, BOB, DRAWS, Draws, Fixture, KEY, Put, T0, appended, at, b64, base, client, code,
    ctl, demote_alice, fixture, fixture_port, json, seed, signed_in, start, text,
};

const CALLBACK: &str = "/v1/oauth/alpaca/callback";

/// A code with characters the query must escape, and the text the vault must receive for it.
const CODE_QUERY: &str = "c0de-%2B%2F~CANARY";
const CODE_TEXT: &str = "c0de-+/~CANARY";

/// One admitted start in a fresh paper workspace: the fixture, Alice's cookie, the `state` and
/// verifier it drew, and the pending connection's id.
struct Started {
    f: Fixture,
    cookie: String,
    state: String,
    verifier: String,
    connection: String,
}

fn started() -> Started {
    started_in(fixture("paper", true, true))
}

fn started_in(f: Fixture) -> Started {
    let cookie = signed_in(&f, ALICE);
    let reply = start(&f, Some(&cookie), Some(KEY), &to_canonical(&base()));
    assert_eq!(
        reply.status,
        201,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    let body = json(&reply);
    let url = text(&body, &["authorization_url"]).to_owned();
    let state = url
        .split_once('?')
        .expect("a query")
        .1
        .split('&')
        .find_map(|pair| pair.strip_prefix("state="))
        .expect("a state")
        .to_owned();
    let [first, second] = DRAWS.map(|draw| b64(&draw));
    assert!(state == first || state == second, "{state}");
    let verifier = if state == first { second } else { first };
    let connection = text(&body, &["connection_id"]).to_owned();
    Started {
        f,
        cookie,
        state,
        verifier,
        connection,
    }
}

/// The callback with `query`, carrying `cookie` as the session's when given.
fn callback(f: &Fixture, cookie: Option<&str>, query: &str) -> Reply {
    let cookie = cookie.map(|c| format!("{COOKIE_NAME}={c}"));
    let mut headers = Vec::new();
    if let Some(cookie) = cookie.as_deref() {
        headers.push(("Cookie", cookie));
    }
    send(f.addr, "GET", &format!("{CALLBACK}?{query}"), &headers, b"")
}

/// The answer is a `303` to `location`, neither cached nor sent onward, with no body that
/// carries anything.
fn redirected(reply: &Reply, location: &str) {
    assert_eq!(
        reply.status,
        303,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    assert_eq!(reply.header("Location"), Some(location));
    assert_eq!(reply.header("Cache-Control"), Some("no-store"));
    assert_eq!(reply.header("Referrer-Policy"), Some("no-referrer"));
    assert!(reply.body.is_empty(), "a redirect carries no body");
}

fn to_connection(s: &Started) -> String {
    format!("{APP}/connections/{}", s.connection)
}

fn to_list() -> String {
    format!("{APP}/connections")
}

fn puts(f: &Fixture) -> Vec<Put> {
    f.codes.puts.lock().expect("unpoisoned").clone()
}

/// Whether the `state` is still held, unredeemed.
fn held(s: &Started) -> bool {
    let connect = s.f.connect.as_ref().expect("offered");
    let states = connect.states.lock().expect("unpoisoned");
    states.holds(&s.state, support_connect::at(s.f.clock.secs()))
}

#[test]
fn a_callback_writes_the_code_for_the_bound_connection_and_redirects_to_it() {
    let s = started();
    let rows_before = appended(&s.f).len();
    let reply = callback(
        &s.f,
        Some(&s.cookie),
        &format!("state={}&code={CODE_QUERY}&scope=trading%20data", s.state),
    );
    redirected(&reply, &to_connection(&s));
    assert_eq!(
        puts(&s.f),
        vec![Put {
            workspace_id: WORKSPACE.to_owned(),
            user_id: ALICE.to_owned(),
            connection_id: s.connection.clone(),
            environment: Environment::Paper,
            code: CODE_TEXT.to_owned(),
            verifier: s.verifier.clone(),
        }],
        "one write, of the decoded code and the verifier issued with the state"
    );
    assert_eq!(appended(&s.f).len(), rows_before, "nothing is journaled");
    assert!(!held(&s), "the state is spent");
    let answer = format!(
        "{:?}{}",
        reply.headers,
        String::from_utf8_lossy(&reply.body)
    );
    for secret in [CODE_TEXT, "CANARY", s.state.as_str(), s.verifier.as_str()] {
        assert!(!answer.contains(secret), "the answer carries no {secret}");
    }
    for row in
        s.f.journal
            .streams
            .lock()
            .expect("unpoisoned")
            .values()
            .flatten()
    {
        let body = String::from_utf8_lossy(&row.body);
        assert!(!body.contains("CANARY") && !body.contains(&s.state));
    }
}

#[test]
fn a_state_is_spent_by_its_first_callback() {
    let s = started();
    let query = format!("state={}&code=first", s.state);
    redirected(&callback(&s.f, Some(&s.cookie), &query), &to_connection(&s));
    let again = format!("state={}&code=second", s.state);
    redirected(&callback(&s.f, Some(&s.cookie), &again), &to_list());
    let written: Vec<String> = puts(&s.f).into_iter().map(|put| put.code).collect();
    assert_eq!(written, ["first"], "only the first callback writes");
}

#[test]
fn without_a_session_the_callback_is_unauthenticated_and_spends_nothing() {
    let s = started();
    let query = format!("state={}&code=abc", s.state);
    for cookie in [None, Some("not-a-session")] {
        let reply = callback(&s.f, cookie, &query);
        assert_eq!(
            code(&reply),
            (401, ProblemCode::Unauthenticated),
            "{cookie:?}"
        );
        assert!(puts(&s.f).is_empty(), "{cookie:?}");
        assert!(held(&s), "{cookie:?}: the state is not spent");
    }
    redirected(&callback(&s.f, Some(&s.cookie), &query), &to_connection(&s));
    assert_eq!(puts(&s.f).len(), 1, "the owner's own callback still writes");
}

#[test]
fn another_users_state_is_refused_and_spent() {
    let s = started();
    let bob = signed_in(&s.f, BOB);
    let query = format!("state={}&code=abc", s.state);
    redirected(&callback(&s.f, Some(&bob), &query), &to_list());
    assert!(!held(&s), "a found state is spent either way");
    redirected(&callback(&s.f, Some(&s.cookie), &query), &to_list());
    assert!(puts(&s.f).is_empty(), "neither callback writes");
}

#[test]
fn an_expired_or_unknown_state_writes_nothing_and_names_no_connection() {
    let s = started();
    redirected(
        &callback(&s.f, Some(&s.cookie), "state=never-issued&code=abc"),
        &to_list(),
    );
    assert!(held(&s), "another state is untouched");
    s.f.clock.set(T0 + 600);
    let cookie = signed_in(&s.f, ALICE);
    let query = format!("state={}&code=abc", s.state);
    redirected(&callback(&s.f, Some(&cookie), &query), &to_list());
    assert!(puts(&s.f).is_empty());
}

#[test]
fn a_declined_grant_writes_nothing_and_spends_the_state() {
    let s = started();
    let query = format!("error=access_denied&state={}&code=abc", s.state);
    redirected(&callback(&s.f, Some(&s.cookie), &query), &to_connection(&s));
    assert!(puts(&s.f).is_empty(), "a declined grant writes no code");
    assert!(!held(&s), "the state is spent");
}

#[test]
fn a_code_that_is_missing_repeated_or_malformed_writes_nothing() {
    let long = "a".repeat(513);
    for code_part in [
        String::new(),
        "code=".to_owned(),
        "code=a&code=b".to_owned(),
        format!("code={long}"),
        "code=a+b".to_owned(),
        "code=a%20b".to_owned(),
        "code=a%09b".to_owned(),
        "code=%C3%A9".to_owned(),
    ] {
        let s = started();
        let query = format!("state={}&{code_part}", s.state);
        redirected(&callback(&s.f, Some(&s.cookie), &query), &to_connection(&s));
        assert!(puts(&s.f).is_empty(), "{code_part}");
        assert!(!held(&s), "{code_part}: the state is spent");
    }
    let s = started();
    let longest = "b".repeat(512);
    let query = format!("state={}&code={longest}", s.state);
    redirected(&callback(&s.f, Some(&s.cookie), &query), &to_connection(&s));
    assert_eq!(puts(&s.f).len(), 1, "512 bytes is a code");
}

#[test]
fn a_repeated_state_or_a_malformed_query_redeems_nothing() {
    let s = started();
    for query in [
        format!("state={0}&state={0}&code=abc", s.state),
        format!("state={}&code=abc&x=%zz", s.state),
        format!("state={}&code=abc%", s.state),
        "code=abc".to_owned(),
    ] {
        redirected(&callback(&s.f, Some(&s.cookie), &query), &to_list());
        assert!(held(&s), "{query}: the state is not redeemed");
    }
    assert!(puts(&s.f).is_empty());
}

#[test]
fn a_request_closed_while_the_owner_was_at_alpaca_gets_no_code() {
    let s = started();
    seed(
        &s.f,
        "ConnectionRevoked",
        1,
        &format!(r#"{{"connection_id":"{}"}}"#, s.connection),
    );
    let query = format!("state={}&code=abc", s.state);
    redirected(&callback(&s.f, Some(&s.cookie), &query), &to_connection(&s));
    assert!(puts(&s.f).is_empty(), "a closed request gets no code");
    assert!(!held(&s), "the state is spent");
}

#[test]
fn a_failed_vault_write_still_redirects_and_appends_nothing() {
    let s = started();
    s.f.codes.failing.store(true, Ordering::SeqCst);
    let rows_before = appended(&s.f).len();
    let query = format!("state={}&code=abc", s.state);
    redirected(&callback(&s.f, Some(&s.cookie), &query), &to_connection(&s));
    assert_eq!(appended(&s.f).len(), rows_before);
    assert!(!held(&s), "the state is spent");
}

/// Through the API's real vault port (DEC-2338 item 3), whose write runs a runtime of its own: the
/// callback calls it off the server's runtime, and the code reaches the vault as format 1's entry.
#[test]
#[ignore = "pending E10-13"]
fn a_callback_writes_through_the_vault_port_off_the_server_s_runtime() {
    let script = support_vault::Script::new(Identity::Api);
    let vault = VaultCodes {
        login: support_vault::Login(Arc::clone(&script)),
        clock: ManualClock::at(T0) as Arc<dyn Clock>,
    };
    let s = started_in(fixture_port(
        "paper",
        true,
        true,
        Box::new(Draws(Mutex::new(0))),
        Some(Arc::new(vault) as Arc<dyn CodeVault>),
    ));
    let query = format!("state={}&code={CODE_QUERY}", s.state);
    redirected(&callback(&s.f, Some(&s.cookie), &query), &to_connection(&s));
    let calls = script.calls();
    let [support_vault::Call::Put(path, bytes)] = calls.as_slice() else {
        panic!("one put and no other call: {calls:?}")
    };
    assert_eq!(
        path,
        &SecretPath::Code(ConnectionId::new(&s.connection).expect("an id"))
    );
    let taken = code_entry::decode(bytes).expect("format 1's entry");
    assert_eq!(taken.code.0.expose_secret(), CODE_TEXT);
    assert_eq!(taken.verifier.0.expose_secret(), s.verifier);
}

#[test]
fn a_deployment_without_a_connect_answers_not_found_before_the_session() {
    let f = fixture("paper", false, true);
    let cookie = signed_in(&f, ALICE);
    for cookie in [None, Some(cookie.as_str())] {
        let reply = callback(&f, cookie, "state=abc&code=abc");
        assert_eq!(code(&reply), (404, ProblemCode::NotFound), "{cookie:?}");
    }
}

/// The session's user must still hold the broker-connection permission when the code arrives: an
/// admin demoted while at Alpaca gets no code written, and the `state` is spent (DEC-2626 item 3).
#[test]
fn an_admin_demoted_while_at_alpaca_gets_no_code() {
    let s = started();
    demote_alice(&s.f);
    let rows_before = appended(&s.f).len();
    let query = format!("state={}&code=abc", s.state);
    redirected(&callback(&s.f, Some(&s.cookie), &query), &to_connection(&s));
    assert!(
        puts(&s.f).is_empty(),
        "a demoted admin gets no code written"
    );
    assert_eq!(appended(&s.f).len(), rows_before, "nothing is journaled");
    assert!(!held(&s), "the state is spent");
}

/// A redeemed `state` bound to a workspace this deployment does not host writes nothing.
#[test]
fn a_binding_for_a_workspace_not_hosted_writes_nothing() {
    let f = fixture("paper", true, true);
    let cookie = signed_in(&f, ALICE);
    let connection = "conn_01JB3K6F0C9R2V7N4M8Q1T5W77";
    let binding = Binding {
        workspace_id: "01JB3K6F0C9R2V7N4M8Q1T5W40".to_owned(),
        user_id: ALICE.to_owned(),
        connection_id: ConnectionId::new(connection).unwrap(),
        environment: Environment::Paper,
    };
    f.connect
        .as_ref()
        .expect("offered")
        .states
        .lock()
        .unwrap()
        .begin(
            &Authorizer {
                page: AuthorizePage::alpaca(),
                client: client(),
            },
            binding,
            "foreign-state",
            PkceVerifier(SecretString::from("v".repeat(43))),
            "challenge",
            at(f.clock.secs()),
        )
        .unwrap();
    redirected(
        &callback(&f, Some(&cookie), "state=foreign-state&code=abc"),
        &format!("{APP}/connections/{connection}"),
    );
    assert!(puts(&f).is_empty(), "no code for a workspace not hosted");
}

/// A control stream that cannot be read writes nothing: neither the permission nor the open
/// request can be confirmed.
#[test]
fn an_unreadable_control_stream_writes_nothing() {
    let s = started();
    s.f.journal.failing.lock().unwrap().insert(ctl());
    let query = format!("state={}&code=abc", s.state);
    redirected(&callback(&s.f, Some(&s.cookie), &query), &to_connection(&s));
    assert!(puts(&s.f).is_empty());
    assert!(!held(&s), "the state is spent");
}
