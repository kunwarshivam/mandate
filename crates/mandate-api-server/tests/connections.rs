//! The connections read's fold (§4.5, DEC-2620), against control-stream rows built by hand, and
//! against an independent oracle that reads each connection backwards from its last decisive
//! record rather than folding forwards.

use mandate_api_server::connections::{ConnectionState, ConnectionView, Refusal, connections};
use mandate_canon::{Digest, parse, to_canonical};
use mandate_journal::StoredEvent;
use proptest::prelude::{any, prop};
use proptest::test_runner::{Config, TestRunner};

const CTL: &str = "ctl:01JB3K6F0C9R2V7N4M8Q1T5W3Y";
const C1: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W50";
const C2: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W51";
const STEP_UP: &str = r#"{"assertion_id":"sa_1","authenticated_at":"2026-10-19T10:00:00.000000000Z","method":"passkey"}"#;

fn at(seq: u64) -> String {
    format!("2026-10-19T10:{:02}:{:02}.000000000Z", seq / 60, seq % 60)
}

fn row(seq: u64, event_type: &str, version: u64, payload: &str) -> StoredEvent {
    let event_id = format!("01JBH3BTZ8K2M4N6P8R0S2T{seq:03}");
    let text = format!(
        r#"{{"environment":"paper","event_id":"{event_id}","event_type":"{event_type}","payload":{payload},"stream_id":"{CTL}"}}"#
    );
    let body = to_canonical(&parse(text.as_bytes()).expect("JSON"));
    StoredEvent {
        stream_id: CTL.to_owned(),
        seq,
        event_id,
        event_type: event_type.to_owned(),
        schema_version: version,
        environment: "paper".to_owned(),
        recorded_at: at(seq),
        prev_hash: Digest::ZERO,
        hash: Digest::of(&body),
        body,
    }
}

/// A row whose body is not JSON at all.
fn unparsed(seq: u64) -> StoredEvent {
    let mut unparsed = requested(seq, "conn_unparsed");
    unparsed.body = b"not json".to_vec();
    unparsed.hash = Digest::of(&unparsed.body);
    unparsed
}

fn requested(seq: u64, id: &str) -> StoredEvent {
    row(
        seq,
        "ConnectionRequested",
        1,
        &format!(
            r#"{{"connection_id":"{id}","account_ref":"01JB3K6F0C9R2V7N4M8Q1T5W60","broker":"alpaca","environment":"paper","user":"usr_1","step_up":{STEP_UP}}}"#
        ),
    )
}

fn established(seq: u64, id: &str, scopes: &str) -> StoredEvent {
    row(
        seq,
        "ConnectionEstablished",
        2,
        &format!(
            r#"{{"connection_id":"{id}","broker":"alpaca","environment":"paper","scopes":{scopes},"account_ref":"01JB3K6F0C9R2V7N4M8Q1T5W60","user":"usr_1","step_up":{STEP_UP},"margin_attestation":null}}"#
        ),
    )
}

fn refused(seq: u64, id: &str, occasion: &str, check: &str, reason: &str) -> StoredEvent {
    row(
        seq,
        "ConnectionRefused",
        1,
        &format!(
            r#"{{"connection_id":"{id}","broker":"alpaca","environment":"paper","occasion":"{occasion}","check":{check},"reason":"{reason}","existing_connection_id":null,"user":"usr_1","step_up":{STEP_UP}}}"#
        ),
    )
}

fn revoked_v1(seq: u64, id: &str) -> StoredEvent {
    row(
        seq,
        "ConnectionRevoked",
        1,
        &format!(r#"{{"connection_id":"{id}"}}"#),
    )
}

fn revoked(seq: u64, id: &str, reason: &str) -> StoredEvent {
    row(
        seq,
        "ConnectionRevoked",
        2,
        &format!(r#"{{"connection_id":"{id}","reason":"{reason}","step_up":{STEP_UP}}}"#),
    )
}

fn rotated(seq: u64, id: &str) -> StoredEvent {
    row(
        seq,
        "ConnectionCredentialRotated",
        1,
        &format!(r#"{{"connection_id":"{id}","user":"usr_1","step_up":{STEP_UP}}}"#),
    )
}

fn deployed(seq: u64, version: u64, agent: &str, id: &str) -> StoredEvent {
    let named = if version == 2 {
        format!(r#","connection_id":"{id}""#)
    } else {
        String::new()
    };
    row(
        seq,
        "AgentDeployed",
        version,
        &format!(
            r#"{{"agent_id":"{agent}","mandate_version":"sha256:{}","record_ref":"sha256:{}"{named}}}"#,
            "ef".repeat(32),
            "ab".repeat(32)
        ),
    )
}

fn stopped(seq: u64, agent: &str) -> StoredEvent {
    row(
        seq,
        "AgentStopped",
        1,
        &format!(r#"{{"agent_id":"{agent}"}}"#),
    )
}

fn view(id: &str, state: ConnectionState, since: u64) -> ConnectionView {
    ConnectionView {
        connection_id: id.to_owned(),
        broker: "alpaca".to_owned(),
        environment: "paper".to_owned(),
        state,
        scopes: None,
        since: at(since),
        refusal: None,
        revoked_reason: None,
        agents: Vec::new(),
    }
}

fn scopes() -> Option<Vec<String>> {
    Some(vec!["trading".to_owned(), "account:read".to_owned()])
}

const SCOPES: &str = r#"["trading","account:read"]"#;

#[test]
fn a_request_is_connecting_and_its_establishment_sets_the_scopes() {
    assert_eq!(
        connections(&[requested(1, C1)]),
        vec![view(C1, ConnectionState::Connecting, 1)]
    );
    let mut expected = view(C1, ConnectionState::Established, 2);
    expected.scopes = scopes();
    assert_eq!(
        connections(&[requested(1, C1), established(2, C1, SCOPES)]),
        vec![expected]
    );
}

#[test]
fn a_connect_refusal_is_refused_with_its_check_and_reason() {
    let mut expected = view(C1, ConnectionState::Refused, 2);
    expected.refusal = Some(Refusal {
        check: Some("scope".to_owned()),
        reason: "scope_mismatch".to_owned(),
    });
    let rows = [
        requested(1, C1),
        refused(2, C1, "connect", r#""scope""#, "scope_mismatch"),
    ];
    assert_eq!(connections(&rows), vec![expected]);
    let mut teardown = view(C1, ConnectionState::Refused, 2);
    teardown.refusal = Some(Refusal {
        check: None,
        reason: "timeout".to_owned(),
    });
    assert_eq!(
        connections(&[
            requested(1, C1),
            refused(2, C1, "connect", "null", "timeout")
        ]),
        vec![teardown]
    );
}

#[test]
fn a_refused_reconnect_or_reauthorize_and_a_rotation_leave_the_state_as_it_was() {
    let base = [requested(1, C1), established(2, C1, SCOPES)];
    let mut expected = view(C1, ConnectionState::Established, 2);
    expected.scopes = scopes();
    for extra in [
        rotated(3, C1),
        refused(3, C1, "reauthorize", r#""scope""#, "scope_mismatch"),
    ] {
        let rows = [base[0].clone(), base[1].clone(), extra];
        assert_eq!(connections(&rows), vec![expected.clone()]);
    }
    let mut gone = view(C1, ConnectionState::Revoked, 3);
    gone.scopes = scopes();
    gone.revoked_reason = Some("owner".to_owned());
    let rows = [
        requested(1, C1),
        established(2, C1, SCOPES),
        revoked(3, C1, "owner"),
        refused(4, C1, "reconnect", r#""account""#, "account_unreadable"),
    ];
    assert_eq!(connections(&rows), vec![gone]);
}

#[test]
fn a_revocation_names_its_reason_at_version_two_and_none_at_version_one() {
    for (row, reason) in [
        (revoked(3, C1, "compromised"), Some("compromised")),
        (revoked_v1(3, C1), None),
    ] {
        let mut expected = view(C1, ConnectionState::Revoked, 3);
        expected.scopes = scopes();
        expected.revoked_reason = reason.map(str::to_owned);
        assert_eq!(
            connections(&[requested(1, C1), established(2, C1, SCOPES), row]),
            vec![expected]
        );
    }
}

#[test]
fn a_later_request_never_moves_a_connection_back() {
    let mut expected = view(C1, ConnectionState::Revoked, 2);
    expected.revoked_reason = Some("owner".to_owned());
    assert_eq!(
        connections(&[requested(1, C1), revoked(2, C1, "owner"), requested(3, C1)]),
        vec![expected]
    );
}

#[test]
fn only_a_request_an_establishment_or_a_connect_refusal_names_a_connection() {
    let rows = [
        revoked(1, C2, "owner"),
        rotated(2, C2),
        refused(3, C2, "reconnect", "null", "timeout"),
        unparsed(4),
        row(5, "ConnectionRequested", 1, r#"{"broker":"alpaca"}"#),
        requested(6, C1),
    ];
    assert_eq!(
        connections(&rows),
        vec![view(C1, ConnectionState::Connecting, 6)]
    );
}

#[test]
fn connections_are_listed_in_the_order_they_are_first_named() {
    let rows = [
        requested(1, C2),
        requested(2, C1),
        established(3, C2, SCOPES),
    ];
    let listed: Vec<String> = connections(&rows)
        .into_iter()
        .map(|c| c.connection_id)
        .collect();
    assert_eq!(listed, vec![C2.to_owned(), C1.to_owned()]);
}

#[test]
fn agents_are_those_whose_latest_deployment_names_it_at_version_two_and_is_not_stopped() {
    let rows = [
        requested(1, C1),
        established(2, C1, SCOPES),
        deployed(3, 2, "agt_b", C1),
        deployed(4, 2, "agt_a", C1),
        deployed(5, 1, "agt_v1", C1),
        deployed(6, 2, "agt_gone", C1),
        stopped(7, "agt_gone"),
        deployed(8, 2, "agt_moved", C1),
        deployed(9, 2, "agt_moved", C2),
    ];
    let agents = connections(&rows)
        .into_iter()
        .next()
        .expect("one connection")
        .agents;
    assert_eq!(agents, vec!["agt_b".to_owned(), "agt_a".to_owned()]);
}

/// The oracle: for each id, the last decisive record (an establishment, a connect refusal, a
/// revocation) decides the state, found by scanning backwards; with none, the first request.
fn oracle(rows: &[StoredEvent], picks: &[(u8, u8)]) -> Vec<(String, &'static str, String)> {
    let ids = [C1, C2];
    let mut first: Vec<(usize, &str)> = Vec::new();
    for (index, (kind, id)) in picks.iter().enumerate() {
        let id = ids[usize::from(*id)];
        if matches!(kind, 0..=2) && !first.iter().any(|(_, known)| *known == id) {
            first.push((index, id));
        }
    }
    first
        .into_iter()
        .map(|(start, id)| {
            let decisive = picks
                .iter()
                .enumerate()
                .rev()
                .find(|(index, (kind, pick))| {
                    *index >= start && ids[usize::from(*pick)] == id && matches!(kind, 1..=3)
                });
            let (index, state) = match decisive {
                Some((index, (1, _))) => (index, "established"),
                Some((index, (2, _))) => (index, "refused"),
                Some((index, _)) => (index, "revoked"),
                None => (start, "connecting"),
            };
            (id.to_owned(), state, rows[index].recorded_at.clone())
        })
        .collect()
}

fn rows_of(picks: &[(u8, u8)]) -> Vec<StoredEvent> {
    picks
        .iter()
        .enumerate()
        .map(|(index, (kind, id))| {
            let seq = u64::try_from(index).expect("small") + 1;
            let id = [C1, C2][usize::from(*id)];
            match kind {
                0 => requested(seq, id),
                1 => established(seq, id, SCOPES),
                2 => refused(seq, id, "connect", "null", "timeout"),
                3 => revoked(seq, id, "owner"),
                4 => rotated(seq, id),
                _ => refused(seq, id, "reauthorize", r#""scope""#, "scope_mismatch"),
            }
        })
        .collect()
}

#[test]
fn the_fold_agrees_with_a_backwards_reading() {
    let mut runner = TestRunner::new(Config::with_cases(512));
    let strategy = prop::collection::vec((0u8..6, any::<bool>()), 0..24);
    runner
        .run(&strategy, |picks| {
            let picks: Vec<(u8, u8)> = picks
                .into_iter()
                .map(|(kind, id)| (kind, u8::from(id)))
                .collect();
            let rows = rows_of(&picks);
            let folded: Vec<(String, &'static str, String)> = connections(&rows)
                .into_iter()
                .map(|c| (c.connection_id, c.state.code(), c.since))
                .collect();
            proptest::prop_assert_eq!(folded, oracle(&rows, &picks));
            Ok(())
        })
        .expect("the fold agrees with the oracle");
}
