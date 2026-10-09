//! What governs a run beside its mandate (E7-19 slice 4 remainder, the brief's slice D4b; DEC-484,
//! DEC-505 item 1, DEC-534): the effective `policy_set` and `model_registry` registrations, the
//! policy read strictly, the registry's one entry for the pinned model, and §4.3's overlay over
//! the confirmed E7-7 paper mandate, which a policy can tighten and never loosen.

use std::collections::BTreeMap;

use mandate_canon::{Digest, Value, to_canonical};
use mandate_shell::control::{
    ConfigRefusal as Refusal, ControlRecord, Governance, Pinned, governance,
};
use mandate_spec::policy::{LevelName, PolicyKey, PolicyValue, values_of};
use mandate_spec::{Mandate, SchemaDec};

const MANDATE: &str = include_str!("fixtures/tracer/mandate.json");
const MODEL: &str = r#"{"kind":"quant_model_content","model_id":"quant.ma_crossover"}"#;
/// §9.2's closed payload with rule 21's nulls: `@K` is the kind and `@H` the content hash.
const RECORD: &str = r#"{"admits_instruments":null,"content_hash":"@H","kind":"@K","model_id":null,"model_version":null,"params":[]}"#;
const MODEL_RECORD: &str = r#"{"admits_instruments":false,"content_hash":"@H","kind":"model_version","model_id":"quant.ma_crossover","model_version":"1.0.0","params":["fast_periods","slow_periods"]}"#;
/// The registry entry for the pinned model, as its registration states it; `@H` is its hash.
const ENTRY: &str = r#"{"admits_instruments":false,"content_hash":"@H","model_id":"quant.ma_crossover","model_version":"1.0.0","params":["fast_periods","slow_periods"]}"#;
const OTHER: &str = r#"{"admits_instruments":false,"content_hash":"sha256:9999999999999999999999999999999999999999999999999999999999999999","model_id":"quant.other","model_version":"2.0.0","params":[]}"#;
const DEPLOYED: &str = r#"{"agent_id":"agent_spy","mandate_version":"sha256:8888888888888888888888888888888888888888888888888888888888888888","record_ref":"sha256:6666666666666666666666666666666666666666666666666666666666666666"}"#;

fn json(text: &str) -> Value {
    mandate_canon::parse(text.as_bytes()).unwrap()
}

fn digest(object: &str) -> Digest {
    Digest::of(&to_canonical(&json(object)))
}

fn hash(object: &str) -> String {
    format!("sha256:{}", digest(object))
}

/// A policy document at `level` with `values`, given as the members' JSON text.
fn level(level: &str, values: &str) -> String {
    format!(r#"{{"level":"{level}","policy_schema_version":1,"values":{{{values}}}}}"#)
}

fn policy_set(levels: &[&str]) -> String {
    let levels = levels.join(",");
    format!(r#"{{"kind":"policy_set","levels":[{levels}],"policy_set_version":1}}"#)
}

/// A registry of `entries`, each an entry's JSON text with `@H` the pinned model's hash.
fn registry(entries: &[&str]) -> String {
    let models = entries.join(",").replace("@H", &hash(MODEL));
    format!(r#"{{"kind":"model_registry","model_registry_version":1,"models":[{models}]}}"#)
}

/// A control stream and the store holding each registered object.
#[derive(Default)]
struct Stream {
    records: Vec<ControlRecord>,
    store: BTreeMap<Digest, Vec<u8>>,
}

impl Stream {
    /// Seq 1: the pinned model's registration; seq 2 and 3: `policy` and `registry`; seq 4, which
    /// changes nothing, an `AgentDeployed`.
    fn governed(policy: &str, registry: &str) -> Self {
        let stream = Self::default().stored(MODEL, MODEL_RECORD);
        let stream = stream.register("policy_set", policy);
        let stream = stream.register("model_registry", registry);
        stream.then("AgentDeployed", DEPLOYED)
    }

    fn register(self, kind: &str, object: &str) -> Self {
        self.stored(object, &RECORD.replace("@K", kind))
    }

    fn stored(mut self, object: &str, record: &str) -> Self {
        let bytes = to_canonical(&json(object));
        self.store.insert(digest(object), bytes);
        let record = record.replace("@H", &hash(object));
        self.then("ConfigSnapshotRegistered", &record)
    }

    fn then(mut self, event_type: &str, payload: &str) -> Self {
        self.records.push(ControlRecord {
            seq: u64::try_from(self.records.len()).unwrap() + 1,
            event_type: event_type.to_owned(),
            payload: json(payload),
        });
        self
    }

    fn read(&self) -> Result<Governance, Refusal> {
        let mandate = Mandate::parse(&json(MANDATE)).unwrap();
        governance(&self.records, &self.store, &pinned(), &mandate)
    }

    fn refused(&self) -> Option<Refusal> {
        self.read().err()
    }
}

fn pinned() -> Pinned {
    Pinned {
        asset_id: "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415".to_owned(),
        symbol: "AAPL".to_owned(),
        model_id: "quant.ma_crossover".to_owned(),
        model_version: "1.0.0".to_owned(),
        content_hash: digest(MODEL),
    }
}

/// The latest `policy_set` and `model_registry` registrations by `seq` govern, in either slice
/// order; a record of another type carrying a registration's payload does not count, and neither
/// does a later `model_version` registration of another id, version or hash.
#[test]
fn the_latest_policy_set_and_registry_govern() {
    let early = policy_set(&[&level("workspace", r#""max_orders_per_day":100"#)]);
    let late = policy_set(&[&level("workspace", r#""max_orders_per_day":60"#)]);
    let entries = registry(&[ENTRY]);
    let lookalike = RECORD
        .replace("@K", "policy_set")
        .replace("@H", &hash(&early));
    let stream = Stream::governed(&early, &registry(&[OTHER]));
    let stream = stream.register("policy_set", &late);
    let stream = stream.register("model_registry", &entries);
    let mut stream = stream.then("MandateConfirmed", &lookalike);
    let other = r#"{"kind":"quant_model_content","model_id":"quant.other"}"#;
    let narrower = MODEL_RECORD.replace(r#""fast_periods","slow_periods""#, r#""lookback_bars""#);
    let not_pinned = [
        (other, narrower.replace("quant.ma_crossover", "quant.other")),
        (MODEL, narrower.replace("1.0.0", "1.0.1")),
        (other, narrower.clone()),
    ];
    for (content, record) in &not_pinned {
        stream = stream.stored(content, record);
    }
    for swapped in [false, true] {
        let read = stream.read().unwrap();
        let policy = (read.policy_set.content_hash, read.policy_set.seq);
        assert_eq!(policy, (digest(&late), 5), "swapped: {swapped}");
        let models = (read.model_registry.content_hash, read.model_registry.seq);
        assert_eq!(models, (digest(&entries), 6), "swapped: {swapped}");
        let bytes = &read.model_registry.bytes;
        assert_eq!(bytes, &to_canonical(&json(&entries)), "swapped: {swapped}");
        assert_eq!(read.policy_set.bytes, to_canonical(&json(&late)));
        stream.records.swap(1, 4);
        stream.records.swap(2, 5);
    }
}

/// A policy looser than the confirmed mandate, at two levels, or one stating nothing, loosens
/// nothing: the mandate conforms, every value it states is the effective one, and `auto` stays as
/// the mandate has it (mandate spec §4.3, stricter-of).
#[test]
fn a_looser_policy_never_loosens_the_confirmed_mandate() {
    let mandate = Mandate::parse(&json(MANDATE)).unwrap();
    let stated = values_of(&mandate).unwrap();
    let loose = r#""auto_allowed":true,"environments":["live","paper"],"max_daily_loss":"0.5","max_order_usd":"1000000","max_orders_per_day":10000"#;
    let platform = level("platform", r#""max_order_usd":"5000000""#);
    let policies = [
        policy_set(&[&platform, &level("workspace", loose)]),
        policy_set(&[]),
    ];
    for policy in policies {
        let read = Stream::governed(&policy, &registry(&[ENTRY]))
            .read()
            .unwrap();
        assert!(read.violations.is_empty(), "{policy}");
        for (key, value) in &stated {
            let effective = read.overlay.effective(*key, value);
            assert_eq!(effective.as_ref(), Ok(value), "{key:?} under {policy}");
        }
        assert!(read.overlay.auto_allowed(), "{policy}");
    }
}

/// A policy registered after the confirmation that the mandate exceeds makes it nonconforming:
/// no refusal, but the violations, which deny every action that adds risk (DEC-534), and an
/// overlay that forbids `auto`.
#[test]
fn a_mandate_beyond_a_later_policy_is_nonconforming_not_refused() {
    let tight = level(
        "workspace",
        r#""auto_allowed":false,"max_daily_loss":"0.01""#,
    );
    let read = Stream::governed(&policy_set(&[&tight]), &registry(&[ENTRY])).read();
    let read = read.unwrap();
    let broken: Vec<(PolicyKey, LevelName)> = read
        .violations
        .iter()
        .map(|violation| (violation.key, violation.limit_level))
        .collect();
    assert!(
        broken.contains(&(PolicyKey::MaxDailyLoss, LevelName::Workspace)),
        "{broken:?}"
    );
    assert!(
        broken.contains(&(PolicyKey::AutoAllowed, LevelName::Workspace)),
        "{broken:?}"
    );
    assert!(!read.overlay.auto_allowed());
    let mandate = Mandate::parse(&json(MANDATE)).unwrap();
    let stated = values_of(&mandate).unwrap();
    let own = &stated[&PolicyKey::MaxDailyLoss];
    let PolicyValue::Decimal(own_loss) = own else {
        panic!("the mandate states max_daily_loss as a decimal: {own:?}");
    };
    let ceiling = SchemaDec::parse("0.01", own_loss.grammar()).unwrap();
    let effective = read.overlay.effective(PolicyKey::MaxDailyLoss, own);
    assert_eq!(effective, Ok(PolicyValue::Decimal(ceiling)));
}

/// Each kind must be registered and its object stored intact; the policy set must read as
/// `policy.schema.json` and journal spec §9 say; and the registry must be shaped as §9 says and
/// hold exactly one entry equal to the pinned model's registration (DEC-484 item 5). Journal spec
/// §9's `model_registry` object has exactly the members `kind`, `model_registry_version` (1) and
/// `models`; `models` are strictly sorted and unique by `model_id`; and each entry has exactly the
/// five members, whose `params` are strictly sorted and unique strings.
#[test]
fn an_unregistered_unreadable_or_mismatched_policy_or_registry_refuses() {
    let policy = policy_set(&[&level("workspace", r#""max_orders_per_day":100"#)]);
    let entries = registry(&[ENTRY]);
    let kind = |kind| Refusal::Malformed { kind };
    let mut no_model = Stream::governed(&policy, &entries);
    no_model.records.remove(0);
    let model = Refusal::Unregistered {
        kind: "model_version",
    };
    assert_eq!(no_model.refused(), Some(model));
    for (index, name) in [(1, "policy_set"), (2, "model_registry")] {
        let mut unregistered = Stream::governed(&policy, &entries);
        unregistered.records.remove(index);
        let refusal = Refusal::Unregistered { kind: name };
        assert_eq!(unregistered.refused(), Some(refusal));
    }
    for (object, name) in [(&policy, "policy_set"), (&entries, "model_registry")] {
        let mut missing = Stream::governed(&policy, &entries);
        missing.store.remove(&digest(object));
        assert_eq!(
            missing.refused(),
            Some(Refusal::ObjectMissing { kind: name })
        );
        let mut corrupt = Stream::governed(&policy, &entries);
        corrupt.store.get_mut(&digest(object)).unwrap().push(b' ');
        assert_eq!(
            corrupt.refused(),
            Some(Refusal::ObjectCorrupt { kind: name })
        );
    }
    let off_schema = policy_set(&[&level("workspace", r#""max_instruments":0"#)]);
    let reversed = policy_set(&[&level("workspace", ""), &level("platform", "")]);
    for bad in [off_schema, reversed] {
        let refused = Stream::governed(&bad, &entries).refused();
        assert_eq!(refused, Some(kind("policy_set")), "{bad}");
    }
    let unsorted_params = ENTRY.replace(
        r#""fast_periods","slow_periods""#,
        r#""slow_periods","fast_periods""#,
    );
    let duplicate_params = ENTRY.replace(
        r#""fast_periods","slow_periods""#,
        r#""fast_periods","fast_periods""#,
    );
    let sixth_member = ENTRY.replace(
        r#""admits_instruments""#,
        r#""extra":1,"admits_instruments""#,
    );
    let renamed_member = ENTRY.replace(r#""params""#, r#""parameters""#);
    let retyped_params = ENTRY.replace(r#""fast_periods","#, "7,");
    let retyped = [
        (r#""model_version":"1.0.0""#, r#""model_version":100"#),
        (r#""content_hash":"@H""#, r#""content_hash":7"#),
        (
            r#""admits_instruments":false"#,
            r#""admits_instruments":"false""#,
        ),
    ];
    for (from, to) in retyped {
        let refused = Stream::governed(&policy, &registry(&[&ENTRY.replace(from, to)])).refused();
        assert_eq!(refused, Some(kind("model_registry")), "{to}");
    }
    let malformed = [
        registry(&[OTHER, ENTRY]),
        registry(&[ENTRY, ENTRY]),
        registry(&[&unsorted_params]),
        registry(&[&duplicate_params]),
        registry(&[&sixth_member]),
        registry(&[&renamed_member]),
        registry(&[&retyped_params]),
        entries.replace(r#""kind":"model_registry""#, r#""kind":"policy_set""#),
        entries.replace(r#""models""#, r#""extra":1,"models""#),
        entries.replace(
            r#""model_registry_version":1"#,
            r#""model_registry_version":2"#,
        ),
    ];
    for bad in malformed {
        let refused = Stream::governed(&policy, &bad).refused();
        assert_eq!(refused, Some(kind("model_registry")), "{bad}");
    }
    let mismatched = [
        registry(&[]),
        registry(&[OTHER]),
        registry(&[&ENTRY.replace("1.0.0", "1.0.1")]),
        registry(&[&ENTRY.replace(r#""fast_periods","#, "")]),
        registry(&[&ENTRY.replace("false", "true")]),
        registry(&[&ENTRY.replace("@H", &format!("sha256:{}", "7".repeat(64)))]),
    ];
    for bad in mismatched {
        let refused = Stream::governed(&policy, &bad).refused();
        assert_eq!(refused, Some(Refusal::RegistryMismatch), "{bad}");
    }
    let beside = registry(&[ENTRY, OTHER]);
    let read = Stream::governed(&policy, &beside).read();
    let read = read.map(|governance| governance.model_registry.content_hash);
    assert_eq!(
        read,
        Ok(digest(&beside)),
        "one pinned entry beside another model's"
    );
}

/// A later `policy_set` or `model_registry` registration that cannot be used refuses the run: the
/// earlier, valid registration of the same kind never governs in its place (DEC-505 item 1, no
/// silent fallback). Each later object is missing from the store, corrupt, off its schema, or, for
/// the registry, without the pinned model's entry.
#[test]
fn a_later_unusable_policy_or_registry_never_falls_back_to_an_earlier_one() {
    let policy = policy_set(&[&level("workspace", r#""max_orders_per_day":100"#)]);
    let entries = registry(&[ENTRY]);
    let later_policy = policy_set(&[&level("workspace", r#""max_orders_per_day":60"#)]);
    let off_schema = policy_set(&[&level("workspace", r#""max_instruments":0"#)]);
    let later_registry = registry(&[ENTRY, OTHER]);
    let extra = entries.replace(r#""models""#, r#""extra":1,"models""#);
    let later = [
        ("policy_set", later_policy.as_str(), off_schema.as_str()),
        ("model_registry", later_registry.as_str(), extra.as_str()),
    ];
    for (kind, usable, malformed) in later {
        let stream = || Stream::governed(&policy, &entries).register(kind, usable);
        let mut missing = stream();
        missing.store.remove(&digest(usable));
        assert_eq!(missing.refused(), Some(Refusal::ObjectMissing { kind }));
        let mut corrupt = stream();
        corrupt.store.get_mut(&digest(usable)).unwrap().push(b' ');
        assert_eq!(corrupt.refused(), Some(Refusal::ObjectCorrupt { kind }));
        let off = Stream::governed(&policy, &entries).register(kind, malformed);
        assert_eq!(off.refused(), Some(Refusal::Malformed { kind }));
    }
    let other = registry(&[OTHER]);
    let unpinned = Stream::governed(&policy, &entries).register("model_registry", &other);
    assert_eq!(unpinned.refused(), Some(Refusal::RegistryMismatch));
}
