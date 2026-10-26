//! What the composer's tests share: the cell's demo mandate, its registration rows, a stored daily
//! series under a temporary cell root, and the closes object written out by hand.

use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};

use mandate_canon::{DecStr, Digest, Value, parse};
use mandate_journal::StoredEvent;
use mandate_marketdata::dataset::Store;
use mandate_marketdata::model::{AssetClass, Bar, DatasetId, Feed, Kind, Records, Symbol};
use mandate_runtime::RiskClock;
use mandate_runtime_models::RECORDING;
use mandate_time::{Date, ExchangeCalendar, UtcNanos, new_york_midnight};

pub const MANDATE: &str = include_str!("../../../../deploy/cell/demo/web-mandate.json");
pub const QQQ: &str = "eb82ffb0-efd6-4f4e-9307-8fead28e04f6";
pub const MODEL: &str = "quant.ma_crossover";
pub const PINNED_HASH: &str = "cf36ebf8746a95c79443a2a0b5b49762809e082bced8a0a5bfdda7a3dd6c469d";
pub const PARAMS: &str = r#"["fast_periods","slow_periods"]"#;
/// Monday 2026-10-26 at 15:00 New York, inside the session: the last completed one is Friday's.
pub const NOW: &str = "2026-10-26T19:00:00Z";
pub const LAST: (u16, u8, u8) = (2026, 10, 23);
/// Friday 2026-10-23's regular close, 16:00 New York in daylight time.
pub const LAST_CLOSE: &str = "2026-10-23T20:00:00Z";
/// Tuesday 2026-10-27 after the close: Monday's session has completed, and the series ends Friday.
pub const STALE: &str = "2026-10-27T21:00:00Z";

pub fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
}

pub fn tick(text: &str) -> RiskClock {
    RiskClock::from_secs(at(text).secs())
}

pub fn document() -> Value {
    parse(MANDATE.as_bytes()).unwrap()
}

/// The demo mandate with its text changed by `edit`.
pub fn edited(edit: impl Fn(String) -> String) -> Value {
    parse(edit(MANDATE.to_owned()).as_bytes()).unwrap()
}

/// A temporary cell root, removed when dropped.
pub struct Root(PathBuf);

impl Root {
    pub fn new(name: &str) -> Self {
        thread_local!(static NEXT: Cell<u64> = const { Cell::new(0) });
        let n = NEXT.with(|next| {
            next.set(next.get() + 1);
            next.get()
        });
        let path = std::env::temp_dir().join(format!(
            "mandate-runtime-models-{name}-{}-{n}",
            std::process::id()
        ));
        fs::remove_dir_all(&path).ok();
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn bars(&self) -> PathBuf {
        self.0.join("datasets").join(RECORDING)
    }

    pub fn artifacts(&self) -> PathBuf {
        self.0.join("artifacts")
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

/// `cents` as the shortest decimal: the form a stored close and a journaled price take.
pub fn decimal(cents: u32) -> String {
    match (cents / 100, cents % 100) {
        (whole, 0) => whole.to_string(),
        (whole, part) if part % 10 == 0 => format!("{whole}.{}", part / 10),
        (whole, part) => format!("{whole}.{part:02}"),
    }
}

/// The `count` regular sessions that end on `last`, oldest first, from the calendar's trading days.
pub fn sessions(count: usize, last: Date) -> Vec<Date> {
    let calendar = ExchangeCalendar::us_equities().unwrap();
    let mut days = Vec::new();
    let mut day = Date::new(2026, 6, 1).unwrap();
    while day <= last {
        if calendar.is_trading_day(day).unwrap() {
            days.push(day);
        }
        day = day.next().unwrap();
    }
    days.split_off(days.len() - count)
}

pub fn last() -> Date {
    Date::new(LAST.0, LAST.1, LAST.2).unwrap()
}

/// Stores `symbol`'s daily bars under `bars`, one a session in `series`, skipping `skip`.
pub fn store_series(bars: &Path, symbol: &str, series: &[(Date, u32)], skip: Option<usize>) {
    let id = DatasetId::new(
        AssetClass::UsEquity,
        Feed::Iex,
        Kind::Bars("1Day".parse().unwrap()),
        Symbol::parse(symbol).unwrap(),
    )
    .unwrap();
    let store = Store::new(bars);
    for (n, (day, cents)) in series.iter().enumerate() {
        if Some(n) == skip {
            continue;
        }
        let price = DecStr::parse(&decimal(*cents)).unwrap();
        let bar = Bar {
            start: new_york_midnight(*day).unwrap(),
            open: price.clone(),
            high: price.clone(),
            low: price.clone(),
            close: price.clone(),
            volume: DecStr::parse("50000000").unwrap(),
            vwap: price,
            trade_count: 400_000,
        };
        store.put_day(&id, *day, &Records::Bars(vec![bar])).unwrap();
    }
}

/// The closes object of `series` for QQQ, written out by hand in canonical JSON (members sorted,
/// no whitespace).
pub fn closes_json(series: &[(Date, u32)]) -> Vec<u8> {
    let pairs: Vec<String> = series
        .iter()
        .map(|(day, cents)| format!(r#"["{day}","{}"]"#, decimal(*cents)))
        .collect();
    format!(
        r#"{{"closes":[{}],"instrument_id":"{QQQ}"}}"#,
        pairs.join(",")
    )
    .into_bytes()
}

/// A control-stream row as the workspace services commit it.
pub fn control_row(seq: u64, event_type: &str, environment: &str, payload: &str) -> StoredEvent {
    let event_id = format!("01J8Z3MC{seq:018}");
    let body = format!(
        r#"{{"envelope_version":1,"environment":"{environment}","event_id":"{event_id}","stream_id":"ctl:ws1","event_type":"{event_type}","schema_version":1,"event_time":"2026-10-16T13:00:00.000000000Z","clock_source":"system","causation_id":null,"correlation_id":null,"actor":{{"kind":"user","id":"user-owner","version":"1","build":null}},"config_refs":{{}},"payload":{payload},"artifact_refs":[],"pii_refs":[]}}"#
    );
    StoredEvent {
        stream_id: "ctl:ws1".into(),
        seq,
        event_id,
        event_type: event_type.into(),
        schema_version: 1,
        environment: environment.into(),
        recorded_at: "2026-10-16T13:00:00.000000000Z".into(),
        prev_hash: Digest::of(b"prev"),
        hash: Digest::of(body.as_bytes()),
        body: body.into_bytes(),
    }
}

/// The `model register` row of `MODEL` 1.0.0 with `hash` and `params`.
pub fn registration(seq: u64, environment: &str, hash: &str, params: &str) -> StoredEvent {
    registration_of(seq, environment, ("1.0.0", hash), params)
}

/// The `model register` row of `MODEL` at `version` with `hash` and `params`.
pub fn registration_of(
    seq: u64,
    environment: &str,
    (version, hash): (&str, &str),
    params: &str,
) -> StoredEvent {
    let payload = format!(
        r#"{{"admits_instruments":false,"content_hash":"sha256:{hash}","kind":"model_version","model_id":"{MODEL}","model_version":"{version}","params":{params}}}"#
    );
    control_row(seq, "ConfigSnapshotRegistered", environment, &payload)
}

/// A registered rule set, which is not a model's registration.
pub fn rule_set(seq: u64) -> StoredEvent {
    let payload = format!(
        r#"{{"content_hash":"sha256:{}","kind":"rule_set"}}"#,
        "a".repeat(64)
    );
    control_row(seq, "ConfigSnapshotRegistered", "paper", &payload)
}

pub fn registered() -> Vec<StoredEvent> {
    vec![rule_set(1), registration(2, "paper", PINNED_HASH, PARAMS)]
}

/// A rising QQQ series of `count` sessions to Friday, from 140 by 0.10 a session.
pub fn rising(count: usize) -> Vec<(Date, u32)> {
    sessions(count, last())
        .into_iter()
        .zip((0u32..).map(|n| 14_000 + 10 * n))
        .collect()
}
