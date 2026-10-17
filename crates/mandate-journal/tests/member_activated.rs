//! Lane ID1 (DEC-991 item 1; journal spec v0.25 §9.12): the control stream's `MemberActivated`,
//! closed at schema version 1, through the journal's own draft check against the vectors'
//! `membership` section. Every `MemberActivated` base and valid draft parses, and every invalid one
//! is refused with its reason at its path: the closed members and types, and rules 96, 98 to 100,
//! 103, 105, and 106. Until it is registered, the journal refuses it as `unknown_event_type`,
//! which is the answer these tests fail on.
//!
//! Rule 103's cool-off and rules 98, 99, and 105's writer are also swept against an oracle of their
//! own, which recomputes each from the identity spec's statement (§8.3, §3.2) rather than the
//! journal's code. The other six `Member*` types stay unregistered here (DEC-991 item 1): E9-7
//! registers them.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::Draft;
use proptest::prelude::*;

/// §9.12's vectors.
const MEMBERSHIP: &str = "membership";

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

/// A base draft by name.
fn base<'a>(section: &'a Value, name: &str) -> &'a Value {
    section.get("drafts").and_then(|d| d.get(name)).unwrap()
}

/// A case's draft: its named base draft with its changes applied, in canonical bytes.
fn draft(section: &Value, case: &Value) -> Vec<u8> {
    let mut body = base(section, text(case, "base_draft"))
        .as_object()
        .cloned()
        .unwrap();
    for change in list(case, "changes") {
        apply(&mut body, change);
    }
    to_canonical(&Value::Object(body))
}

/// Whether a case's clause cites `rule`: the text before its first `:`, such as `rule 108` or
/// `§3 rule 83`.
fn cites(case: &Value, rule: u32) -> bool {
    let cited = text(case, "clause").split(':').next().unwrap_or_default();
    let named = format!("rule {rule}");
    cited == named || cited.ends_with(&format!(" {named}"))
}

/// Every base and valid draft of `event_type` in the vectors' section `name` parses as its own type
/// at `version`, and every invalid one is refused with its reason at its path. There are never
/// fewer than `valid` valid and `invalid` invalid cases on its base drafts, and each of `rules` has
/// an invalid case.
fn every_case_of(
    (name, event_type, version): (&str, &str, u64),
    valid: usize,
    invalid: usize,
    rules: &[u32],
) {
    let section = section(name);
    let ours =
        |case: &&Value| text(base(&section, text(case, "base_draft")), "event_type") == event_type;
    let bases = section.get("drafts").and_then(Value::as_object).unwrap();
    let valid_cases: Vec<&Value> = list(&section, "valid_drafts").iter().filter(ours).collect();
    let invalid_cases: Vec<&Value> = list(&section, "invalid_drafts")
        .iter()
        .filter(ours)
        .collect();
    let mut failed = Vec::new();
    let own_bases = bases
        .values()
        .filter(|b| text(b, "event_type") == event_type);
    let valid_drafts = valid_cases.iter().map(|case| draft(&section, case));
    for bytes in own_bases.map(to_canonical).chain(valid_drafts) {
        let got = Draft::parse(&bytes).map(|d| (d.event_type().to_owned(), d.schema_version()));
        if got != Ok((event_type.to_owned(), version)) {
            let body = String::from_utf8_lossy(&bytes);
            failed.push(format!("{body}: {got:?}"));
        }
    }
    for case in &invalid_cases {
        let expect = case.get("expect").unwrap();
        let want = (text(expect, "reason"), text(expect, "path"));
        let got = Draft::parse(&draft(&section, case)).err();
        let got = got.as_ref().map(|e| (e.reason.code(), e.path.as_str()));
        if got != Some(want) {
            let name = text(case, "name");
            failed.push(format!("{name}: want {want:?}, got {got:?}"));
        }
    }
    let counts = (valid_cases.len(), invalid_cases.len());
    assert!(
        counts.0 >= valid && counts.1 >= invalid,
        "{event_type}: {counts:?} cases; never fewer than {:?}",
        (valid, invalid)
    );
    for rule in rules {
        assert!(
            invalid_cases.iter().any(|case| cites(case, *rule)),
            "{event_type}: rule {rule} has no invalid draft"
        );
    }
    assert!(failed.is_empty(), "{event_type}:\n{}", failed.join("\n"));
}

/// `MemberActivated`: its closed members and types; rule 96's sorted roles; rule 98's writer per
/// reason; rule 99's invitation and invitee; rule 100's non-empty roles and the founding grant's
/// admin; rule 103's cool-off; rule 105's session; and rule 106's instant.
#[test]
fn every_member_activated_draft_parses_or_is_refused_as_its_case_says() {
    every_case_of(
        (MEMBERSHIP, "MemberActivated", 1),
        4,
        37,
        &[96, 98, 99, 100, 103, 105],
    );
}

fn base_object(name: &str) -> Object {
    base(&section(MEMBERSHIP), name)
        .as_object()
        .cloned()
        .unwrap()
}

fn set(draft: &mut Object, path: &str, value: Value) {
    let change = Value::Object(Object::from_iter([
        (Key::new("path").unwrap(), Value::Str(path.to_owned())),
        (Key::new("value").unwrap(), value),
    ]));
    apply(draft, &change);
}

fn refusal(draft: &Object) -> Option<(String, String)> {
    Draft::parse(&to_canonical(&Value::Object(draft.clone())))
        .err()
        .map(|e| (e.reason.code().to_owned(), e.path))
}

fn actor(kind: &str, id: &str, build: Option<&str>) -> Value {
    let mut actor = Object::new();
    for (name, value) in [("kind", kind), ("id", id), ("version", "1")] {
        actor.insert(Key::new(name).unwrap(), Value::Str(value.to_owned()));
    }
    let build = build.map_or(Value::Null, |b| Value::Str(b.to_owned()));
    actor.insert(Key::new("build").unwrap(), build);
    Value::Object(actor)
}

/// A founding grant by anyone but the system is refused at the actor (rule 98, ID-13): a user, an
/// agent, platform staff, or a client, which rule 83 refuses before the payload is read.
#[test]
fn only_the_system_writes_the_founding_grant() {
    let build = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
    for (kind, build) in [
        ("user", None),
        ("agent", Some(build)),
        ("platform_operator", None),
    ] {
        let mut draft = base_object("founding");
        set(
            &mut draft,
            "actor",
            actor(kind, "01J8Z4M0AD0000000000000AD1", build),
        );
        let got = refusal(&draft);
        assert_eq!(
            got,
            Some(("schema".to_owned(), "actor.kind".to_owned())),
            "{kind}"
        );
    }
    let mut draft = base_object("founding");
    let mut client = actor("client", "client_01", None);
    if let Value::Object(o) = &mut client {
        o.insert(
            Key::new("on_behalf_of").unwrap(),
            Value::Str("01J8Z4M0AD0000000000000AD1".to_owned()),
        );
    }
    set(&mut draft, "actor", client);
    assert_eq!(
        refusal(&draft),
        Some(("schema".to_owned(), "actor.kind".to_owned()))
    );
}

/// The membership records are the control stream's alone (journal spec §9's table).
#[test]
fn the_founding_grant_is_a_control_stream_record() {
    let mut draft = base_object("founding");
    set(
        &mut draft,
        "stream_id",
        Value::Str("agent:ws_01J8Z2:agent_1".to_owned()),
    );
    assert_eq!(
        refusal(&draft),
        Some(("wrong_stream".to_owned(), "event_type".to_owned()))
    );
}

/// Rule 106: the record's instant is the envelope's `event_time` exactly, a nanosecond off either
/// way refused; rule 105: a user's activation names its session.
#[test]
fn the_instant_is_the_envelope_time_and_a_user_names_a_session() {
    for instant in [
        "2026-10-05T14:00:00.000000001Z",
        "2026-10-05T13:59:59.999999999Z",
    ] {
        let mut draft = base_object("founding");
        set(
            &mut draft,
            "payload.activated_at",
            Value::Str(instant.to_owned()),
        );
        assert_eq!(
            refusal(&draft),
            Some(("schema".to_owned(), "payload.activated_at".to_owned())),
            "{instant}"
        );
    }
    let mut draft = base_object("activated");
    set(&mut draft, "payload.session_ref", Value::Null);
    assert_eq!(
        refusal(&draft),
        Some(("schema".to_owned(), "payload.session_ref".to_owned()))
    );
}

const DAY_NANOS: i128 = 86_400_000_000_000;

/// The oracle's cool-off end, from identity spec §8.3 as rule 103 states it: a day after the
/// grant when independence is required and an accepted invitation brings operator or approver,
/// and the grant's own instant otherwise; a founding grant never cools off.
fn oracle_cool_off(founding: bool, independent: bool, roles: &[&str]) -> i128 {
    let sensitive = roles.iter().any(|r| *r == "operator" || *r == "approver");
    if !founding && independent && sensitive {
        DAY_NANOS
    } else {
        0
    }
}

const ROLES: [&str; 5] = [
    "approver",
    "auditor",
    "operator",
    "viewer",
    "workspace_admin",
];

/// The oracle's verdict on one activation: whether it is valid, judged member by member from the
/// rules' statements, in rule order.
fn oracle_valid(
    founding: bool,
    by_system: bool,
    independent: bool,
    roles: &[&str],
    offset: i128,
) -> bool {
    let writer_fits = by_system == founding;
    let non_empty = !roles.is_empty();
    let admin = !founding || roles.contains(&"workspace_admin");
    let cool_off = offset == oracle_cool_off(founding, independent, roles);
    writer_fits && non_empty && admin && cool_off
}

fn stamp(nanos: i128) -> String {
    let secs = i64::try_from(nanos.div_euclid(1_000_000_000)).unwrap();
    let sub = u32::try_from(nanos.rem_euclid(1_000_000_000)).unwrap();
    mandate_time::UtcNanos::from_parts(secs, sub)
        .unwrap()
        .to_string()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, failure_persistence: None, ..ProptestConfig::default() })]

    /// Rules 98, 99, 100, 103, and 105 over random activations: the journal admits exactly what the
    /// oracle does. The writer is the system with no session for a founding grant and the invitee
    /// with a session for an accepted invitation, or swapped; the cool-off is none, a day, or a
    /// nanosecond either side of a day.
    #[test]
    fn activations_are_admitted_exactly_as_the_oracle_says(
        founding in any::<bool>(),
        by_system in any::<bool>(),
        independent in any::<bool>(),
        mask in 0_u8..32,
        offset in prop_oneof![Just(0_i128), Just(DAY_NANOS), Just(DAY_NANOS - 1), Just(DAY_NANOS + 1), Just(1_i128)],
    ) {
        let roles: Vec<&str> = ROLES.iter().enumerate().filter(|(i, _)| mask & (1 << i) != 0).map(|(_, r)| *r).collect();
        let mut draft = base_object(if founding { "founding" } else { "activated" });
        let member = text(&Value::Object(draft.clone()).get("payload").cloned().unwrap(), "member").to_owned();
        let build = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
        let (writer, session) = if by_system {
            (actor("system", "control_services", Some(build)), Value::Null)
        } else {
            (actor("user", &member, None), Value::Str("session_x".to_owned()))
        };
        set(&mut draft, "actor", writer);
        set(&mut draft, "payload.session_ref", session);
        set(&mut draft, "payload.independent_approval_required", Value::Bool(independent));
        set(&mut draft, "payload.roles", Value::Array(roles.iter().map(|r| Value::Str((*r).to_owned())).collect()));
        let event_time = mandate_time::UtcNanos::parse(text(&Value::Object(draft.clone()), "event_time")).unwrap();
        let base_nanos = i128::from(event_time.secs()) * 1_000_000_000 + i128::from(event_time.nanos());
        set(&mut draft, "payload.cool_off_ends_at", Value::Str(stamp(base_nanos + offset)));
        let got = refusal(&draft);
        let want = oracle_valid(founding, by_system, independent, &roles, offset);
        prop_assert_eq!(got.is_none(), want, "{:?}", got);
    }
}
