//! The effective configuration registrations (E19-11, DEC-505 item 1; the brief's slice D3, part
//! b): the latest by `seq` per kind, the asset-id and model-triple narrowings, the fee date, and
//! each registered object re-hashed; then the effective snapshot read exactly as DEC-523 pins it
//! (part c), a later one that fails refused rather than passed over. Each object is written here
//! and registered in journal spec §9.2's closed `ConfigSnapshotRegistered` payload, among records
//! of other types and kinds.

use std::collections::BTreeMap;

use mandate_canon::{Digest, Key, Value, to_canonical};
use mandate_journal::{ArtifactError, ArtifactRef, ArtifactSource};
use mandate_shell::control::{
    ConfigRefusal as Refusal, Configuration, ControlRecord, Pinned, SnapshotExchange, configuration,
};
use mandate_time::{Date, UtcNanos};

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

/// `object`'s reference as a payload names it.
fn hash(object: &str) -> String {
    format!("sha256:{}", digest(object))
}

/// A control stream, its store of objects, and the digest, if any, whose read finds the store down.
#[derive(Default)]
struct Stream {
    records: Vec<ControlRecord>,
    store: BTreeMap<Digest, Vec<u8>>,
    down: Option<Digest>,
}

impl ArtifactSource for Stream {
    fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
        let wanted = reference.digest();
        if self.down == Some(wanted) {
            return Err(ArtifactError::Unavailable);
        }
        let bytes = self.store.get(&wanted).cloned();
        bytes.ok_or(ArtifactError::Missing)
    }
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
        let stream = stream.stored(MODEL, MODEL_RECORD);
        let stream = stream.register("policy_set", POLICY);
        let stream = stream.then("AgentDeployed", DEPLOYED);
        stream.then("MandateConfirmed", CONFIRMED)
    }

    fn register(self, kind: &str, object: &str) -> Self {
        self.stored(object, &RECORD.replace("@K", kind))
    }

    /// `content` stored and registered by `record`, with `@H` its content hash.
    fn stored(mut self, content: &str, record: &str) -> Self {
        let bytes = to_canonical(&json(content));
        self.store.insert(digest(content), bytes);
        let record = record.replace("@H", &hash(content));
        self.then("ConfigSnapshotRegistered", &record)
    }

    fn then(mut self, event_type: &str, payload: &str) -> Self {
        let seq = u64::try_from(self.records.len()).unwrap() + 1;
        self.records.push(ControlRecord {
            seq,
            event_type: event_type.to_owned(),
            payload: json(payload),
        });
        self
    }

    fn read(&self, trade_date: &str) -> Result<Configuration, Refusal> {
        let trade_date = Date::parse(trade_date).unwrap();
        configuration(&self.records, self, &pinned(), trade_date)
    }

    /// Why the stream is refused on [`TODAY`], if it is.
    fn refused(&self) -> Option<Refusal> {
        self.read(TODAY).err()
    }
}

fn pinned() -> Pinned {
    Pinned {
        asset_id: SPY.to_owned(),
        symbol: "SPY".to_owned(),
        model_id: "quant.ma_crossover".to_owned(),
        model_version: "1.0.0".to_owned(),
        content_hash: digest(MODEL),
    }
}

/// Each kind's effective registration is its own record, naming its object and carrying the
/// stored bytes; records of other types and kinds change nothing.
#[test]
#[ignore = "pending E19-11"]
fn every_kind_has_its_registered_object() {
    let config = Stream::registered(SNAPSHOT).read(TODAY).unwrap();
    let kinds = [
        (config.fee_config, FEE, 1),
        (config.trading_calendar, CALENDAR, 2),
        (config.rule_set, RULES, 3),
        (config.instrument_snapshot, SNAPSHOT, 4),
        (config.model_version, MODEL, 5),
    ];
    for (registered, object, seq) in kinds {
        let read = (registered.content_hash, registered.bytes, registered.seq);
        let expected = (digest(object), to_canonical(&json(object)), seq);
        assert_eq!(read, expected, "{object}");
    }
}

/// The latest registration by `seq` counts, in whatever order the slice lists them, for a plain kind
/// and both narrowed ones; a later fee schedule not yet effective on the trade date refuses rather
/// than falling back, and counts from its `effective_from` on.
#[test]
#[ignore = "pending E19-11"]
fn the_latest_registration_counts_and_a_future_fee_schedule_refuses() {
    let rules = r#"{"rules":"reviewed_v2"}"#;
    let newer = SNAPSHOT.replace("2026-10-05", "2026-10-06");
    let later = Stream::registered(SNAPSHOT).register("rule_set", rules);
    let later = later.register("instrument_snapshot", &newer);
    let mut later = later.stored(MODEL, MODEL_RECORD);
    let hashes = [rules, newer.as_str(), MODEL].map(digest);
    let expected = [(hashes[0], 9), (hashes[1], 10), (hashes[2], 11)];
    for swapped in [false, true] {
        let c = later.read(TODAY).unwrap();
        let kinds = [c.rule_set, c.instrument_snapshot, c.model_version];
        let latest = kinds.map(|registered| (registered.content_hash, registered.seq));
        assert_eq!(latest, expected, "swapped: {swapped}");
        for (earlier, latest) in [(3, 9), (4, 10), (5, 11)] {
            let at = |seq| later.records.iter().position(|r| r.seq == seq).unwrap();
            let (earlier, latest) = (at(earlier), at(latest));
            later.records.swap(earlier, latest);
        }
    }
    let fee = FEE.replace("2026-01-01", "2026-10-09");
    let future = Stream::registered(SNAPSHOT).register("fee_config", &fee);
    assert_eq!(future.refused(), Some(Refusal::FeeNotYetEffective));
    let effective = future.read("2026-10-09").map(|c| c.fee_config.content_hash);
    assert_eq!(effective, Ok(digest(&fee)), "effective on its own date");
}

/// A snapshot counts only for the pinned asset id and a `model_version` only for the pinned triple
/// (DEC-505 item 1): later ones for anything else, the pinned hash under another id or version
/// included, do not displace them, and without one there is none. Nor does a record of another type
/// carrying a registration's payload, for a plain kind or a narrowed one.
#[test]
#[ignore = "pending E19-11"]
fn only_the_pinned_instrument_and_model_registrations_count() {
    let aapl = SNAPSHOT.replace(SPY, AAPL).replace("SPY", "AAPL");
    let other_model = r#"{"kind":"quant_model_content","model_id":"quant.other"}"#;
    let renamed = MODEL_RECORD.replace("quant.ma_crossover", "quant.other");
    let bumped = MODEL_RECORD.replace("1.0.0", "1.0.1");
    let lookalikes = [
        RECORD.replace("@K", "rule_set").replace("@H", &hash(FEE)),
        RECORD
            .replace("@K", "instrument_snapshot")
            .replace("@H", &hash(SNAPSHOT)),
        MODEL_RECORD.replace("@H", &hash(MODEL)),
    ];
    let later = Stream::registered(SNAPSHOT).register("instrument_snapshot", &aapl);
    let later = later.stored(other_model, MODEL_RECORD);
    let later = later.stored(MODEL, &renamed);
    let mut later = later.stored(MODEL, &bumped);
    for lookalike in &lookalikes {
        later = later.then("MandateConfirmed", lookalike);
    }
    let config = later.read(TODAY).unwrap();
    let snapshot = &config.instrument_snapshot;
    let snapshot = (snapshot.content_hash, snapshot.seq);
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

/// A kind with no registration, a fee object without `effective_from`, and for every kind an object
/// missing from the store, one that does not re-hash, or one the store is down for, each refuse.
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
        let mut down = Stream::registered(SNAPSHOT);
        down.down = Some(digest(object));
        assert_eq!(down.refused(), Some(Refusal::StoreUnavailable), "{kind}");
    }
}

/// The DEC-523 members, in canonical order.
const MEMBERS: [&str; 8] = [
    "asset_class",
    "etp",
    "etp_classified_at",
    "etp_source",
    "exchange",
    "increment",
    "instrument_id",
    "symbol",
];

/// `SNAPSHOT` with `member` set to `value`, or removed when there is none.
fn snapshot_with(member: &str, value: Option<Value>) -> String {
    let Value::Object(mut object) = json(SNAPSHOT) else {
        panic!("the snapshot is an object");
    };
    let key = Key::new(member).unwrap();
    if let Some(value) = value {
        object.insert(key, value);
    } else {
        object.remove(&key);
    }
    String::from_utf8(to_canonical(&Value::Object(object))).unwrap()
}

/// A valid SPY snapshot at seq 4, then `snapshot` registered at seq 9.
fn later(snapshot: &str) -> Stream {
    Stream::registered(SNAPSHOT).register("instrument_snapshot", snapshot)
}

/// The effective snapshot is read into SPY's id, symbol, exchange and classification instant, for
/// both exchanges DEC-523 admits.
#[test]
#[ignore = "pending E19-11"]
fn the_snapshot_is_read_as_dec_523_pins_it() {
    let read = Stream::registered(SNAPSHOT).read(TODAY).unwrap().instrument;
    assert_eq!(
        (read.instrument_id.as_str(), read.symbol.as_str()),
        (SPY, "SPY")
    );
    assert_eq!(read.exchange, SnapshotExchange::Arca);
    let classified = UtcNanos::parse_rfc3339("2026-10-05T00:00:00Z").unwrap();
    assert_eq!(read.etp_classified_at, classified);
    let nasdaq = later(&SNAPSHOT.replace("arca", "nasdaq"))
        .read(TODAY)
        .unwrap();
    assert_eq!(nasdaq.instrument.exchange, SnapshotExchange::Nasdaq);
}

/// No silent fallback (DEC-505 item 1, rule 3): after a valid SPY snapshot, a later one that lacks
/// any DEC-523 member, holds one as a number or `null`, has an extra member, is no object, or whose
/// object is missing, corrupt or unreadable refuses rather than leaving the earlier one in force.
#[test]
#[ignore = "pending E19-11"]
fn a_later_snapshot_that_fails_dec_523_refuses_rather_than_falling_back() {
    for member in MEMBERS {
        let rows = [
            (None, Refusal::SnapshotMissingMember { member }),
            (Some(json("7")), Refusal::SnapshotWrongType { member }),
            (Some(Value::Null), Refusal::SnapshotWrongType { member }),
        ];
        for (value, refusal) in rows {
            let snapshot = snapshot_with(member, value);
            assert_eq!(later(&snapshot).refused(), Some(refusal), "{snapshot}");
        }
    }
    let extra = SNAPSHOT.replace(r#""symbol""#, r#""cusip":"78462F103","symbol""#);
    assert_eq!(later(&extra).refused(), Some(Refusal::SnapshotExtraMember));
    let kind = "instrument_snapshot";
    let not_object = later(r#"["SPY"]"#).refused();
    assert_eq!(not_object, Some(Refusal::Malformed { kind }));
    let newer = SNAPSHOT.replace("2026-10-05", "2026-10-06");
    let mut missing = later(&newer);
    missing.store.remove(&digest(&newer));
    assert_eq!(missing.refused(), Some(Refusal::ObjectMissing { kind }));
    let mut corrupt = later(&newer);
    corrupt.store.get_mut(&digest(&newer)).unwrap().push(b' ');
    assert_eq!(corrupt.refused(), Some(Refusal::ObjectCorrupt { kind }));
    let mut down = later(&newer);
    down.down = Some(digest(&newer));
    assert_eq!(down.refused(), Some(Refusal::StoreUnavailable));
}

/// DEC-523's value sets, each on a later SPY snapshot: `etp_source` is closed (another source, or a
/// prefix of its one value), `exchange` is `arca` or `nasdaq`, the one-value members, an instant
/// with an upper-case `T`, and the pinned symbol.
#[test]
#[ignore = "pending E19-11"]
fn a_value_outside_dec_523s_sets_refuses() {
    let value = |member| Refusal::SnapshotValue { member };
    let rows = [
        ("nasdaq_trader", "issuer_website", value("etp_source")),
        ("_symbol_directory", "_symbol", value("etp_source")),
        (r#""arca""#, r#""nyse""#, value("exchange")),
        ("us_equity", "crypto", value("asset_class")),
        (r#""plain""#, r#""leveraged""#, value("etp")),
        ("whole", "fractional", value("increment")),
        ("05T00:00:00Z", "05", value("etp_classified_at")),
        ("05T00", "05t00", value("etp_classified_at")),
        (r#""SPY""#, r#""SPYG""#, value("symbol")),
    ];
    for (from, to, refusal) in rows {
        let snapshot = SNAPSHOT.replace(from, to);
        assert_ne!(snapshot, SNAPSHOT, "the edit applied for {refusal:?}");
        assert_eq!(later(&snapshot).refused(), Some(refusal), "{snapshot}");
    }
}

/// A fee schedule effective tomorrow refuses today and counts tomorrow; an `effective_from` that is
/// no date refuses as malformed.
#[test]
#[ignore = "pending E19-11"]
fn the_fee_date_is_read_to_the_day() {
    let fee = FEE.replace("2026-01-01", "2026-10-08");
    let tomorrow = Stream::registered(SNAPSHOT).register("fee_config", &fee);
    assert_eq!(tomorrow.refused(), Some(Refusal::FeeNotYetEffective));
    let counted = tomorrow.read("2026-10-08").map(|c| c.fee_config.seq);
    assert_eq!(counted, Ok(9), "from its own date");
    let kind = "fee_config";
    for date in ["2026-13-01", "2026-10-7", "soon"] {
        let fee = FEE.replace("2026-01-01", date);
        let malformed = Stream::registered(SNAPSHOT).register(kind, &fee);
        assert_eq!(
            malformed.refused(),
            Some(Refusal::Malformed { kind }),
            "{date}"
        );
    }
}

/// Each refusal has its own stable code (ADR-0001 ES-09); a live test for the mutation gate.
#[test]
fn every_config_refusal_has_its_own_code() {
    let kind = "rule_set";
    let member = "exchange";
    let rows = [
        (Refusal::Unimplemented { story: "E19-11" }, "unimplemented"),
        (Refusal::Unregistered { kind }, "unregistered"),
        (Refusal::ObjectMissing { kind }, "object_missing"),
        (Refusal::ObjectCorrupt { kind }, "object_corrupt"),
        (Refusal::StoreUnavailable, "store_unavailable"),
        (Refusal::Malformed { kind }, "malformed"),
        (Refusal::FeeNotYetEffective, "fee_not_yet_effective"),
        (
            Refusal::SnapshotMissingMember { member },
            "snapshot_missing_member",
        ),
        (Refusal::SnapshotExtraMember, "snapshot_extra_member"),
        (Refusal::SnapshotWrongType { member }, "snapshot_wrong_type"),
        (Refusal::SnapshotValue { member }, "snapshot_value"),
    ];
    for (refusal, code) in rows {
        assert_eq!(refusal.code(), code, "{refusal:?}");
    }
}
