//! Parquet partitions hold every vendor value exactly, as `Decimal128(38, s)` with the scale the
//! manifest records (ADR-0001 ES-23, DEC-89).

mod common;

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::path::Path;

use arrow_schema::{DataType, TimeUnit};
use common::{Scratch, btc_1hour, btc_trades, day, scenario, shy_iex_trades, spy_sip_1hour};
use mandate_canon::DecStr;
use mandate_marketdata::alpaca;
use mandate_marketdata::dataset::{self, DatasetError, Outcome, Status, Store};
use mandate_marketdata::model::{DatasetId, Records};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

fn recorded(name: &str, dataset: &DatasetId) -> Records {
    let s = scenario(name);
    let mut all = Records::empty(dataset.kind());
    for body in &s.bodies {
        let page = alpaca::parse_page(dataset, day("2026-09-24"), body).unwrap();
        match (&mut all, page.records) {
            (Records::Bars(a), Records::Bars(b)) => a.extend(b),
            (Records::Trades(a), Records::Trades(b)) => a.extend(b),
            _ => panic!("mixed kinds"),
        }
    }
    all
}

fn every_recording() -> Vec<(DatasetId, Records)> {
    [
        ("stock-bars-sip-spy-1hour-2026-09-24", spy_sip_1hour()),
        ("stock-trades-iex-shy-2026-09-24-paged", shy_iex_trades()),
        ("crypto-bars-btcusd-1hour-2026-09-24", btc_1hour()),
        ("crypto-trades-btcusd-2026-09-24-paged", btc_trades()),
    ]
    .into_iter()
    .map(|(name, id)| {
        let records = recorded(name, &id);
        (id, records)
    })
    .collect()
}

#[test]
fn parquet_round_trip_preserves_every_recorded_value_exactly() {
    let scratch = Scratch::new("round-trip");
    for (id, records) in every_recording() {
        assert!(!records.is_empty());
        let bytes = dataset::encode(&id, &records).unwrap();
        let path = scratch
            .path()
            .join(format!("{}.parquet", id.symbol().slug()));
        fs::write(&path, &bytes).unwrap();
        assert_eq!(dataset::read(&path, id.kind()).unwrap(), records, "{id:?}");
    }
}

#[test]
fn encoding_is_deterministic() {
    for (id, records) in every_recording() {
        assert_eq!(
            dataset::encode(&id, &records).unwrap(),
            dataset::encode(&id, &records.clone()).unwrap()
        );
    }
}

#[test]
fn columns_are_decimal128_with_the_scales_the_manifest_records() {
    let scratch = Scratch::new("scales");
    let store = Store::new(scratch.path());
    for (id, records) in every_recording() {
        let outcome = store.put_day(&id, day("2026-09-24"), &records).unwrap();
        assert!(outcome.file.is_some());
        let dir = store.dataset_dir(&id);
        let manifest = fs::read(dir.join(dataset::MANIFEST)).unwrap();
        let manifest = mandate_canon::parse(&manifest).unwrap();
        let scales = manifest.get("scales").and_then(|s| s.as_object()).unwrap();
        let file = File::open(dir.join("2026-09-24.parquet")).unwrap();
        let schema = ParquetRecordBatchReaderBuilder::try_new(file)
            .unwrap()
            .schema()
            .clone();
        assert_eq!(
            scales.len(),
            dataset::decimal_scales(id.kind()).len(),
            "{id:?}"
        );
        for (column, scale) in scales {
            let scale = u8::try_from(scale.as_int().unwrap()).unwrap();
            let field = schema.field_with_name(column.as_str()).unwrap();
            assert_eq!(
                field.data_type(),
                &DataType::Decimal128(38, i8::try_from(scale).unwrap()),
                "{id:?} {}",
                column.as_str()
            );
        }
        let time = schema.field(1);
        assert_eq!(
            time.data_type(),
            &DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into())),
            "{id:?}"
        );
        assert_eq!(schema.field(0).name(), "symbol");
    }
    assert_eq!(dataset::PRICE_SCALE, 9);
    assert_eq!(dataset::SIZE_SCALE, 9);
    assert_eq!(dataset::BAR_SCALE, 18);
}

#[test]
fn a_value_that_does_not_fit_its_column_fails_the_partition() {
    let id = spy_sip_1hour();
    let Records::Bars(mut bars) = recorded("stock-bars-sip-spy-1hour-2026-09-24", &id) else {
        panic!("bars expected")
    };
    bars[3].vwap = DecStr::parse("764.0000000000000000001").unwrap();
    let err = dataset::encode(&id, &Records::Bars(bars.clone())).unwrap_err();
    assert!(
        matches!(
            err,
            DatasetError::Number {
                column: "vwap",
                row: 3,
                ..
            }
        ),
        "{err}"
    );
    bars[3].vwap = DecStr::parse("764.000000000000000001").unwrap();
    bars[5].open = DecStr::parse("1.0000000000000000001").unwrap();
    let err = dataset::encode(&id, &Records::Bars(bars)).unwrap_err();
    assert!(
        matches!(
            err,
            DatasetError::Number {
                column: "open",
                row: 5,
                ..
            }
        ),
        "{err}"
    );

    let id = shy_iex_trades();
    let Records::Trades(mut trades) = recorded("stock-trades-iex-shy-2026-09-24-paged", &id) else {
        panic!("trades expected")
    };
    trades[7].price = DecStr::parse("81.1900000001").unwrap();
    let err = dataset::encode(&id, &Records::Trades(trades)).unwrap_err();
    assert!(
        matches!(
            err,
            DatasetError::Number {
                column: "price",
                row: 7,
                ..
            }
        ),
        "{err}"
    );
}

/// The recorded SPY day, and a revision of it with one bar's open changed.
fn spy_day_and_revision() -> (DatasetId, Records, Records) {
    let id = spy_sip_1hour();
    let original = recorded("stock-bars-sip-spy-1hour-2026-09-24", &id);
    let Records::Bars(mut bars) = original.clone() else {
        panic!("bars expected")
    };
    bars[0].open = DecStr::parse("764.54").unwrap();
    (id, original, Records::Bars(bars))
}

fn dir_contents(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (name, fs::read(&path).unwrap())
        })
        .collect()
}

fn is_conflict(outcome: &Result<Outcome, DatasetError>) -> bool {
    matches!(outcome, Err(DatasetError::Conflict { .. }))
}

#[test]
fn a_day_that_cannot_be_encoded_creates_nothing() {
    let (id, _, revision) = spy_day_and_revision();
    let Records::Bars(mut bars) = revision else {
        panic!("bars expected")
    };
    bars[3].vwap = DecStr::parse("764.0000000000000000001").unwrap();
    let scratch = Scratch::new("unencodable");
    let store = Store::new(scratch.path());
    let outcome = store.put_day(&id, day("2026-09-24"), &Records::Bars(bars));
    assert!(
        matches!(outcome, Err(DatasetError::Number { .. })),
        "{outcome:?}"
    );
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
}

#[test]
fn a_partition_the_manifest_does_not_list_is_never_overwritten_or_adopted() {
    let (id, original, revision) = spy_day_and_revision();
    let scratch = Scratch::new("unlisted-partition");
    let store = Store::new(scratch.path());
    let dir = store.dataset_dir(&id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("2026-09-24.parquet"),
        dataset::encode(&id, &original).unwrap(),
    )
    .unwrap();
    let before = dir_contents(&dir);
    let outcome = store.put_day(&id, day("2026-09-24"), &revision);
    assert!(is_conflict(&outcome), "different records: {outcome:?}");
    assert_eq!(dir_contents(&dir), before);
    let outcome = store.put_day(&id, day("2026-09-24"), &Records::empty(id.kind()));
    assert!(is_conflict(&outcome), "no records: {outcome:?}");
    assert_eq!(dir_contents(&dir), before);
}

#[test]
fn a_listed_partition_that_was_deleted_is_restored_only_with_the_same_records() {
    let (id, original, revision) = spy_day_and_revision();
    let scratch = Scratch::new("deleted-partition");
    let store = Store::new(scratch.path());
    let dir = store.dataset_dir(&id);
    let d = day("2026-09-24");
    store.put_day(&id, d, &original).unwrap();
    let stored = dir_contents(&dir);
    fs::remove_file(dir.join("2026-09-24.parquet")).unwrap();
    let deleted = dir_contents(&dir);
    for records in [revision, Records::empty(id.kind())] {
        let outcome = store.put_day(&id, d, &records);
        assert!(
            is_conflict(&outcome),
            "{} records: {outcome:?}",
            records.len()
        );
        assert_eq!(dir_contents(&dir), deleted, "a conflict writes nothing");
    }
    let outcome = store.put_day(&id, d, &original).unwrap();
    assert_eq!(outcome.status, Status::Written);
    assert_eq!(dir_contents(&dir), stored);
}

#[test]
fn crypto_bar_aggregates_with_ten_fraction_digits_are_stored_exactly() {
    let id = btc_1hour();
    let Records::Bars(mut bars) = recorded("crypto-bars-btcusd-1hour-2026-09-24", &id) else {
        panic!("bars expected")
    };
    bars[0].open = DecStr::parse("82226.7012690355").unwrap();
    bars[0].close = DecStr::parse("82225.9512690355").unwrap();
    bars[0].vwap = DecStr::parse("81519.1593975867").unwrap();
    bars[0].volume = DecStr::parse("0.000024101").unwrap();
    let records = Records::Bars(bars);
    let scratch = Scratch::new("crypto-aggregates");
    let path = scratch.path().join("bars.parquet");
    fs::write(&path, dataset::encode(&id, &records).unwrap()).unwrap();
    assert_eq!(dataset::read(&path, id.kind()).unwrap(), records);
}
