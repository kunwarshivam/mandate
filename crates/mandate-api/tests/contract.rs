//! The workspace API's contract, part A1a (workspace API spec §3.1, §3.4, §3.5), pending E10-10.
//! Every oracle here is the test's own: §3.5's table is parsed from the spec, §3.4's ids are hashed
//! with `sha2` and encoded in Crockford base 32 here, and the closed enums' values are typed from
//! the spec's text, never read back from the crate.

use std::collections::BTreeSet;
use std::fmt::Debug;

use mandate_api::idempotency::{Derivation, IdempotencyKey, KeyError, event_id};
use mandate_api::problem::{
    AncestorLevel, CurrentBase, Effect, PolicyLevel, PolicyValue, Problem, ProblemCode,
    ProblemError, Violation,
};
use mandate_api::responses::CommandStatus;
use mandate_api::wire::{
    Asset, Decimal, EventId, Id, Ref, Refused, Timestamp, Validate, decode, encode,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const SPEC: &str = include_str!("../../../docs/specs/workspace-api.md");

/// One code of §3.5's table: its status, and the story that will serve it when the row carries a
/// `(planned: <story>)` marker (DEC-683).
struct SpecCode {
    name: String,
    status: u16,
    planned: Option<String>,
}

/// Every code of §3.5's code table: the rows of the section whose second cell is an HTTP status,
/// each naming one or more codes in its first cell.
fn spec_codes() -> Vec<SpecCode> {
    let section = SPEC
        .split("### 3.5 Errors")
        .nth(1)
        .and_then(|rest| rest.split("### 3.6").next())
        .unwrap_or_default();
    let mut codes = Vec::new();
    for line in section.lines() {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        let Some(status) = cells.get(2).and_then(|c| c.parse::<u16>().ok()) else {
            continue;
        };
        let planned = line
            .split("(planned: ")
            .nth(1)
            .and_then(|rest| rest.split(')').next())
            .map(str::to_owned);
        let names = cells.get(1).copied().unwrap_or_default();
        for name in names.split('`').skip(1).step_by(2) {
            codes.push(SpecCode {
                name: name.to_owned(),
                status,
                planned: planned.clone(),
            });
        }
    }
    codes
}

/// The variants serde knows for `ProblemCode`, read from its own "expected one of" refusal.
fn rust_codes() -> BTreeSet<String> {
    let refusal = serde_json::from_str::<ProblemCode>("\"no_such_code\"")
        .expect_err("no such code")
        .to_string();
    let listed = refusal.split("expected one of").nth(1).unwrap_or_default();
    listed
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

fn id(text: &str) -> Id {
    Id::try_from(text.to_owned()).expect("a valid id")
}

fn ulid(text: &str) -> EventId {
    decode(format!("\"{text}\"").as_bytes()).expect("a valid event id")
}

/// A base a `stale_base` refusal names: a content reference, whose parsing is not a stub.
fn base() -> CurrentBase {
    CurrentBase::Ref(Ref::try_from(format!("sha256:{}", "ab".repeat(32))).expect("a valid ref"))
}

/// The `current_base` a code's problem takes: one for `stale_base`, none for any other.
fn base_for(name: &str) -> Option<CurrentBase> {
    (name == "stale_base").then(base)
}

/// The effect a code's problem is made with when only the code is being checked: `unknown` for
/// `outcome_unknown`, which carries nothing else, and `none` for every other code.
fn plain_effect(name: &str) -> Effect {
    if name == "outcome_unknown" {
        Effect::Unknown
    } else {
        Effect::None
    }
}

/// The title `envelope.schema.json#/$defs/Problem` pins for each code (API-10, DEC-682 item 28),
/// and for identity spec §4.5's six codes, which neither spec titles, the one DEC-681 item 14
/// fixes: generic text that names no content.
const TITLES: &[(&str, &str)] = &[
    ("unauthenticated", "Sign in required"),
    ("forbidden", "Not allowed"),
    ("not_found", "Not found"),
    ("invalid", "Invalid request"),
    ("idempotency_conflict", "Key already used"),
    ("stale_base", "Changed since loaded"),
    ("classification_changed", "Classification changed"),
    ("step_up_required", "Confirmation required"),
    ("step_up_missing", "Confirmation not valid"),
    ("step_up_stale", "Confirmation expired"),
    ("step_up_reused", "Confirmation already used"),
    ("step_up_method", "Confirmation method not allowed"),
    ("step_up_mismatch", "Confirmation does not match"),
    ("live_unavailable", "Live trading unavailable"),
    ("control_stream_frozen", "Changes are frozen"),
    ("journal_unavailable", "Temporarily unavailable"),
    ("rate_limited", "Too many requests"),
    ("outcome_unknown", "Result unknown"),
    ("own_roles", "Cannot change own roles"),
    ("owner_role_reserved", "Owner role reserved"),
    ("last_owner", "Owner must remain"),
    ("last_admin", "Admin must remain"),
    ("reduction_only", "Risk reduction only"),
    ("membership_unavailable", "Membership unavailable"),
];

fn title_of(name: &str) -> &'static str {
    TITLES
        .iter()
        .find(|(code, _)| *code == name)
        .map(|(_, title)| *title)
        .unwrap_or_else(|| panic!("no title pinned for {name}"))
}

#[test]
#[ignore = "pending E10-10"]
fn every_problem_code_has_the_spec_tables_status() {
    let served: Vec<SpecCode> = spec_codes()
        .into_iter()
        .filter(|c| c.planned.is_none())
        .collect();
    assert!(
        served.len() >= 18,
        "§3.5 lists at least v0.1's twelve codes and identity spec §4.5's six"
    );
    let event = ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV");
    for SpecCode { name, status, .. } in served {
        let code: ProblemCode = decode(format!("\"{name}\"").as_bytes()).expect(&name);
        let effect = plain_effect(&name);
        let id = (effect != Effect::None).then(|| event.clone());
        let problem = Problem::of(code, effect, id, base_for(&name)).expect(&name);
        assert_eq!(problem.status, status, "{name}");
        assert_eq!(problem.code, code);
        let retryable = matches!(
            name.as_str(),
            "journal_unavailable" | "rate_limited" | "membership_unavailable"
        );
        assert_eq!(problem.retryable, retryable, "{name}");
        assert_eq!(problem.title, title_of(&name), "{name}");
        assert_eq!(
            problem.type_uri,
            format!("https://mandate.dev/problems/{name}")
        );
        assert_eq!(problem.event_id.is_some(), effect != Effect::None, "{name}");
        assert!(problem.violations.is_empty(), "{name}");
        assert_eq!(problem.current_base, base_for(&name), "{name}");
    }
}

#[test]
#[ignore = "pending E10-10"]
fn event_id_is_present_exactly_when_something_may_be_recorded() {
    let event = ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV");
    let invalid = Problem::of(
        ProblemCode::Invalid,
        Effect::None,
        Some(event.clone()),
        None,
    );
    assert_eq!(invalid, Err(ProblemError::EventIdMismatch));
    let missing = Problem::of(ProblemCode::StepUpRequired, Effect::Recorded, None, None);
    assert_eq!(missing, Err(ProblemError::EventIdMismatch));
    let named = Problem::of(
        ProblemCode::StepUpRequired,
        Effect::Recorded,
        Some(event.clone()),
        None,
    );
    assert_eq!(named.map(|p| p.event_id), Ok(Some(event)));
}

/// The effects each code may carry, read from the spec: a refusal for authentication, role, scope,
/// existence, shape, idempotency, base, classification, live, rate, identity, or an unavailable
/// journal comes before anything is written (§3.5, API-1, API-2, API-13; "`effect: none`" in
/// `journal_unavailable`'s row). Identity spec §4.5's refusals (`own_roles`,
/// `owner_role_reserved`, `last_owner`, `last_admin`, `reduction_only`, `membership_unavailable`)
/// are the authorization step's, which commits nothing (§4.5, DEC-643), so they carry only `none`. Only §5.6's batch refuses its revocation after its kill switch was
/// recorded, with a `step_up_*` code (DEC-686) or while frozen with `control_stream_frozen`. Only
/// `outcome_unknown` reports `unknown`, and it reports nothing else (#788's §3.5 row, DEC-681 item
/// 11).
fn may_carry(name: &str, effect: Effect) -> bool {
    match effect {
        Effect::None => name != "outcome_unknown",
        Effect::Recorded => name.starts_with("step_up_") || name == "control_stream_frozen",
        Effect::Unknown => name == "outcome_unknown",
    }
}

#[test]
#[ignore = "pending E10-10"]
fn each_code_carries_only_the_effects_the_spec_allows_it() {
    let event = ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV");
    let served: Vec<SpecCode> = spec_codes()
        .into_iter()
        .filter(|c| c.planned.is_none())
        .collect();
    assert!(
        served.len() >= 18,
        "§3.5 lists at least v0.1's twelve codes and identity spec §4.5's six"
    );
    for SpecCode { name, .. } in served {
        let code: ProblemCode = decode(format!("\"{name}\"").as_bytes()).expect(&name);
        for effect in [Effect::None, Effect::Recorded, Effect::Unknown] {
            let id = (effect != Effect::None).then(|| event.clone());
            let made = Problem::of(code, effect, id, base_for(&name));
            if may_carry(&name, effect) {
                assert_eq!(made.map(|p| p.effect), Ok(effect), "{name} {effect:?}");
            } else {
                assert_eq!(
                    made,
                    Err(ProblemError::EffectNotAllowed),
                    "{name} {effect:?}"
                );
            }
        }
    }
}

#[test]
#[ignore = "pending E10-10"]
fn a_problem_serializes_every_member_and_refuses_one_it_does_not_name() {
    let problem = Problem::of(ProblemCode::Invalid, Effect::None, None, None).expect("invalid");
    let written: Value = serde_json::from_slice(&encode(&problem).expect("encodes")).expect("json");
    let members: BTreeSet<&str> = written
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    let expected = [
        "type",
        "status",
        "code",
        "title",
        "effect",
        "event_id",
        "retryable",
        "violations",
    ];
    assert_eq!(members, BTreeSet::from(expected));
    assert_eq!(
        written["type"],
        json!("https://mandate.dev/problems/invalid")
    );
    assert_eq!(written["status"], json!(422));
    assert_eq!(written["code"], json!("invalid"));
    assert_eq!(written["effect"], json!("none"));
    assert_eq!(written["event_id"], Value::Null);
    assert_eq!(written["retryable"], json!(false));
    assert_eq!(written["violations"], json!([]));
    assert_eq!(written["title"], json!("Invalid request"));
    let event = ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV");
    let recorded = Problem::of(
        ProblemCode::StepUpRequired,
        Effect::Recorded,
        Some(event),
        None,
    );
    let written: Value =
        serde_json::from_slice(&encode(&recorded.expect("recorded")).expect("encodes"))
            .expect("json");
    assert_eq!(written["event_id"], json!("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
    assert_eq!(
        written["type"],
        json!("https://mandate.dev/problems/step_up_required")
    );
    let mut extra = serde_json::to_value(&problem).expect("json");
    extra["detail"] = json!("anything");
    let body = serde_json::to_vec(&extra).expect("json");
    assert_eq!(
        refusal::<Problem>(&body),
        [("/detail".to_owned(), "unknown_member".to_owned())]
    );
}

/// A closed request shape, standing for every request type: a decimal and a closed enum.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    bid: Decimal,
    effect: Effect,
}

impl Validate for Fixture {
    fn validate(&self) -> Result<(), Refused> {
        Ok(())
    }
}

/// The `(path, code)` of each violation `decode` reports for `body`.
fn refusal<T: DeserializeOwned + Validate + Debug>(body: &[u8]) -> Vec<(String, String)> {
    match decode::<T>(body) {
        Err(Refused::Invalid { violations }) => violations
            .into_iter()
            .map(|v| match v {
                Violation::Schema { path, code, .. } => (path, code),
                other => panic!("a body's violation is a schema finding: {other:?}"),
            })
            .collect(),
        other => panic!("expected invalid, got {other:?}"),
    }
}

fn is_pointer(path: &str) -> bool {
    path.is_empty() || path.starts_with('/')
}

#[test]
#[ignore = "pending E10-10"]
fn decode_refuses_every_malformed_body_with_one_located_violation() {
    let good = decode::<Fixture>(br#"{"bid": "1.5", "effect": "recorded"}"#);
    assert!(good.is_ok(), "{good:?}");
    let whole = [("".to_owned(), "malformed".to_owned())];
    for body in [
        &b"not json"[..],
        b"",
        b"   ",
        br#"{"bid": "1.5", "effect": "none"} x"#,
        b"{} {}",
    ] {
        assert_eq!(
            refusal::<Fixture>(body),
            whole,
            "{}",
            String::from_utf8_lossy(body)
        );
    }
    let located: [(&[u8], &str, &str); 3] = [
        (
            br#"{"bid": "1.5", "effect": "none", "requested_by": "owner"}"#,
            "/requested_by",
            "unknown_member",
        ),
        (br#"{"bid": "1.5"}"#, "", "missing"),
        (br#"{"bid": "1.5", "effect": "maybe"}"#, "/effect", "enum"),
    ];
    for (body, path, code) in located {
        assert_eq!(
            refusal::<Fixture>(body),
            [(path.to_owned(), code.to_owned())]
        );
    }
    let coded: [(&[u8], &str); 4] = [
        (
            br#"{"bid": "1", "bid": "2", "effect": "none"}"#,
            "duplicate_member",
        ),
        (
            br#"{"bid": {"a": 1, "a": 2}, "effect": "none"}"#,
            "duplicate_member",
        ),
        (br#"{"bid": 1.5, "effect": "none"}"#, "type"),
        (br#"{"bid": "1.50", "effect": "none"}"#, "non_canonical"),
    ];
    for (body, code) in coded {
        let found = refusal::<Fixture>(body);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found.iter().all(|(p, c)| c == code && is_pointer(p)),
            "{found:?}"
        );
    }
}

/// A refusal on a later line of a pretty-printed body is located there (DEC-681 item 10): the
/// unknown member `x` on the third line is `/x`.
#[test]
#[ignore = "pending E10-10"]
fn decode_locates_a_refusal_on_a_later_line_of_the_body() {
    let body = b"{\n  \"bid\": \"1.5\",\n  \"x\": 1,\n  \"effect\": \"none\"\n}\n";
    assert_eq!(
        refusal::<Fixture>(body),
        [("/x".to_owned(), "unknown_member".to_owned())]
    );
}

/// A refusal inside an array is located by the item's index (RFC 6901, DEC-681 item 10): an
/// unknown member of a command status's second step is `/steps/1/extra`, after an earlier array.
#[test]
#[ignore = "pending E10-10"]
fn decode_locates_a_refusal_inside_an_array_by_its_index() {
    let example =
        include_str!("../../../schemas/workspace-api/examples/commands.command-status.json");
    let mut status: Value = serde_json::from_str(example).expect("JSON");
    let mut second = status["steps"][0].clone();
    second["extra"] = json!(1);
    status["steps"].as_array_mut().expect("steps").push(second);
    assert_eq!(
        refusal::<CommandStatus>(status.to_string().as_bytes()),
        [("/steps/1/extra".to_owned(), "unknown_member".to_owned())]
    );
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
        format!("{}\n", "a".repeat(16)),
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
fn closed<T: Serialize + DeserializeOwned + Validate + PartialEq + Debug + Copy>(
    cases: &[(T, &str)],
) {
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
    let finest = format!("0.{}1", "0".repeat(27));
    for good in ["101.5", "0", "-3", "0.000001", finest.as_str()] {
        let decimal = decode::<Decimal>(quoted(good).as_bytes()).expect(good);
        assert_eq!(encode(&decimal).expect(good), quoted(good).as_bytes());
    }
    for bad in ["101.5", "101", "1e2"] {
        assert!(
            is_invalid(decode::<Decimal>(bad.as_bytes())),
            "the number {bad}"
        );
    }
    let too_fine = format!("0.{}1", "0".repeat(28));
    let too_large = [
        "79000000000000000000000000000",
        "100000000000000000000000000000",
    ];
    let more = [too_fine.as_str(), too_large[0], too_large[1]];
    let spellings = [
        ".5", "1.", "-", "NaN", "Infinity", "1_000", "-0.0", "1.50", "+1", "1e2",
    ];
    for bad in spellings
        .iter()
        .chain(more.iter())
        .chain(["01", "-0", "", " 1", "one"].iter())
    {
        assert!(
            is_invalid(decode::<Decimal>(quoted(bad).as_bytes())),
            "{bad:?}"
        );
    }
    let refs = [format!("sha256:{}", "ab".repeat(32))];
    let widest = "a".repeat(64);
    let ids = [
        "a",
        "agent_1",
        "A-z_9",
        "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        widest.as_str(),
    ];
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
    for letter in ['I', 'L', 'O', 'U'] {
        let ulid = format!("01ARZ3NDEKTSV4RRFFQ69G5FA{letter}");
        assert!(
            is_invalid(decode::<EventId>(quoted(&ulid).as_bytes())),
            "{ulid}"
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
    for bad in [
        "2026-10-08T14:30:00Z",
        "2026-10-08T14:30:00.000000000+00:00",
        "2026-10-08T14:30:00.000Z",
        "1969-12-31T23:59:59.000000000Z",
        "2026-10-08 14:30:00.000000000Z",
    ] {
        assert!(
            is_invalid(decode::<Timestamp>(quoted(bad).as_bytes())),
            "{bad}"
        );
    }
}

fn round_trips<T: Serialize + DeserializeOwned + Validate + Debug>(texts: &[&str]) {
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
        .filter(|c| c.planned.is_none())
        .map(|SpecCode { name, .. }| {
            let code = serde_json::from_str(&format!("\"{name}\"")).expect(&name);
            (code, name)
        })
        .collect();
    assert!(
        codes.len() >= 18,
        "§3.5 lists at least v0.1's twelve codes and identity spec §4.5's six"
    );
    let cases: Vec<(ProblemCode, &str)> = codes.iter().map(|(c, n)| (*c, n.as_str())).collect();
    closed(&cases);
}

/// The stories whose planned values the code may already serve (DEC-683 item 5): a value both a
/// variant and marked planned passes only under a story named here. E10-10 is this crate's own
/// story: its implementation serves `outcome_unknown`, which #788 lists as planned under it.
const IN_FLIGHT: &[&str] = &["E10-10"];

/// DEC-683: every code the server can emit is in §3.5's table, and every code the table serves is a
/// variant. A code the table marks `(planned: <story>)` may be absent, or present while its story is
/// in flight.
#[test]
fn the_problem_code_enum_is_exactly_the_spec_tables_codes() {
    let codes = spec_codes();
    let rust = rust_codes();
    let listed: BTreeSet<String> = codes.iter().map(|c| c.name.clone()).collect();
    assert_eq!(listed.len(), codes.len(), "no code is listed twice");
    let served: BTreeSet<String> = codes
        .iter()
        .filter(|c| c.planned.is_none())
        .map(|c| c.name.clone())
        .collect();
    assert!(rust.len() >= 18, "the refusal lists the variants: {rust:?}");
    let unlisted: Vec<_> = rust.difference(&listed).collect();
    assert!(
        unlisted.is_empty(),
        "variants §3.5 does not list: {unlisted:?}"
    );
    let missing: Vec<_> = served.difference(&rust).collect();
    assert!(
        missing.is_empty(),
        "served codes with no variant: {missing:?}"
    );
    for code in codes.iter().filter(|c| rust.contains(&c.name)) {
        if let Some(story) = &code.planned {
            assert!(
                IN_FLIGHT.contains(&story.as_str()),
                "{} is served but still planned under {story}, which is not in flight",
                code.name
            );
        }
    }
    for name in &rust {
        let code: ProblemCode = serde_json::from_str(&format!("\"{name}\"")).expect(name);
        assert_eq!(
            serde_json::to_string(&code).expect(name),
            format!("\"{name}\"")
        );
    }
}

#[test]
fn every_stub_reports_its_story() {
    assert_eq!(
        mandate_api::Unimplemented.to_string(),
        format!("{} has not been implemented yet", mandate_api::STORY)
    );
}

#[test]
#[ignore = "pending E10-10"]
fn the_last_batch_position_and_only_route_names_derive_an_id() {
    let text = "0123456789abcdef";
    let last = Some(u32::MAX);
    assert_eq!(
        derived("ws_01", "user_7", "kill_switch", text, last),
        oracle("ws_01", "user_7", "kill_switch", text, last)
    );
    let key = IdempotencyKey::parse(text).expect("a valid key");
    for operation in [
        "",
        "Kill_switch",
        "kill-switch",
        "kill switch",
        "kill\"switch",
        "_pause",
    ] {
        let derivation = Derivation {
            workspace: &id("ws_01"),
            principal: &id("user_7"),
            operation,
            key: &key,
            position: None,
        };
        assert_eq!(
            event_id(&derivation),
            Err(KeyError::Operation),
            "{operation:?}"
        );
    }
}

#[test]
#[ignore = "pending E10-10"]
fn a_stale_base_problem_names_the_current_base_and_no_other_code_does() {
    let stale = Problem::of(ProblemCode::StaleBase, Effect::None, None, Some(base()));
    let stale = stale.expect("stale_base with its base");
    assert_eq!(stale.current_base, Some(base()));
    let written: Value = serde_json::from_slice(&encode(&stale).expect("encodes")).expect("json");
    assert_eq!(
        written["current_base"],
        json!(format!("sha256:{}", "ab".repeat(32)))
    );
    assert_eq!(written["code"], json!("stale_base"));
    assert_eq!(written["status"], json!(409));
    assert_eq!(
        Problem::of(ProblemCode::StaleBase, Effect::None, None, None),
        Err(ProblemError::CurrentBaseMismatch)
    );
    for code in [
        ProblemCode::Invalid,
        ProblemCode::IdempotencyConflict,
        ProblemCode::ClassificationChanged,
    ] {
        assert_eq!(
            Problem::of(code, Effect::None, None, Some(base())),
            Err(ProblemError::CurrentBaseMismatch),
            "{code:?}"
        );
    }
    let by_id = Problem::of(
        ProblemCode::StaleBase,
        Effect::None,
        None,
        Some(CurrentBase::Id(id("draft_7"))),
    );
    let written: Value =
        serde_json::from_slice(&encode(&by_id.expect("by id")).expect("encodes")).expect("json");
    assert_eq!(written["current_base"], json!("draft_7"));
}

/// `outcome_unknown` (#788's §3.5 row, planned under E10-10): an append whose outcome could not be
/// confirmed, 503, `effect: unknown` with the derived `event_id`, never retryable, and nothing else.
#[test]
#[ignore = "pending E10-10"]
fn outcome_unknown_reports_unknown_with_its_event_and_nothing_else() {
    let code: ProblemCode = decode(b"\"outcome_unknown\"").expect("outcome_unknown is served");
    let event = ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV");
    let problem = Problem::of(code, Effect::Unknown, Some(event.clone()), None).expect("unknown");
    assert_eq!(problem.status, 503);
    assert_eq!(problem.effect, Effect::Unknown);
    assert_eq!(problem.event_id, Some(event.clone()));
    assert!(!problem.retryable);
    assert_eq!(problem.title, title_of("outcome_unknown"));
    assert_eq!(
        problem.type_uri,
        "https://mandate.dev/problems/outcome_unknown"
    );
    assert_eq!(
        Problem::of(code, Effect::Unknown, None, None),
        Err(ProblemError::EventIdMismatch)
    );
    assert_eq!(
        Problem::of(code, Effect::None, None, None),
        Err(ProblemError::EffectNotAllowed)
    );
    assert_eq!(
        Problem::of(code, Effect::Recorded, Some(event), None),
        Err(ProblemError::EffectNotAllowed)
    );
}

/// A problem written by hand, as a client reads one: serde alone, no stub.
fn written_problem() -> Value {
    json!({
        "type": "https://mandate.dev/problems/stale_base",
        "status": 409,
        "code": "stale_base",
        "title": "Changed since loaded",
        "effect": "none",
        "event_id": null,
        "retryable": false,
        "violations": [],
        "current_base": format!("sha256:{}", "ab".repeat(32)),
    })
}

#[test]
fn a_problem_reads_its_current_base_and_writes_none_when_absent() {
    let read: Problem = serde_json::from_value(written_problem()).expect("a stale_base problem");
    assert_eq!(read.current_base, Some(base()));
    assert_eq!(
        serde_json::to_value(&read).expect("json"),
        written_problem()
    );
    let mut other = read;
    other.code = ProblemCode::Invalid;
    other.current_base = None;
    let written = serde_json::to_value(&other).expect("json");
    let object = written.as_object().expect("an object");
    assert!(!object.contains_key("current_base"), "{written}");
    assert!(object.contains_key("event_id"), "{written}");
    assert_eq!(written["event_id"], Value::Null);
}

#[test]
fn a_problem_without_its_event_id_member_is_refused() {
    let mut body = written_problem();
    body.as_object_mut().expect("an object").remove("event_id");
    let refused = serde_json::from_value::<Problem>(body).expect_err("event_id is required");
    assert!(
        refused.to_string().contains("missing field `event_id`"),
        "{refused}"
    );
}

/// A policy finding as the validate read model writes it (`envelope.schema.json#/$defs/Violation`):
/// the looser level is never the platform's and the ancestor is never the mandate's.
#[test]
fn a_policy_violation_serializes_its_levels_and_values() {
    let finding = Violation::Policy {
        path: "/universe/asset_classes".to_owned(),
        key: "asset_classes".to_owned(),
        level: PolicyLevel::Mandate,
        value: PolicyValue::Set(vec!["crypto".to_owned(), "equity".to_owned()]),
        ancestor_level: AncestorLevel::Workspace,
        ancestor_value: PolicyValue::Flag(false),
        message: "Looser than the workspace allows".to_owned(),
    };
    let written = serde_json::to_value(&finding).expect("json");
    assert_eq!(
        written,
        json!({
            "kind": "policy",
            "path": "/universe/asset_classes",
            "key": "asset_classes",
            "level": "mandate",
            "value": ["crypto", "equity"],
            "ancestor_level": "workspace",
            "ancestor_value": false,
            "message": "Looser than the workspace allows",
        })
    );
    let read: Violation = serde_json::from_value(written.clone()).expect("reads back");
    assert_eq!(read, finding);
    for (member, level) in [("level", "platform"), ("ancestor_level", "mandate")] {
        let mut wrong = written.clone();
        wrong[member] = json!(level);
        assert!(
            serde_json::from_value::<Violation>(wrong).is_err(),
            "{member} {level}"
        );
    }
    for level in ["organization", "workspace", "mandate"] {
        let parsed: PolicyLevel = serde_json::from_value(json!(level)).expect(level);
        assert_eq!(serde_json::to_value(parsed).expect(level), json!(level));
    }
    for level in ["platform", "organization", "workspace"] {
        let parsed: AncestorLevel = serde_json::from_value(json!(level)).expect(level);
        assert_eq!(serde_json::to_value(parsed).expect(level), json!(level));
    }
}

#[test]
#[ignore = "pending E10-10"]
fn a_policy_violation_carries_a_number_as_its_decimal_string() {
    let body = br#"{"kind": "policy", "path": "/limits/max_drawdown", "key": "max_drawdown", "level": "workspace", "value": "0.25", "ancestor_level": "organization", "ancestor_value": "0.2", "message": "Looser than the organization allows"}"#;
    let finding = decode::<Violation>(body).expect("a policy finding");
    let Violation::Policy {
        value,
        ancestor_value,
        ..
    } = &finding
    else {
        panic!("a policy finding: {finding:?}");
    };
    assert!(matches!(value, PolicyValue::Number(_)), "{value:?}");
    assert!(
        matches!(ancestor_value, PolicyValue::Number(_)),
        "{ancestor_value:?}"
    );
    let written: Value = serde_json::from_slice(&encode(&finding).expect("encodes")).expect("json");
    assert_eq!(written["value"], json!("0.25"));
    assert_eq!(written["ancestor_value"], json!("0.2"));
    let number = br#"{"kind": "policy", "path": "/limits/max_drawdown", "key": "max_drawdown", "level": "workspace", "value": 0.25, "ancestor_level": "organization", "ancestor_value": "0.2", "message": "m"}"#;
    assert!(is_invalid(decode::<Violation>(number)));
}
