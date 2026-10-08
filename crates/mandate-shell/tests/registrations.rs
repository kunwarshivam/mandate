//! The effective configuration registrations (E19-11, DEC-505 item 1; the brief's slice D3, part
//! b): the latest by `seq` per kind, the asset-id and model-triple narrowings, the fee date, and
//! each registered object re-hashed. Each object is written here and registered in journal spec
//! §9.2's closed `ConfigSnapshotRegistered` payload, among records of other types and kinds.

use std::collections::BTreeMap;

use mandate_canon::{Digest, Value, to_canonical};
use mandate_journal::{ArtifactError, ArtifactRef, ArtifactSource};
use mandate_shell::control::{
    ConfigRefusal as Refusal, Configuration, ControlRecord, Pinned, configuration,
};
use mandate_time::Date;

const SPY: &str = "b28f4066-5c6d-479b-a2af-85dc1a8f16fb";
const AAPL: &str = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415";
const SNAPSHOT: &str = r#"{"asset_class":"us_equity","etp":"plain","etp_classified_at":"2026-10-05T00:00:00Z","etp_source":"nasdaq_trader_symbol_directory","exchange":"arca","increment":"whole","instrument_id":"b28f4066-5c6d-479b-a2af-85dc1a8f16fb","symbol":"SPY"}"#;
const FEE: &str =
    r#"{"effective_from":"2026-01-01","environment":"paper","schedule":"conservative_v1"}"#;
const CALENDAR: &str =
    r#"{"first":"2026-01-01","holidays":[],"last":"2026-12-31","special_sessions":[]}"#;
const RULES: &str = r#"{"rules":"reviewed_v1"}"#;
const MODEL: &str = r#"{"kind":"quant_model_content","model_id":"quant.ma_crossover"}"#;
const POLICY: &str = r#"{"kind":"policy_set","levels":[],"policy_set_version":1}"#;
/// §9.2's closed payload with rule 21's nulls: `@K` is the kind and `@H` the content hash.
const RECORD: &str = r#"{"admits_instruments":null,"content_hash":"@H","kind":"@K","model_id":null,"model_version":null,"params":[]}"#;
const MODEL_RECORD: &str = r#"{"admits_instruments":false,"content_hash":"@H","kind":"model_version","model_id":"quant.ma_crossover","model_version":"1.0.0","params":["fast_periods","slow_periods"]}"#;
const DEPLOYED: &str = r#"{"agent_id":"agent_spy","mandate_version":"sha256:8888888888888888888888888888888888888888888888888888888888888888","record_ref":"sha256:6666666666666666666666666666666666666666666666666666666666666666"}"#;
const CONFIRMED: &str = r#"{"confirmed_paths":["/name"],"mandate_version":"sha256:8888888888888888888888888888888888888888888888888888888888888888"}"#;
const TODAY: &str = "2026-10-07";

fn json(text: &str) -> Value {
    mandate_canon::parse(text.as_bytes()).unwrap()
}

fn digest(object: &str) -> Digest {
    Digest::of(&to_canonical(&json(object)))
}

/// A control stream and the store holding each registered object.
#[derive(Default)]
struct Stream {
    records: Vec<ControlRecord>,
    store: BTreeMap<Digest, Vec<u8>>,
}

impl Stream {
    /// Seq 1 to 5: one registration of each kind the run uses, `snapshot` and the pinned model's;
    /// then seq 6 to 8, which change nothing: a `policy_set` registration (D4's kind), and an
    /// `AgentDeployed` and a `MandateConfirmed`.
    fn registered(snapshot: &str) -> Self {
        let stream = Self::default().register("fee_config", FEE);
        let stream = stream.register("trading_calendar", CALENDAR);
        let stream = stream.register("rule_set", RULES);
        let stream = stream.register("instrument_snapshot", snapshot);
        let stream = stream
            .stored(MODEL, MODEL_RECORD)
            .register("policy_set", POLICY);
        stream
            .then("AgentDeployed", DEPLOYED)
            .then("MandateConfirmed", CONFIRMED)
    }

    fn register(self, kind: &str, object: &str) -> Self {
        self.stored(object, &RECORD.replace("@K", kind))
    }

    /// `content` stored and registered by `record`, with `@H` its content hash.
    fn stored(mut self, content: &str, record: &str) -> Self {
        let bytes = to_canonical(&json(content));
        self.store.insert(digest(content), bytes);
        let record = record.replace("@H", &format!("sha256:{}", digest(content)));
        self.then("ConfigSnapshotRegistered", &record)
    }

    fn then(mut self, event_type: &str, payload: &str) -> Self {
        let seq = u64::try_from(self.records.len()).unwrap() + 1;
        let (event_type, payload) = (event_type.to_owned(), json(payload));
        let record = ControlRecord {
            seq,
            event_type,
            payload,
        };
        self.records.push(record);
        self
    }

    fn read(&self, trade_date: &str) -> Result<Configuration, Refusal> {
        let trade_date = Date::parse(trade_date).unwrap();
        configuration(&self.records, &self.store, &pinned(), trade_date)
    }

    /// Why the stream is refused on [`TODAY`], if it is.
    fn refused(&self) -> Option<Refusal> {
        self.read(TODAY).err()
    }
}

/// An artifact store that cannot be read.
struct Unavailable;
impl ArtifactSource for Unavailable {
    fn read_artifact(&self, _: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
        Err(ArtifactError::Unavailable)
    }
}

fn pinned() -> Pinned {
    Pinned {
        asset_id: SPY.to_owned(),
        model_id: "quant.ma_crossover".to_owned(),
        model_version: "1.0.0".to_owned(),
        content_hash: digest(MODEL),
    }
}

/// Each kind's effective registration names its object and carries the stored bytes; records of
/// other types and kinds change nothing.
#[test]
#[ignore = "pending E19-11"]
fn every_kind_has_its_registered_object() {
    let config = Stream::registered(SNAPSHOT).read(TODAY).unwrap();
    let hashes = [
        (config.fee_config.content_hash, FEE),
        (config.trading_calendar.content_hash, CALENDAR),
        (config.rule_set.content_hash, RULES),
        (config.instrument_snapshot.content_hash, SNAPSHOT),
        (config.model_version.content_hash, MODEL),
    ];
    for (hash, object) in hashes {
        assert_eq!(hash, digest(object), "{object}");
    }
    let fee_bytes = to_canonical(&json(FEE));
    assert_eq!(config.fee_config.bytes, fee_bytes, "the stored bytes");
}

/// The latest registration by `seq` counts, in whatever order the slice lists them; a later fee
/// schedule not yet effective on the trade date refuses rather than falling back, and counts from
/// its `effective_from` on.
#[test]
#[ignore = "pending E19-11"]
fn the_latest_registration_counts_and_a_future_fee_schedule_refuses() {
    let rules = r#"{"rules":"reviewed_v2"}"#;
    let mut later = Stream::registered(SNAPSHOT).register("rule_set", rules);
    for swapped in [false, true] {
        let config = later.read(TODAY).unwrap();
        let latest = (config.rule_set.content_hash, config.rule_set.seq);
        assert_eq!(latest, (digest(rules), 9), "swapped: {swapped}");
        later.records.swap(2, 8);
    }
    let fee = FEE.replace("2026-01-01", "2026-10-09");
    let future = Stream::registered(SNAPSHOT).register("fee_config", &fee);
    assert_eq!(future.refused(), Some(Refusal::FeeNotYetEffective));
    let effective = future.read("2026-10-09").map(|c| c.fee_config.content_hash);
    assert_eq!(effective, Ok(digest(&fee)), "effective on its own date");
}

/// A snapshot counts only for the pinned asset id and a `model_version` only for the pinned triple
/// (DEC-505 item 1): later ones for anything else, the pinned hash under another id or version
/// included, do not displace them, and without one there is none. Neither does a record of another
/// type that carries a kind and a hash.
#[test]
#[ignore = "pending E19-11"]
fn only_the_pinned_instrument_and_model_registrations_count() {
    let aapl = SNAPSHOT.replace(SPY, AAPL).replace("SPY", "AAPL");
    let other_model = r#"{"kind":"quant_model_content","model_id":"quant.other"}"#;
    let renamed = MODEL_RECORD.replace("quant.ma_crossover", "quant.other");
    let bumped = MODEL_RECORD.replace("1.0.0", "1.0.1");
    let lookalike = RECORD.replace("@K", "rule_set");
    let lookalike = lookalike.replace("@H", &format!("sha256:{}", digest(FEE)));
    let later = Stream::registered(SNAPSHOT).register("instrument_snapshot", &aapl);
    let later = later
        .stored(other_model, MODEL_RECORD)
        .stored(MODEL, &renamed);
    let later = later
        .stored(MODEL, &bumped)
        .then("MandateConfirmed", &lookalike);
    let config = later.read(TODAY).unwrap();
    let snapshot = (
        config.instrument_snapshot.content_hash,
        config.instrument_snapshot.seq,
    );
    assert_eq!(snapshot, (digest(SNAPSHOT), 4), "SPY's earlier one");
    let model = (config.model_version.content_hash, config.model_version.seq);
    assert_eq!(model, (digest(MODEL), 5), "the pinned triple's own");
    assert_eq!(config.rule_set.seq, 3, "not the lookalike");
    let kind = "instrument_snapshot";
    let only_aapl = Stream::registered(&aapl).refused();
    assert_eq!(only_aapl, Some(Refusal::Unregistered { kind }));
    let mut no_model = Stream::registered(SNAPSHOT);
    no_model.records.remove(4);
    let kind = "model_version";
    assert_eq!(no_model.refused(), Some(Refusal::Unregistered { kind }));
}

/// A kind with no registration, a fee object without `effective_from`, a store that cannot be
/// read, and for every kind an object missing from the store or one that does not re-hash, each
/// refuse.
#[test]
#[ignore = "pending E19-11"]
fn a_missing_registration_or_object_refuses() {
    let mut no_rules = Stream::registered(SNAPSHOT);
    no_rules.records.remove(2);
    let kind = "rule_set";
    assert_eq!(no_rules.refused(), Some(Refusal::Unregistered { kind }));
    let undated = r#"{"environment":"paper","schedule":"conservative_v1"}"#;
    let malformed = Stream::registered(SNAPSHOT).register("fee_config", undated);
    let kind = "fee_config";
    assert_eq!(malformed.refused(), Some(Refusal::Malformed { kind }));
    let stream = Stream::registered(SNAPSHOT);
    let date = Date::parse(TODAY).unwrap();
    let unavailable = configuration(&stream.records, &Unavailable, &pinned(), date).err();
    assert_eq!(unavailable, Some(Refusal::StoreUnavailable));
    let objects = [
        ("fee_config", FEE),
        ("trading_calendar", CALENDAR),
        ("rule_set", RULES),
        ("instrument_snapshot", SNAPSHOT),
        ("model_version", MODEL),
    ];
    for (kind, object) in objects {
        let mut missing = Stream::registered(SNAPSHOT);
        missing.store.remove(&digest(object));
        assert_eq!(missing.refused(), Some(Refusal::ObjectMissing { kind }));
        let mut corrupt = Stream::registered(SNAPSHOT);
        corrupt.store.get_mut(&digest(object)).unwrap().push(b' ');
        assert_eq!(corrupt.refused(), Some(Refusal::ObjectCorrupt { kind }));
    }
}

/// Each refusal has its own stable code (ADR-0001 ES-09); a live test for the mutation gate.
#[test]
fn every_config_refusal_has_its_own_code() {
    let kind = "rule_set";
    let rows = [
        (Refusal::Unimplemented { story: "E19-11" }, "unimplemented"),
        (Refusal::Unregistered { kind }, "unregistered"),
        (Refusal::ObjectMissing { kind }, "object_missing"),
        (Refusal::ObjectCorrupt { kind }, "object_corrupt"),
        (Refusal::StoreUnavailable, "store_unavailable"),
        (Refusal::Malformed { kind }, "malformed"),
        (Refusal::FeeNotYetEffective, "fee_not_yet_effective"),
    ];
    for (refusal, code) in rows {
        assert_eq!(refusal.code(), code, "{refusal:?}");
    }
}
