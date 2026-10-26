//! Journal spec v0.89 §9.35 (DEC-2398) folded for the day-trade budget (DEC-2811): a version 2
//! `AccountSnapshotRecorded` carries the connector's declared account type and day-trading regime
//! and the broker's prior-close equity and maintenance margin; a version 1 carries none of them,
//! and the latest snapshot wins whatever its version. Every expectation is the vectors'
//! `snapshot_prior_close` section's, computed by the reference implementation, never by this crate.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_gate_context::{DeclaredAccount, DeclaredRegime, Journaled, Row};
use mandate_journal::Draft;
use mandate_num::Usd;

fn fixture() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    parse(&std::fs::read(path).unwrap()).unwrap()
}

fn text<'a>(value: &'a Value, name: &str) -> &'a str {
    value.get(name).and_then(Value::as_str).unwrap_or_default()
}

fn list<'a>(value: &'a Value, name: &str) -> &'a [Value] {
    value
        .get(name)
        .and_then(Value::as_array)
        .unwrap_or_default()
}

/// A `{path, value}` change, dotted from the envelope.
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
    node.insert(
        Key::new(last).unwrap(),
        change.get("value").cloned().unwrap(),
    );
}

fn draft(section: &Value, name: &str, changes: &[Value]) -> Object {
    let mut draft = section
        .get("drafts")
        .and_then(|drafts| drafts.get(name))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_else(|| panic!("no draft {name}"));
    for change in changes {
        apply(&mut draft, change);
    }
    draft
}

/// A base draft, or a valid draft case by its name: its base with its changes.
fn named(section: &Value, name: &str) -> Object {
    if section.get("drafts").and_then(|d| d.get(name)).is_some() {
        return draft(section, name, &[]);
    }
    let case = list(section, "valid_drafts")
        .iter()
        .find(|case| text(case, "name") == name)
        .unwrap_or_else(|| panic!("no draft or valid case {name}"));
    draft(section, text(case, "base_draft"), list(case, "changes"))
}

/// `draft` as the journal stores it: accepted by `Draft::parse`, with its decimals normalized.
fn stored(draft: &Object) -> Value {
    let accepted = Draft::parse(&to_canonical(&Value::Object(draft.clone())))
        .unwrap_or_else(|e| panic!("a valid draft is accepted: {e:?}"));
    parse(accepted.canonical_bytes()).unwrap()
}

/// The account `drafts` leave, folded in order on their own account stream.
fn folded(drafts: &[Object]) -> Journaled {
    let stream = drafts
        .first()
        .and_then(|d| d.get("stream_id"))
        .and_then(Value::as_str)
        .unwrap()
        .to_owned();
    let mut state = Journaled::new(&stream, "clock:ws1", "agent-1");
    for draft in drafts {
        let stored = stored(draft);
        let payload = stored.get("payload").unwrap();
        state
            .fold(Row {
                stream: &stream,
                event_type: "AccountSnapshotRecorded",
                payload,
            })
            .unwrap_or_else(|e| panic!("a valid snapshot folds: {e}"));
    }
    state
}

/// The account's reading as the vectors spell one.
fn reading(state: &Journaled) -> Value {
    let account = state.account().expect("a snapshot was folded");
    let mut out = Object::new();
    let put = |out: &mut Object, key: &str, value: Value| {
        out.insert(Key::new(key).unwrap(), value);
    };
    match &account.figures {
        None => put(&mut out, "reading", Value::Str("no_figures".to_owned())),
        Some(figures) => {
            put(&mut out, "reading", Value::Str("figures".to_owned()));
            let account_type = match figures.account_type {
                None => Value::Null,
                Some(DeclaredAccount::Cash) => Value::Str("cash".to_owned()),
                Some(DeclaredAccount::Margin) => Value::Str("margin".to_owned()),
            };
            let regime = match figures.day_trading_regime {
                None => Value::Null,
                Some(DeclaredRegime::IntradayMargin) => Value::Str("intraday_margin".to_owned()),
                Some(DeclaredRegime::LegacyPdt) => Value::Str("legacy_pdt".to_owned()),
            };
            put(&mut out, "account_type", account_type);
            put(&mut out, "day_trading_regime", regime);
            put(
                &mut out,
                "prior_close_equity",
                Value::Str(figures.last_equity.to_string()),
            );
            put(
                &mut out,
                "maintenance_excess",
                figures
                    .maintenance_excess(account.equity)
                    .map_or(Value::Null, |excess| Value::Str(excess.to_string())),
            );
        }
    }
    Value::Object(out)
}

/// The vectors' expectation, without the case's own name.
fn want(case: &Value, drop: &[&str]) -> Value {
    let mut out = case.as_object().cloned().unwrap();
    for key in drop {
        out.remove(*key);
    }
    Value::Object(out)
}

#[test]
fn every_reading_is_the_vectors() {
    let fixture = fixture();
    let section = fixture.get("snapshot_prior_close").unwrap();
    let readings = list(section, "readings");
    let mut failed = Vec::new();
    for case in readings {
        let name = text(case, "draft");
        let got = reading(&folded(&[named(section, name)]));
        if got != want(case, &["draft"]) {
            failed.push(format!("{name}: got {got:?}, want {case:?}"));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(
        readings.len() >= 15,
        "{} readings; never fewer",
        readings.len()
    );
}

#[test]
fn the_latest_snapshot_wins_whatever_its_version() {
    let fixture = fixture();
    let section = fixture.get("snapshot_prior_close").unwrap();
    let streams = list(section, "streams");
    let mut failed = Vec::new();
    for case in streams {
        let drafts: Vec<Object> = list(case, "steps")
            .iter()
            .map(|step| draft(section, text(step, "base_draft"), list(step, "changes")))
            .collect();
        let got = reading(&folded(&drafts));
        let expect = case.get("expect").unwrap();
        if &got != expect {
            failed.push(format!(
                "{}: got {got:?}, want {expect:?}",
                text(case, "name")
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(
        streams
            .iter()
            .any(|case| text(case, "name") == "v2_then_v1"),
        "a version 1 after a version 2 leaves no figures"
    );
}

#[test]
fn the_maintenance_excess_is_signed_and_absent_for_a_negative_margin() {
    let usd = |text: &str| Usd::parse(text).unwrap();
    let figures = |margin: &str| mandate_gate_context::SnapshotFigures {
        account_type: None,
        day_trading_regime: None,
        last_equity: usd("0"),
        maintenance_margin: usd(margin),
    };
    assert_eq!(figures("10").maintenance_excess(usd("4")), Some(usd("-6")));
    assert_eq!(figures("0").maintenance_excess(usd("4")), Some(usd("4")));
    assert_eq!(figures("-1").maintenance_excess(usd("4")), None);
}
