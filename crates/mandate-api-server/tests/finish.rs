//! The connect's finish (DEC-2342 items 1, 2, 5 to 7): the open request folded from the control
//! stream, the executor's latest `connect` check read from the account stream, and what
//! `plan_finish` is asked for each, the deadline and the fingerprint port included.

use mandate_api_server::finish::{
    CONNECT_DEADLINE_SECS, Checked, Fingerprints, Judged, Pass, Request, Unbacked, Unreadable,
    checked, decide, request,
};
use mandate_canon::Digest;
use mandate_connections::checks::Reason;
use mandate_connections::manager::{
    FinishPlan, FinishRefusal, ManagerEffect, RefusedAs, RequestFold, RequestMembers,
};
use mandate_connections::record::{Broker, ConnectionId, Environment};
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;
use proptest::prelude::{Just, prop_oneof};
use proptest::strategy::Strategy;
use proptest::test_runner::{Config, TestRunner};
use serde_json::{Value, json};

const CONNECTION: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W50";
const OTHER: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W51";
const ACCOUNT: &str = "01JB3K6F0C9R2V7N4M8Q1T5W60";
const OPENED: i64 = 1_791_000_000;
const PII: &str = "pii_01JB3K6F0C9R2V7N4M8Q1T5W70";

fn connection(id: &str) -> ConnectionId {
    ConnectionId::new(id).unwrap()
}

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap()
}

fn stamp(secs: i64) -> String {
    let t = at(secs);
    format!("{t}")
}

/// One stored row of `kind` and `version` with `payload`, recorded at `secs`.
fn row(event_id: &str, kind: &str, version: u64, secs: i64, payload: Value) -> StoredEvent {
    StoredEvent {
        stream_id: "ctl:ws".to_owned(),
        seq: 1,
        event_id: event_id.to_owned(),
        event_type: kind.to_owned(),
        schema_version: version,
        environment: "paper".to_owned(),
        recorded_at: stamp(secs),
        prev_hash: Digest::of(b""),
        hash: Digest::of(event_id.as_bytes()),
        body: serde_json::to_vec(&json!({ "payload": payload })).unwrap(),
    }
}

fn step_up() -> Value {
    json!({
        "assertion_id": "asrt-1",
        "authenticated_at": "2026-10-22T09:00:00Z",
        "method": "passkey",
    })
}

fn requested(connection: &str, version: u64, credential: &str) -> StoredEvent {
    let mut payload = json!({
        "connection_id": connection,
        "account_ref": ACCOUNT,
        "broker": "alpaca",
        "environment": "paper",
        "user": "01JB3K6F0C9R2V7N4M8Q1T5W3Z",
        "step_up": step_up(),
    });
    if version == 2 {
        payload["credential"] = json!(credential);
        payload["breadth"] = Value::Null;
    }
    row("ev-req", "ConnectionRequested", version, OPENED, payload)
}

fn closed(kind: &str, connection: &str) -> StoredEvent {
    row(
        &format!("ev-{kind}"),
        kind,
        1,
        OPENED + 1,
        json!({ "connection_id": connection }),
    )
}

fn check(
    event_id: &str,
    connection: &str,
    occasion: &str,
    results: Value,
    pii: Value,
) -> StoredEvent {
    row(
        event_id,
        "ConnectionChecked",
        1,
        OPENED + 30,
        json!({
            "connection_id": connection,
            "occasion": occasion,
            "results": results,
            "account_pii_ref": pii,
            "risk_clock": "2026-10-22T09:00:30Z",
        }),
    )
}

fn passed() -> Value {
    json!([
        {"check": "account", "reason": null, "result": "passed"},
        {"check": "environment", "reason": null, "result": "passed"},
        {"check": "scope", "reason": null, "result": "passed"},
    ])
}

fn open_request() -> Request {
    request(
        &[requested(CONNECTION, 2, "oauth")],
        &connection(CONNECTION),
    )
}

fn members() -> RequestMembers {
    match open_request().fold {
        RequestFold::Open(members) => members,
        other => panic!("an open request, not {other:?}"),
    }
}

/// A fingerprint port that answers `judged` and records what it was asked.
struct Answers(Judged, std::cell::RefCell<Vec<String>>);

impl Fingerprints for Answers {
    fn judge(&self, request: &RequestMembers, account_pii_ref: &str) -> Judged {
        assert_eq!(request, &members());
        self.1.borrow_mut().push(account_pii_ref.to_owned());
        self.0.clone()
    }
}

fn answering(judged: Judged) -> Answers {
    Answers(judged, std::cell::RefCell::default())
}

fn refused(
    check: Option<&'static str>,
    reason: &'static str,
    existing: Option<&str>,
) -> FinishPlan {
    FinishPlan::Commit(vec![ManagerEffect::ConnectRefused {
        request: members(),
        refused: RefusedAs {
            check,
            reason,
            existing_connection_id: existing.map(connection),
        },
    }])
}

#[test]
fn an_oauth_version_2_request_is_open_from_its_record() {
    let request = open_request();
    assert_eq!(request.opened_at, Some(at(OPENED)));
    assert!(request.oauth);
    let members = members();
    assert_eq!(members.connection_id, connection(CONNECTION));
    assert_eq!(members.account_ref.as_str(), ACCOUNT);
    assert_eq!(members.user, "01JB3K6F0C9R2V7N4M8Q1T5W3Z");
    assert_eq!(members.step_up.assertion_id, "asrt-1");
    assert_eq!(members.step_up.method, "passkey");
}

#[test]
fn only_a_version_2_oauth_request_is_this_finish_s() {
    for row in [
        requested(CONNECTION, 1, ""),
        requested(CONNECTION, 2, "api_key"),
    ] {
        let request = request(&[row], &connection(CONNECTION));
        assert!(matches!(request.fold, RequestFold::Open(_)));
        assert!(!request.oauth);
    }
}

#[test]
fn the_record_that_closes_a_request_ends_it() {
    let id = connection(CONNECTION);
    for (kind, fold) in [
        ("ConnectionEstablished", RequestFold::NotOpen),
        ("ConnectionRefused", RequestFold::NotOpen),
        ("ConnectionRevoked", RequestFold::ClosedByRevoke),
    ] {
        let request = request(
            &[requested(CONNECTION, 2, "oauth"), closed(kind, CONNECTION)],
            &id,
        );
        assert_eq!(request.fold, fold, "{kind}");
        assert_eq!(request.opened_at, None, "{kind}");
        assert!(!request.oauth, "{kind}");
    }
    assert_eq!(request(&[], &id).fold, RequestFold::NotOpen);
    let reopened = request(
        &[
            requested(CONNECTION, 2, "oauth"),
            closed("ConnectionRefused", CONNECTION),
            requested(CONNECTION, 2, "oauth"),
        ],
        &id,
    );
    assert!(matches!(reopened.fold, RequestFold::Open(_)));
}

#[test]
fn another_connection_s_rows_leave_the_request_alone() {
    let request = request(
        &[
            requested(CONNECTION, 2, "oauth"),
            closed("ConnectionRefused", OTHER),
            closed("ConnectionRevoked", OTHER),
        ],
        &connection(CONNECTION),
    );
    assert!(matches!(request.fold, RequestFold::Open(_)));
}

#[test]
fn a_connection_row_it_cannot_read_leaves_the_fold_unanswered() {
    let id = connection(CONNECTION);
    let nameless = row(
        "ev-x",
        "ConnectionRefused",
        1,
        OPENED,
        json!({ "reason": "timeout" }),
    );
    let mut stepless = requested(CONNECTION, 2, "oauth");
    stepless.body = serde_json::to_vec(&json!({ "payload": {
        "connection_id": CONNECTION, "account_ref": ACCOUNT, "broker": "alpaca",
        "environment": "paper", "user": "u", "credential": "oauth", "breadth": null,
    }}))
    .unwrap();
    let mut unparsable = requested(CONNECTION, 2, "oauth");
    unparsable.body = b"not json".to_vec();
    for rows in [vec![nameless], vec![stepless], vec![unparsable]] {
        assert_eq!(request(&rows, &id).fold, RequestFold::CannotAnswer);
    }
}

#[test]
fn the_latest_connect_check_of_the_connection_is_read() {
    let id = connection(CONNECTION);
    assert_eq!(checked(&[], &id), Ok(None));
    let failed = json!([
        {"check": "account", "reason": null, "result": "passed"},
        {"check": "environment", "reason": "reaches_both", "result": "failed"},
        {"check": "scope", "reason": "scope_mismatch", "result": "failed"},
    ]);
    let rows = [
        check("ev-1", CONNECTION, "connect", failed, json!(PII)),
        check("ev-2", OTHER, "connect", passed(), json!(PII)),
        check("ev-3", CONNECTION, "reconnect", passed(), json!(PII)),
    ];
    assert_eq!(
        checked(&rows, &id),
        Ok(Some(Checked::Failed {
            event_id: "ev-1".to_owned(),
            reason: Reason::ReachesBoth,
        })),
        "the first failed result's reason; other connections and occasions ignored"
    );
    let later = [
        rows[0].clone(),
        check("ev-4", CONNECTION, "connect", passed(), json!(PII)),
    ];
    assert_eq!(
        checked(&later, &id),
        Ok(Some(Checked::Passed {
            event_id: "ev-4".to_owned(),
            pii_ref: PII.to_owned(),
        }))
    );
}

#[test]
#[ignore = "pending E10-13"]
fn a_check_that_omits_scope_environment_or_account_is_unreadable() {
    let id = connection(CONNECTION);
    for omitted in ["account", "environment", "scope"] {
        for failed in [false, true] {
            let results: Vec<Value> = ["account", "environment", "scope"]
                .into_iter()
                .filter(|name| *name != omitted)
                .map(|name| match (failed, name) {
                    (true, "environment") => {
                        json!({"check": name, "reason": "reaches_both", "result": "failed"})
                    }
                    (true, "scope") => {
                        json!({"check": name, "reason": "scope_mismatch", "result": "failed"})
                    }
                    _ => json!({"check": name, "reason": null, "result": "passed"}),
                })
                .collect();
            let row = check("ev-1", CONNECTION, "connect", json!(results), json!(PII));
            assert_eq!(
                checked(&[row], &id),
                Err(Unreadable),
                "rule 57: a check that omits {omitted} (failed: {failed}) cannot be read"
            );
        }
    }
}

#[test]
fn a_check_it_cannot_read_is_unreadable() {
    let id = connection(CONNECTION);
    let unknown = json!([{"check": "scope", "reason": "made_up", "result": "failed"}]);
    let reasonless = json!([{"check": "scope", "reason": null, "result": "failed"}]);
    for row in [
        check("ev-1", CONNECTION, "connect", passed(), Value::Null),
        check("ev-1", CONNECTION, "connect", json!([]), json!(PII)),
        check("ev-1", CONNECTION, "connect", unknown, json!(PII)),
        check("ev-1", CONNECTION, "connect", reasonless, json!(PII)),
    ] {
        assert_eq!(checked(&[row], &id), Err(Unreadable));
    }
}

#[test]
fn a_unique_passed_check_establishes_with_the_scopes_caused_by_the_check() {
    let port = answering(Judged::Unique);
    let pass = decide(
        &open_request(),
        Ok(Some(Checked::Passed {
            event_id: "ev-check".to_owned(),
            pii_ref: PII.to_owned(),
        })),
        &port,
        at(OPENED + 60),
        false,
    )
    .unwrap();
    assert_eq!(
        pass,
        Pass::Finish {
            plan: FinishPlan::Commit(vec![ManagerEffect::Established {
                request: members(),
                scopes: ["data".to_owned(), "trading".to_owned()].into(),
            }]),
            causation: Some("ev-check".to_owned()),
        }
    );
    assert_eq!(
        *port.1.borrow(),
        [PII.to_owned()],
        "judged by its reference"
    );
}

#[test]
fn a_failed_check_refuses_with_its_reason_caused_by_the_check() {
    let pass = decide(
        &open_request(),
        Ok(Some(Checked::Failed {
            event_id: "ev-check".to_owned(),
            reason: Reason::ScopeMismatch,
        })),
        &Unbacked,
        at(OPENED + 60),
        false,
    )
    .unwrap();
    assert_eq!(
        pass,
        Pass::Finish {
            plan: refused(Some("scope"), "scope_mismatch", None),
            causation: Some("ev-check".to_owned()),
        }
    );
}

#[test]
fn checks_3_and_4_refuse_uncaused() {
    let passed = || {
        Ok(Some(Checked::Passed {
            event_id: "ev-check".to_owned(),
            pii_ref: PII.to_owned(),
        }))
    };
    for (judged, plan) in [
        (
            Judged::AccountMismatch,
            refused(Some("account"), "account_mismatch", None),
        ),
        (
            Judged::AlreadyConnected(connection(OTHER)),
            refused(Some("uniqueness"), "already_connected", Some(OTHER)),
        ),
    ] {
        assert_eq!(
            decide(
                &open_request(),
                passed(),
                &answering(judged),
                at(OPENED + 60),
                false
            )
            .unwrap(),
            Pass::Finish {
                plan,
                causation: None
            }
        );
    }
}

/// With no check, or a passed one the port cannot judge, the request waits until its deadline,
/// then is refused as `timeout`, or `restart_past_deadline` on the first pass after a restart.
#[test]
fn without_an_answer_the_deadline_tears_down() {
    let unjudged = || {
        Ok(Some(Checked::Passed {
            event_id: "ev-check".to_owned(),
            pii_ref: PII.to_owned(),
        }))
    };
    let last_wait = OPENED + CONNECT_DEADLINE_SECS - 1;
    let deadline = OPENED + CONNECT_DEADLINE_SECS;
    for checked in [Ok(None), unjudged()] {
        assert_eq!(
            decide(
                &open_request(),
                checked.clone(),
                &Unbacked,
                at(last_wait),
                false
            )
            .unwrap(),
            Pass::Wait,
            "{checked:?}"
        );
        assert_eq!(
            decide(
                &open_request(),
                checked.clone(),
                &Unbacked,
                at(deadline),
                false
            )
            .unwrap(),
            Pass::Finish {
                plan: refused(None, "timeout", None),
                causation: None
            },
            "{checked:?}"
        );
        assert_eq!(
            decide(
                &open_request(),
                checked.clone(),
                &Unbacked,
                at(deadline),
                true
            )
            .unwrap(),
            Pass::Finish {
                plan: refused(None, "restart_past_deadline", None),
                causation: None
            },
            "{checked:?}"
        );
    }
}

#[test]
fn a_request_that_is_not_this_finish_s_waits_even_past_its_deadline() {
    let api_key = request(
        &[requested(CONNECTION, 2, "api_key")],
        &connection(CONNECTION),
    );
    assert_eq!(
        decide(&api_key, Ok(None), &Unbacked, at(OPENED + 10_000), true).unwrap(),
        Pass::Wait
    );
}

#[test]
fn an_unreadable_check_or_fold_appends_nothing() {
    assert_eq!(
        decide(
            &open_request(),
            Err(Unreadable),
            &Unbacked,
            at(OPENED + 10_000),
            false
        )
        .unwrap(),
        Pass::Finish {
            plan: FinishPlan::Refused(FinishRefusal::FoldCannotAnswer),
            causation: None
        }
    );
    let unanswered = Request {
        fold: RequestFold::CannotAnswer,
        opened_at: None,
        oauth: false,
    };
    assert_eq!(
        decide(&unanswered, Ok(None), &Unbacked, at(OPENED), false).unwrap(),
        Pass::Finish {
            plan: FinishPlan::Refused(FinishRefusal::FoldCannotAnswer),
            causation: None
        }
    );
}

/// A request a revoke closed runs the teardown and journals nothing; one already closed has nothing
/// to close.
#[test]
fn a_closed_request_is_not_finished_again() {
    let id = connection(CONNECTION);
    let revoked = request(
        &[
            requested(CONNECTION, 2, "oauth"),
            closed("ConnectionRevoked", CONNECTION),
        ],
        &id,
    );
    let refused = request(
        &[
            requested(CONNECTION, 2, "oauth"),
            closed("ConnectionRefused", CONNECTION),
        ],
        &id,
    );
    let passed = Ok(Some(Checked::Passed {
        event_id: "ev-check".to_owned(),
        pii_ref: PII.to_owned(),
    }));
    assert_eq!(
        decide(
            &revoked,
            passed.clone(),
            &answering(Judged::Unique),
            at(OPENED + 60),
            false
        )
        .unwrap(),
        Pass::Finish {
            plan: FinishPlan::ClosedByRevoke,
            causation: None
        }
    );
    assert_eq!(
        decide(
            &refused,
            passed,
            &answering(Judged::Unique),
            at(OPENED + 60),
            false
        )
        .unwrap(),
        Pass::Finish {
            plan: FinishPlan::Refused(FinishRefusal::NotOpen),
            causation: None
        }
    );
}

/// Over any open request, check, judgment, time and restart: an establishment happens only for a
/// version-2 `oauth` request whose latest check passed and was judged unique, caused by that
/// check; a refusal caused by a check is caused by a failed one; and nothing is decided before the
/// deadline without a check.
#[test]
fn only_a_unique_passed_check_establishes() {
    let checks = prop_oneof![
        Just(Ok(None)),
        Just(Err(Unreadable)),
        Just(Ok(Some(Checked::Passed {
            event_id: "ev-pass".to_owned(),
            pii_ref: PII.to_owned(),
        }))),
        Just(Ok(Some(Checked::Failed {
            event_id: "ev-fail".to_owned(),
            reason: Reason::NotDedicated,
        }))),
    ];
    let judgments = prop_oneof![
        Just(Judged::Unique),
        Just(Judged::AccountMismatch),
        Just(Judged::AlreadyConnected(connection(OTHER))),
        Just(Judged::CannotAnswer),
    ];
    let strategy = (
        checks,
        judgments,
        0_i64..2 * CONNECT_DEADLINE_SECS,
        proptest::bool::ANY,
        proptest::bool::ANY,
    )
        .boxed();
    TestRunner::new(Config::with_cases(512))
        .run(&strategy, |(checked, judged, after, restarted, oauth)| {
            let request = if oauth {
                open_request()
            } else {
                request(
                    &[requested(CONNECTION, 2, "api_key")],
                    &connection(CONNECTION),
                )
            };
            let pass = decide(
                &request,
                checked.clone(),
                &answering(judged.clone()),
                at(OPENED + after),
                restarted,
            )
            .unwrap();
            let established = matches!(
                &pass,
                Pass::Finish { plan: FinishPlan::Commit(effects), .. }
                    if matches!(effects.as_slice(), [ManagerEffect::Established { .. }])
            );
            let passed_unique = oauth
                && judged == Judged::Unique
                && matches!(checked, Ok(Some(Checked::Passed { .. })));
            proptest::prop_assert_eq!(established, passed_unique);
            if let Pass::Finish {
                causation: Some(cause),
                ..
            } = &pass
            {
                let expected = if established { "ev-pass" } else { "ev-fail" };
                proptest::prop_assert_eq!(cause.as_str(), expected);
            }
            if oauth && checked == Ok(None) && after < CONNECT_DEADLINE_SECS {
                proptest::prop_assert_eq!(pass, Pass::Wait);
            }
            Ok(())
        })
        .unwrap();
}

/// Each broker and environment is read by its journal code; any other refuses the fold.
#[test]
fn each_broker_and_environment_is_read_by_its_code() {
    let with = |broker: &str, environment: &str| {
        let mut row = requested(CONNECTION, 2, "api_key");
        let mut body: Value = serde_json::from_slice(&row.body).unwrap();
        body["payload"]["broker"] = json!(broker);
        body["payload"]["environment"] = json!(environment);
        row.body = serde_json::to_vec(&body).unwrap();
        request(&[row], &connection(CONNECTION)).fold
    };
    for (code, broker) in [
        ("alpaca", Broker::Alpaca),
        ("robinhood", Broker::Robinhood),
        ("kraken_derivatives_us", Broker::KrakenDerivativesUs),
    ] {
        for (text, environment) in [("paper", Environment::Paper), ("live", Environment::Live)] {
            let RequestFold::Open(members) = with(code, text) else {
                panic!("{code} {text} opens")
            };
            assert_eq!((members.broker, members.environment), (broker, environment));
        }
    }
    assert_eq!(with("binance", "paper"), RequestFold::CannotAnswer);
    assert_eq!(with("alpaca", "sandbox"), RequestFold::CannotAnswer);
}

/// While a request is open, a second `ConnectionRequested` of the connection (rule 131 refuses
/// one) changes nothing; a revoke with no request open closes nothing that a revoke-closed
/// teardown would act on.
#[test]
fn only_an_open_request_is_replaced_or_revoked() {
    let id = connection(CONNECTION);
    let mut second = requested(CONNECTION, 2, "oauth");
    let mut body: Value = serde_json::from_slice(&second.body).unwrap();
    body["payload"]["account_ref"] = json!("01JB3K6F0C9R2V7N4M8Q1T5W61");
    second.body = serde_json::to_vec(&body).unwrap();
    second.recorded_at = stamp(OPENED + 5);
    let kept = request(&[requested(CONNECTION, 2, "oauth"), second], &id);
    let RequestFold::Open(members) = kept.fold else {
        panic!("still open")
    };
    assert_eq!(
        members.account_ref.as_str(),
        ACCOUNT,
        "the first request's members"
    );
    assert_eq!(kept.opened_at, Some(at(OPENED)), "the first request's time");
    let late_revoke = request(
        &[
            requested(CONNECTION, 2, "oauth"),
            closed("ConnectionRefused", CONNECTION),
            closed("ConnectionRevoked", CONNECTION),
        ],
        &id,
    );
    assert_eq!(late_revoke.fold, RequestFold::NotOpen);
}
