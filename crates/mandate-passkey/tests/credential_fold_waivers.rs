//! The credentials fold reads journal spec v0.113 §9.38's `StepUpCoolOffWaived` (DEC-2146) and
//! judges stream rule 318, failing closed: a waiver that names a credential its actor does not
//! hold, another `cool_off_ends` than that enrolment's or one not after the waiver, or a challenge
//! already waived; and the converse, an assertion inside its credential's cool-off that the next
//! record does not waive. Through the vectors' `cool_off_waivers` section, and a property test
//! against an oracle that rebuilds rule 318 from each record's own history. Step-up's view
//! discounts the member and the credential's holder from the first break; route 2's view reads no
//! waiver.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mandate_canon::{Key, Object, Value, decode_ulid, parse};
use mandate_identity::PrincipalId;
use mandate_passkey::credentials::{
    Assertion, CredentialError, CredentialFold, CredentialRecord, Enrolment, PasskeyAlg, Removal,
    Waiver, credential_record, waiver,
};
use mandate_time::UtcNanos;
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("cool_off_waivers").cloned().unwrap()
}

fn list<'a>(value: &'a Value, name: &str) -> &'a [Value] {
    value
        .get(name)
        .and_then(Value::as_array)
        .unwrap_or_default()
}

fn text<'a>(value: &'a Value, path: &str) -> &'a str {
    path.split('.')
        .try_fold(value, |node, name| node.get(name))
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn principal(ulid: &str) -> PrincipalId {
    PrincipalId(decode_ulid(ulid).unwrap())
}

fn instant(text: &str) -> UtcNanos {
    UtcNanos::parse(text).unwrap()
}

/// A case's base draft with its `{path, value}` changes applied.
fn vector_draft(section: &Value, case: &Value) -> Value {
    let base = section
        .get("drafts")
        .and_then(|d| d.get(text(case, "base_draft")));
    let mut body: Object = base.and_then(Value::as_object).cloned().unwrap();
    for change in list(case, "changes") {
        let path = text(change, "path");
        let (outer, inner) = path.split_once('.').unwrap_or(("", path));
        let node = if outer.is_empty() {
            &mut body
        } else {
            match body.get_mut(outer) {
                Some(Value::Object(node)) => node,
                _ => panic!("{path}"),
            }
        };
        node.insert(
            Key::new(inner).unwrap(),
            change.get("value").cloned().unwrap(),
        );
    }
    Value::Object(body)
}

/// The waiver `envelope` holds, read here member by member from the envelope itself.
fn expected_waiver(seq: u64, envelope: &Value) -> CredentialRecord {
    CredentialRecord::Waived(Waiver {
        seq,
        member: principal(text(envelope, "actor.id")),
        credential: text(envelope, "payload.credential").to_owned(),
        challenge: text(envelope, "payload.challenge").to_owned(),
        cool_off_ends: instant(text(envelope, "payload.cool_off_ends")),
        event_time: instant(text(envelope, "event_time")),
    })
}

/// A stream's records at `seq` 1 onwards: each waiver through [`waiver`], every other record
/// through `credential_record`.
fn records(stream: &Value) -> Vec<CredentialRecord> {
    list(stream, "records")
        .iter()
        .zip(1..)
        .map(|(envelope, seq)| {
            let mapped = if text(envelope, "event_type") == "StepUpCoolOffWaived" {
                waiver(seq, envelope).map(Some)
            } else {
                credential_record(seq, envelope)
            };
            match mapped {
                Ok(Some(record)) => record,
                other => panic!("{} record {seq}: {other:?}", text(stream, "name")),
            }
        })
        .collect()
}

/// `record`, an assertion moved past every cool-off: the stream rule 254 alone judges.
fn past_every_cool_off(record: &CredentialRecord) -> CredentialRecord {
    match record {
        CredentialRecord::Asserted(a) => CredentialRecord::Asserted(Assertion {
            event_time: UtcNanos::from_parts(4_102_444_800, 0).unwrap(),
            ..a.clone()
        }),
        other => other.clone(),
    }
}

/// The `seq` rule 318 first breaks at, as the vectors expect it, or `None` for a valid stream.
fn expected_break(stream: &Value) -> Option<u64> {
    let expect = stream.get("expect").unwrap();
    (text(expect, "outcome") == "Invalid").then(|| {
        let index = expect.get("index").and_then(Value::as_int).unwrap();
        index + 1
    })
}

/// The base and each valid waiver map whole, the same through [`waiver`] and through
/// `credential_record`: the actor as member, the credential, the challenge, the `cool_off_ends`
/// and the record's `event_time`.
#[test]
#[ignore = "pending E9-4"]
fn each_waiver_maps_its_member_credential_challenge_end_and_time() {
    let section = section();
    let base = section
        .get("drafts")
        .and_then(|d| d.get("cool_off_waived"))
        .cloned()
        .unwrap();
    let cases: Vec<Value> = std::iter::once(base)
        .chain(
            list(&section, "valid_drafts")
                .iter()
                .map(|c| vector_draft(&section, c)),
        )
        .collect();
    assert!(cases.len() >= 3, "the base and the vectors' valid drafts");
    for (envelope, seq) in cases.iter().zip(1..) {
        let expected = expected_waiver(seq, envelope);
        assert_eq!(waiver(seq, envelope), Ok(expected.clone()));
        assert_eq!(credential_record(seq, envelope), Ok(Some(expected)));
    }
}

/// A waiver whose challenge, credential or `cool_off_ends` is of the wrong kind, null, or not a
/// ULID or credential id is refused at that member, and one at another schema version at
/// `schema_version`, so the fold reads it as unreadable rather than as no waiver.
#[test]
#[ignore = "pending E9-4"]
fn an_unreadable_waiver_is_refused_at_its_member() {
    let section = section();
    let mut checked = 0;
    for case in list(&section, "invalid_drafts") {
        let name = text(case, "name");
        let path = text(case.get("expect").unwrap(), "path");
        let member = match (name, path) {
            ("at_version_2", _) => "schema_version",
            ("cool_off_ends.form", _) => continue,
            (_, "payload.challenge") => "payload.challenge",
            (_, "payload.credential") => "payload.credential",
            (_, "payload.cool_off_ends") => "payload.cool_off_ends",
            _ => continue,
        };
        let envelope = vector_draft(&section, case);
        let refused = CredentialError::Unreadable { seq: 4, member };
        assert_eq!(waiver(4, &envelope), Err(refused), "{name}");
        assert_eq!(credential_record(4, &envelope), Err(refused), "{name}");
        checked += 1;
    }
    assert!(checked >= 9, "{checked} unreadable waivers checked");
}

/// The fold finds each rule-318 stream's first break at the `seq` the vectors name, and none in a
/// valid stream.
#[test]
#[ignore = "pending E9-4"]
fn the_fold_finds_each_rule_318_break_where_the_vectors_name_it() {
    let section = section();
    let streams = list(&section, "streams");
    assert!(streams.len() >= 17, "the vectors hold rule 318's streams");
    for stream in streams {
        let fold = CredentialFold::new(records(stream));
        assert_eq!(
            fold.first_waiver_break(),
            Ok(expected_break(stream)),
            "{}",
            text(stream, "name")
        );
    }
}

/// Step-up's view fails closed: on a stream that breaks rule 318, the member of the breaking
/// record and the credential's holder have no passkey that counts, from the first `seq` that breaks
/// rule 318 or rule 254 for them. On a valid stream each member's passkeys and counters are those
/// rule 254 alone leaves, the stream without its waivers and with each assertion past every
/// cool-off: a waiver changes no counter. Route 2's view reads no waiver.
#[test]
#[ignore = "pending E9-4"]
fn step_up_fails_closed_from_the_first_rule_318_break_and_route_two_reads_no_waiver() {
    let section = section();
    for stream in list(&section, "streams") {
        let name = text(stream, "name");
        let all = records(stream);
        let fold = CredentialFold::new(all.iter().cloned());
        let without: Vec<CredentialRecord> = all
            .iter()
            .filter(|r| !matches!(r, CredentialRecord::Waived(_)))
            .cloned()
            .collect();
        let unwaived = CredentialFold::new(without.iter().map(past_every_cool_off));
        let expected = expected_break(stream);
        assert_eq!(
            fold.first_waiver_break().map(|b| b.is_some()),
            Ok(expected.is_some()),
            "{name}"
        );
        let mut members: BTreeSet<PrincipalId> = BTreeSet::new();
        let mut credentials: BTreeSet<&str> = BTreeSet::new();
        for record in &all {
            let (member, credential) = match record {
                CredentialRecord::Enrolled(e) => (e.member, e.credential.as_str()),
                CredentialRecord::Removed(r) => (r.member, r.credential.as_str()),
                CredentialRecord::Asserted(a) => (a.member, a.credential.as_str()),
                CredentialRecord::Waived(w) => (w.member, w.credential.as_str()),
            };
            members.insert(member);
            credentials.insert(credential);
        }
        let route_two = CredentialFold::new(
            without
                .iter()
                .filter(|r| !matches!(r, CredentialRecord::Asserted(_)))
                .cloned(),
        );
        for credential in &credentials {
            assert_eq!(
                fold.held(credential),
                route_two.held(credential),
                "{name}: route 2 reads no waiver, for {credential}"
            );
        }
        let Some(seq) = expected else {
            for member in &members {
                assert_eq!(fold.passkeys(*member), unwaived.passkeys(*member), "{name}");
            }
            for credential in &credentials {
                assert_eq!(
                    fold.last_count(credential),
                    unwaived.last_count(credential),
                    "{name}"
                );
            }
            continue;
        };
        let index = usize::try_from(seq - 1).unwrap();
        let (breaker, credential) = match &all[index] {
            CredentialRecord::Asserted(a) => (a.member, a.credential.as_str()),
            CredentialRecord::Waived(w) => (w.member, w.credential.as_str()),
            other => panic!("{name}: rule 318 breaks at {other:?}"),
        };
        let holder = all[..index].iter().rev().find_map(|r| match r {
            CredentialRecord::Enrolled(e) if e.credential == credential => Some(e.member),
            _ => None,
        });
        for member in std::iter::once(breaker).chain(holder) {
            let first = match unwaived.passkeys(member) {
                Err(CredentialError::Broken { seq: earlier }) => earlier.min(seq),
                _ => seq,
            };
            assert_eq!(
                fold.passkeys(member),
                Err(CredentialError::Broken { seq: first }),
                "{name}"
            );
        }
    }
}

/// Rule 318's converse wants the waiver immediately after its assertion on the stream, the very
/// next `seq`. A waiver of the same challenge after any other row (here one at `seq` 3 that no
/// credential record maps) does not settle it: the rule breaks at the assertion, for the member.
#[test]
#[ignore = "pending E9-4"]
fn a_waiver_after_another_row_does_not_settle_its_assertion() {
    let fold = CredentialFold::new([
        record(1, Step::Enrol(0, 0, true)),
        record(2, Step::Assert(0, 0, 0, 0)),
        record(4, Step::Waive(0, 0, 0, 0, 0)),
    ]);
    assert_eq!(fold.first_waiver_break(), Ok(Some(2)));
    assert_eq!(
        fold.passkeys(member(0)),
        Err(CredentialError::Broken { seq: 2 })
    );
    let immediate = CredentialFold::new([
        record(1, Step::Enrol(0, 0, true)),
        record(2, Step::Assert(0, 0, 0, 0)),
        record(3, Step::Waive(0, 0, 0, 0, 0)),
    ]);
    assert_eq!(immediate.first_waiver_break(), Ok(None));
}

/// One random step among two members, three credentials and three challenges: an enrolment with
/// or without a cool-off, a removal, an assertion at one of three instants, or a waiver of one of
/// two ends at one of three instants.
#[derive(Debug, Clone, Copy)]
enum Step {
    Enrol(u8, u8, bool),
    Remove(u8, u8),
    Assert(u8, u8, u8, u8),
    Waive(u8, u8, u8, u8, u8),
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        (0..2_u8, 0..3_u8, any::<bool>()).prop_map(|(m, c, cools)| Step::Enrol(m, c, cools)),
        (0..2_u8, 0..3_u8).prop_map(|(m, c)| Step::Remove(m, c)),
        (0..2_u8, 0..3_u8, 0..3_u8, 0..3_u8).prop_map(|(m, c, h, t)| Step::Assert(m, c, h, t)),
        (0..2_u8, 0..3_u8, 0..3_u8, 0..2_u8, 0..3_u8)
            .prop_map(|(m, c, h, e, t)| Step::Waive(m, c, h, e, t)),
    ]
}

const T0: i64 = 1_790_000_000;

/// One of three instants a step is taken at: before, at, and after the one cool-off end.
fn at(t: u8) -> UtcNanos {
    UtcNanos::from_parts(T0 + 100 * i64::from(t), 0).unwrap()
}

/// The one `cool_off_ends` an enrolment with a cool-off carries, `at(1)`, and the other end a
/// waiver may name instead, `at(2)`.
fn end(e: u8) -> UtcNanos {
    at(1 + e)
}

fn member(m: u8) -> PrincipalId {
    PrincipalId(u128::from(m) + 1)
}

fn credential(c: u8) -> String {
    format!("credential-{c}-AAAAAAAAAAAAA")
}

fn challenge(h: u8) -> String {
    format!("challenge-{h}")
}

fn record(seq: u64, step: Step) -> CredentialRecord {
    match step {
        Step::Enrol(m, c, cools) => CredentialRecord::Enrolled(Enrolment {
            seq,
            member: member(m),
            credential: credential(c),
            public_key: format!("sha256:{seq:064}"),
            alg: PasskeyAlg::EdDsa,
            sign_count: 0,
            cool_off_ends: cools.then(|| end(0)),
            backup_eligible: Some(false),
        }),
        Step::Remove(m, c) => CredentialRecord::Removed(Removal {
            seq,
            member: member(m),
            credential: credential(c),
        }),
        Step::Assert(m, c, h, t) => CredentialRecord::Asserted(Assertion {
            seq,
            member: member(m),
            credential: credential(c),
            challenge: challenge(h),
            sign_count: u32::try_from(seq).unwrap(),
            event_time: at(t),
        }),
        Step::Waive(m, c, h, e, t) => CredentialRecord::Waived(Waiver {
            seq,
            member: member(m),
            credential: credential(c),
            challenge: challenge(h),
            cool_off_ends: end(e),
            event_time: at(t),
        }),
    }
}

/// One credential id's history for the oracle: how many enrolments and removals name it, the
/// member who enrolled it and whether with a cool-off, while rule 251 keeps that enrolment, and
/// whether that member then removed it.
type History = (u32, Option<(u8, bool)>, bool);

/// The oracle: rule 318 as journal spec §9.38 words it, rebuilt here from each credential's own
/// history, the challenges waived so far, and the one assertion, if any, that the very next record
/// must waive. A credential is held by whoever enrolled it while rule 251 keeps that history: an
/// enrolment counts only as the id's first record, and a removal only as its second, by the member
/// who enrolled it.
fn oracle(steps: &[Step]) -> Option<u64> {
    let mut history: BTreeMap<u8, History> = BTreeMap::new();
    let mut waived: BTreeSet<u8> = BTreeSet::new();
    let mut owed: Option<(u64, u8)> = None;
    for (step, seq) in steps.iter().zip(1_u64..) {
        if let Some((assertion, h)) = owed.take()
            && !matches!(step, Step::Waive(_, _, wh, _, _) if *wh == h)
        {
            return Some(assertion);
        }
        let held = |c: u8| match history.get(&c) {
            Some((_, Some(enrolment), false)) => Some(*enrolment),
            _ => None,
        };
        match *step {
            Step::Enrol(m, c, cools) => {
                let entry = history.entry(c).or_insert((0, None, false));
                if entry.0 == 0 {
                    entry.1 = Some((m, cools));
                }
                entry.0 += 1;
            }
            Step::Remove(m, c) => {
                let entry = history.entry(c).or_insert((0, None, false));
                if entry.0 == 1 && entry.1.is_some_and(|(by, _)| by == m) {
                    entry.2 = true;
                }
                entry.0 += 1;
            }
            Step::Assert(m, c, h, t) => {
                if held(c).is_some_and(|(by, cools)| by == m && cools && at(t) < end(0)) {
                    owed = Some((seq, h));
                }
            }
            Step::Waive(m, c, h, e, t) => {
                let keeps = held(c).is_some_and(|(by, cools)| by == m && cools && e == 0)
                    && end(e) > at(t)
                    && !waived.contains(&h);
                if !keeps {
                    return Some(seq);
                }
                waived.insert(h);
            }
        }
    }
    owed.map(|(assertion, _)| assertion)
}

/// Over random streams of enrolments, removals, assertions and waivers, the fold's first
/// rule-318 break is the oracle's. The runner keeps no failure file (DEC-381).
#[test]
#[ignore = "pending E9-4"]
fn the_first_rule_318_break_is_the_oracles() {
    let config = ProptestConfig {
        cases: 512,
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    let mut runner = TestRunner::new(config);
    let outcome = runner.run(&prop::collection::vec(step(), 0..12), |steps| {
        let records: Vec<CredentialRecord> = steps
            .iter()
            .zip(1_u64..)
            .map(|(s, seq)| record(seq, *s))
            .collect();
        let fold = CredentialFold::new(records);
        prop_assert_eq!(fold.first_waiver_break(), Ok(oracle(&steps)));
        Ok(())
    });
    if let Err(failure) = outcome {
        panic!("{failure}");
    }
}
