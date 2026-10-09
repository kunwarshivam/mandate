//! E7-17 (DEC-800 item 9; journal spec v0.20 §9.8 rules 66 to 68, §11): the connection's stream
//! rules and cross-stream causes, judged against the vectors' `connections` section, whose
//! `sequences` and `chains` `reference/journal/connections.py` builds and judges with its own fold.
//! Each case's records become stored rows in commit order, and the answer must be the vector's
//! exactly: valid, or the first failing record's index with its check and, for a lifecycle
//! mismatch, its rule.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mandate_canon::{Digest, Key, Object, Value, parse, to_canonical};
use mandate_journal::{
    ConnectionCheck, ConnectionFailure, ConnectionStreamRule, ConnectionVerifyError, StoredEvent,
    verify_connection_causes, verify_connection_lifecycle,
};

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("connections").cloned().unwrap()
}

fn list<'a>(value: &'a Value, name: &str) -> &'a [Value] {
    value
        .get(name)
        .and_then(Value::as_array)
        .unwrap_or_default()
}

fn text<'a>(value: &'a Value, name: &str) -> &'a str {
    value.get(name).and_then(Value::as_str).unwrap_or_default()
}

/// A `{path, value}` or `{path, delete: true}` change, dotted from the envelope.
fn apply(draft: &mut Object, change: &Value) {
    let path = text(change, "path");
    let mut names: Vec<&str> = path.split('.').collect();
    let last = names.pop().unwrap();
    let mut node = draft;
    for name in names {
        node = match node.get_mut(name) {
            Some(Value::Object(inner)) => inner,
            other => panic!("`{path}` has no object `{name}`: {other:?}"),
        };
    }
    if change.get("delete") == Some(&Value::Bool(true)) {
        assert!(node.remove(last).is_some(), "`{path}` deletes nothing");
    } else {
        node.insert(
            Key::new(last).unwrap(),
            change.get("value").cloned().unwrap(),
        );
    }
}

/// A case's records as stored rows, in commit order, each stream numbered from 1.
fn rows(section: &Value, case: &Value) -> Vec<StoredEvent> {
    let mut next_seq: BTreeMap<String, u64> = BTreeMap::new();
    list(case, "records")
        .iter()
        .map(|record| {
            let base = section
                .get("drafts")
                .and_then(|d| d.get(text(record, "base_draft")));
            let mut body = base.and_then(Value::as_object).cloned().unwrap();
            for change in list(record, "changes") {
                apply(&mut body, change);
            }
            let body = Value::Object(body);
            let stream_id = text(&body, "stream_id").to_owned();
            let seq = next_seq.entry(stream_id.clone()).or_insert(0);
            *seq += 1;
            StoredEvent {
                seq: *seq,
                event_id: text(&body, "event_id").to_owned(),
                event_type: text(&body, "event_type").to_owned(),
                schema_version: body.get("schema_version").and_then(Value::as_int).unwrap(),
                environment: text(&body, "environment").to_owned(),
                recorded_at: "2026-10-09T12:00:00.000000000Z".to_owned(),
                prev_hash: Digest::of(b""),
                hash: Digest::of(b""),
                body: to_canonical(&body),
                stream_id,
            }
        })
        .collect()
}

/// The vector's rule, by the number §9.8 gives it.
fn rule(number: &str) -> ConnectionStreamRule {
    match number {
        "66" => ConnectionStreamRule::Established,
        "67" => ConnectionStreamRule::Rotated,
        "68" => ConnectionStreamRule::AccountStream,
        other => panic!("no §9.8 stream rule {other}"),
    }
}

/// The answer a case's `expect` names.
fn expected(case: &Value) -> Result<(), ConnectionVerifyError> {
    let expect = case.get("expect").unwrap();
    if text(expect, "outcome") == "Valid" {
        return Ok(());
    }
    assert_eq!(text(expect, "outcome"), "Mismatch");
    let check = match text(expect, "code") {
        "connection_lifecycle_mismatch" => {
            ConnectionCheck::LifecycleMismatch(rule(text(expect, "rule")))
        }
        "connection_cause_mismatch" => ConnectionCheck::CauseMismatch,
        other => panic!("no connection check {other}"),
    };
    let index = expect.get("index").and_then(Value::as_int).unwrap();
    Err(ConnectionVerifyError::Mismatch(ConnectionFailure {
        index: usize::try_from(index).unwrap(),
        check,
    }))
}

/// Judges every case of `kind` with `verify`, and returns the failures and the expected answers.
fn judge(
    kind: &str,
    verify: fn(&[StoredEvent]) -> Result<(), ConnectionVerifyError>,
) -> (Vec<String>, Vec<Result<(), ConnectionVerifyError>>) {
    let section = section();
    let mut failed = Vec::new();
    let mut answers = Vec::new();
    for case in list(&section, kind) {
        let want = expected(case);
        let got = verify(&rows(&section, case));
        if got != want {
            failed.push(format!(
                "{} ({}): expected {want:?}, got {got:?}",
                text(case, "name"),
                text(case, "clause")
            ));
        }
        answers.push(want);
    }
    (failed, answers)
}

/// Rules 66, 67, and 68 over the 39 `sequences`: each first failing record, and its rule.
#[test]
#[ignore = "pending E7-17"]
fn every_connection_sequence_is_judged_as_its_vector_says() {
    let (failed, answers) = judge("sequences", verify_connection_lifecycle);
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(
        answers.len() >= 39,
        "{} sequences; never fewer",
        answers.len()
    );
    assert!(answers.contains(&Ok(())), "a valid sequence");
    let rules: BTreeSet<ConnectionStreamRule> = answers
        .iter()
        .filter_map(|a| match a {
            Err(ConnectionVerifyError::Mismatch(ConnectionFailure {
                check: ConnectionCheck::LifecycleMismatch(r),
                ..
            })) => Some(*r),
            _ => None,
        })
        .collect();
    assert_eq!(
        rules.len(),
        3,
        "rules 66, 67, and 68 each have a mismatch case"
    );
}

/// §11's `connection_cause_mismatch` over the 9 `chains`, across both streams.
#[test]
#[ignore = "pending E7-17"]
fn every_connection_chain_is_judged_as_its_vector_says() {
    let (failed, answers) = judge("chains", verify_connection_causes);
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(answers.len() >= 9, "{} chains; never fewer", answers.len());
    assert!(answers.contains(&Ok(())), "a valid chain");
    assert!(answers.iter().filter(|a| a.is_err()).count() >= 8);
}

#[test]
fn the_rules_and_checks_carry_the_specs_numbers_and_codes() {
    let rules = [
        (ConnectionStreamRule::Established, 66),
        (ConnectionStreamRule::Rotated, 67),
        (ConnectionStreamRule::AccountStream, 68),
    ];
    for (rule, number) in rules {
        assert_eq!(rule.number(), number);
        assert_eq!(
            ConnectionCheck::LifecycleMismatch(rule).code(),
            "connection_lifecycle_mismatch"
        );
    }
    assert_eq!(
        ConnectionCheck::CauseMismatch.code(),
        "connection_cause_mismatch"
    );
}

/// A case written here rather than in the vectors: each record is `bind` (the establishment's
/// copy), `rotate` (a rotation's copy), or a check's occasion, passing, or with `:reason` failing
/// its scope check. Record `i` has event id `01J8Z4S{i+1}A...`.
fn hand_case(records: &[&str]) -> Value {
    let records: Vec<String> = records
        .iter()
        .enumerate()
        .map(|(i, record)| {
            let id = format!(r#"{{"path":"event_id","value":"01J8Z4S{:02}A0000000000000000"}}"#, i + 1);
            let (base, more) = match *record {
                "bind" => ("established_copy", String::new()),
                "rotate" => ("rotated_copy", String::new()),
                check => {
                    let (occasion, failed) = check.split_once(':').unwrap_or((check, ""));
                    let scope = if failed.is_empty() {
                        r#""passed","reason":null"#.to_owned()
                    } else {
                        format!(r#""failed","reason":"{failed}""#)
                    };
                    let more = format!(
                        r#",{{"path":"payload.occasion","value":"{occasion}"}},{{"path":"payload.results","value":[{{"check":"account","result":"passed","reason":null}},{{"check":"environment","result":"passed","reason":null}},{{"check":"scope","result":{scope}}}]}}"#
                    );
                    ("checked_start", more)
                }
            };
            format!(r#"{{"base_draft":"{base}","changes":[{id}{more}]}}"#)
        })
        .collect();
    parse(format!(r#"{{"records":[{}]}}"#, records.join(",")).as_bytes()).unwrap()
}

/// Rule 68 reads the latest check of the occasion, never an earlier one that passed: a binding
/// after a passing then a failed `connect` check, and a rotation's copy after a passing then a
/// failed `reauthorize` check, are refused at the copy. The vectors have no such case; the
/// expected answers are §9.8's text, written by hand.
#[test]
#[ignore = "pending E7-17"]
fn the_latest_check_decides_a_binding_and_a_rotation() {
    let section = section();
    let verdict =
        |records: &[&str]| verify_connection_lifecycle(&rows(&section, &hand_case(records)));
    let refused_at = |index| {
        Err(ConnectionVerifyError::Mismatch(ConnectionFailure {
            index,
            check: ConnectionCheck::LifecycleMismatch(ConnectionStreamRule::AccountStream),
        }))
    };
    assert_eq!(
        verdict(&["connect:scope_mismatch", "connect", "bind"]),
        Ok(())
    );
    assert_eq!(
        verdict(&["connect", "connect:scope_mismatch", "bind"]),
        refused_at(2)
    );
    assert_eq!(
        verdict(&[
            "connect",
            "bind",
            "reauthorize",
            "reauthorize:fund_movement",
            "rotate"
        ]),
        refused_at(4)
    );
}

/// Record `i`'s event id in a [`written`] case.
fn id(i: usize) -> String {
    format!(r#""01J8Z4T{:02}A0000000000000000""#, i + 1)
}

/// A `{path, value}` change, `value` written as JSON, led by a comma.
fn set(path: &str, value: &str) -> String {
    format!(r#",{{"path":"{path}","value":{value}}}"#)
}

/// A `ConnectionStateChanged`'s `from`, `to`, and `reason`.
fn moved(from: &str, to: &str, reason: &str) -> String {
    let quoted = |s: &str| format!("\"{s}\"");
    set("payload.from", &quoted(from))
        + &set("payload.to", &quoted(to))
        + &set("payload.reason", &quoted(reason))
}

/// A check of `occasion` that passed every result.
fn check(occasion: &str) -> (&'static str, String) {
    (
        "checked_start",
        set("payload.occasion", &format!("\"{occasion}\"")),
    )
}

/// Version 1 of the establishment: no `account_ref`, `user`, `step_up`, or attestation.
fn version_1() -> (&'static str, String) {
    let deleted = ["account_ref", "user", "step_up", "margin_attestation"]
        .map(|m| format!(r#",{{"path":"payload.{m}","delete":true}}"#))
        .concat();
    ("established_v2", set("schema_version", "1") + &deleted)
}

/// The connection's revocation on the control stream; §11's checks read only its type and id.
fn revoked() -> (&'static str, String) {
    (
        "established_v2",
        set("event_type", r#""ConnectionRevoked""#),
    )
}

/// A second connection's record on its own account stream, `account_ref` `...B7`.
fn on_b(base: &'static str, more: &str) -> (&'static str, String) {
    let changes = set(
        "stream_id",
        r#""acct:ws_01J8Z2:01J8Z2ACCT00000000000000B7""#,
    ) + &set("payload.connection_id", r#""conn_alpaca_paper_2""#)
        + if base == "established_copy" {
            r#",{"path":"payload.account_ref","value":"01J8Z2ACCT00000000000000B7"}"#
        } else {
            ""
        }
        + more;
    (base, changes)
}

/// Judges a case written here: each record is a base draft and its changes after its event id.
fn written(
    records: &[(&'static str, String)],
    verify: fn(&[StoredEvent]) -> Result<(), ConnectionVerifyError>,
) -> Result<(), ConnectionVerifyError> {
    let records: Vec<String> = records
        .iter()
        .enumerate()
        .map(|(i, (base, more))| {
            let event_id = set("event_id", &id(i));
            let changes = event_id.trim_start_matches(',').to_owned() + more;
            format!(r#"{{"base_draft":"{base}","changes":[{changes}]}}"#)
        })
        .collect();
    let case = parse(format!(r#"{{"records":[{}]}}"#, records.join(",")).as_bytes()).unwrap();
    verify(&rows(&section(), &case))
}

fn refused(index: usize, rule: ConnectionStreamRule) -> Result<(), ConnectionVerifyError> {
    Err(ConnectionVerifyError::Mismatch(ConnectionFailure {
        index,
        check: ConnectionCheck::LifecycleMismatch(rule),
    }))
}

/// Rule 68 (v0.24): a `condition_cleared` out of `suspended` names a rotation's copy whose
/// `reauthorize` check came after the `ConnectionStateChanged` that last *entered* `suspended`. A
/// `suspended` to `suspended` record with a second suspending reason (admitted by rule 59) enters
/// nothing, so the rotation before it still clears; a `degraded` to `suspended` after the rotation
/// does enter, and the same rotation no longer clears. DEC-687's owed item (b) reads
/// `suspended` to `suspended` as entering; when §9.8 takes it, the first answer flips to a refusal
/// at record 6.
#[test]
#[ignore = "pending E7-17"]
fn only_a_record_that_enters_suspended_moves_the_suspension_a_rotation_must_follow() {
    let clears_after = |into_suspended: String, second: String| {
        let records = [
            check("connect"),
            ("established_copy", String::new()),
            ("state_degraded", into_suspended),
            check("reauthorize"),
            ("rotated_copy", String::new()),
            ("state_degraded", second),
            (
                "state_degraded",
                moved("suspended", "suspended", "condition_cleared") + &set("causation_id", &id(4)),
            ),
        ];
        written(&records, verify_connection_lifecycle)
    };
    assert_eq!(
        clears_after(
            moved("active", "suspended", "authorization_failed"),
            moved("suspended", "suspended", "credential_expired"),
        ),
        Ok(()),
        "suspended to suspended does not enter suspended under v0.24"
    );
    assert_eq!(
        clears_after(
            moved("active", "degraded", "network_errors"),
            moved("degraded", "suspended", "credential_expired"),
        ),
        refused(6, ConnectionStreamRule::AccountStream),
        "degraded to suspended enters it after the rotation's check"
    );
}

/// Rule 66: "A version-1 first establishment has no `account_ref`, so it is never
/// re-established", not even by another version 1 whose absent `account_ref` matches it.
/// `reference/journal/connections.py` admits that one; §9.8's text refuses it.
#[test]
#[ignore = "pending E7-17"]
fn a_version_1_first_establishment_is_never_established_again() {
    let again = |first: (&'static str, String), second: (&'static str, String)| {
        written(&[first, revoked(), second], verify_connection_lifecycle)
    };
    let v2 = || ("established_v2", String::new());
    assert_eq!(
        again(version_1(), version_1()),
        refused(2, ConnectionStreamRule::Established)
    );
    assert_eq!(
        again(version_1(), v2()),
        refused(2, ConnectionStreamRule::Established)
    );
    assert_eq!(again(v2(), v2()), Ok(()));
}

/// §11: a version-2 establishment's cause is a `connect` check, "`reconnect` for a later
/// establishment of the same id", whatever the earlier establishment's version.
/// `reference/journal/connections.py` counts only version 2; §9.8's text counts either.
#[test]
#[ignore = "pending E7-17"]
fn a_later_establishment_after_a_version_1_rests_on_a_reconnect_check() {
    let after_v1 = |occasion: &str| {
        let records = [
            version_1(),
            revoked(),
            check(occasion),
            ("established_v2", set("causation_id", &id(2))),
        ];
        written(&records, verify_connection_causes)
    };
    assert_eq!(after_v1("reconnect"), Ok(()));
    assert_eq!(
        after_v1("connect"),
        Err(ConnectionVerifyError::Mismatch(ConnectionFailure {
            index: 3,
            check: ConnectionCheck::CauseMismatch,
        }))
    );
}

/// Rule 68 holds on each account stream on its own: two connections' streams interleave and
/// each binds on its own `connect` check, and one stream's `reauthorize` check never admits the
/// other stream's rotation.
#[test]
#[ignore = "pending E7-17"]
fn each_account_stream_is_folded_on_its_own() {
    let mut records = vec![
        check("connect"),
        on_b("checked_start", ""),
        ("established_copy", String::new()),
        on_b("established_copy", ""),
        check("reauthorize"),
        on_b("state_degraded", ""),
    ];
    assert_eq!(written(&records, verify_connection_lifecycle), Ok(()));
    records.push(on_b("rotated_copy", ""));
    assert_eq!(
        written(&records, verify_connection_lifecycle),
        refused(6, ConnectionStreamRule::AccountStream)
    );
}

/// Rows of other event types, and connection types on other streams, are skipped: a control
/// record of another type naming a never-established id, an account record of another type naming
/// another connection, and a rotation on an agent stream break no rule.
#[test]
#[ignore = "pending E7-17"]
fn rows_that_are_not_connection_records_are_skipped() {
    let other = |event_type: &str| {
        set("event_type", &format!("\"{event_type}\""))
            + &set("payload.connection_id", r#""conn_never""#)
    };
    let records = [
        ("established_v2", String::new()),
        ("rotated", other("OwnerCommandRefused")),
        check("connect"),
        ("state_degraded", other("AccountRestrictionChanged")),
        ("established_copy", String::new()),
        (
            "rotated_copy",
            set("stream_id", r#""agent:ws_01J8Z2:agent_01""#),
        ),
    ];
    assert_eq!(written(&records, verify_connection_lifecycle), Ok(()));
}

/// Rule 67: a rotation names a connection established and not since revoked, so one naming an id
/// never established is refused, though another connection is live.
#[test]
#[ignore = "pending E7-17"]
fn a_rotation_of_a_connection_never_established_is_refused() {
    let records = [
        ("established_v2", String::new()),
        ("rotated", set("payload.connection_id", r#""conn_never""#)),
    ];
    assert_eq!(
        written(&records, verify_connection_lifecycle),
        refused(1, ConnectionStreamRule::Rotated)
    );
}
