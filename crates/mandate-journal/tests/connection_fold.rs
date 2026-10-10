//! E7-17 (DEC-800 item 9; journal spec v0.20 §9.8 rules 66 to 68, §11): the connection's stream
//! rules and cross-stream causes, judged against the vectors' `connections` section, whose
//! `sequences` and `chains` `reference/journal/connections.py` builds and judges with its own fold.
//! Each case's records become stored rows in commit order, and the answer must be the vector's
//! exactly: valid, or the first failing record's index with its check and, for a lifecycle
//! mismatch, its rule. Journal spec v0.32 (DEC-699) adds stream rule 131, the pending connection,
//! and rule 67's two exceptions for a revocation that closed a request; they are judged against the
//! `connection_requests` section and the hand cases at the end of this file. Journal spec v0.35
//! (DEC-885) runs a range from its connection anchor, or fails closed without one: judged against the
//! `connection_ranges` section, and on every split of every full-chain sequence against the
//! full-chain run and an independent scan of the records the rules judge. Journal spec v0.37
//! (DEC-888) refuses a `ConnectionRevoked` on an account stream under rule 68, and fails a range
//! closed at it without an anchor: judged against the `connection_revocations` section.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mandate_canon::{Digest, Int, Key, Object, Value, parse, to_canonical};
use mandate_journal::{
    ConnectionAnchor, ConnectionAnchorError, ConnectionCheck, ConnectionCheckError,
    ConnectionFailure, ConnectionStart, ConnectionStreamRule, ConnectionVerifyError, EventCheck,
    EventFailure, JUDGED_ON_ACCOUNT, JUDGED_ON_CONTROL, LocatedConnectionFailure, PrefixError,
    StoredEvent, TrustedStart, VerifiedPrefix, verify_connection_causes,
    verify_connection_causes_from_genesis, verify_connection_lifecycle,
    verify_connection_lifecycle_from,
};

fn section() -> Value {
    section_named("connections")
}

fn section_named(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get(name).cloned().unwrap()
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
        "131" => ConnectionStreamRule::Requested,
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
    judge_in(&section(), kind, |_| true, verify)
}

/// Judges every case of `kind` in `section` that `scoped` keeps with `verify`.
fn judge_in(
    section: &Value,
    kind: &str,
    scoped: fn(&Value) -> bool,
    verify: fn(&[StoredEvent]) -> Result<(), ConnectionVerifyError>,
) -> (Vec<String>, Vec<Result<(), ConnectionVerifyError>>) {
    let mut failed = Vec::new();
    let mut answers = Vec::new();
    for case in list(section, kind).iter().filter(|c| scoped(c)) {
        let want = expected(case);
        let got = verify(&rows(section, case));
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
        (ConnectionStreamRule::Requested, 131),
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
    verify(&written_rows(records))
}

/// The rows of a case written here, each record a base draft and its changes after its event id.
fn written_rows(records: &[(&'static str, String)]) -> Vec<StoredEvent> {
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
    rows(&section(), &case)
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

/// The records before a second suspension: a binding, a first suspension that a `reauthorize`
/// check (record 3) and its rotation (record 4) clear, the owner's acknowledgment back to `active`,
/// and a second entry into `suspended` at record 7.
fn suspended_twice() -> Vec<(&'static str, String)> {
    vec![
        check("connect"),
        ("established_copy", String::new()),
        (
            "state_degraded",
            moved("active", "suspended", "authorization_failed"),
        ),
        check("reauthorize"),
        ("rotated_copy", String::new()),
        (
            "state_degraded",
            moved("suspended", "suspended", "condition_cleared") + &set("causation_id", &id(4)),
        ),
        (
            "state_degraded",
            moved("suspended", "active", "acknowledged"),
        ),
        (
            "state_degraded",
            moved("active", "suspended", "credential_expired"),
        ),
    ]
}

/// Rule 68 (journal spec v0.35 §9.8): a `condition_cleared` out of `suspended` names a rotation
/// whose `reauthorize` check came after the `ConnectionStateChanged` that last entered `suspended`,
/// the *latest* entry, not any earlier one. After a second entry into `suspended`, neither the
/// first suspension's rotation nor a new rotation resting on the first suspension's check clears
/// it: each is refused at the clearing record. A check after the second entry, and its rotation,
/// clear it. Every vector enters `suspended` at most once, so none tells the two suspensions apart;
/// the expected answers are §9.8's text, written by hand.
#[test]
fn a_clearing_rotation_follows_the_latest_entry_into_suspended() {
    let clearing = |cause: usize| {
        (
            "state_degraded",
            moved("suspended", "suspended", "condition_cleared") + &set("causation_id", &id(cause)),
        )
    };
    let judged = |more: Vec<(&'static str, String)>| {
        let mut records = suspended_twice();
        records.extend(more);
        written(&records, verify_connection_lifecycle)
    };
    assert_eq!(judged(Vec::new()), Ok(()), "the prefix itself is valid");
    assert_eq!(
        judged(vec![clearing(4)]),
        refused(8, ConnectionStreamRule::AccountStream),
        "the first suspension's rotation does not clear the second"
    );
    assert_eq!(
        judged(vec![("rotated_copy", String::new()), clearing(8)]),
        refused(9, ConnectionStreamRule::AccountStream),
        "a rotation after the second entry rests on the first suspension's check"
    );
    assert_eq!(
        judged(vec![
            check("reauthorize"),
            ("rotated_copy", String::new()),
            clearing(9),
            (
                "state_degraded",
                moved("suspended", "active", "acknowledged")
            ),
        ]),
        Ok(()),
        "a check after the latest entry into suspended, and its rotation, clear it"
    );
}

/// Rule 66: "A version-1 first establishment has no `account_ref`, so it is never
/// re-established", not even by another version 1 whose absent `account_ref` matches it.
/// §9.8's text refuses it, and so does `reference/journal/connections.py` since v0.30 (DEC-696).
#[test]
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
/// §9.8's text counts either version, and so does `reference/journal/connections.py` since v0.30
/// (DEC-696).
#[test]
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

/// Rule 131 and rule 67's exceptions (journal spec v0.32, DEC-699) over the 27 full-chain
/// `sequences` of the `connection_requests` section: each first failing record, and its rule.
#[test]
fn every_connection_request_sequence_is_judged_as_its_vector_says() {
    let section = section_named("connection_requests");
    let full_chain = |case: &Value| case.get("scope").is_none();
    let (failed, answers) = judge_in(
        &section,
        "sequences",
        full_chain,
        verify_connection_lifecycle,
    );
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(
        answers.len() >= 27,
        "{} sequences; never fewer",
        answers.len()
    );
    assert!(answers.contains(&Ok(())), "a valid sequence");
    for rule in [
        ConnectionStreamRule::Requested,
        ConnectionStreamRule::Rotated,
    ] {
        let named = answers.iter().any(|a| {
            matches!(a, Err(ConnectionVerifyError::Mismatch(ConnectionFailure {
                check: ConnectionCheck::LifecycleMismatch(r), ..
            })) if *r == rule)
        });
        assert!(named, "rule {} has a mismatch case", rule.number());
    }
}

/// The connection ids a [`control`] case names.
fn connection(short: &str) -> &'static str {
    match short {
        "X" => "conn_alpaca_paper",
        "Y" => "conn_alpaca_paper_2",
        "Z" => "conn_alpaca_paper_3",
        other => panic!("no connection {other}"),
    }
}

/// Rows of a case written here on the control stream, from the `connection_requests` drafts. Each
/// record is `request X A1` (a `ConnectionRequested` of connection `X` on the account stream whose
/// `account_ref` ends `A1`), `establish X A1` (version 2), `version_1 X`, `refuse X connect` (the
/// deadline's teardown), `refuse X reconnect` (a failed scope check), or `revoke X`, then any
/// `member=text` sets (`ws=ws_01J8Z3` writes it on that workspace's control stream instead).
fn control(records: &[&str]) -> Vec<StoredEvent> {
    let records: Vec<String> = records
        .iter()
        .enumerate()
        .map(|(i, record)| {
            let (words, sets): (Vec<&str>, Vec<&str>) =
                record.split(' ').partition(|w| !w.contains('='));
            let quoted = |s: &str| format!("\"{s}\"");
            let sets: String = sets
                .iter()
                .filter_map(|w| w.split_once('='))
                .map(|(member, v)| match member {
                    "ws" => set("stream_id", &quoted(&format!("ctl:{v}"))),
                    _ => set(&format!("payload.{member}"), &quoted(v)),
                })
                .collect();
            let id = set(
                "event_id",
                &quoted(&format!("01J8Z4R{:02}A0000000000000000", i + 1)),
            ) + &set("payload.connection_id", &quoted(connection(words[1])));
            let account = |r: &str| {
                set(
                    "payload.account_ref",
                    &quoted(&format!("01J8Z2ACCT00000000000000{r}")),
                )
            };
            let (base, more) = match words[..] {
                ["request", _, r] => ("requested", id + &account(r)),
                ["establish", _, r] => ("established_v2", id + &account(r)),
                ["version_1", _] => ("established_v2", version_1().1 + &id),
                ["refuse", _, "connect"] => (
                    "refused_scope",
                    id + &set("payload.check", "null") + &set("payload.reason", r#""timeout""#),
                ),
                ["refuse", _, "reconnect"] => (
                    "refused_scope",
                    id + &set("payload.occasion", r#""reconnect""#),
                ),
                ["revoke", x] => (
                    "rotated",
                    set("event_type", r#""ConnectionRevoked""#)
                        + &set(
                            "payload",
                            &format!(r#"{{"connection_id":"{}"}}"#, connection(x)),
                        )
                        + &id,
                ),
                _ => panic!("no record `{record}`"),
            };
            format!(
                r#"{{"base_draft":"{base}","changes":[{}]}}"#,
                (more + &sets).trim_start_matches(',')
            )
        })
        .collect();
    let case = parse(format!(r#"{{"records":[{}]}}"#, records.join(",")).as_bytes()).unwrap();
    rows(&section_named("connection_requests"), &case)
}

/// I7, forward-only: rule 131 binds a control stream from its first `ConnectionRequested` on. A
/// connect before it, with no request of its own, is not judged, and once a request exists a
/// connect of another id that skips its own breaks the rule.
#[test]
fn rule_131_binds_a_stream_only_from_its_first_request() {
    let lifecycle = |records: &[&str]| verify_connection_lifecycle(&control(records));
    assert_eq!(
        lifecycle(&["refuse X connect", "request Y B7", "establish Y B7"]),
        Ok(())
    );
    assert_eq!(
        lifecycle(&[
            "establish X A1",
            "request Y B7",
            "refuse Y connect",
            "refuse Z connect"
        ]),
        refused(3, ConnectionStreamRule::Requested)
    );
}

/// I8 and rule 67's exceptions: a revocation that closed a request is no earlier revocation for a
/// later connect's refusal, however often the id starts again, but a revocation that closed none
/// is; a revocation is never refused; and a `reconnect` is refused only when the id's latest
/// revocation closed a request, so a connection established after its request was revoked once
/// reconnects normally.
#[test]
fn rule_67_excepts_only_a_revocation_that_closed_a_request() {
    let lifecycle = |records: &[&str]| verify_connection_lifecycle(&control(records));
    let retried = [
        "request X A1",
        "revoke X",
        "request X B7",
        "revoke X",
        "request X C8",
        "refuse X connect",
    ];
    assert_eq!(lifecycle(&retried), Ok(()));
    assert_eq!(
        lifecycle(&["revoke X", "request X A1", "refuse X connect"]),
        refused(2, ConnectionStreamRule::Rotated)
    );
    let once_plain = [
        "revoke X",
        "request X A1",
        "revoke X",
        "request X B7",
        "refuse X connect",
    ];
    assert_eq!(
        lifecycle(&once_plain),
        refused(4, ConnectionStreamRule::Rotated)
    );
    assert_eq!(
        lifecycle(&["request X A1", "revoke X", "revoke X", "revoke Y"]),
        Ok(())
    );
    let reconnected = [
        "request X A1",
        "revoke X",
        "request X B7",
        "establish X B7",
        "revoke X",
        "refuse X reconnect",
    ];
    assert_eq!(lifecycle(&reconnected), Ok(()));
}

/// Rule 131's version-1 clause: from the stream's first request on, a version-1 establishment of an
/// id never established breaks the rule, and closes no request; before the first request it is not
/// judged.
#[test]
fn a_version_1_connect_is_judged_only_from_the_first_request() {
    let lifecycle = |records: &[&str]| verify_connection_lifecycle(&control(records));
    assert_eq!(
        lifecycle(&["version_1 X", "request Y B7", "establish Y B7"]),
        Ok(())
    );
    assert_eq!(
        lifecycle(&["request Y B7", "establish Y B7", "version_1 X"]),
        refused(2, ConnectionStreamRule::Requested)
    );
    assert_eq!(
        lifecycle(&["request X A1", "version_1 X"]),
        refused(1, ConnectionStreamRule::Requested)
    );
}

/// Rule 131 on each control stream on its own: a connect's refusal repeats its request's `broker`,
/// `environment`, and `user`; a request's `account_ref` is one no earlier establishment holds,
/// even one from before the first request; and another workspace's request neither binds this
/// stream nor counts as its open request or its used `account_ref`.
#[test]
fn a_request_binds_its_members_and_account_ref_on_its_own_stream() {
    let lifecycle = |records: &[&str]| verify_connection_lifecycle(&control(records));
    for member in ["broker=robinhood", "environment=live", "user=user_owner_02"] {
        let refusal = format!("refuse X connect {member}");
        assert_eq!(
            lifecycle(&["request X A1", &refusal]),
            refused(1, ConnectionStreamRule::Requested),
            "{member}"
        );
    }
    assert_eq!(
        lifecycle(&["establish X A1", "request Y A1"]),
        refused(1, ConnectionStreamRule::Requested)
    );
    assert_eq!(
        lifecycle(&["request Y B7 ws=ws_01J8Z3", "refuse X connect"]),
        Ok(())
    );
    assert_eq!(
        lifecycle(&["request X A1 ws=ws_01J8Z3", "request X B7"]),
        Ok(())
    );
    assert_eq!(
        lifecycle(&["request X A1 ws=ws_01J8Z3", "request Y A1"]),
        Ok(())
    );
}

/// A failure at `row`, by its stream and `seq`.
fn at(row: &StoredEvent, check: ConnectionCheck) -> Result<(), ConnectionCheckError> {
    Err(ConnectionCheckError::Failed(LocatedConnectionFailure {
        stream_id: row.stream_id.clone(),
        seq: row.seq,
        check,
    }))
}

/// An answer by index in `rows`, by its row's stream and `seq` instead.
fn located(
    rows: &[StoredEvent],
    answer: Result<(), ConnectionVerifyError>,
) -> Result<(), ConnectionCheckError> {
    match answer {
        Ok(()) => Ok(()),
        Err(ConnectionVerifyError::Mismatch(ConnectionFailure { index, check })) => {
            at(&rows[index], check)
        }
    }
}

/// Every full-chain sequence of the `connections` and `connection_requests` sections.
fn full_chains() -> Vec<Vec<StoredEvent>> {
    let mut chains = Vec::new();
    for name in ["connections", "connection_requests"] {
        let section = section_named(name);
        let full_chain = list(&section, "sequences")
            .iter()
            .filter(|case| case.get("scope").is_none());
        chains.extend(full_chain.map(|case| rows(&section, case)));
    }
    chains
}

/// The records rules 66, 67, 68, and 131 judge, read here from DEC-885 item 4 by event type and
/// stream type: a request, establishment, rotation, or refusal on a control stream, and every
/// connection record on an account stream. A revocation is never judged.
fn judged(row: &StoredEvent) -> bool {
    let control = [
        "ConnectionRequested",
        "ConnectionEstablished",
        "ConnectionCredentialRotated",
        "ConnectionRefused",
    ];
    match row.stream_id.split(':').next() {
        Some("ctl") => control.contains(&row.event_type.as_str()),
        Some("acct") => row.event_type.starts_with("Connection"),
        _ => false,
    }
}

/// Judges a range case of `section`: the chain `before` it is folded into its anchor, or the range
/// is unanchored when `before` is `null` (its rows then start at `seq` 101), and the answer must be
/// the vector's, located by the range row's stream and `seq`, with its code. A `before` that breaks
/// a rule anchors nothing (I5).
#[allow(
    deprecated,
    reason = "the raw fold's tests, deleted with it once their verified twins are live (DEC-889 item 3)"
)]
fn judge_range(section: &Value, case: &Value) {
    let label = text(case, "name");
    let unanchored = case.get("before") == Some(&Value::Null);
    let before = list(case, "before");
    let records = [before, list(case, "records")].concat();
    let records = Object::from([(Key::new("records").unwrap(), Value::Array(records))]);
    let mut chain = rows(section, &Value::Object(records));
    if unanchored {
        chain.iter_mut().for_each(|row| row.seq += 100);
    }
    let (prefix, range) = chain.split_at(before.len());
    let anchor = ConnectionAnchor::fold(prefix);
    let start = match (unanchored, anchor.clone()) {
        (false, Some(anchor)) => ConnectionStart::Anchored(anchor),
        _ => ConnectionStart::Unanchored,
    };
    let expect = case.get("expect").unwrap();
    let want = match text(expect, "outcome") {
        "Valid" => Ok(()),
        outcome => {
            let check = match outcome {
                "Unanchored" => ConnectionCheck::Unanchored,
                _ => ConnectionCheck::LifecycleMismatch(rule(text(expect, "rule"))),
            };
            assert_eq!(check.code(), text(expect, "code"), "{label}");
            let index = expect.get("index").and_then(Value::as_int).unwrap();
            at(&range[usize::try_from(index).unwrap()], check)
        }
    };
    let got = verify_connection_lifecycle_from(start, range);
    assert_eq!(got, want, "{label}");
    if let Err(ConnectionCheckError::Failed(failure)) = &got {
        assert_eq!(failure.code(), text(expect, "code"), "{label}");
    }
    if !unanchored {
        let clean = verify_connection_lifecycle(prefix).is_ok();
        assert_eq!(
            anchor.is_some(),
            clean,
            "{label}: I5, a broken chain anchors nothing"
        );
    }
}

/// §11 (v0.35, DEC-885) over the 22 `connection_ranges` and `connection_requests`'
/// `range_after_an_unseen_request`, each judged by [`judge_range`].
#[test]
#[allow(
    deprecated,
    reason = "the raw fold's tests, deleted with it once their verified twins are live (DEC-889 item 3)"
)]
fn every_range_vector_is_judged_from_its_anchor_as_its_vector_says() {
    let mut judged_cases = 0;
    for name in ["connection_ranges", "connection_requests"] {
        let section = section_named(name);
        let ranges = list(&section, "sequences")
            .iter()
            .filter(|case| text(case, "scope") == "range");
        for case in ranges {
            judge_range(&section, case);
            judged_cases += 1;
        }
    }
    assert_eq!(
        judged_cases, 23,
        "22 `connection_ranges` and one request range"
    );
}

/// DEC-885's I1, I4, and I5 on every split `k` of every full-chain sequence: from the anchor of
/// the rows before `k`, the rows from `k` are judged exactly as the full-chain run judges them, at
/// the same row; a range from `seq` 1, anchored on nothing or from genesis, is the full chain; and
/// rows before `k` that break a rule anchor nothing.
#[test]
#[allow(
    deprecated,
    reason = "the raw fold's tests, deleted with it once their verified twins are live (DEC-889 item 3)"
)]
fn an_anchored_range_agrees_with_the_full_chain_on_every_split() {
    let chains = full_chains();
    assert!(
        chains.len() >= 66,
        "{} full chains; never fewer",
        chains.len()
    );
    for chain in &chains {
        let answer = verify_connection_lifecycle(chain);
        let first = answer
            .map_err(|ConnectionVerifyError::Mismatch(f)| f.index)
            .err();
        let full = located(chain, answer);
        let genesis = verify_connection_lifecycle_from(ConnectionStart::Genesis, chain);
        assert_eq!(genesis, full, "I4: genesis is the full chain");
        for k in 0..=chain.len() {
            let (prefix, range) = chain.split_at(k);
            let anchor = ConnectionAnchor::fold(prefix);
            let clean = first.is_none_or(|index| index >= k);
            if clean {
                let start = anchor
                    .clone()
                    .map_or(ConnectionStart::Unanchored, ConnectionStart::Anchored);
                let got = verify_connection_lifecycle_from(start, range);
                assert_eq!(got, full, "I1: split at {k} of {chain:?}");
            }
            assert_eq!(anchor.is_some(), clean, "I5: split at {k} of {chain:?}");
        }
    }
}

/// DEC-885's I2, I3, and I6 on every split of every full-chain sequence: unanchored, the rows from
/// `k` fail closed at the first one [`judged`] names, with no rule, and pass when it names none, so
/// a range of revocations passes, anchored or not.
#[test]
#[allow(
    deprecated,
    reason = "the raw fold's tests, deleted with it once their verified twins are live (DEC-889 item 3)"
)]
fn an_unanchored_range_fails_closed_at_its_first_judged_record() {
    let mut revocations_passed = 0;
    for chain in full_chains() {
        for k in 0..=chain.len() {
            let range = &chain[k..];
            let first = range.iter().position(judged);
            let want = first.map_or(Ok(()), |i| at(&range[i], ConnectionCheck::Unanchored));
            let got = verify_connection_lifecycle_from(ConnectionStart::Unanchored, range);
            assert_eq!(got, want, "split at {k} of {chain:?}");
            revocations_passed += usize::from(first.is_none_or(|i| i > 0) && !range.is_empty());
        }
    }
    assert!(revocations_passed > 0, "a range that opens on a revocation");
    let chain = control(&["request X A1", "establish X A1", "revoke X", "revoke Y"]);
    let (prefix, range) = chain.split_at(2);
    let anchored = ConnectionAnchor::fold(prefix).map(ConnectionStart::Anchored);
    let unanchored = ConnectionStart::Unanchored;
    for start in [anchored.unwrap(), unanchored] {
        assert_eq!(verify_connection_lifecycle_from(start, range), Ok(()));
    }
}

/// §11's `connection_cause_mismatch` located: over the 9 `chains`, the full-chain cause check
/// reports the row [`verify_connection_causes`] reports, by its stream and `seq`.
#[test]
fn the_located_cause_check_reports_the_full_chain_row() {
    let section = section();
    let cases = list(&section, "chains");
    assert!(cases.len() >= 9, "{} chains; never fewer", cases.len());
    for case in cases {
        let chain = rows(&section, case);
        let want = located(&chain, verify_connection_causes(&chain));
        let got = verify_connection_causes_from_genesis(&chain);
        assert_eq!(got, want, "{}", text(case, "name"));
    }
}

/// DEC-885 item 5: the fail-closed cause keeps the lifecycle check's code, so rule 111's codes are
/// unchanged, and a located failure carries its check's code.
#[test]
fn the_unanchored_cause_keeps_the_lifecycle_code() {
    assert_eq!(
        ConnectionCheck::Unanchored.code(),
        "connection_lifecycle_mismatch"
    );
    let failure = |check| LocatedConnectionFailure {
        stream_id: "ctl:ws_01J8Z2".to_owned(),
        seq: 3,
        check,
    };
    let codes = [
        (ConnectionCheck::Unanchored, "connection_lifecycle_mismatch"),
        (ConnectionCheck::CauseMismatch, "connection_cause_mismatch"),
    ];
    for (check, code) in codes {
        assert_eq!(failure(check).code(), code);
    }
}

/// `rows` as the journal stores them, each stream chained on its own: the body carries its `seq`,
/// `prev_hash`, and `recorded_at` and no artifact references, and `hash` is its digest, so every
/// row passes checks 1 to 6. The members the rules read are unchanged.
fn sealed(rows: &[StoredEvent]) -> Vec<StoredEvent> {
    let mut tails: BTreeMap<String, Digest> = BTreeMap::new();
    let mut sealed = Vec::new();
    for row in rows {
        let prev_hash = tails.get(&row.stream_id).copied().unwrap_or(Digest::ZERO);
        let mut body = parse(&row.body).unwrap().as_object().cloned().unwrap();
        for (name, value) in [
            ("seq", Value::Int(Int::new(row.seq).unwrap())),
            ("prev_hash", Value::Str(prev_hash.to_hex())),
            ("recorded_at", Value::Str(row.recorded_at.clone())),
            ("artifact_refs", Value::Array(vec![])),
            ("config_refs", Value::Object(BTreeMap::new())),
        ] {
            body.insert(Key::new(name).unwrap(), value);
        }
        let body = to_canonical(&Value::Object(body));
        let hash = Digest::of(&body);
        tails.insert(row.stream_id.clone(), hash);
        sealed.push(StoredEvent {
            prev_hash,
            hash,
            body,
            ..row.clone()
        });
    }
    sealed
}

/// The trusted start of the range of `prefix`'s stream just after it.
fn start_after(prefix: &[StoredEvent]) -> TrustedStart {
    TrustedStart {
        from_seq: prefix.last().map_or(1, |row| row.seq + 1),
        prev_hash: prefix.last().map_or(Digest::ZERO, |row| row.hash),
    }
}

fn bind(prefix: &[StoredEvent], start: TrustedStart) -> Result<VerifiedPrefix<'_>, PrefixError> {
    VerifiedPrefix::bind(prefix, start, &BTreeMap::<Digest, Vec<u8>>::new())
}

/// Where a caller runs a range from (DEC-892 item 3, DEC-889): anchored on the fold of `prefix`
/// once `bind` verifies it and binds it to `start`, and unanchored when `bind` refuses it or the
/// fold finds it broken. Anything else, a stub's report included, fails with its variant named.
fn verified_start(prefix: &[StoredEvent], start: TrustedStart) -> ConnectionStart {
    match bind(prefix, start).map(|bound| ConnectionAnchor::from_verified(&bound)) {
        Ok(Ok(anchor)) => ConnectionStart::Anchored(anchor),
        Ok(Err(ConnectionAnchorError::Broken))
        | Err(PrefixError::Unverified(_) | PrefixError::Unbound) => ConnectionStart::Unanchored,
        unexpected => panic!("neither an anchor nor a refusal: {unexpected:?}"),
    }
}

/// `every_range_vector_is_judged_from_its_anchor_as_its_vector_says` with each `before` sealed and
/// bound through `bind` at the range's own trusted start. An unanchored vector's range starts at
/// `seq` 101 with no rows before it, so `bind` refuses the empty prefix and the range fails closed.
#[test]
fn every_range_vector_is_judged_from_its_verified_anchor_as_its_vector_says() {
    let mut judged_cases = 0;
    for name in ["connection_ranges", "connection_requests"] {
        let section = section_named(name);
        let ranges = list(&section, "sequences")
            .iter()
            .filter(|case| text(case, "scope") == "range");
        for case in ranges {
            let label = text(case, "name");
            let unanchored = case.get("before") == Some(&Value::Null);
            let before = list(case, "before");
            let records = [before, list(case, "records")].concat();
            let records = Object::from([(Key::new("records").unwrap(), Value::Array(records))]);
            let mut chain = rows(&section, &Value::Object(records));
            if unanchored {
                chain.iter_mut().for_each(|row| row.seq += 100);
            }
            let chain = sealed(&chain);
            let (prefix, range) = chain.split_at(before.len());
            let start = TrustedStart {
                from_seq: range[0].seq,
                prev_hash: range[0].prev_hash,
            };
            let start = verified_start(prefix, start);
            let clean = !unanchored && verify_connection_lifecycle(prefix).is_ok();
            let anchored = matches!(start, ConnectionStart::Anchored(_));
            assert_eq!(anchored, clean, "{label}: I5, and no anchor unread");
            let expect = case.get("expect").unwrap();
            let want = match text(expect, "outcome") {
                "Valid" => Ok(()),
                outcome => {
                    let check = match outcome {
                        "Unanchored" => ConnectionCheck::Unanchored,
                        _ => ConnectionCheck::LifecycleMismatch(rule(text(expect, "rule"))),
                    };
                    let index = expect.get("index").and_then(Value::as_int).unwrap();
                    at(&range[usize::try_from(index).unwrap()], check)
                }
            };
            assert_eq!(
                verify_connection_lifecycle_from(start, range),
                want,
                "{label}"
            );
            judged_cases += 1;
        }
    }
    assert_eq!(judged_cases, 23, "22 `connection_ranges` and one request");
}

/// Each stream's rows of `chain`, in commit order (DEC-889).
fn per_stream(chain: &[StoredEvent]) -> Vec<Vec<StoredEvent>> {
    let mut streams: BTreeMap<&str, Vec<StoredEvent>> = BTreeMap::new();
    for row in chain {
        streams.entry(&row.stream_id).or_default().push(row.clone());
    }
    streams.into_values().collect()
}

/// `an_anchored_range_agrees_with_the_full_chain_on_every_split` per stream, each prefix bound
/// through `bind` (DEC-885 I1, I4, I5; DEC-889). A chain of several streams is first checked to be
/// judged as its streams are, at the earliest row any of them fails.
#[test]
fn a_verified_anchor_agrees_with_the_full_chain_on_every_split_of_every_stream() {
    let chains = full_chains();
    assert!(chains.len() >= 66, "{} full chains", chains.len());
    let mut several = 0;
    for chain in chains.iter().map(|chain| sealed(chain)) {
        let streams = per_stream(&chain);
        several += usize::from(streams.len() > 1);
        let commit_order = |failure: &ConnectionCheckError| {
            let ConnectionCheckError::Failed(f) = failure;
            chain
                .iter()
                .position(|row| row.stream_id == f.stream_id && row.seq == f.seq)
        };
        let earliest = streams
            .iter()
            .filter_map(|rows| located(rows, verify_connection_lifecycle(rows)).err())
            .min_by_key(commit_order);
        let whole = located(&chain, verify_connection_lifecycle(&chain));
        assert_eq!(whole, earliest.map_or(Ok(()), Err), "streams of {chain:?}");
        for rows in &streams {
            let answer = verify_connection_lifecycle(rows);
            let first = answer.map_err(|ConnectionVerifyError::Mismatch(f)| f.index);
            let full = located(rows, answer);
            for k in 0..=rows.len() {
                let (prefix, range) = rows.split_at(k);
                let start = verified_start(prefix, start_after(prefix));
                let clean = first.err().is_none_or(|index| index >= k);
                let anchored = matches!(start, ConnectionStart::Anchored(_));
                assert_eq!(anchored, clean, "I5: split at {k} of {rows:?}");
                if clean {
                    let got = verify_connection_lifecycle_from(start, range);
                    assert_eq!(got, full, "I1: split at {k} of {rows:?}");
                }
            }
        }
    }
    assert!(several >= 3, "{several} chains of several streams");
}

/// A sound control-stream chain whose third row, `revoke Y`, a hot-store rewrite into `revoke X`
/// would make an anchor read as `X` revoked, passing the refused `reconnect` of `X` after it. The
/// chain, and that rewritten row under the stored hash.
fn attacked() -> (Vec<StoredEvent>, StoredEvent) {
    let records = ["request X A1", "establish X A1", "revoke Y"];
    let chain = sealed(&control(&[&records[..], &["refuse X reconnect"]].concat()));
    let mut forged = sealed(&control(&[records[0], records[1], "revoke X"]))[2].clone();
    forged.hash = chain[2].hash;
    (chain, forged)
}

/// DEC-892 items 1 and 3: a prefix that is rewritten under its stored hashes, truncated, bound to
/// another tail, or given for a range from `seq` 1 does not bind, so the range runs unanchored and
/// fails closed at its first judged record; the sound prefix anchors it as the full chain judges.
#[test]
fn a_forged_short_or_unbound_prefix_runs_the_range_unanchored() {
    let (chain, forged_row) = attacked();
    let (prefix, range) = chain.split_at(3);
    let start = start_after(prefix);
    let full = located(&chain, verify_connection_lifecycle(&chain));
    let rotated = ConnectionCheck::LifecycleMismatch(ConnectionStreamRule::Rotated);
    assert_eq!(full, at(&range[0], rotated), "rule 67 refuses it");
    let sound = verify_connection_lifecycle_from(verified_start(prefix, start), range);
    assert_eq!(sound, full, "the sound prefix");
    let mut forged = prefix.to_vec();
    forged[2] = forged_row;
    let short = TrustedStart {
        prev_hash: prefix[1].hash,
        ..start
    };
    let other_tail = TrustedStart {
        prev_hash: Digest::of(b"another chain"),
        ..start
    };
    let rehashed = PrefixError::Unverified(EventFailure {
        seq: 3,
        check: EventCheck::RehashMismatch,
    });
    let cases = [
        (
            &forged[..],
            start,
            rehashed,
            "a body rewritten under kept hashes",
        ),
        (
            &prefix[..2],
            short,
            PrefixError::Unbound,
            "a prefix short of from_seq - 1",
        ),
        (
            prefix,
            other_tail,
            PrefixError::Unbound,
            "a tail hash other than prev_hash",
        ),
        (
            prefix,
            TrustedStart::GENESIS,
            PrefixError::Unbound,
            "rows before seq 1",
        ),
    ];
    for (rows, start, refused, name) in cases {
        assert_eq!(bind(rows, start).err(), Some(refused), "{name}");
        let got = verify_connection_lifecycle_from(verified_start(rows, start), range);
        assert_eq!(got, at(&range[0], ConnectionCheck::Unanchored), "{name}");
    }
}

/// DEC-889 item 2: an anchored run judges its anchor's stream only, and fails closed at the first
/// judged record of another; an empty prefix's anchor takes the range's first judged record's stream.
#[test]
fn an_anchored_run_fails_closed_at_another_streams_record() {
    let records = ["request X A1", "establish X A1", "revoke Y"];
    let foreign = "establish Z A2 ws=ws_01J8Z3";
    let chain = sealed(&control(&[&records[..], &[foreign]].concat()));
    for k in [0, 2] {
        let (prefix, range) = chain.split_at(k);
        let start = verified_start(prefix, start_after(prefix));
        let got = verify_connection_lifecycle_from(start, range);
        assert_eq!(
            got,
            at(&chain[3], ConnectionCheck::Unanchored),
            "split at {k}"
        );
    }
}

/// DEC-889 item 2 and DEC-888: an account stream's judged record is another stream's, so an
/// anchored control run fails closed at it, an account-stream `ConnectionRevoked` included, from
/// an empty anchor, a part of the prefix, or the whole of it.
#[test]
fn an_anchored_run_fails_closed_at_an_account_streams_record() {
    let own = control(&["request X A1", "establish X A1"]);
    let revoked_on_account = ("checked_start", set("event_type", r#""ConnectionRevoked""#));
    for foreign in [check("connect"), revoked_on_account] {
        let chain = sealed(&[own.clone(), written_rows(&[foreign])].concat());
        assert!(chain[2].stream_id.starts_with("acct:"), "an account stream");
        for k in 0..=2 {
            let (prefix, range) = chain.split_at(k);
            let start = verified_start(prefix, start_after(prefix));
            let got = verify_connection_lifecycle_from(start, range);
            let want = at(&chain[2], ConnectionCheck::Unanchored);
            assert_eq!(got, want, "{} after a split at {k}", chain[2].event_type);
        }
    }
}

/// DEC-889 item 2 and DEC-885 I6: another control stream's `ConnectionRevoked` is never judged, so
/// it never fails an anchored run, which still fails closed at that stream's next judged record.
#[test]
fn an_anchored_run_passes_another_streams_revocation() {
    let records = ["request X A1", "establish X A1", "revoke Y"];
    let foreign = ["revoke X ws=ws_01J8Z3", "establish Z A2 ws=ws_01J8Z3"];
    let chain = sealed(&control(&[&records[..], &foreign[..]].concat()));
    let (prefix, _) = chain.split_at(2);
    for (range, want) in [
        (&chain[2..4], Ok(())),
        (&chain[2..], at(&chain[4], ConnectionCheck::Unanchored)),
    ] {
        let start = verified_start(prefix, start_after(prefix));
        assert_eq!(verify_connection_lifecycle_from(start, range), want);
    }
}

/// DEC-889 item 2: an empty prefix's anchor takes the stream of the range's first judged record,
/// not of a leading revocation on another control stream, so that stream is judged as the full
/// chain judges it, and a later judged record of the revoking stream fails closed.
#[test]
fn an_empty_anchor_takes_the_stream_of_the_first_judged_record() {
    let own = ["revoke Y ws=ws_01J8Z3", "request X A1", "establish X A1"];
    let rotated = ConnectionCheck::LifecycleMismatch(ConnectionStreamRule::Rotated);
    for (last, check) in [
        ("refuse X reconnect", rotated),
        ("establish Z A2 ws=ws_01J8Z3", ConnectionCheck::Unanchored),
    ] {
        let chain = sealed(&control(&[&own[..], &[last]].concat()));
        let start = verified_start(&[], TrustedStart::GENESIS);
        let got = verify_connection_lifecycle_from(start, &chain);
        assert_eq!(got, at(&chain[3], check), "{last}");
        let full = located(&chain, verify_connection_lifecycle(&chain));
        assert!(
            check != rotated || got == full,
            "as the full chain judges it"
        );
    }
}

/// Journal spec v0.37 (DEC-888) over the 6 `connection_revocations`: a full-chain case through
/// [`verify_connection_lifecycle`] and from [`ConnectionStart::Genesis`], whose answers agree, and
/// a range case from its anchor, folded by [`ConnectionAnchor::fold`], or unanchored, by
/// [`judge_range`]. On every split of a full-chain case, an anchored range agrees with the full
/// chain (I1), and an unanchored one fails closed at the first record [`judged`] names (I2), an
/// account-stream revocation included.
#[test]
#[allow(
    deprecated,
    reason = "the raw fold's tests, deleted with it once their verified twins are live (DEC-889 item 3)"
)]
fn every_revocation_vector_is_judged_as_its_vector_says() {
    let section = section_named("connection_revocations");
    let cases = list(&section, "sequences");
    let (ranges, full): (Vec<&Value>, Vec<&Value>) = cases
        .iter()
        .partition(|case| text(case, "scope") == "range");
    assert_eq!((full.len(), ranges.len()), (3, 3), "the 6 DEC-888 cases");
    for case in ranges {
        judge_range(&section, case);
    }
    let (failed, answers) = judge_in(
        &section,
        "sequences",
        |case| case.get("scope").is_none(),
        verify_connection_lifecycle,
    );
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    let refused = Err(ConnectionVerifyError::Mismatch(ConnectionFailure {
        index: 0,
        check: ConnectionCheck::LifecycleMismatch(ConnectionStreamRule::AccountStream),
    }));
    assert!(answers.contains(&refused), "an unbound stream's revocation");
    for case in full {
        let chain = rows(&section, case);
        let answer = verify_connection_lifecycle(&chain);
        let first = answer
            .map_err(|ConnectionVerifyError::Mismatch(f)| f.index)
            .err();
        let full = located(&chain, answer);
        let genesis = verify_connection_lifecycle_from(ConnectionStart::Genesis, &chain);
        assert_eq!(
            genesis,
            full,
            "{}: genesis is the full chain",
            text(case, "name")
        );
        for k in 0..=chain.len() {
            let (prefix, range) = chain.split_at(k);
            if first.is_none_or(|index| index >= k) {
                let anchor = ConnectionAnchor::fold(prefix).unwrap();
                let got =
                    verify_connection_lifecycle_from(ConnectionStart::Anchored(anchor), range);
                assert_eq!(got, full, "I1: split at {k} of {chain:?}");
            }
            let first_judged = range.iter().position(judged);
            let want = first_judged.map_or(Ok(()), |i| at(&range[i], ConnectionCheck::Unanchored));
            let got = verify_connection_lifecycle_from(ConnectionStart::Unanchored, range);
            assert_eq!(got, want, "I2: split at {k} of {chain:?}");
        }
    }
}

/// The judged-record lists lane L5's verifier CLI reads (#1206) are §11's: on a control stream a
/// request, establishment, rotation, or refusal, never a revocation (DEC-885 item 4, I6); on an
/// account stream every connection record, a revocation included (journal spec v0.37, DEC-888).
#[test]
fn the_exported_judged_lists_are_the_specs() {
    let set = |names: &[&str]| -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    };
    let control = [
        "ConnectionRequested",
        "ConnectionEstablished",
        "ConnectionCredentialRotated",
        "ConnectionRefused",
    ];
    let account = [
        "ConnectionChecked",
        "ConnectionStateChanged",
        "ConnectionCredentialRefreshed",
        "ConnectionEstablished",
        "ConnectionCredentialRotated",
        "ConnectionRevoked",
    ];
    assert_eq!(
        set(JUDGED_ON_CONTROL),
        set(&control),
        "§11's control-stream set"
    );
    assert_eq!(
        set(JUDGED_ON_ACCOUNT),
        set(&account),
        "§11's account-stream set"
    );
    assert_eq!(JUDGED_ON_CONTROL.len(), control.len(), "no type twice");
    assert_eq!(JUDGED_ON_ACCOUNT.len(), account.len(), "no type twice");
}
