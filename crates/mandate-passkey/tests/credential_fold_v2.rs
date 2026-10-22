//! The credentials fold reads journal spec v0.71 (DEC-2122, DEC-2323; E9-4 S3): `CredentialEnrolled` version
//! 2's `backup_eligible`, and `StepUpAsserted`'s counter under stream rule 254. Through the
//! vectors' `credential_enrolled_v2` and `step_up_assertions` sections, and a property test against
//! a replay that keeps each credential's counter apart from the fold. Step-up's view fails closed
//! on a breaking assertion; route 2's view reads no counter.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mandate_canon::{Key, Object, Value, decode_ulid, parse};
use mandate_identity::PrincipalId;
use mandate_passkey::credentials::{
    Assertion, CredentialError, CredentialFold, CredentialRecord, Enrolment, PasskeyAlg,
    credential_record,
};
use proptest::prelude::*;

fn section(name: &str) -> Value {
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

fn text<'a>(value: &'a Value, path: &str) -> &'a str {
    path.split('.')
        .try_fold(value, |node, name| node.get(name))
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn principal(ulid: &str) -> PrincipalId {
    PrincipalId(decode_ulid(ulid).unwrap())
}

/// A case's base draft with its `{path, value}` or `{path, delete: true}` changes applied.
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
        if change.get("delete") == Some(&Value::Bool(true)) {
            node.remove(inner);
        } else {
            node.insert(
                Key::new(inner).unwrap(),
                change.get("value").cloned().unwrap(),
            );
        }
    }
    Value::Object(body)
}

/// A stream's records, mapped, at `seq` 1 onwards.
fn records(stream: &Value) -> Vec<CredentialRecord> {
    list(stream, "records")
        .iter()
        .zip(1..)
        .map(|(envelope, seq)| {
            let mapped = credential_record(seq, envelope);
            match mapped {
                Ok(Some(record)) => record,
                other => panic!("{} record {seq}: {other:?}", text(stream, "name")),
            }
        })
        .collect()
}

/// The index rule 254 first breaks at, as the vectors expect it, or `None` for a valid stream.
fn expected_break(stream: &Value) -> Option<usize> {
    let expect = stream.get("expect").unwrap();
    (text(expect, "outcome") == "Invalid").then(|| {
        let index = expect.get("index").and_then(Value::as_int).unwrap();
        usize::try_from(index).unwrap()
    })
}

/// Each version 2 base and valid enrolment maps with its `backup_eligible` read, and a version 1
/// enrolment maps with none.
#[test]
#[ignore = "pending E9-4"]
fn version_2_enrolments_map_with_their_backup_flag() {
    let v2 = section("credential_enrolled_v2");
    let drafts = v2.get("drafts").and_then(Value::as_object).unwrap();
    let cases = drafts.values().cloned().chain(
        list(&v2, "valid_drafts")
            .iter()
            .map(|c| vector_draft(&v2, c)),
    );
    for (envelope, seq) in cases.zip(1..) {
        let mapped = credential_record(seq, &envelope);
        let Ok(Some(CredentialRecord::Enrolled(e))) = mapped else {
            panic!("version 2 maps to an enrolment: {mapped:?}");
        };
        let flag = envelope
            .get("payload")
            .and_then(|p| p.get("backup_eligible"));
        assert_eq!(e.backup_eligible.map(Value::Bool).as_ref(), flag);
        assert_eq!(e.credential, text(&envelope, "payload.credential"));
        assert_eq!(e.member, principal(text(&envelope, "payload.member")));
    }
    let first = section("credentials");
    let v1 = first
        .get("drafts")
        .and_then(|d| d.get("first_enrolled"))
        .unwrap();
    let Ok(Some(CredentialRecord::Enrolled(e))) = credential_record(1, v1) else {
        panic!("version 1 maps to an enrolment");
    };
    assert_eq!(e.backup_eligible, None);
}

/// Every invalid version 2 enrolment whose fault is `backup_eligible` is refused there; a
/// journal `unknown_schema` at `payload` is the mapping's `schema_version`.
#[test]
#[ignore = "pending E9-4"]
fn each_invalid_version_2_flag_is_refused_at_backup_eligible() {
    let section = section("credential_enrolled_v2");
    let mut checked = 0;
    for case in list(&section, "invalid_drafts") {
        let expect = case.get("expect").unwrap();
        if text(expect, "path") != "payload.backup_eligible" {
            continue;
        }
        assert_eq!(
            credential_record(4, &vector_draft(&section, case)),
            Err(CredentialError::Unreadable {
                seq: 4,
                member: "payload.backup_eligible"
            }),
            "{}",
            text(case, "name")
        );
        checked += 1;
    }
    assert!(checked >= 4, "{checked} invalid flags checked");
}

/// `StepUpAsserted`'s base and valid drafts map to an assertion: its actor as member, credential,
/// challenge and counter.
#[test]
#[ignore = "pending E9-4"]
fn step_up_asserted_maps_its_member_credential_challenge_and_counter() {
    let section = section("step_up_assertions");
    let base = section
        .get("drafts")
        .and_then(|d| d.get("step_up_asserted"))
        .unwrap();
    let cases = std::iter::once(base.clone()).chain(
        list(&section, "valid_drafts")
            .iter()
            .map(|c| vector_draft(&section, c)),
    );
    for (envelope, seq) in cases.zip(1..) {
        let sign_count = envelope
            .get("payload")
            .and_then(|p| p.get("sign_count"))
            .and_then(Value::as_int)
            .and_then(|n| u32::try_from(n).ok())
            .unwrap();
        let expected = CredentialRecord::Asserted(Assertion {
            seq,
            member: principal(text(&envelope, "actor.id")),
            credential: text(&envelope, "payload.credential").to_owned(),
            challenge: text(&envelope, "payload.challenge").to_owned(),
            sign_count,
        });
        assert_eq!(credential_record(seq, &envelope), Ok(Some(expected)));
    }
}

/// Every invalid `StepUpAsserted` whose fault is a member the mapping reads is refused at that
/// member's path, and another schema version at `schema_version`.
#[test]
#[ignore = "pending E9-4"]
fn each_invalid_step_up_asserted_is_refused_at_the_member_the_mapping_reads() {
    let section = section("step_up_assertions");
    let read = [
        "payload.challenge",
        "payload.credential",
        "payload.sign_count",
        "payload.assertion",
        "actor.id",
    ];
    let mut checked = 0;
    for case in list(&section, "invalid_drafts") {
        let expect = case.get("expect").unwrap();
        let path: &'static str = match (text(expect, "path"), text(expect, "reason")) {
            ("payload", "unknown_schema") => "schema_version",
            (path, _) => match read.iter().find(|p| **p == path) {
                Some(p) => p,
                None => continue,
            },
        };
        assert_eq!(
            credential_record(5, &vector_draft(&section, case)),
            Err(CredentialError::Unreadable {
                seq: 5,
                member: path
            }),
            "{}",
            text(case, "name")
        );
        checked += 1;
    }
    assert!(checked >= 13, "{checked} invalid assertions checked");
}

/// Every rule-254 stream's records are admitted one by one up to the index the vectors name, and
/// the record there is refused `Breaks`.
#[test]
#[ignore = "pending E9-4"]
fn admits_refuses_each_rule_254_stream_at_the_index_the_vectors_name() {
    let section = section("step_up_assertions");
    for stream in list(&section, "streams") {
        let all = records(stream);
        let stop = expected_break(stream).unwrap_or(all.len());
        for (i, next) in all.iter().enumerate().take(stop + 1) {
            let fold = CredentialFold::new(all[..i].iter().cloned());
            let expected = if i == stop {
                Err(CredentialError::Breaks)
            } else {
                Ok(())
            };
            assert_eq!(
                fold.admits(next),
                expected,
                "{} at {i}",
                text(stream, "name")
            );
        }
    }
}

/// On a valid stream each held credential's last count is the greatest its assertions carried,
/// else its enrolment's. On a breaking stream the asserting member and the credential's holder
/// are disabled for step-up from the breaking `seq`, and route 2 still holds what the records
/// that keep rule 251 leave.
#[test]
#[ignore = "pending E9-4"]
fn step_up_reads_the_greatest_kept_counter_and_fails_closed_on_rule_254() {
    let section = section("step_up_assertions");
    for stream in list(&section, "streams") {
        let name = text(stream, "name");
        let all = records(stream);
        let fold = CredentialFold::new(all.iter().cloned());
        let without: Vec<CredentialRecord> = all
            .iter()
            .filter(|r| !matches!(r, CredentialRecord::Asserted(_)))
            .cloned()
            .collect();
        let route_two = CredentialFold::new(without);
        let mut last: BTreeMap<String, u32> = BTreeMap::new();
        for record in &all {
            match record {
                CredentialRecord::Enrolled(e) => {
                    last.insert(e.credential.clone(), e.sign_count);
                }
                CredentialRecord::Removed(r) => {
                    last.remove(&r.credential);
                }
                CredentialRecord::Asserted(a) => {
                    if let Some(count) = last.get_mut(&a.credential) {
                        *count = (*count).max(a.sign_count);
                    }
                }
            }
        }
        let ids: BTreeSet<&str> = all
            .iter()
            .map(|r| match r {
                CredentialRecord::Enrolled(e) => e.credential.as_str(),
                CredentialRecord::Removed(r) => r.credential.as_str(),
                CredentialRecord::Asserted(a) => a.credential.as_str(),
            })
            .collect();
        for credential in ids {
            assert_eq!(
                fold.held(credential),
                route_two.held(credential),
                "{name}: route 2 reads no assertion, for {credential}"
            );
        }
        let Some(index) = expected_break(stream) else {
            for (credential, count) in &last {
                assert_eq!(fold.last_count(credential), Ok(Some(*count)), "{name}");
            }
            continue;
        };
        let CredentialRecord::Asserted(breaking) = &all[index] else {
            panic!("{name}: rule 254 breaks at an assertion");
        };
        let seq = u64::try_from(index + 1).unwrap();
        let holder = all[..index].iter().rev().find_map(|r| match r {
            CredentialRecord::Enrolled(e) if e.credential == breaking.credential => Some(e.member),
            _ => None,
        });
        for member in std::iter::once(breaking.member).chain(holder) {
            assert_eq!(
                fold.passkeys(member),
                Err(CredentialError::Broken { seq }),
                "{name}"
            );
        }
    }
}

/// A version 1 credential and a version 2 one on one stream: each keeps its own flag and counter.
#[test]
#[ignore = "pending E9-4"]
fn a_version_1_and_a_version_2_credential_keep_their_own_flag_and_counter() {
    let section = section("step_up_assertions");
    let stream = list(&section, "streams")
        .iter()
        .find(|s| text(s, "name") == "version_1_then_version_2")
        .cloned()
        .unwrap();
    let all = records(&stream);
    let fold = CredentialFold::new(all.iter().cloned());
    let flags: Vec<(String, Option<bool>)> = all
        .iter()
        .filter_map(|r| match r {
            CredentialRecord::Enrolled(e) => Some((e.credential.clone(), e.backup_eligible)),
            _ => None,
        })
        .collect();
    assert_eq!(flags.len(), 2);
    assert_eq!(flags[0].1, None, "the version 1 credential carries no flag");
    assert_eq!(
        flags[1].1,
        Some(false),
        "the version 2 credential carries its flag"
    );
    for (credential, _) in &flags {
        let asserted = all.iter().find_map(|r| match r {
            CredentialRecord::Asserted(a) if &a.credential == credential => Some(a.sign_count),
            _ => None,
        });
        assert_eq!(fold.last_count(credential), Ok(asserted), "{credential}");
    }
}

/// One random step: member, credential, and what it does, an enrolment, a removal, or an
/// assertion with a counter over a challenge.
#[derive(Debug, Clone, Copy)]
enum Step {
    Enrol(u8, u8, u8),
    Remove(u8, u8),
    Assert(u8, u8, u8, u8),
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        (0..2_u8, 0..3_u8, 0..4_u8).prop_map(|(m, c, n)| Step::Enrol(m, c, n)),
        (0..2_u8, 0..3_u8).prop_map(|(m, c)| Step::Remove(m, c)),
        (0..2_u8, 0..3_u8, 0..8_u8, 0..4_u8).prop_map(|(m, c, n, h)| Step::Assert(m, c, n, h)),
    ]
}

fn member(m: u8) -> PrincipalId {
    PrincipalId(u128::from(m) + 1)
}

fn credential(c: u8) -> String {
    format!("credential-{c}-AAAAAAAAAAAAA")
}

fn record(seq: u64, step: Step) -> CredentialRecord {
    match step {
        Step::Enrol(m, c, n) => CredentialRecord::Enrolled(Enrolment {
            seq,
            member: member(m),
            credential: credential(c),
            public_key: format!("sha256:{seq:064}"),
            alg: PasskeyAlg::EdDsa,
            sign_count: u32::from(n),
            cool_off_ends: None,
            backup_eligible: Some(false),
        }),
        Step::Remove(m, c) => CredentialRecord::Removed(mandate_passkey::credentials::Removal {
            seq,
            member: member(m),
            credential: credential(c),
        }),
        Step::Assert(m, c, n, h) => CredentialRecord::Asserted(Assertion {
            seq,
            member: member(m),
            credential: credential(c),
            challenge: format!("challenge-{h}"),
            sign_count: u32::from(n),
        }),
    }
}

/// The oracle: rule 251 by each credential's history and rule 254 by each credential's counter,
/// kept apart from the fold. It gives the first breaking `seq` per member, each held credential's
/// holder and last count, and whether the last record broke either rule.
fn replay(steps: &[Step]) -> Replayed {
    let mut history: BTreeMap<String, Vec<(bool, PrincipalId)>> = BTreeMap::new();
    let mut holder: BTreeMap<String, PrincipalId> = BTreeMap::new();
    let mut last: BTreeMap<String, u32> = BTreeMap::new();
    let mut used: Vec<String> = Vec::new();
    let mut broken: BTreeMap<PrincipalId, u64> = BTreeMap::new();
    let mut last_breaks = false;
    for (step, seq) in steps.iter().zip(1..) {
        let breaks: Vec<PrincipalId> = match *step {
            Step::Enrol(m, c, n) => {
                let events = history.entry(credential(c)).or_default();
                events.push((true, member(m)));
                if events.len() == 1 {
                    holder.insert(credential(c), member(m));
                    last.insert(credential(c), u32::from(n));
                    Vec::new()
                } else {
                    let first = events[0];
                    let mut out = vec![member(m)];
                    if first.0 && first.1 != member(m) {
                        out.push(first.1);
                    }
                    out
                }
            }
            Step::Remove(m, c) => {
                let events = history.entry(credential(c)).or_default();
                events.push((false, member(m)));
                let keeps = events.len() == 2 && events[0] == (true, member(m));
                if keeps {
                    holder.remove(&credential(c));
                    last.remove(&credential(c));
                    Vec::new()
                } else {
                    let first = events[0];
                    let mut out = vec![member(m)];
                    if first.0 && first.1 != member(m) {
                        out.push(first.1);
                    }
                    out
                }
            }
            Step::Assert(m, c, n, h) => {
                let id = credential(c);
                let challenge = format!("challenge-{h}");
                let before = last.get(&id).copied();
                let own = holder.get(&id) == Some(&member(m));
                let rises = before.is_some_and(|b| u32::from(n) > b || (n == 0 && b == 0));
                let fresh = !used.contains(&challenge);
                if own && rises && fresh {
                    used.push(challenge);
                    last.insert(id, u32::from(n));
                    Vec::new()
                } else {
                    let mut out = vec![member(m)];
                    if let Some(h) = holder.get(&id).filter(|h| **h != member(m)) {
                        out.push(*h);
                    }
                    out
                }
            }
        };
        last_breaks = !breaks.is_empty();
        for m in breaks {
            broken.entry(m).or_insert(seq);
        }
    }
    Replayed {
        broken,
        holder,
        last,
        last_breaks,
    }
}

/// What the oracle finds.
struct Replayed {
    broken: BTreeMap<PrincipalId, u64>,
    holder: BTreeMap<String, PrincipalId>,
    last: BTreeMap<String, u32>,
    last_breaks: bool,
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Over random streams of enrolments, removals and assertions among two members and three
    /// ids, the fold agrees with the replay on every id's last count (asked first, so a pending run
    /// stops at the stub), every member's step-up, and whether the next record is admitted; and
    /// route 2 holds for every id what the same records hold with the assertions taken out.
    #[test]
    #[ignore = "pending E9-4"]
    fn the_fold_reads_counters_as_an_independent_replay_does(
        steps in prop::collection::vec(step(), 0..20),
        next in step(),
    ) {
        let stream: Vec<CredentialRecord> =
            steps.iter().zip(1..).map(|(s, seq)| record(seq, *s)).collect();
        let Replayed { broken, holder, last, .. } = replay(&steps);
        let fold = CredentialFold::new(stream.iter().cloned());
        let route_two = CredentialFold::new(
            stream.iter().filter(|r| !matches!(r, CredentialRecord::Asserted(_))).cloned(),
        );
        for id in (0..3).map(credential) {
            let want = match (holder.get(&id), last.get(&id)) {
                (Some(h), _) if broken.contains_key(h) => {
                    Err(CredentialError::Broken { seq: broken[h] })
                }
                (Some(_), Some(count)) => Ok(Some(*count)),
                _ => Ok(None),
            };
            prop_assert_eq!(fold.last_count(&id), want, "last count of {}", &id);
        }
        for id in (0..3).map(credential) {
            prop_assert_eq!(fold.held(&id), route_two.held(&id), "route 2 holds {}", &id);
        }
        for m in (0..2).map(member) {
            let got = fold.passkeys(m).map(|_| ());
            let want = broken.get(&m).map_or(Ok(()), |seq| Err(CredentialError::Broken { seq: *seq }));
            prop_assert_eq!(got, want);
        }
        let mut extended = steps.clone();
        extended.push(next);
        let seq = u64::try_from(stream.len() + 1).unwrap();
        let breaks_now = replay(&extended).last_breaks;
        prop_assert_eq!(fold.admits(&record(seq, next)).is_err(), breaks_now);
    }
}
