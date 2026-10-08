//! The effective configuration registrations (E19-11, DEC-505 item 1, DEC-523; the brief's slice
//! D3, part b): the latest by `seq` per kind, the asset-id and model-triple narrowings, the fee
//! date, and the registered instrument snapshot read exactly as DEC-523 pins it. Each object is
//! written here and registered as journal spec §9.2's `ConfigSnapshotRegistered` names it.

use std::collections::BTreeMap;

use mandate_canon::{Digest, Value, to_canonical};
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
const MODEL_RECORD: &str = r#"{"admits_instruments":false,"content_hash":"@H","kind":"model_version","model_id":"quant.ma_crossover","model_version":"1.0.0","params":["fast_periods","slow_periods"]}"#;
const TODAY: &str = "2026-10-07";

fn json(text: &str) -> Value {
    mandate_canon::parse(text.as_bytes()).unwrap()
}

fn digest(object: &str) -> Digest {
    Digest::of(&to_canonical(&json(object)))
}

/// A control stream of registrations and the store holding each registered object.
#[derive(Default)]
struct Stream {
    records: Vec<ControlRecord>,
    store: BTreeMap<Digest, Vec<u8>>,
}

impl Stream {
    /// One registration of every kind the run uses, `snapshot` and the pinned model's.
    fn registered(snapshot: &str) -> Self {
        let stream = Self::default().register("fee_config", FEE);
        let stream = stream.register("trading_calendar", CALENDAR);
        let stream = stream.register("rule_set", RULES);
        let stream = stream.register("instrument_snapshot", snapshot);
        stream.model(MODEL, MODEL_RECORD)
    }

    fn register(self, kind: &str, object: &str) -> Self {
        let hash = digest(object);
        let record = format!(r#"{{"content_hash":"sha256:{hash}","kind":"{kind}"}}"#);
        self.append(object, &record)
    }

    /// `content` registered by `record`, a `MODEL_RECORD` with its own triple.
    fn model(self, content: &str, record: &str) -> Self {
        let record = record.replace("@H", &format!("sha256:{}", digest(content)));
        self.append(content, &record)
    }

    fn append(mut self, object: &str, payload: &str) -> Self {
        let bytes = to_canonical(&json(object));
        self.store.insert(digest(object), bytes);
        let seq = u64::try_from(self.records.len()).unwrap() + 1;
        let (event_type, payload) = ("ConfigSnapshotRegistered".to_owned(), json(payload));
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

fn pinned() -> Pinned {
    Pinned {
        asset_id: SPY.to_owned(),
        symbol: "SPY".to_owned(),
        model_id: "quant.ma_crossover".to_owned(),
        model_version: "1.0.0".to_owned(),
        content_hash: digest(MODEL),
    }
}

/// Each kind's effective registration names its object and carries the stored bytes; the snapshot
/// is read into SPY's id, symbol, `arca`, and its classification instant.
#[test]
#[ignore = "pending E19-11"]
fn every_kind_has_its_registered_object_and_the_snapshot_is_read() {
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
    assert_eq!(config.instrument.instrument_id, SPY);
    assert_eq!(config.instrument.symbol, "SPY");
    assert_eq!(config.instrument.exchange, SnapshotExchange::Arca);
    let classified = UtcNanos::parse_rfc3339("2026-10-05T00:00:00Z").unwrap();
    assert_eq!(config.instrument.etp_classified_at, classified);
}

/// The latest registration by `seq` counts; a later fee schedule not yet effective on the trade
/// date refuses rather than falling back, and counts from its `effective_from` on.
#[test]
#[ignore = "pending E19-11"]
fn the_latest_registration_counts_and_a_future_fee_schedule_refuses() {
    let rules = r#"{"rules":"reviewed_v2"}"#;
    let later = Stream::registered(SNAPSHOT).register("rule_set", rules);
    let config = later.read(TODAY).unwrap();
    assert_eq!(config.rule_set.content_hash, digest(rules), "the later one");
    assert_eq!(config.rule_set.seq, 6, "its seq");
    let fee = FEE.replace("2026-01-01", "2026-10-09");
    let future = Stream::registered(SNAPSHOT).register("fee_config", &fee);
    assert_eq!(future.refused(), Some(Refusal::FeeNotYetEffective));
    let effective = future.read("2026-10-09").map(|c| c.fee_config.content_hash);
    assert_eq!(effective, Ok(digest(&fee)), "effective on its own date");
}

/// A snapshot counts only for the pinned asset id and a `model_version` only for the pinned triple
/// (DEC-505 item 1): later ones for anything else, the pinned hash under another id or version
/// included, do not displace them; a later one for the pinned asset that fails DEC-523 refuses
/// rather than falling back; and without one there is none.
#[test]
#[ignore = "pending E19-11"]
fn only_the_pinned_instrument_and_model_registrations_count() {
    let aapl = SNAPSHOT.replace(SPY, AAPL).replace("SPY", "AAPL");
    let other_model = r#"{"kind":"quant_model_content","model_id":"quant.other"}"#;
    let renamed = MODEL_RECORD.replace("quant.ma_crossover", "quant.other");
    let bumped = MODEL_RECORD.replace("1.0.0", "1.0.1");
    let later = Stream::registered(SNAPSHOT).register("instrument_snapshot", &aapl);
    let later = later
        .model(other_model, MODEL_RECORD)
        .model(MODEL, &renamed);
    let config = later.model(MODEL, &bumped).read(TODAY).unwrap();
    assert_eq!(config.instrument.instrument_id, SPY, "SPY's earlier one");
    assert_eq!(config.model_version.content_hash, digest(MODEL));
    assert_eq!(config.model_version.seq, 5, "the pinned triple's own");
    let nyse = SNAPSHOT.replace("arca", "nyse");
    let later_spy = Stream::registered(SNAPSHOT).register("instrument_snapshot", &nyse);
    let member = "exchange";
    assert_eq!(later_spy.refused(), Some(Refusal::SnapshotValue { member }));
    let kind = "instrument_snapshot";
    let only_aapl = Stream::registered(&aapl).refused();
    assert_eq!(only_aapl, Some(Refusal::Unregistered { kind }));
    let mut no_model = Stream::registered(SNAPSHOT);
    no_model.records.pop();
    let kind = "model_version";
    assert_eq!(no_model.refused(), Some(Refusal::Unregistered { kind }));
}

/// A kind with no registration, an object missing from the store, an object without the member
/// the reader needs, and one that does not re-hash each refuse.
#[test]
#[ignore = "pending E19-11"]
fn a_missing_registration_or_object_refuses() {
    let mut no_rules = Stream::registered(SNAPSHOT);
    no_rules.records.remove(2);
    let kind = "rule_set";
    assert_eq!(no_rules.refused(), Some(Refusal::Unregistered { kind }));
    let mut missing = Stream::registered(SNAPSHOT);
    missing.store.remove(&digest(FEE));
    let kind = "fee_config";
    assert_eq!(missing.refused(), Some(Refusal::ObjectMissing { kind }));
    let undated = r#"{"environment":"paper","schedule":"conservative_v1"}"#;
    let malformed = Stream::registered(SNAPSHOT).register(kind, undated);
    assert_eq!(malformed.refused(), Some(Refusal::Malformed { kind }));
    let mut corrupt = Stream::registered(SNAPSHOT);
    corrupt.store.get_mut(&digest(CALENDAR)).unwrap().push(b' ');
    let kind = "trading_calendar";
    assert_eq!(corrupt.refused(), Some(Refusal::ObjectCorrupt { kind }));
}

/// DEC-523: exactly eight string members, each in its value set, and the symbol the pinned one;
/// the asset id another instrument's is no snapshot of this run's (DEC-505 item 1).
#[test]
#[ignore = "pending E19-11"]
fn the_snapshot_is_exactly_what_dec_523_pins() {
    let source = r#","etp_source":"nasdaq_trader_symbol_directory""#;
    let extra = r#""cusip":"78462F103","symbol""#;
    let value = |member| Refusal::SnapshotValue { member };
    let missing = |member| Refusal::SnapshotMissingMember { member };
    let wrong = Refusal::SnapshotWrongType { member: "exchange" };
    let kind = "instrument_snapshot";
    let rows = [
        (source, "", missing("etp_source")),
        (r#""symbol""#, extra, Refusal::SnapshotExtraMember),
        (r#""arca""#, "1", wrong),
        ("nasdaq_trader", "issuer_website", value("etp_source")),
        (r#""arca""#, r#""nyse""#, value("exchange")),
        ("us_equity", "crypto", value("asset_class")),
        (r#""plain""#, r#""leveraged""#, value("etp")),
        ("whole", "fractional", value("increment")),
        ("05T00:00:00Z", "05", value("etp_classified_at")),
        (r#""SPY""#, r#""SPYG""#, value("symbol")),
        (SPY, AAPL, Refusal::Unregistered { kind }),
    ];
    for (from, to, refusal) in rows {
        let snapshot = SNAPSHOT.replace(from, to);
        assert_ne!(snapshot, SNAPSHOT, "the edit applied for {refusal:?}");
        let refused = Stream::registered(&snapshot).refused();
        assert_eq!(refused, Some(refusal), "{snapshot}");
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
