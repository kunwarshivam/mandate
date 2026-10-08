//! D1 (E10-16, DEC-526): `config register` and `model register` store their object and commit one
//! `ConfigSnapshotRegistered`. Each committed draft is read back through `mandate_journal`'s own
//! schema check (§9.2) and compared with a payload written out here. The instrument snapshot is
//! held to DEC-523, the model's content and hash come from `mandate_modelhost::content`, and
//! nothing is registered outside paper.

mod common;

use std::collections::BTreeMap;

use common::{CONTROL, Journal, at, owner, stream};
use mandate_canon::{Digest, Key, Value, parse, to_canonical};
use mandate_cli::config::{ConfigKind, register, register_model};
use mandate_cli::control::{ControlError, ControlJournal, Owner, Submitted};
use mandate_journal::{
    ArtifactError, ArtifactRef, ArtifactSource, ArtifactStore, Draft, Environment,
};

type Store = BTreeMap<Digest, Vec<u8>>;

const NOW: i64 = 1_790_000_000;
const MODEL: (&str, &str) = ("quant.ma_crossover", "1.0.0");
const SPY: &str = r#"{"asset_class":"us_equity","etp":"plain","etp_classified_at":"2026-10-05T13:30:00Z","etp_source":"nasdaq_trader_symbol_directory","exchange":"arca","increment":"whole","instrument_id":"b0b6dd9d-8b9b-48a9-ba46-b9d54906e415","symbol":"SPY"}"#;

/// The control stream's one event, accepted by the journal's schema check, and its payload.
fn registered(journal: &Journal) -> (Draft, Value) {
    let rows = journal.rows(&stream(CONTROL)).unwrap();
    let [row] = rows.as_slice() else {
        panic!("one event on the control stream: {rows:?}")
    };
    let draft = Draft::parse(&row.body).unwrap();
    (
        draft,
        parse(&row.body).unwrap().get("payload").unwrap().clone(),
    )
}

/// The payload §9.2 gives a registration of a non-model `kind` naming `canonical`.
fn plain_payload(kind: &str, canonical: &[u8]) -> Value {
    let hash = Digest::of(canonical).to_hex();
    let text = format!(
        r#"{{"admits_instruments":null,"content_hash":"sha256:{hash}","kind":"{kind}","model_id":null,"model_version":null,"params":[]}}"#
    );
    parse(text.as_bytes()).unwrap()
}

/// SPY's snapshot with `change` applied to its members.
fn spy_with(change: impl FnOnce(&mut BTreeMap<Key, Value>)) -> Vec<u8> {
    let Value::Object(mut members) = parse(SPY.as_bytes()).unwrap() else {
        panic!("SPY is an object")
    };
    change(&mut members);
    to_canonical(&Value::Object(members))
}

/// `config register` of an instrument snapshot by `owner` at [`NOW`].
fn snapshot(
    journal: &mut Journal,
    store: &mut dyn ArtifactStore,
    owner: &Owner,
    object: &[u8],
) -> Result<Submitted, ControlError> {
    let kind = ConfigKind::InstrumentSnapshot;
    register(journal, store, owner, kind, object, at(NOW))
}

/// `model register` of `(id, version)` by `owner` at [`NOW`].
fn model(
    journal: &mut Journal,
    store: &mut dyn ArtifactStore,
    owner: &Owner,
    (id, version): (&str, &str),
) -> Result<Submitted, ControlError> {
    register_model(journal, store, owner, id, version, at(NOW))
}

/// Whether anything was stored or any append attempted.
fn written(journal: &Journal, store: &Store) -> bool {
    !store.is_empty() || !journal.attempts.is_empty()
}

fn refused(result: Result<impl std::fmt::Debug, ControlError>) -> &'static str {
    match result {
        Err(ControlError::Refused { reason }) => reason,
        other => panic!("not refused: {other:?}"),
    }
}

#[test]
fn an_instrument_snapshot_of_dec_523_is_stored_registered_and_found_on_a_rerun() {
    for exchange in ["arca", "nasdaq"] {
        let canonical_input = spy_with(set_key("exchange", Value::Str(exchange.into())));
        let object = [b"\n ", canonical_input.as_slice(), b" \n"].concat();
        let (mut journal, mut store) = (Journal::default(), Store::new());
        let first = snapshot(&mut journal, &mut store, &owner(), &object).unwrap();
        let canonical = to_canonical(&parse(&object).unwrap());
        assert_eq!(
            store.values().collect::<Vec<_>>(),
            [&canonical],
            "{exchange}: the canonical form, stored under its hash"
        );
        assert_eq!(store.keys().next(), Some(&Digest::of(&canonical)));
        let (draft, payload) = registered(&journal);
        let shape = (draft.event_type(), draft.schema_version());
        assert_eq!(shape, ("ConfigSnapshotRegistered", 1));
        assert_eq!(payload, plain_payload("instrument_snapshot", &canonical));
        let kind = ConfigKind::InstrumentSnapshot;
        let again = register(
            &mut journal,
            &mut store,
            &owner(),
            kind,
            &object,
            at(NOW + 60),
        );
        assert_eq!(again.unwrap(), first, "a re-run finds it (DEC-290)");
        registered(&journal);
    }
}

#[test]
fn a_snapshot_outside_dec_523_is_refused_before_anything_is_stored() {
    let changes = [
        ("note", "an extra member"),
        ("etp_source", "manual"),
        ("exchange", "nyse"),
        ("asset_class", "crypto"),
        ("etp", "leveraged"),
        ("increment", "fractional"),
        ("instrument_id", ""),
        ("symbol", ""),
        ("etp_classified_at", "2026-10-05t13:30:00Z"),
        ("etp_classified_at", "yesterday"),
    ];
    let mut cases: Vec<(String, Vec<u8>)> = changes
        .iter()
        .map(|(name, value)| {
            let object = spy_with(set_key(name, Value::Str((*value).into())));
            (format!("{name} {value:?}"), object)
        })
        .collect();
    let Value::Object(names) = parse(SPY.as_bytes()).unwrap() else {
        panic!("SPY is an object")
    };
    for name in names.keys() {
        let gone = spy_with(|m| {
            m.remove(name.as_str());
        });
        let number = spy_with(set_key(name.as_str(), Value::Bool(true)));
        cases.push((format!("{} missing", name.as_str()), gone));
        cases.push((format!("{} not a string", name.as_str()), number));
    }
    assert_eq!(cases.len(), 26, "every member removed and mistyped once");
    for (case, object) in cases {
        let (mut journal, mut store) = (Journal::default(), Store::new());
        let result = snapshot(&mut journal, &mut store, &owner(), &object);
        assert_eq!(refused(result), "instrument_snapshot_invalid", "{case}");
        assert!(!written(&journal, &store), "{case}");
    }
}

fn set_key(name: &str, value: Value) -> impl FnOnce(&mut BTreeMap<Key, Value>) {
    let name = Key::new(name).unwrap();
    move |m| {
        m.insert(name, value);
    }
}

#[test]
fn the_other_kinds_register_at_their_schema_version_and_a_bad_object_is_refused() {
    let kinds = [
        (ConfigKind::FeeConfig, "fee_config", 1),
        (ConfigKind::TradingCalendar, "trading_calendar", 1),
        (ConfigKind::RuleSet, "rule_set", 1),
        (ConfigKind::PolicySet, "policy_set", 2),
        (ConfigKind::ModelRegistry, "model_registry", 2),
    ];
    for (kind, code, version) in kinds {
        let object = format!(r#"{{"kind":"{code}","note":"a {code} object"}}"#);
        let (mut journal, mut store) = (Journal::default(), Store::new());
        let given = object.as_bytes();
        register(&mut journal, &mut store, &owner(), kind, given, at(NOW)).unwrap();
        let canonical = to_canonical(&parse(object.as_bytes()).unwrap());
        assert_eq!(
            store.get(&Digest::of(&canonical)),
            Some(&canonical),
            "{code}"
        );
        let (draft, payload) = registered(&journal);
        assert_eq!(draft.schema_version(), version, "{code}");
        assert_eq!(payload, plain_payload(code, &canonical), "{code}");
    }
    let cases: [(ConfigKind, &[u8], &str); 5] = [
        (
            ConfigKind::PolicySet,
            br#"{"kind":"model_registry"}"#,
            "config_kind_mismatch",
        ),
        (
            ConfigKind::ModelRegistry,
            br#"{"note":"no kind"}"#,
            "config_kind_mismatch",
        ),
        (ConfigKind::FeeConfig, b"[1,2]", "config_object_invalid"),
        (
            ConfigKind::InstrumentSnapshot,
            b"\"SPY\"",
            "config_object_invalid",
        ),
        (ConfigKind::RuleSet, b"not json", "config_object_invalid"),
    ];
    for (kind, object, reason) in cases {
        let (mut journal, mut store) = (Journal::default(), Store::new());
        let result = register(&mut journal, &mut store, &owner(), kind, object, at(NOW));
        assert_eq!(refused(result), reason, "{kind:?}");
        assert!(!written(&journal, &store), "{kind:?}");
    }
}

#[test]
fn a_model_is_registered_with_the_hosts_content_and_hash_only() {
    let (mut journal, mut store) = (Journal::default(), Store::new());
    let (id, version) = MODEL;
    model(&mut journal, &mut store, &owner(), MODEL).unwrap();
    let content = mandate_modelhost::content(id, version).unwrap();
    assert_eq!(Digest::of(&content.canonical), content.hash);
    assert_eq!(store.get(&content.hash), Some(&content.canonical));
    let object = parse(&content.canonical).unwrap();
    let names: Vec<&str> = object
        .get("params_schema")
        .and_then(|schema| schema.get("params"))
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .map(|p| p.get("name").and_then(Value::as_str).unwrap())
        .collect();
    assert_eq!(names, ["fast_periods", "slow_periods"]);
    let text = format!(
        r#"{{"admits_instruments":false,"content_hash":"sha256:{}","kind":"model_version","model_id":"{id}","model_version":"{version}","params":["fast_periods","slow_periods"]}}"#,
        content.hash.to_hex()
    );
    let (draft, payload) = registered(&journal);
    assert_eq!(
        (draft.schema_version(), payload),
        (1, parse(text.as_bytes()).unwrap())
    );
    for unknown in [("quant.other", "1.0.0"), ("quant.ma_crossover", "9.9.9")] {
        let (mut journal, mut store) = (Journal::default(), Store::new());
        let result = model(&mut journal, &mut store, &owner(), unknown);
        assert_eq!(refused(result), "model_unknown", "{unknown:?}");
        assert!(!written(&journal, &store), "{unknown:?}");
    }
}

#[test]
fn nothing_is_registered_outside_paper() {
    for environment in [Environment::Live, Environment::Backtest] {
        let owner = Owner {
            environment,
            ..owner()
        };
        let (mut journal, mut store) = (Journal::default(), Store::new());
        let registered = snapshot(&mut journal, &mut store, &owner, SPY.as_bytes());
        let modelled = model(&mut journal, &mut store, &owner, MODEL);
        let reasons = [refused(registered), refused(modelled)];
        assert_eq!(reasons, ["paper_only"; 2], "{environment:?}");
        assert!(!written(&journal, &store), "{environment:?}");
    }
}

/// A store every write to fails.
struct Unwritable;

impl ArtifactSource for Unwritable {
    fn read_artifact(&self, _: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
        Err(ArtifactError::Unavailable)
    }
}

impl ArtifactStore for Unwritable {
    fn put_artifact(&mut self, _: &[u8]) -> Result<ArtifactRef, ArtifactError> {
        Err(ArtifactError::Unavailable)
    }
}

/// The object is stored before the event is committed, and a store that fails commits nothing:
/// both commands report the failure, and the journal sees no append (DEC-526 item 5).
#[test]
fn a_store_that_fails_commits_nothing() {
    let mut journal = Journal::default();
    let registered = snapshot(&mut journal, &mut Unwritable, &owner(), SPY.as_bytes());
    let modelled = model(&mut journal, &mut Unwritable, &owner(), MODEL);
    for result in [registered, modelled] {
        assert!(
            matches!(result, Err(ControlError::Journal(_))),
            "{result:?}"
        );
    }
    assert!(journal.attempts.is_empty(), "{:?}", journal.attempts);
}
