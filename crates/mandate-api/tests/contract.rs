//! The workspace API's contract, part A1a (workspace API spec §3.1, §3.4, §3.5), pending E10-10.
//! Every oracle here is the test's own: §3.5's table is parsed from the spec, §3.4's ids are hashed
//! with `sha2` and encoded in Crockford base 32 here, and the closed enums' values are typed from
//! the spec's text, never read back from the crate.

use std::collections::BTreeSet;
use std::fmt::Debug;

use mandate_api::idempotency::{Derivation, IdempotencyKey, KeyError, event_id};
use mandate_api::problem::{Effect, Problem, ProblemCode, ProblemError};
use mandate_api::wire::{Asset, Decimal, EventId, Id, Ref, Refused, Timestamp, decode, encode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};

const SPEC: &str = include_str!("../../../docs/specs/workspace-api.md");

/// `(code, status)` for each row of §3.5's code table: the rows of the section whose second cell is
/// an HTTP status.
fn spec_codes() -> Vec<(String, u16)> {
    let section = SPEC
        .split("### 3.5 Errors")
        .nth(1)
        .and_then(|rest| rest.split("### 3.6").next())
        .unwrap_or_default();
    section
        .lines()
        .filter_map(|line| {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            let code = cells.get(1)?.strip_prefix('`')?.strip_suffix('`')?;
            let status = cells.get(2)?.parse().ok()?;
            Some((code.to_owned(), status))
        })
        .collect()
}

fn id(text: &str) -> Id {
    Id::try_from(text.to_owned()).expect("a valid id")
}

fn ulid(text: &str) -> EventId {
    decode(format!("\"{text}\"").as_bytes()).expect("a valid event id")
}

#[test]
#[ignore = "pending E10-10"]
fn every_problem_code_has_the_spec_tables_status() {
    let rows = spec_codes();
    assert_eq!(rows.len(), 12, "§3.5 lists twelve codes");
    let mut seen = BTreeSet::new();
    for (name, status) in rows {
        let code: ProblemCode = decode(format!("\"{name}\"").as_bytes()).expect(&name);
        let problem = Problem::of(code, Effect::None, None).expect(&name);
        assert_eq!(problem.status, status, "{name}");
        assert_eq!(problem.code, code);
        let retryable = matches!(name.as_str(), "journal_unavailable" | "rate_limited");
        assert_eq!(problem.retryable, retryable, "{name}");
        assert!(!problem.title.is_empty() && !problem.type_uri.is_empty());
        assert!(problem.event_id.is_none() && problem.violations.is_empty());
        seen.insert(name);
    }
    assert_eq!(seen.len(), 12, "no two rows decode to one code");
}

#[test]
#[ignore = "pending E10-10"]
fn event_id_is_present_exactly_when_something_may_be_recorded() {
    let event = ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV");
    let invalid = Problem::of(ProblemCode::Invalid, Effect::None, Some(event.clone()));
    assert_eq!(invalid, Err(ProblemError::EventIdMismatch));
    for effect in [Effect::Recorded, Effect::Unknown] {
        let missing = Problem::of(ProblemCode::StepUpRequired, effect, None);
        assert_eq!(missing, Err(ProblemError::EventIdMismatch));
        let named = Problem::of(ProblemCode::StepUpRequired, effect, Some(event.clone()));
        assert_eq!(named.map(|p| p.event_id), Ok(Some(event.clone())));
    }
}

#[test]
#[ignore = "pending E10-10"]
fn idempotency_keys_are_16_to_64_url_safe_characters() {
    for good in [
        "a".repeat(16),
        "Z9_-".repeat(16),
        "k1Z-9_aaaaaaaaaa".to_owned(),
    ] {
        IdempotencyKey::parse(&good).expect(&good);
    }
    let refused = [
        "a".repeat(15),
        "a".repeat(65),
        String::new(),
        format!("{} ", "a".repeat(16)),
        format!("{}.", "a".repeat(16)),
        format!("{}é", "a".repeat(16)),
        format!("{}/", "a".repeat(16)),
    ];
    for bad in refused {
        assert_eq!(
            IdempotencyKey::parse(&bad),
            Err(KeyError::Malformed),
            "{bad:?}"
        );
    }
}

/// The oracle: canonical JSON written out by hand (keys in byte order, ASCII values that need no
/// escaping), its SHA-256's first 16 bytes, in 26 Crockford base-32 digits.
fn oracle(
    workspace: &str,
    principal: &str,
    operation: &str,
    key: &str,
    position: Option<u32>,
) -> String {
    let body = match position {
        None => format!(
            r#"{{"key":"{key}","operation":"{operation}","principal":"{principal}","workspace":"{workspace}"}}"#
        ),
        Some(n) => format!(
            r#"{{"key":"{key}","operation":"{operation}","position":{n},"principal":"{principal}","workspace":"{workspace}"}}"#
        ),
    };
    let digest = Sha256::digest(body.as_bytes());
    let n = digest
        .iter()
        .take(16)
        .fold(0_u128, |n, b| (n << 8) | u128::from(*b));
    let alphabet: Vec<char> = "0123456789ABCDEFGHJKMNPQRSTVWXYZ".chars().collect();
    (0..26)
        .rev()
        .map(|i| alphabet[((n >> (5 * i)) & 31) as usize])
        .collect()
}

fn derived(
    workspace: &str,
    principal: &str,
    operation: &str,
    key: &str,
    position: Option<u32>,
) -> String {
    let key = IdempotencyKey::parse(key).expect("a valid key");
    let derivation = Derivation {
        workspace: &id(workspace),
        principal: &id(principal),
        operation,
        key: &key,
        position,
    };
    serde_json::to_string(&event_id(&derivation).expect("an id"))
        .expect("an id serializes")
        .trim_matches('"')
        .to_owned()
}

#[test]
#[ignore = "pending E10-10"]
fn event_ids_derive_from_workspace_principal_operation_key_and_position() {
    let long = "A".repeat(64);
    let fixed = [
        (
            ("ws_01", "user_7", "kill_switch", "k1Z-9_aaaaaaaaaa", None),
            "7KXV6BGDW36KYC7RE5BPQG28Z0",
        ),
        (
            (
                "ws_01",
                "client_3",
                "revoke_connection",
                long.as_str(),
                Some(0),
            ),
            "30BTDHDAAYQS5P4DHCDA6TX62Y",
        ),
        (
            (
                "ws_01",
                "client_3",
                "revoke_connection",
                long.as_str(),
                Some(1),
            ),
            "0PWJFZ1SD9PDT3JG21719JXBXR",
        ),
    ];
    for ((ws, principal, op, key, position), expected) in fixed {
        assert_eq!(
            oracle(ws, principal, op, key, position),
            expected,
            "the oracle itself"
        );
        assert_eq!(derived(ws, principal, op, key, position), expected);
    }
    let base = ("ws_01", "user_7", "pause", "0123456789abcdef");
    let variants = [
        ("ws_02", base.1, base.2, base.3, None),
        (base.0, "user_8", base.2, base.3, None),
        (base.0, base.1, "resume", base.3, None),
        (base.0, base.1, base.2, "0123456789abcdeg", None),
        (base.0, base.1, base.2, base.3, Some(0)),
    ];
    let mut ids = BTreeSet::from([derived(base.0, base.1, base.2, base.3, None)]);
    for (ws, principal, op, key, position) in variants {
        let got = derived(ws, principal, op, key, position);
        assert_eq!(got, oracle(ws, principal, op, key, position));
        ids.insert(got);
    }
    assert_eq!(ids.len(), 6, "each input changes the id");
}

fn is_invalid<T: Debug>(decoded: Result<T, Refused>) -> bool {
    match decoded {
        Err(Refused::Invalid { .. }) => true,
        other => panic!("expected invalid, got {other:?}"),
    }
}

/// Every value encodes to the spec's spelling and decodes back; a value outside the set, or the
/// right word in another case, is refused.
fn closed<T: Serialize + DeserializeOwned + PartialEq + Debug + Copy>(cases: &[(T, &str)]) {
    for (value, spelling) in cases {
        let quoted = format!("\"{spelling}\"");
        assert_eq!(
            encode(value).expect(spelling),
            quoted.as_bytes(),
            "{spelling}"
        );
        assert_eq!(decode::<T>(quoted.as_bytes()).expect(spelling), *value);
        assert!(is_invalid(decode::<T>(quoted.to_uppercase().as_bytes())));
    }
    assert!(is_invalid(decode::<T>(b"\"halted\"")));
    assert!(is_invalid(decode::<T>(b"1")));
}

/// Each scalar takes its canonical text and refuses a JSON number, a non-canonical spelling, and
/// anything else of the wrong shape (§3.1; DEC-681 items 2 and 3).
#[test]
#[ignore = "pending E10-10"]
fn scalars_are_canonical_strings_and_never_numbers() {
    let quoted = |text: &str| format!("\"{text}\"");
    for good in ["101.5", "0", "-3", "0.000001"] {
        let decimal = decode::<Decimal>(quoted(good).as_bytes()).expect(good);
        assert_eq!(encode(&decimal).expect(good), quoted(good).as_bytes());
    }
    for bad in ["101.5", "101", "1e2"] {
        assert!(
            is_invalid(decode::<Decimal>(bad.as_bytes())),
            "the number {bad}"
        );
    }
    for bad in ["1.50", "+1", "1e2", "01", "-0", "", " 1", "one"] {
        assert!(
            is_invalid(decode::<Decimal>(quoted(bad).as_bytes())),
            "{bad:?}"
        );
    }
    let refs = [format!("sha256:{}", "ab".repeat(32))];
    let ids = ["agent_1", "A-z_9", "01ARZ3NDEKTSV4RRFFQ69G5FAV"];
    let events = ["01ARZ3NDEKTSV4RRFFQ69G5FAV", "7ZZZZZZZZZZZZZZZZZZZZZZZZZ"];
    let times = ["2026-10-08T14:30:00.000000000Z"];
    let assets = ["b0b6dd9d-8b9b-48a9-ba46-b9d54906e415"];
    round_trips::<Ref>(&refs.iter().map(String::as_str).collect::<Vec<_>>());
    round_trips::<Id>(&ids);
    round_trips::<EventId>(&events);
    round_trips::<Timestamp>(&times);
    round_trips::<Asset>(&assets);
    let long = "a".repeat(65);
    for bad in ["", "agent 1", "agent/1", "agent.1", "émile", long.as_str()] {
        assert!(
            is_invalid(decode::<Id>(quoted(bad).as_bytes())),
            "id {bad:?}"
        );
    }
    let lower = "01arz3ndektsv4rrffq69g5fav";
    for bad in [
        lower,
        "01ARZ3NDEKTSV4RRFFQ69G5FA",
        "01ARZ3NDEKTSV4RRFFQ69G5FAVX",
        "8ZZZZZZZZZZZZZZZZZZZZZZZZZ",
        "01ARZ3NDEKTSV4RRFFQ69G5FAU",
    ] {
        assert!(
            is_invalid(decode::<EventId>(quoted(bad).as_bytes())),
            "event id {bad:?}"
        );
    }
    assert!(is_invalid(decode::<Ref>(
        quoted(&"ab".repeat(32)).as_bytes()
    )));
    assert!(is_invalid(decode::<Asset>(
        quoted(&assets[0].to_uppercase()).as_bytes()
    )));
    assert!(is_invalid(decode::<Timestamp>(
        quoted("2026-10-08T14:30:00Z").as_bytes()
    )));
}

fn round_trips<T: Serialize + DeserializeOwned + Debug>(texts: &[&str]) {
    for text in texts {
        let quoted = format!("\"{text}\"");
        let value = decode::<T>(quoted.as_bytes()).expect(text);
        assert_eq!(encode(&value).expect(text), quoted.as_bytes(), "{text}");
    }
}

#[test]
#[ignore = "pending E10-10"]
fn the_safety_enums_of_a_problem_are_closed() {
    closed(&[
        (Effect::None, "none"),
        (Effect::Recorded, "recorded"),
        (Effect::Unknown, "unknown"),
    ]);
    let codes: Vec<(ProblemCode, String)> = spec_codes()
        .into_iter()
        .map(|(name, _)| {
            let code = serde_json::from_str(&format!("\"{name}\"")).expect(&name);
            (code, name)
        })
        .collect();
    let cases: Vec<(ProblemCode, &str)> = codes.iter().map(|(c, n)| (*c, n.as_str())).collect();
    closed(&cases);
}

#[test]
fn the_problem_code_enum_is_exactly_the_spec_tables_codes() {
    let rows = spec_codes();
    let spelled: BTreeSet<String> = rows.iter().map(|(name, _)| name.clone()).collect();
    assert_eq!(spelled.len(), 12, "§3.5 lists twelve distinct codes");
    for name in &spelled {
        let code: ProblemCode = serde_json::from_str(&format!("\"{name}\"")).expect(name);
        assert_eq!(
            serde_json::to_string(&code).expect(name),
            format!("\"{name}\"")
        );
    }
    assert!(serde_json::from_str::<ProblemCode>("\"unimplemented\"").is_err());
}

#[test]
fn every_stub_reports_its_story() {
    assert_eq!(
        mandate_api::Unimplemented.to_string(),
        format!("{} has not been implemented yet", mandate_api::STORY)
    );
}
