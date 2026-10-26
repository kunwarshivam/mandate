//! Journal spec v0.89 §9.35 (DEC-2398; E21-30): the account stream's `AccountSnapshotRecorded` at
//! `schema_version` 2, through the journal's own checks against the vectors' `snapshot_prior_close`
//! section. Every base and valid draft parses, version 1's among them, every invalid draft is
//! refused with its reason at its path, rule 24 at version 2 included, and every step of every
//! latest-wins stream parses. A property test fuzzes the four new members.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::Draft;
use proptest::prelude::*;

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("snapshot_prior_close").cloned().unwrap()
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

/// A `{path, value}` change, dotted from the envelope, or `{path, delete}` removing the member.
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
        node.remove(last);
    } else {
        node.insert(
            Key::new(last).unwrap(),
            change.get("value").cloned().unwrap(),
        );
    }
}

fn base(section: &Value, name: &str) -> Object {
    section
        .get("drafts")
        .and_then(|d| d.get(name))
        .and_then(Value::as_object)
        .cloned()
        .unwrap()
}

/// A case's draft: its named base draft with its changes applied, in canonical bytes.
fn draft(section: &Value, case: &Value) -> Vec<u8> {
    let mut body = base(section, text(case, "base_draft"));
    for change in list(case, "changes") {
        apply(&mut body, change);
    }
    to_canonical(&Value::Object(body))
}

/// The three base snapshots (two version 2s and a version 1) and every figure or declaration no
/// rule refuses (undeclared, every declared pair, a deficit, negative or zero figures) parse.
#[test]
fn every_snapshot_base_and_valid_draft_parses() {
    let section = section();
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let mut failed = Vec::new();
    for (name, body) in drafts {
        if let Err(e) = Draft::parse(&to_canonical(body)) {
            failed.push(format!("base {}: {e:?}", name.as_str()));
        }
    }
    let valid = list(&section, "valid_drafts");
    for case in valid {
        if let Err(e) = Draft::parse(&draft(&section, case)) {
            failed.push(format!("valid {}: {e:?}", text(case, "name")));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert_eq!(drafts.len(), 3, "the base drafts");
    assert!(
        valid.len() >= 15,
        "{} valid drafts; never fewer",
        valid.len()
    );
}

/// The closed schema and rule 24: a mistyped, misspelt, unlisted, `null` or absent new member, a
/// version-2 member on version 1, an extra member, a cash comparison that contradicts itself, or
/// a third version, refused at the member named.
#[test]
fn every_invalid_snapshot_is_refused_with_its_reason_at_its_path() {
    let section = section();
    let invalid = list(&section, "invalid_drafts");
    let mut failed = Vec::new();
    for case in invalid {
        let expect = case.get("expect").unwrap();
        let want = (text(expect, "reason"), text(expect, "path"));
        let got = Draft::parse(&draft(&section, case)).err();
        let got = got.as_ref().map(|e| (e.reason.code(), e.path.as_str()));
        if got != Some(want) {
            failed.push(format!(
                "{}: expected {want:?}, got {got:?}",
                text(case, "name")
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(
        invalid.len() >= 32,
        "{} invalid drafts; never fewer",
        invalid.len()
    );
}

/// Every step of every latest-wins stream (a version 1 then a version 2, and the reverse) is a
/// record the journal appends, so a reader's "the latest counts" is tested over real records.
#[test]
fn every_latest_wins_stream_step_parses() {
    let section = section();
    let streams = list(&section, "streams");
    let mut versions = Vec::new();
    for stream in streams {
        for step in list(stream, "steps") {
            let parsed = Draft::parse(&draft(&section, step));
            assert!(parsed.is_ok(), "{}: {parsed:?}", text(stream, "name"));
            versions.push(parsed.map(|d| d.schema_version()).unwrap_or_default());
        }
    }
    assert!(streams.len() >= 5, "{} streams", streams.len());
    assert!(versions.contains(&1) && versions.contains(&2), "{versions:?}");
}

/// A figure as journal decimal text: `cents` hundredths, signed.
fn figure(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    format!("{sign}{}.{:02}", abs / 100, abs % 100)
}

/// The same figure in journal spec §4.6's canonical form, computed apart from the journal: no
/// trailing fractional zero, no point without a fraction, and zero unsigned.
fn canonical(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    match abs % 100 {
        0 => format!("{sign}{}", abs / 100),
        tenths if tenths % 10 == 0 => format!("{sign}{}.{}", abs / 100, tenths / 10),
        hundredths => format!("{sign}{}.{hundredths:02}", abs / 100),
    }
}

fn with(mut body: Object, member: &str, value: Value) -> Object {
    if let Some(Value::Object(payload)) = body.get_mut("payload") {
        payload.insert(Key::new(member).unwrap(), value);
    }
    body
}

fn declared(options: &[&str], pick: Option<usize>) -> Value {
    pick.map_or(Value::Null, |i| Value::Str(options[i % options.len()].to_owned()))
}

proptest! {
    /// Any signed figures and any declaration, listed or `null`, parse at version 2, and the
    /// journal keeps each figure as the same number: §9.35 records the broker's figures as they
    /// stand, and no rule refuses a deficit or a negative figure.
    #[test]
    fn any_figures_and_declaration_parse_and_keep_their_value(
        last_equity in -10_000_000_000i64..10_000_000_000,
        margin in -10_000_000_000i64..10_000_000_000,
        account_type in proptest::option::of(0usize..2),
        regime in proptest::option::of(0usize..2),
    ) {
        let section = section();
        let mut body = base(&section, "snapshot_v2");
        body = with(body, "last_equity", Value::Str(figure(last_equity)));
        body = with(body, "maintenance_margin", Value::Str(figure(margin)));
        body = with(body, "account_type", declared(&["cash", "margin"], account_type));
        body = with(body, "day_trading_regime", declared(&["intraday_margin", "legacy_pdt"], regime));
        let parsed = Draft::parse(&to_canonical(&Value::Object(body)));
        prop_assert!(parsed.is_ok(), "{parsed:?}");
        let body = parse(parsed.unwrap().canonical_bytes()).unwrap();
        let payload = body.get("payload").cloned().unwrap();
        for (member, cents) in [("last_equity", last_equity), ("maintenance_margin", margin)] {
            prop_assert_eq!(text(&payload, member), canonical(cents), "{}", member);
        }
    }

    /// A declaration outside its list is refused at its own member, whatever the figures.
    #[test]
    fn an_unlisted_declaration_is_refused_at_its_member(
        name in "[a-z_]{1,20}",
        on_regime in any::<bool>(),
        cents in -1_000_000i64..1_000_000,
    ) {
        let (member, listed): (&str, &[&str]) = if on_regime {
            ("day_trading_regime", &["intraday_margin", "legacy_pdt"])
        } else {
            ("account_type", &["cash", "margin"])
        };
        prop_assume!(!listed.contains(&name.as_str()));
        let section = section();
        let body = with(base(&section, "snapshot_v2"), "last_equity", Value::Str(figure(cents)));
        let body = with(body, member, Value::Str(name));
        let refused = Draft::parse(&to_canonical(&Value::Object(body))).err();
        let refused = refused.map(|e| (e.reason.code().to_owned(), e.path));
        prop_assert_eq!(refused, Some(("non_canonical".to_owned(), format!("payload.{member}"))));
    }
}
