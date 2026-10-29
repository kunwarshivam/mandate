//! The credentials fold (journal spec §9.31 rule 251, DEC-2108 items 2 and 8, DEC-2121; E9-4
//! S3.2): [`credential_record`] maps the vectors' records, and [`CredentialFold`] keeps each
//! member's unremoved passkeys, refuses every append rule 251 refuses at the index the vectors
//! name, and fails closed for the members a breaking record names. The property test replays
//! random streams against an oracle that keeps each credential's history apart from the fold.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mandate_canon::{Int, Key, Object, Value, decode_ulid, parse};
use mandate_identity::PrincipalId;
use mandate_passkey::credentials::{
    Assertion, CredentialError, CredentialFold, CredentialRecord, Enrolment, PasskeyAlg, Removal,
    credential_record,
};
use mandate_time::UtcNanos;
use proptest::prelude::*;

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("credentials").cloned().unwrap()
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

/// A stream's records, mapped, at `seq` 1 onwards.
fn records(stream: &Value) -> Vec<CredentialRecord> {
    list(stream, "records")
        .iter()
        .zip(1..)
        .map(|(envelope, seq)| credential_record(seq, envelope).unwrap().unwrap())
        .collect()
}

/// The index rule 251 first breaks at, as the vectors expect it, or `None` for a valid stream.
fn expected_break(stream: &Value) -> Option<usize> {
    let expect = stream.get("expect").unwrap();
    (text(expect, "outcome") == "Invalid").then(|| {
        let index = expect.get("index").and_then(Value::as_int).unwrap();
        usize::try_from(index).unwrap()
    })
}

fn credential_of(record: &CredentialRecord) -> (&str, PrincipalId) {
    match record {
        CredentialRecord::Enrolled(e) => (&e.credential, e.member),
        CredentialRecord::Removed(r) => (&r.credential, r.member),
        CredentialRecord::Asserted(a) => panic!("no credential record at seq {}", a.seq),
        CredentialRecord::Waived(w) => panic!("no credential record at seq {}", w.seq),
    }
}

/// Every vector stream's records are admitted one by one up to the index the vectors name, and
/// the record there is refused `Breaks`; a valid stream is admitted whole.
#[test]
fn admits_refuses_each_stream_at_the_index_the_vectors_name() {
    let section = section();
    let streams = list(&section, "streams");
    assert!(streams.len() >= 8, "the vectors hold rule 251's streams");
    for stream in streams {
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

/// A valid stream's members hold what they enrolled and did not remove, in enrolment order, and
/// `held` names each credential's holder.
#[test]
fn a_valid_stream_holds_each_members_unremoved_passkeys_in_enrolment_order() {
    for stream in list(&section(), "streams") {
        if expected_break(stream).is_some() {
            continue;
        }
        let all = records(stream);
        let mut kept: Vec<(PrincipalId, String)> = Vec::new();
        for record in &all {
            let (credential, member) = credential_of(record);
            match record {
                CredentialRecord::Enrolled(_) => kept.push((member, credential.to_owned())),
                CredentialRecord::Removed(_) => kept.retain(|(_, c)| c != credential),
                CredentialRecord::Asserted(_) | CredentialRecord::Waived(_) => {}
            }
        }
        let fold = CredentialFold::new(all.iter().cloned());
        let members: BTreeSet<PrincipalId> = all.iter().map(|r| credential_of(r).1).collect();
        for member in members {
            let got: Vec<&str> = fold
                .passkeys(member)
                .unwrap()
                .into_iter()
                .map(|e| e.credential.as_str())
                .collect();
            let want: Vec<&str> = kept
                .iter()
                .filter(|(m, _)| *m == member)
                .map(|(_, c)| c.as_str())
                .collect();
            assert_eq!(got, want, "{}", text(stream, "name"));
        }
        for record in &all {
            let (credential, _) = credential_of(record);
            let holder = kept.iter().find(|(_, c)| c == credential).map(|(m, _)| *m);
            let got = fold.held(credential).map(|e| e.member);
            assert_eq!(got, holder, "{}: {credential}", text(stream, "name"));
        }
    }
}

/// The record that breaks rule 251 disables step-up for its own member and for the member who
/// first enrolled the credential it names, from its `seq` on. Route 2's `held` still keeps the
/// first enrolment of the id, unless the enroller's own removal followed it.
#[test]
fn a_breaking_record_disables_step_up_for_both_members_and_the_first_enrolment_still_holds() {
    for stream in list(&section(), "streams") {
        let Some(index) = expected_break(stream) else {
            continue;
        };
        let all = records(stream);
        let (credential, member) = credential_of(&all[index]);
        let first = all[..index].iter().find_map(|r| match r {
            CredentialRecord::Enrolled(e) if e.credential == credential => Some(e.member),
            _ => None,
        });
        let seq = u64::try_from(index + 1).unwrap();
        let fold = CredentialFold::new(all.iter().cloned());
        for broken in std::iter::once(member).chain(first) {
            assert_eq!(
                fold.passkeys(broken),
                Err(CredentialError::Broken { seq }),
                "{}",
                text(stream, "name")
            );
        }
        let mut history = all.iter().filter(|r| credential_of(r).0 == credential);
        let holder = match (history.next(), history.next()) {
            (Some(CredentialRecord::Enrolled(e)), Some(CredentialRecord::Removed(r)))
                if r.member == e.member =>
            {
                None
            }
            (Some(CredentialRecord::Enrolled(e)), _) => Some(e.member),
            _ => None,
        };
        assert_eq!(
            fold.held(credential).map(|e| e.member),
            holder,
            "{}",
            text(stream, "name")
        );
    }
}

/// `admits` judges only the next record: a record that keeps rule 251 is admitted from a member
/// an earlier record broke, and step-up still discounts that member (§9.31).
#[test]
fn admits_a_clean_record_from_a_broken_member_whom_step_up_still_discounts() {
    let stream = list(&section(), "streams")
        .iter()
        .find(|s| text(s, "name") == "enrolled_twice")
        .cloned()
        .unwrap();
    let all = records(&stream);
    let CredentialRecord::Enrolled(base) = &all[0] else {
        panic!("enrolled_twice opens with an enrolment");
    };
    let fresh = CredentialRecord::Enrolled(Enrolment {
        seq: 3,
        credential: "fresh-credential-AAAAAAAA".to_owned(),
        ..base.clone()
    });
    let fold = CredentialFold::new(all.iter().cloned());
    assert_eq!(fold.admits(&fresh), Ok(()));
    let after = CredentialFold::new(all.iter().cloned().chain([fresh]));
    assert_eq!(
        after.passkeys(base.member),
        Err(CredentialError::Broken { seq: 2 })
    );
}

/// Each base draft maps to its record, member by member, and a record §9.31 does not add maps to
/// `None`.
#[test]
fn credential_record_maps_both_base_drafts_and_passes_over_other_records() {
    let section = section();
    let drafts = section.get("drafts").unwrap();
    let enrolled = drafts.get("first_enrolled").unwrap();
    let Some(CredentialRecord::Enrolled(e)) = credential_record(7, enrolled).unwrap() else {
        panic!("the enrolment base draft is an enrolment");
    };
    let payload = |name: &str| text(enrolled, &format!("payload.{name}"));
    assert_eq!(e.seq, 7);
    assert_eq!(e.member, principal(payload("member")));
    assert_eq!(e.credential, payload("credential"));
    assert_eq!(e.public_key, payload("public_key"));
    assert_eq!(e.alg, PasskeyAlg::Es256);
    let count = enrolled.get("payload").and_then(|p| p.get("sign_count"));
    assert_eq!(Some(u64::from(e.sign_count)), count.and_then(Value::as_int));
    let cool_off = UtcNanos::parse(payload("cool_off_ends")).ok();
    assert_eq!(e.cool_off_ends, cool_off);
    assert!(cool_off.is_some(), "a first enrolment waits its cool-off");
    let stepped = drafts.get("stepped_up_enrolled").unwrap();
    let Some(CredentialRecord::Enrolled(s)) = credential_record(9, stepped).unwrap() else {
        panic!("the stepped-up base draft is an enrolment");
    };
    assert_eq!(s.cool_off_ends, None);
    let mut dashed = enrolled.as_object().cloned().unwrap();
    if let Some(Value::Object(p)) = dashed.get_mut("payload") {
        p.insert(
            Key::new("credential").unwrap(),
            Value::Str("AAEC-wQFBgcI_QoLDA0ODw".to_owned()),
        );
    }
    let mapped = credential_record(10, &Value::Object(dashed));
    assert!(
        matches!(mapped, Ok(Some(CredentialRecord::Enrolled(_)))),
        "an id may hold `-` and `_`: {mapped:?}"
    );

    let removed = drafts.get("removed").unwrap();
    let expected = CredentialRecord::Removed(Removal {
        seq: 8,
        member: principal(text(removed, "payload.member")),
        credential: text(removed, "payload.credential").to_owned(),
    });
    assert_eq!(credential_record(8, removed), Ok(Some(expected)));

    let mut other = enrolled.as_object().cloned().unwrap();
    other.insert(
        Key::new("event_type").unwrap(),
        Value::Str("StreamOpened".to_owned()),
    );
    assert_eq!(credential_record(1, &Value::Object(other)), Ok(None));
}

/// A §9.31 record maps whole or not at all: another schema version, or a member absent or of
/// another type, is refused at its path.
#[test]
fn credential_record_refuses_a_record_that_does_not_map_at_its_path() {
    let section = section();
    let enrolled = section
        .get("drafts")
        .and_then(|d| d.get("first_enrolled"))
        .unwrap();
    let changed = |path: &'static str, value: Option<Value>| {
        let mut body: Object = enrolled.as_object().cloned().unwrap();
        let (outer, inner) = path.split_once('.').unwrap_or(("", path));
        let node = if outer.is_empty() {
            &mut body
        } else {
            match body.get_mut(outer) {
                Some(Value::Object(node)) => node,
                _ => panic!("{path}"),
            }
        };
        match value {
            Some(value) => node.insert(Key::new(inner).unwrap(), value),
            None => node.remove(inner),
        };
        (path, Value::Object(body))
    };
    for (path, envelope) in [
        changed("schema_version", Some(Value::Int(Int::new(3).unwrap()))),
        changed("payload.member", Some(Value::Str("not a ulid".to_owned()))),
        changed("payload.credential", None),
        changed("payload.public_key", Some(Value::Null)),
        changed("payload.alg", Some(Value::Str("rs256".to_owned()))),
        changed(
            "payload.sign_count",
            Some(Value::Int(Int::new(4_294_967_296).unwrap())),
        ),
        changed(
            "payload.cool_off_ends",
            Some(Value::Str("tomorrow".to_owned())),
        ),
        changed(
            "payload.public_key",
            Some(Value::Str(format!("sha256:{}", "a".repeat(63)))),
        ),
        changed(
            "payload.public_key",
            Some(Value::Str(format!("sha256:{}", "A".repeat(64)))),
        ),
        changed("payload.credential", Some(Value::Str("A".repeat(1368)))),
    ] {
        assert_eq!(
            credential_record(3, &envelope),
            Err(CredentialError::Unreadable {
                seq: 3,
                member: path
            }),
            "{path}"
        );
    }
}

/// The vectors' invalid drafts that rule 249 refuses on a well-formed cool-off: the mapping reads
/// the instant and leaves its arithmetic to the journal.
const COOL_OFF_RULE: [&str; 4] = [
    "first_without_cool_off",
    "first_cools_a_second_short",
    "first_cools_from_the_whole_second",
    "stepped_up_cools_off",
];

/// The payload members the mapping reads, each refused at its own path.
const READ: [&str; 6] = [
    "payload.member",
    "payload.credential",
    "payload.public_key",
    "payload.alg",
    "payload.sign_count",
    "payload.cool_off_ends",
];

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

/// Every invalid draft whose fault is a member the mapping reads, or the schema version, is
/// refused at that member's path; the journal's `unknown_schema` at `payload` is the mapping's
/// `schema_version`.
#[test]
fn credential_record_refuses_each_invalid_vector_draft_at_the_member_it_reads() {
    let section = section();
    let mut checked = 0;
    for case in list(&section, "invalid_drafts") {
        let expect = case.get("expect").unwrap();
        let path: &'static str = match (text(expect, "path"), text(expect, "reason")) {
            ("payload", "unknown_schema") => "schema_version",
            (path, _) => match READ.iter().find(|p| **p == path) {
                Some(p) => p,
                None => continue,
            },
        };
        if COOL_OFF_RULE.contains(&text(case, "name")) {
            continue;
        }
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
    assert!(checked >= 30, "{checked} invalid drafts checked");
}

/// Every valid draft maps to its record: the other algorithm, the counter's top, the shortest
/// and longest ids, a removal in `live`, and the envelope members the mapping does not read.
#[test]
fn credential_record_maps_every_valid_vector_draft() {
    let section = section();
    for case in list(&section, "valid_drafts") {
        let envelope = vector_draft(&section, case);
        let mapped = credential_record(6, &envelope);
        let Ok(Some(record)) = mapped else {
            panic!("{} maps to its record: {mapped:?}", text(case, "name"));
        };
        assert_eq!(
            credential_of(&record).0,
            text(&envelope, "payload.credential")
        );
        let enrolled = match &record {
            CredentialRecord::Enrolled(e) => Some((e.alg, e.sign_count)),
            CredentialRecord::Removed(_) => None,
            CredentialRecord::Asserted(a) => panic!("a valid draft is no assertion: {}", a.seq),
            CredentialRecord::Waived(w) => panic!("a valid draft is no waiver: {}", w.seq),
        };
        match text(case, "name") {
            "eddsa" => assert_eq!(enrolled.map(|e| e.0), Some(PasskeyAlg::EdDsa)),
            "kept_counter" => assert_eq!(enrolled.map(|e| e.1), Some(u32::MAX)),
            "in_live" => assert_eq!(enrolled, None),
            _ => assert_eq!(enrolled.map(|e| e.0), Some(PasskeyAlg::Es256)),
        }
    }
}

/// Route 2's view reads no counter: a `StepUpAsserted` changes nothing `held` answers, and
/// `admits` reads an assertion only for rule 254 (`credential_fold_v2.rs` pins step-up's view).
#[test]
fn route_two_holds_through_a_step_up_assertion() {
    let section = section();
    let enrolled = section
        .get("drafts")
        .and_then(|d| d.get("first_enrolled"))
        .unwrap();
    let enrolment = credential_record(1, enrolled).unwrap().unwrap();
    let (credential, member) = credential_of(&enrolment);
    let (credential, member) = (credential.to_owned(), member);
    let asserted = CredentialRecord::Asserted(Assertion {
        seq: 2,
        member,
        credential: credential.clone(),
        challenge: "01JAA3P0A000000000000000H1".to_owned(),
        sign_count: 7,
        event_time: UtcNanos::EPOCH,
    });
    let fold = CredentialFold::new([enrolment, asserted]);
    assert_eq!(fold.held(&credential).map(|e| e.member), Some(member));
}

/// One random step: member, credential, and whether it enrols or removes.
fn step() -> impl Strategy<Value = (u8, u8, bool)> {
    (0..3_u8, 0..4_u8, any::<bool>())
}

fn record(seq: u64, (member, credential, enrols): (u8, u8, bool)) -> CredentialRecord {
    let member = PrincipalId(u128::from(member) + 1);
    let credential = format!("credential-{credential}-AAAAAAAAAAAAA");
    if enrols {
        CredentialRecord::Enrolled(Enrolment {
            seq,
            member,
            credential,
            public_key: format!("sha256:{seq:064}"),
            alg: PasskeyAlg::EdDsa,
            sign_count: 0,
            cool_off_ends: None,
            backup_eligible: Some(false),
        })
    } else {
        CredentialRecord::Removed(Removal {
            seq,
            member,
            credential,
        })
    }
}

/// What the oracle finds: the first breaking `seq` per member, the unbroken members' passkeys in
/// enrolment order, and whether the stream's last record breaks rule 251.
struct Replayed {
    broken: BTreeMap<PrincipalId, u64>,
    held: BTreeMap<PrincipalId, Vec<String>>,
    holders: BTreeMap<String, PrincipalId>,
    last_breaks: bool,
}

/// The oracle: each credential's history kept apart, as the reference generator's own oracle
/// keeps it. A record breaks rule 251 unless it is its credential's first record and an
/// enrolment, or its second and the first enroller's removal.
fn replay(records: &[CredentialRecord]) -> Replayed {
    let mut history: BTreeMap<&str, Vec<(bool, PrincipalId)>> = BTreeMap::new();
    let mut broken: BTreeMap<PrincipalId, u64> = BTreeMap::new();
    let mut order: Vec<(PrincipalId, String)> = Vec::new();
    let mut last_breaks = false;
    for (record, seq) in records.iter().zip(1..) {
        let (credential, member) = credential_of(record);
        let enrols = matches!(record, CredentialRecord::Enrolled(_));
        let events = history.entry(credential).or_default();
        events.push((enrols, member));
        let enrolments = events.iter().filter(|(e, _)| *e).count();
        let removals = events.len() - enrolments;
        let breaks =
            !events[0].0 || enrolments > 1 || removals > 1 || (!enrols && events[0].1 != member);
        last_breaks = breaks;
        if breaks {
            broken.entry(member).or_insert(seq);
            if events[0].0 && events[0].1 != member {
                broken.entry(events[0].1).or_insert(seq);
            }
        } else if enrols {
            order.push((member, credential.to_owned()));
        } else {
            order.retain(|(_, c)| c != credential);
        }
    }
    let holders = order.iter().map(|(m, c)| (c.clone(), *m)).collect();
    let mut held: BTreeMap<PrincipalId, Vec<String>> = BTreeMap::new();
    for (member, credential) in order {
        if !broken.contains_key(&member) {
            held.entry(member).or_default().push(credential);
        }
    }
    Replayed {
        broken,
        held,
        holders,
        last_breaks,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Over random streams of enrolments and removals among three members and four ids, the fold
    /// agrees with the replay on every member's passkeys and every id's holder, and admits exactly
    /// the next steps the replay finds break nobody new.
    #[test]
    fn the_fold_agrees_with_an_independent_replay(
        steps in prop::collection::vec(step(), 0..24),
        next in step(),
    ) {
        let stream: Vec<CredentialRecord> =
            steps.iter().zip(1..).map(|(s, seq)| record(seq, *s)).collect();
        let Replayed {
            broken,
            held,
            holders,
            ..
        } = replay(&stream);
        let fold = CredentialFold::new(stream.iter().cloned());
        for member in (1..=3).map(PrincipalId) {
            let got = fold
                .passkeys(member)
                .map(|keys| keys.into_iter().map(|e| e.credential.clone()).collect::<Vec<_>>());
            let want = match broken.get(&member) {
                Some(seq) => Err(CredentialError::Broken { seq: *seq }),
                None => Ok(held.get(&member).cloned().unwrap_or_default()),
            };
            prop_assert_eq!(got, want);
        }
        for credential in (0..4).map(|c| format!("credential-{c}-AAAAAAAAAAAAA")) {
            let got = fold.held(&credential).map(|e| e.member);
            prop_assert_eq!(got, holders.get(&credential).copied());
        }
        let seq = u64::try_from(stream.len() + 1).unwrap();
        let mut extended = stream.clone();
        extended.push(record(seq, next));
        let breaks_now = replay(&extended).last_breaks;
        let admitted = fold.admits(&record(seq, next));
        prop_assert_eq!(admitted.is_err(), breaks_now);
    }
}
