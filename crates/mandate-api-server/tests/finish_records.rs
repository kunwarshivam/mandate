//! The records the connect's finish appends (DEC-2342 item 9): each draft is one the journal's
//! own draft check admits, with exactly journal spec §9.8's members, the system actor, the derived
//! event id and the cause item 8 gives.

use mandate_api_server::finish_records::{ACTOR_ID, drafted, event_id};
use mandate_canon::{is_ulid, parse};
use mandate_connections::manager::{ManagerEffect, RefusedAs, RequestMembers, StepUpEvidence};
use mandate_connections::record::{AccountRef, Broker, ConnectionId, Environment};
use mandate_journal::Draft;
use mandate_time::UtcNanos;
use serde_json::{Value, json};

const WORKSPACE: &str = "ws_01JB3K6F0C9R2V7N4M8Q1T5W3Z";
const CONNECTION: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W50";
const OTHER: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W51";
const ACCOUNT: &str = "01JB3K6F0C9R2V7N4M8Q1T5W60";
const CHECK: &str = "01JB3K6F0C9R2V7N4M8Q1T5W99";

fn members(account_ref: &str) -> RequestMembers {
    RequestMembers {
        connection_id: ConnectionId::new(CONNECTION).unwrap(),
        account_ref: AccountRef::new(account_ref).unwrap(),
        broker: Broker::Alpaca,
        environment: Environment::Paper,
        user: "01JB3K6F0C9R2V7N4M8Q1T5W3Z".to_owned(),
        step_up: StepUpEvidence {
            assertion_id: "asrt-1".to_owned(),
            authenticated_at: UtcNanos::from_parts(1_791_000_000, 0).unwrap(),
            method: "passkey".to_owned(),
        },
    }
}

fn now() -> UtcNanos {
    UtcNanos::from_parts(1_791_000_100, 0).unwrap()
}

fn established() -> ManagerEffect {
    ManagerEffect::Established {
        request: members(ACCOUNT),
        scopes: ["data".to_owned(), "trading".to_owned()].into(),
    }
}

fn refused(
    check: Option<&'static str>,
    reason: &'static str,
    existing: Option<&str>,
) -> ManagerEffect {
    ManagerEffect::ConnectRefused {
        request: members(ACCOUNT),
        refused: RefusedAs {
            check,
            reason,
            existing_connection_id: existing.map(|id| ConnectionId::new(id).unwrap()),
        },
    }
}

/// The draft as JSON, after checking the journal admits it.
fn admitted(draft: &[u8]) -> Value {
    Draft::parse(draft).expect("the journal admits the draft");
    let parsed = parse(draft).expect("canonical");
    assert_eq!(
        mandate_canon::to_canonical(&parsed),
        draft,
        "canonical form"
    );
    serde_json::from_slice(draft).unwrap()
}

fn step_up() -> Value {
    json!({
        "assertion_id": "asrt-1",
        "authenticated_at": UtcNanos::from_parts(1_791_000_000, 0).unwrap().to_string(),
        "method": "passkey",
    })
}

#[test]
fn an_establishment_is_version_2_caused_by_the_passing_check() {
    let draft = admitted(&drafted(&established(), Some(CHECK), WORKSPACE, now()).unwrap());
    assert_eq!(draft["event_type"], "ConnectionEstablished");
    assert_eq!(draft["schema_version"], 2);
    assert_eq!(draft["stream_id"], format!("ctl:{WORKSPACE}"));
    assert_eq!(draft["environment"], "paper");
    assert_eq!(draft["causation_id"], CHECK);
    assert_eq!(draft["event_time"], now().to_string());
    assert_eq!(
        draft["payload"],
        json!({
            "account_ref": ACCOUNT,
            "broker": "alpaca",
            "connection_id": CONNECTION,
            "environment": "paper",
            "margin_attestation": null,
            "scopes": ["data", "trading"],
            "step_up": step_up(),
            "user": "01JB3K6F0C9R2V7N4M8Q1T5W3Z",
        })
    );
}

#[test]
fn the_finish_writes_as_the_connection_manager() {
    let draft = admitted(&drafted(&established(), Some(CHECK), WORKSPACE, now()).unwrap());
    assert_eq!(draft["actor"]["kind"], "system");
    assert_eq!(draft["actor"]["id"], ACTOR_ID);
    assert_eq!(ACTOR_ID, "connection_manager");
    assert_eq!(draft["event_id"], event_id(&members(ACCOUNT)).unwrap());
}

#[test]
fn a_refusal_carries_its_check_reason_and_the_request_s_members() {
    let draft = admitted(
        &drafted(
            &refused(Some("scope"), "scope_mismatch", None),
            Some(CHECK),
            WORKSPACE,
            now(),
        )
        .unwrap(),
    );
    assert_eq!(draft["event_type"], "ConnectionRefused");
    assert_eq!(draft["schema_version"], 1);
    assert_eq!(draft["causation_id"], CHECK);
    assert_eq!(
        draft["payload"],
        json!({
            "broker": "alpaca",
            "check": "scope",
            "connection_id": CONNECTION,
            "environment": "paper",
            "existing_connection_id": null,
            "occasion": "connect",
            "reason": "scope_mismatch",
            "step_up": step_up(),
            "user": "01JB3K6F0C9R2V7N4M8Q1T5W3Z",
        })
    );
}

/// A teardown, check 3 and check 4 refuse uncaused; check 4 names the holder (rules 54, 55, 65).
#[test]
fn the_uncaused_refusals_are_admitted() {
    for (effect, existing) in [
        (refused(None, "timeout", None), Value::Null),
        (refused(None, "restart_past_deadline", None), Value::Null),
        (
            refused(Some("account"), "account_mismatch", None),
            Value::Null,
        ),
        (
            refused(Some("uniqueness"), "already_connected", Some(OTHER)),
            json!(OTHER),
        ),
    ] {
        let draft = admitted(&drafted(&effect, None, WORKSPACE, now()).unwrap());
        assert_eq!(draft["causation_id"], Value::Null, "{effect:?}");
        assert_eq!(
            draft["payload"]["existing_connection_id"], existing,
            "{effect:?}"
        );
    }
}

/// The journal's check is a real oracle here: a teardown given a cause breaks rule 65.
#[test]
fn a_caused_teardown_is_one_the_journal_refuses() {
    let draft = drafted(
        &refused(None, "timeout", None),
        Some(CHECK),
        WORKSPACE,
        now(),
    )
    .unwrap();
    assert!(Draft::parse(&draft).is_err());
}

#[test]
fn the_event_id_is_the_request_s_own() {
    let first = event_id(&members(ACCOUNT)).unwrap();
    assert!(is_ulid(&first), "{first}");
    assert_eq!(
        first,
        event_id(&members(ACCOUNT)).unwrap(),
        "the same each time"
    );
    assert_ne!(
        first,
        event_id(&members("01JB3K6F0C9R2V7N4M8Q1T5W61")).unwrap(),
        "another request, another id"
    );
    let refusal =
        admitted(&drafted(&refused(None, "timeout", None), None, WORKSPACE, now()).unwrap());
    assert_eq!(refusal["event_id"], first, "whichever record closes it");
}

/// The derivation's known vector, computed outside the crate: the ULID of the first 128 bits of
/// the SHA-256 of `{"account_ref":"01JB3K6F0C9R2V7N4M8Q1T5W60","connection_id":
/// "conn_01JB3K6F0C9R2V7N4M8Q1T5W50","operation":"connect_finish"}`. A retried finish, after a
/// restart or a deploy, must write the same event id, so the derivation never drifts.
#[test]
fn the_event_id_matches_its_known_vector() {
    assert_eq!(
        event_id(&members(ACCOUNT)).unwrap(),
        "3YDBVFBYB1Z871QCDAYQ1DYF81"
    );
}

#[test]
fn the_finish_never_drafts_a_request() {
    assert_eq!(
        drafted(
            &ManagerEffect::Requested(members(ACCOUNT)),
            None,
            WORKSPACE,
            now()
        ),
        None
    );
}
