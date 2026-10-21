//! The connect start over HTTP (§4.5, DEC-2623, DEC-2621 items 1 and 4): what one admitted start
//! commits and answers, the `state` it issues, what never leaves, and retries under one key.

mod common;
mod support_connect;

use mandate_api::problem::ProblemCode;
use std::sync::Mutex;

use common::Scripted;
use mandate_canon::{Value, parse, to_canonical};
use mandate_connections::ConnectError;
use mandate_connections::start::Environment;
use mandate_identity::StepUpActionKind;
use support_connect::{
    ALICE, Counting, DRAWS, KEY, T0, appended, at, b64, base, challenge, code, ctl, derived,
    digest, fixture, fixture_random, json, signed_in, start, text, with,
};

/// The authorization URL's query pairs, sorted.
fn pairs(url: &str) -> Vec<String> {
    let (page, query) = url.split_once('?').expect("a query");
    assert_eq!(page, "https://app.alpaca.markets/oauth/authorize");
    let mut pairs: Vec<String> = query.split('&').map(str::to_owned).collect();
    pairs.sort();
    pairs
}

/// The `state` and verifier the start drew, whichever order it drew them in, from the URL.
fn drawn(url: &str) -> (String, String) {
    let state = pairs(url)
        .into_iter()
        .find_map(|pair| pair.strip_prefix("state=").map(str::to_owned))
        .expect("a state");
    let [first, second] = DRAWS.map(|draw| b64(&draw));
    let verifier = if state == first { second } else { first };
    assert!(
        state == b64(&DRAWS[0]) || state == b64(&DRAWS[1]),
        "{state}"
    );
    (state, verifier)
}

fn admitted() -> (support_connect::Fixture, Value) {
    let f = fixture("paper", true, true);
    let cookie = signed_in(&f, ALICE);
    let reply = start(&f, Some(&cookie), Some(KEY), &to_canonical(&base()));
    assert_eq!(
        reply.status,
        201,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    assert_eq!(reply.header("Content-Type"), Some("application/json"));
    (f, json(&reply))
}

#[test]
fn a_start_answers_the_pending_connection_and_its_authorization_url() {
    let (_f, body) = admitted();
    let connection = format!("conn_{}", derived("connect_connection"));
    assert_eq!(text(&body, &["connection_id"]), connection);
    assert_eq!(text(&body, &["expires_at"]), at(T0 + 600).to_string());
    let url = text(&body, &["authorization_url"]);
    let (state, verifier) = drawn(url);
    let mut expected = vec![
        "response_type=code".to_owned(),
        "client_id=client-1".to_owned(),
        "redirect_uri=https%3A%2F%2Fapi.owlhead.ai%2Fv1%2Foauth%2Falpaca%2Fcallback".to_owned(),
        format!("state={state}"),
        format!("code_challenge={}", challenge(&verifier)),
        "code_challenge_method=S256".to_owned(),
        "scope=trading%20data".to_owned(),
        "env=paper".to_owned(),
    ];
    expected.sort();
    assert_eq!(pairs(url), expected);
}

#[test]
fn a_start_commits_connection_requested_version_two_with_the_step_up_it_spent() {
    let (f, _) = admitted();
    let rows = appended(&f);
    assert_eq!(rows.len(), 1, "one record");
    let row = &rows[0];
    assert_eq!(
        (row.event_type.as_str(), row.schema_version),
        ("ConnectionRequested", 2)
    );
    assert_eq!(row.event_id, derived("connect"));
    let payload = parse(&row.body)
        .expect("JSON")
        .get("payload")
        .cloned()
        .expect("a payload");
    let expected = format!(
        r#"{{"account_ref":"{}","breadth":{{"disclosure":{{"document":"alpaca_oauth_token_breadth","version":"1"}},"other_environment":"may_reach"}},"broker":"alpaca","connection_id":"conn_{}","credential":"oauth","environment":"paper","step_up":{},"user":"{ALICE}"}}"#,
        derived("connect_account"),
        derived("connect_connection"),
        String::from_utf8(to_canonical(base().get("step_up").expect("evidence"))).expect("UTF-8"),
    );
    assert_eq!(
        to_canonical(&payload),
        to_canonical(&parse(expected.as_bytes()).expect("JSON"))
    );
    let asked = f.port.asked.lock().expect("unpoisoned").clone();
    assert_eq!(asked.len(), 1);
    assert_eq!(
        (asked[0].kind, asked[0].digest),
        (StepUpActionKind::Connection, digest("1"))
    );
}

#[test]
fn the_state_redeems_once_to_this_connection_this_admin_and_paper() {
    let (f, body) = admitted();
    let (state, _) = drawn(text(&body, &["authorization_url"]));
    let connect = f.connect.as_ref().expect("offered");
    let mut states = connect.states.lock().expect("unpoisoned");
    let redeemed = states.redeem(&state, ALICE, at(T0 + 1)).expect("issued");
    assert_eq!(
        redeemed.binding.workspace_id,
        support_connect::ctl().trim_start_matches("ctl:")
    );
    assert_eq!(redeemed.binding.user_id, ALICE);
    assert_eq!(redeemed.binding.environment, Environment::Paper);
    assert_eq!(
        redeemed.binding.connection_id.as_str(),
        text(&body, &["connection_id"])
    );
    assert_eq!(
        states.redeem(&state, ALICE, at(T0 + 2)).map(|_| ()),
        Err(ConnectError::StateUnknown)
    );
}

#[test]
fn neither_the_state_nor_the_verifier_reaches_the_journal_and_the_verifier_never_leaves() {
    let (f, body) = admitted();
    let (state, verifier) = drawn(text(&body, &["authorization_url"]));
    let answered = String::from_utf8(to_canonical(&body)).expect("UTF-8");
    assert!(
        !answered.contains(&verifier),
        "the verifier stays in the process"
    );
    for row in f.journal.rows_of(&support_connect::ctl()) {
        let stored = String::from_utf8_lossy(&row.body).into_owned();
        assert!(
            !stored.contains(&state) && !stored.contains(&verifier),
            "{stored}"
        );
    }
    let secret_shaped = [
        "token",
        "secret",
        "password",
        "credential",
        "verifier",
        "vault",
        "fingerprint",
    ];
    for name in body.as_object().expect("an object").keys() {
        assert!(
            !secret_shaped.iter().any(|s| name.as_str().contains(s)),
            "{}",
            name.as_str()
        );
    }
}

#[test]
fn a_retry_while_the_state_is_held_answers_the_same_201_and_records_once() {
    let (f, first) = admitted();
    let cookie = signed_in(&f, ALICE);
    let again = start(&f, Some(&cookie), Some(KEY), &to_canonical(&base()));
    assert_eq!(again.status, 201);
    assert_eq!(json(&again), first);
    assert_eq!(appended(&f).len(), 1);
}

#[test]
fn a_retry_after_the_state_is_redeemed_or_expired_finds_the_key_spent() {
    let (redeemed, body) = admitted();
    let (state, _) = drawn(text(&body, &["authorization_url"]));
    let connect = redeemed.connect.as_ref().expect("offered");
    connect
        .states
        .lock()
        .expect("unpoisoned")
        .redeem(&state, ALICE, at(T0 + 1))
        .expect("issued");
    let cookie = signed_in(&redeemed, ALICE);
    let again = start(&redeemed, Some(&cookie), Some(KEY), &to_canonical(&base()));
    assert_eq!(code(&again), (409, ProblemCode::IdempotencyConflict));

    let (expired, _) = admitted();
    expired.clock.set(T0 + 600);
    let cookie = signed_in(&expired, ALICE);
    let again = start(&expired, Some(&cookie), Some(KEY), &to_canonical(&base()));
    assert_eq!(code(&again), (409, ProblemCode::IdempotencyConflict));
    assert_eq!(
        appended(&redeemed).len() + appended(&expired).len(),
        2,
        "nothing more recorded"
    );
}

#[test]
fn another_body_under_the_key_conflicts_and_records_nothing() {
    let (f, _) = admitted();
    let cookie = signed_in(&f, ALICE);
    let other = with(&["environment"], r#""live""#);
    assert_eq!(
        code(&start(&f, Some(&cookie), Some(KEY), &other)),
        (409, ProblemCode::IdempotencyConflict)
    );
    assert_eq!(appended(&f).len(), 1);
}

/// A restart holds no `state`: a fresh store refuses the old one, and a retry finds the key spent,
/// so a restart fails closed (DEC-2623 item 4).
#[test]
fn after_a_restart_the_old_state_is_unknown_and_the_key_spent() {
    let (f, body) = admitted();
    let (state, _) = drawn(text(&body, &["authorization_url"]));
    let connect = f.connect.as_ref().expect("offered");
    *connect.states.lock().expect("unpoisoned") =
        mandate_connections::start::PendingStates::default();
    let redeemed = connect
        .states
        .lock()
        .expect("unpoisoned")
        .redeem(&state, ALICE, at(T0 + 1))
        .map(|_| ());
    assert_eq!(redeemed, Err(ConnectError::StateUnknown));
    let cookie = signed_in(&f, ALICE);
    let again = start(&f, Some(&cookie), Some(KEY), &to_canonical(&base()));
    assert_eq!(code(&again), (409, ProblemCode::IdempotencyConflict));
}

/// The commit is pinned to the head the route read and judged: when the control stream moves
/// between that read and the append, nothing is committed and no `state` is issued, and the start
/// answers `journal_unavailable` (DEC-2623 item 6; rule 131 judged at the head it read).
#[test]
fn a_head_that_moves_after_the_read_commits_nothing_and_issues_no_state() {
    let f = fixture("paper", true, true);
    f.journal.script(&[Scripted::HeadMismatch]);
    let cookie = signed_in(&f, ALICE);
    let reply = start(&f, Some(&cookie), Some(KEY), &to_canonical(&base()));
    assert_eq!(code(&reply), (503, ProblemCode::JournalUnavailable));
    assert!(appended(&f).is_empty(), "nothing committed");
    let held = f
        .connect
        .as_ref()
        .expect("offered")
        .states
        .lock()
        .expect("unpoisoned")
        .len();
    assert_eq!(held, 0, "no state issued");
    let appends = f.journal.appends.lock().expect("unpoisoned").clone();
    assert_eq!(
        appends
            .iter()
            .map(|(stream, head, _)| (stream.clone(), *head))
            .collect::<Vec<_>>(),
        vec![(ctl(), 1)],
        "one attempt, at the head read, never retried at a fresh one"
    );
}

/// Two starts under one key that each read the stream before either commit is visible: the
/// second's append finds the first's identical record. One request never has two `state`s: the
/// second answers the first's `201`, and only one `state` is held (DEC-2623 items 7 and 8).
#[test]
fn a_concurrent_start_under_the_same_key_issues_no_second_state() {
    let f = fixture_random("paper", true, true, Box::new(Counting(Mutex::new(0))));
    let cookie = signed_in(&f, ALICE);
    let body = to_canonical(&base());
    let first = start(&f, Some(&cookie), Some(KEY), &body);
    assert_eq!(
        first.status,
        201,
        "{}",
        String::from_utf8_lossy(&first.body)
    );
    let committed = f
        .journal
        .streams
        .lock()
        .expect("unpoisoned")
        .get_mut(&ctl())
        .expect("the control stream")
        .pop()
        .expect("the first start's record");
    assert_eq!(committed.event_type, "ConnectionRequested");
    f.journal.script(&[Scripted::RaceIdentical]);
    let second = start(&f, Some(&cookie), Some(KEY), &body);
    assert_eq!(
        second.status,
        201,
        "{}",
        String::from_utf8_lossy(&second.body)
    );
    assert_eq!(json(&second), json(&first), "the first start's answer");
    let held = f
        .connect
        .as_ref()
        .expect("offered")
        .states
        .lock()
        .expect("unpoisoned")
        .len();
    assert_eq!(held, 1, "one state for one request");
    assert_eq!(appended(&f).len(), 1, "one record");
}
