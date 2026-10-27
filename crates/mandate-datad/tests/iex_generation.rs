//! DEC-2283 items 4 to 6, data-plane spec §3.8 "Generations", "When", "Batched" and "Closes
//! object", DP-15 and DP-16: the writer stores each instrument's daily generation, its 30 sessions
//! with the span's corporate actions, and each minute generation, asking one request for every
//! instrument due. An instrument's dataset appears whole or not at all, is never rewritten, waits
//! `CONFIRM` before an empty newest day is stored, and a bar on a day the span does not hold refuses
//! it. The oracles are the bars a fake source serves and the readers of `mandate-bars`.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use mandate_bars::generations::{
    GENERATIONS_ROOT, daily_dir_at, daily_generation, minute_dir_at, minute_generation,
};
use mandate_bars::{closes_object, daily_dir, minute_dir, trailing_minutes, trusted_daily_closes};
use mandate_canon::DecStr;
use mandate_datad::iex::generation::{BarsSource, Writer};
use mandate_datad::iex::instruments::Instrument;
use mandate_datad::iex::schedule::{CONFIRM_EVERY_S, CONFIRM_S, span};
use mandate_journal::ArtifactRef;
use mandate_marketdata::actions::read_actions;
use mandate_marketdata::batch::BarsBatch;
use mandate_marketdata::client::FetchError;
use mandate_marketdata::model::{Bar, CorporateActions, DayRange, Split, Symbol, split_ratio};
use mandate_time::{Date, ExchangeCalendar, UtcNanos, new_york_midnight};

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
}

fn day(text: &str) -> Date {
    Date::parse(text).unwrap()
}

fn symbol(text: &str) -> Symbol {
    Symbol::parse(text).unwrap()
}

fn calendar() -> ExchangeCalendar {
    ExchangeCalendar::us_equities().unwrap()
}

/// Monday 2026-10-26, settled at 00:15Z the next day.
const SESSION: &str = "2026-10-26";
const SETTLED: &str = "2026-10-27T00:20:00Z";

fn bar(start: UtcNanos, close: &str, volume: &str) -> Bar {
    let price = DecStr::parse(close).unwrap();
    Bar {
        start,
        open: price.clone(),
        high: price.clone(),
        low: price.clone(),
        close: price.clone(),
        volume: DecStr::parse(volume).unwrap(),
        vwap: price,
        trade_count: 10,
    }
}

/// What the fake source asked for, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Asked {
    Bars(Vec<String>, String, UtcNanos, UtcNanos),
    Actions(Vec<String>, Date, Date),
}

/// A source serving each symbol's bars that fall in the asked span, and each symbol's actions,
/// recording every request; `fail` makes every request a server error.
#[derive(Default)]
struct Fake {
    bars: BTreeMap<String, Vec<Bar>>,
    actions: BTreeMap<String, CorporateActions>,
    asked: RefCell<Vec<Asked>>,
    fail: bool,
}

impl Fake {
    fn names(symbols: &BTreeSet<Symbol>) -> Vec<String> {
        symbols.iter().map(|s| s.as_str().to_owned()).collect()
    }
}

impl BarsSource for Fake {
    fn bars(&self, batch: &BarsBatch) -> Result<BTreeMap<Symbol, Vec<Bar>>, FetchError> {
        self.asked.borrow_mut().push(Asked::Bars(
            Self::names(batch.symbols()),
            batch.timeframe().to_string(),
            batch.start(),
            batch.end(),
        ));
        if self.fail {
            return Err(FetchError::Status { status: 503 });
        }
        Ok(batch
            .symbols()
            .iter()
            .map(|s| {
                let served = self
                    .bars
                    .get(s.as_str())
                    .into_iter()
                    .flatten()
                    .filter(|b| b.start >= batch.start() && b.start < batch.end())
                    .cloned()
                    .collect();
                (s.clone(), served)
            })
            .collect())
    }

    fn actions(
        &self,
        symbols: &BTreeSet<Symbol>,
        range: DayRange,
    ) -> Result<BTreeMap<Symbol, CorporateActions>, FetchError> {
        self.asked.borrow_mut().push(Asked::Actions(
            Self::names(symbols),
            range.first(),
            range.last(),
        ));
        if self.fail {
            return Err(FetchError::Status { status: 503 });
        }
        Ok(symbols
            .iter()
            .map(|s| {
                let actions = self
                    .actions
                    .get(s.as_str())
                    .cloned()
                    .unwrap_or_else(|| CorporateActions::none(s.clone()));
                (s.clone(), actions)
            })
            .collect())
    }
}

/// `symbol`'s daily bars for each of `days`, session `n` closing at `base + n`.
fn daily_bars(days: &[Date], base: u32) -> Vec<Bar> {
    days.iter()
        .enumerate()
        .map(|(n, d)| {
            bar(
                new_york_midnight(*d).unwrap(),
                &(base + u32::try_from(n).unwrap()).to_string(),
                "1000000",
            )
        })
        .collect()
}

fn instrument(asset: &str, sym: &str) -> Instrument {
    Instrument {
        asset_id: asset.to_owned(),
        symbol: symbol(sym),
    }
}

struct Cell(PathBuf);

impl Cell {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("mandate-datad-iex-{name}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn root(&self) -> PathBuf {
        self.0.join("datasets").join(GENERATIONS_ROOT)
    }

    fn artifacts(&self) -> PathBuf {
        self.0.join("artifacts")
    }

    fn writer(&self) -> Writer {
        Writer::new(self.root(), self.artifacts()).unwrap()
    }
}

impl Drop for Cell {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

/// Every path under `dir`, relative to it.
fn tree(dir: &Path) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut stack = vec![dir.to_owned()];
    while let Some(next) = stack.pop() {
        for entry in fs::read_dir(&next).into_iter().flatten().flatten() {
            let path = entry.path();
            found.insert(path.strip_prefix(dir).unwrap().display().to_string());
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
    found
}

fn closes_of(root: &Path, sym: &str, now: UtcNanos) -> Vec<String> {
    trusted_daily_closes(&daily_dir_at(root, sym, now).unwrap(), sym, now)
        .unwrap()
        .iter()
        .map(|(_, close)| close.to_string())
        .collect()
}

#[test]
fn a_daily_generation_holds_each_instrument_s_thirty_sessions_and_their_actions() {
    let cell = Cell::new("daily");
    let days = span(&calendar(), day(SESSION)).unwrap();
    let split = Split {
        id: "s1".to_owned(),
        ex_date: day("2026-08-03"),
        ratio: split_ratio(2, 1).unwrap(),
    };
    let mut spy_actions = CorporateActions::none(symbol("SPY"));
    spy_actions.splits.push(split);
    let fake = Fake {
        bars: BTreeMap::from([
            ("SPY".to_owned(), daily_bars(&days, 600)),
            ("QQQ".to_owned(), daily_bars(&days, 500)),
        ]),
        actions: BTreeMap::from([("SPY".to_owned(), spy_actions)]),
        ..Fake::default()
    };
    let mut writer = cell.writer();
    let instruments = [instrument("b", "SPY"), instrument("a", "QQQ")];
    let done = writer
        .daily(&fake, &calendar(), day(SESSION), &instruments, at(SETTLED))
        .unwrap();
    assert_eq!(done.written, vec![symbol("QQQ"), symbol("SPY")]);
    assert!(done.waiting.is_empty() && done.refused.is_empty());
    assert_eq!(
        *fake.asked.borrow(),
        vec![
            Asked::Bars(
                vec!["QQQ".to_owned(), "SPY".to_owned()],
                "1Day".to_owned(),
                new_york_midnight(days[0]).unwrap(),
                new_york_midnight(day(SESSION).next().unwrap()).unwrap(),
            ),
            Asked::Actions(
                vec!["QQQ".to_owned(), "SPY".to_owned()],
                days[0],
                day(SESSION)
            ),
        ],
        "one bars request and one actions request for every instrument"
    );
    let now = at(SETTLED);
    let expected: Vec<String> = (600..630).map(|c: u32| c.to_string()).collect();
    assert_eq!(closes_of(&cell.root(), "SPY", now), expected);
    let generation = daily_generation(&cell.root(), day(SESSION));
    let recorded = read_actions(&daily_dir(&generation, "SPY").unwrap(), &symbol("SPY"))
        .unwrap()
        .unwrap();
    assert_eq!(
        recorded.range,
        DayRange::new(days[0], day(SESSION)).unwrap()
    );
    assert!(
        recorded.actions.splits.is_empty(),
        "a split before the span is not the span's"
    );
    assert!(
        !tree(&cell.root()).iter().any(|p| p.contains("staging")),
        "{:?}",
        tree(&cell.root())
    );

    fake.asked.borrow_mut().clear();
    let again = writer
        .daily(&fake, &calendar(), day(SESSION), &instruments, now)
        .unwrap();
    assert!(again.written.is_empty());
    assert!(
        fake.asked.borrow().is_empty(),
        "a written instrument is never asked again"
    );
    let mut fresh = cell.writer();
    fresh
        .daily(&fake, &calendar(), day(SESSION), &instruments, now)
        .unwrap();
    assert!(fake.asked.borrow().is_empty(), "nor after a restart");
}

#[test]
fn a_split_inside_the_span_is_stored_and_refused_by_the_trust_check() {
    let cell = Cell::new("split");
    let days = span(&calendar(), day(SESSION)).unwrap();
    let mut actions = CorporateActions::none(symbol("SPY"));
    actions.splits.push(Split {
        id: "s2".to_owned(),
        ex_date: days[20],
        ratio: split_ratio(2, 1).unwrap(),
    });
    let fake = Fake {
        bars: BTreeMap::from([("SPY".to_owned(), daily_bars(&days, 600))]),
        actions: BTreeMap::from([("SPY".to_owned(), actions)]),
        ..Fake::default()
    };
    let mut writer = cell.writer();
    writer
        .daily(
            &fake,
            &calendar(),
            day(SESSION),
            &[instrument("b", "SPY")],
            at(SETTLED),
        )
        .unwrap();
    let now = at(SETTLED);
    let dir = daily_dir_at(&cell.root(), "SPY", now).unwrap();
    assert!(dir.join("manifest.json").is_file());
    assert!(trusted_daily_closes(&dir, "SPY", now).is_err());
}

#[test]
fn an_empty_newest_day_waits_confirm_asked_again_every_five_minutes_alone() {
    let cell = Cell::new("confirm");
    let days = span(&calendar(), day(SESSION)).unwrap();
    let mut thin = daily_bars(&days, 50);
    thin.pop();
    let fake = Fake {
        bars: BTreeMap::from([
            ("SPY".to_owned(), daily_bars(&days, 600)),
            ("THIN".to_owned(), thin),
        ]),
        ..Fake::default()
    };
    let mut writer = cell.writer();
    let instruments = [instrument("b", "SPY"), instrument("t", "THIN")];
    let start = at(SETTLED).secs();
    let tick = |offset: i64| UtcNanos::from_parts(start + offset, 0).unwrap();
    let first = writer
        .daily(&fake, &calendar(), day(SESSION), &instruments, tick(0))
        .unwrap();
    assert_eq!(first.written, vec![symbol("SPY")]);
    assert_eq!(first.waiting, vec![symbol("THIN")]);
    let dir = daily_dir(&daily_generation(&cell.root(), day(SESSION)), "THIN").unwrap();
    assert!(!dir.exists(), "nothing of THIN is stored while it waits");

    fake.asked.borrow_mut().clear();
    writer
        .daily(
            &fake,
            &calendar(),
            day(SESSION),
            &instruments,
            tick(CONFIRM_EVERY_S - 1),
        )
        .unwrap();
    assert!(
        fake.asked.borrow().is_empty(),
        "not asked again within five minutes"
    );
    let again = writer
        .daily(
            &fake,
            &calendar(),
            day(SESSION),
            &instruments,
            tick(CONFIRM_EVERY_S),
        )
        .unwrap();
    assert_eq!(again.waiting, vec![symbol("THIN")]);
    assert!(
        fake.asked
            .borrow()
            .iter()
            .all(|a| matches!(a, Asked::Bars(s, ..) | Asked::Actions(s, ..) if *s == vec!["THIN".to_owned()])),
        "only the waiting instrument is asked: {:?}",
        fake.asked.borrow()
    );
    let last = writer
        .daily(
            &fake,
            &calendar(),
            day(SESSION),
            &instruments,
            tick(CONFIRM_S),
        )
        .unwrap();
    assert_eq!(last.written, vec![symbol("THIN")]);
    assert!(dir.join("manifest.json").is_file());
    assert!(
        trusted_daily_closes(&dir, "THIN", tick(CONFIRM_S)).is_err(),
        "the empty day is stored as fetched, and refused"
    );
}

#[test]
fn a_bar_on_a_day_the_span_does_not_hold_refuses_that_instrument_alone() {
    let cell = Cell::new("off-span");
    let days = span(&calendar(), day(SESSION)).unwrap();
    let mut odd = daily_bars(&days, 600);
    odd.push(bar(new_york_midnight(day("2026-10-25")).unwrap(), "1", "1"));
    odd.sort_by_key(|b| b.start);
    let fake = Fake {
        bars: BTreeMap::from([
            ("ODD".to_owned(), odd),
            ("QQQ".to_owned(), daily_bars(&days, 500)),
        ]),
        ..Fake::default()
    };
    let done = cell
        .writer()
        .daily(
            &fake,
            &calendar(),
            day(SESSION),
            &[instrument("o", "ODD"), instrument("a", "QQQ")],
            at(SETTLED),
        )
        .unwrap();
    assert_eq!(done.written, vec![symbol("QQQ")]);
    assert_eq!(done.refused, vec![symbol("ODD")]);
    assert!(
        !daily_dir(&daily_generation(&cell.root(), day(SESSION)), "ODD")
            .unwrap()
            .exists()
    );
}

#[test]
fn a_failed_request_stores_nothing() {
    let cell = Cell::new("failed");
    let fake = Fake {
        fail: true,
        ..Fake::default()
    };
    let mut writer = cell.writer();
    let failed = writer.daily(
        &fake,
        &calendar(),
        day(SESSION),
        &[instrument("a", "QQQ")],
        at(SETTLED),
    );
    assert_eq!(failed.unwrap_err().code(), "fetch");
    let minute = writer.minute(&fake, at("2026-10-27T14:30:00Z"), &[instrument("a", "QQQ")]);
    assert_eq!(minute.unwrap_err().code(), "fetch");
    assert!(
        tree(&cell.root()).iter().all(|p| !p.contains("bars-")),
        "{:?}",
        tree(&cell.root())
    );
}

#[test]
fn a_minute_generation_holds_the_trailing_window_s_complete_bars() {
    let cell = Cell::new("minute");
    let minute = at("2026-10-27T14:30:00Z");
    let bars: Vec<Bar> = (0..12)
        .map(|n| {
            bar(
                UtcNanos::from_parts(at("2026-10-27T14:20:00Z").secs() + 60 * n, 0).unwrap(),
                "110",
                &(100 + n).to_string(),
            )
        })
        .collect();
    let fake = Fake {
        bars: BTreeMap::from([("QQQ".to_owned(), bars)]),
        ..Fake::default()
    };
    let mut writer = cell.writer();
    let written = writer
        .minute(
            &fake,
            minute,
            &[instrument("a", "QQQ"), instrument("q", "QUIET")],
        )
        .unwrap();
    assert_eq!(written, vec![symbol("QQQ"), symbol("QUIET")]);
    assert_eq!(
        *fake.asked.borrow(),
        vec![Asked::Bars(
            vec!["QQQ".to_owned(), "QUIET".to_owned()],
            "1Min".to_owned(),
            at("2026-10-27T14:25:00Z"),
            minute
        )]
    );
    let now = at("2026-10-27T14:30:20Z");
    let window = trailing_minutes(
        &minute_dir_at(&cell.root(), "QQQ", now).unwrap(),
        "QQQ",
        now,
    )
    .unwrap();
    let volumes: Vec<String> = window.iter().map(|m| m.volume.to_string()).collect();
    assert_eq!(
        volumes,
        ["106", "107", "108", "109"],
        "the reader keeps the minutes from 14:25:20, so the 14:25 bar is not in its window"
    );
    let quiet = minute_dir(&minute_generation(&cell.root(), minute).unwrap(), "QUIET").unwrap();
    assert!(
        quiet.join("manifest.json").is_file(),
        "a quiet window is stored, empty"
    );
    assert!(
        trailing_minutes(&quiet, "QUIET", now).unwrap().is_empty(),
        "and its empty window refuses the opening downstream"
    );
    fake.asked.borrow_mut().clear();
    writer
        .minute(&fake, minute, &[instrument("a", "QQQ")])
        .unwrap();
    assert!(
        fake.asked.borrow().is_empty(),
        "a written minute is never asked again"
    );
}

#[test]
fn the_closes_object_is_put_from_the_generation_once_and_is_the_readers_bytes() {
    let cell = Cell::new("closes");
    let days = span(&calendar(), day(SESSION)).unwrap();
    let fake = Fake {
        bars: BTreeMap::from([("QQQ".to_owned(), daily_bars(&days, 500))]),
        ..Fake::default()
    };
    let mut writer = cell.writer();
    let instruments = [instrument("asset-q", "QQQ"), instrument("asset-x", "NONE")];
    let now = at(SETTLED);
    assert!(
        writer.closes(day(SESSION), &instruments, now).is_empty(),
        "nothing stored, nothing put"
    );
    writer
        .daily(&fake, &calendar(), day(SESSION), &instruments[..1], now)
        .unwrap();
    let put = writer.closes(day(SESSION), &instruments, now);
    let closes =
        trusted_daily_closes(&daily_dir_at(&cell.root(), "QQQ", now).unwrap(), "QQQ", now).unwrap();
    let bytes = closes_object(&closes, "asset-q").unwrap();
    assert_eq!(put, vec![("asset-q".to_owned(), ArtifactRef::of(&bytes))]);
    let store = mandate_artifacts_fs::FsArtifactStore::open(cell.artifacts()).unwrap();
    assert_eq!(
        mandate_journal::get_artifact(&store, &ArtifactRef::of(&bytes)).unwrap(),
        bytes,
        "the object is in the artifact store"
    );
    assert!(
        writer.closes(day(SESSION), &instruments, now).is_empty(),
        "put once"
    );
}

#[test]
fn generations_past_their_retention_are_removed_and_nothing_else() {
    let cell = Cell::new("prune");
    let root = cell.root();
    for d in ["2026-10-22", "2026-10-23", "2026-10-26", "notadate"] {
        fs::create_dir_all(root.join("daily").join(d)).unwrap();
    }
    for m in [
        "2026-10-27T1419",
        "2026-10-27T1420",
        "2026-10-27T1430",
        "junk",
    ] {
        fs::create_dir_all(root.join("minutes").join(m)).unwrap();
    }
    cell.writer()
        .prune(&calendar(), at("2026-10-27T14:30:10Z"))
        .unwrap();
    let left: BTreeSet<String> = tree(&root)
        .into_iter()
        .filter(|p| p.matches('/').count() == 1)
        .collect();
    assert_eq!(
        left,
        [
            "daily/2026-10-23",
            "daily/2026-10-26",
            "daily/notadate",
            "minutes/2026-10-27T1420",
            "minutes/2026-10-27T1430",
            "minutes/junk",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        "two sessions of daily generations, ten minutes of minute generations"
    );
}

#[test]
fn a_restart_clears_what_an_interrupted_write_left() {
    let cell = Cell::new("restart");
    let staging = cell.root().join(".staging").join("left-behind");
    fs::create_dir_all(&staging).unwrap();
    fs::write(staging.join("x"), b"partial").unwrap();
    let _writer = cell.writer();
    assert!(!cell.root().join(".staging").join("left-behind").exists());
}

/// Every bar stored in `symbol`'s daily dataset of `session`'s generation under `root`, by day.
fn stored_bars(root: &Path, session: Date, sym: &str) -> Option<BTreeMap<Date, Vec<String>>> {
    let dir = daily_dir(&daily_generation(root, session), sym).unwrap();
    if !dir.exists() {
        return None;
    }
    let (id, listed) = mandate_marketdata::dataset::read_manifest(&dir).unwrap();
    let mut out = BTreeMap::new();
    for listed in listed {
        let closes = if listed.file.is_some() {
            let path = dir.join(mandate_marketdata::dataset::partition_name(listed.day));
            match mandate_marketdata::dataset::read(&path, id.kind()).unwrap() {
                mandate_marketdata::model::Records::Bars(bars) => bars
                    .iter()
                    .map(|b| format!("{}@{}", b.close.as_str(), b.start))
                    .collect(),
                other => panic!("{other:?}"),
            }
        } else {
            Vec::new()
        };
        out.insert(listed.day, closes);
    }
    Some(out)
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(32))]

    /// DP-15 and DP-16: whatever the source answers for each instrument (any sessions of the span
    /// missing, its newest among them, or a bar on a day the span does not hold), every bar stored
    /// is one it answered for that instrument and day, every session of the span is listed, no
    /// dataset holds an off-span day, and an empty newest session is stored only after `CONFIRM`.
    #[test]
    fn every_stored_bar_is_one_the_source_answered(
        present in proptest::collection::vec(proptest::collection::vec(proptest::bool::weighted(0.85), 30), 1..4),
        off_span in proptest::collection::vec(proptest::bool::weighted(0.2), 4),
    ) {
        let cell = Cell::new(&format!("prop-{}", present.len()));
        let days = span(&calendar(), day(SESSION)).unwrap();
        let names = ["AAA", "BBB", "CCC"];
        let mut served: BTreeMap<String, Vec<Bar>> = BTreeMap::new();
        for (n, keep) in present.iter().enumerate() {
            let mut bars: Vec<Bar> = days
                .iter()
                .zip(keep)
                .enumerate()
                .filter(|(_, (_, kept))| **kept)
                .map(|(k, (d, _))| bar(new_york_midnight(*d).unwrap(), &format!("{}.{k}", n + 1), "7"))
                .collect();
            if off_span[n] {
                bars.push(bar(new_york_midnight(day("2026-10-24")).unwrap(), "9", "9"));
            }
            bars.sort_by_key(|b| b.start);
            served.insert(names[n].to_owned(), bars);
        }
        let fake = Fake { bars: served.clone(), ..Fake::default() };
        let instruments: Vec<Instrument> = (0..present.len())
            .map(|n| instrument(&format!("id{n}"), names[n]))
            .collect();
        let mut writer = cell.writer();
        let start = at(SETTLED).secs();
        for offset in [0, CONFIRM_EVERY_S, CONFIRM_S - 1, CONFIRM_S] {
            let now = UtcNanos::from_parts(start + offset, 0).unwrap();
            writer.daily(&fake, &calendar(), day(SESSION), &instruments, now).unwrap();
            for (n, name) in names.iter().take(present.len()).enumerate() {
                let stored = stored_bars(&cell.root(), day(SESSION), name);
                let newest_empty = !present[n][29];
                let expect_stored = !off_span[n] && (!newest_empty || offset >= CONFIRM_S);
                proptest::prop_assert_eq!(stored.is_some(), expect_stored, "{} at +{}", name, offset);
                if let Some(stored) = stored {
                    proptest::prop_assert_eq!(
                        stored.keys().copied().collect::<Vec<_>>(),
                        days.clone(),
                        "every session of the span is listed"
                    );
                    for (d, closes) in &stored {
                        let answered: Vec<String> = served[*name]
                            .iter()
                            .filter(|b| b.start.date() == *d)
                            .map(|b| format!("{}@{}", b.close.as_str(), b.start))
                            .collect();
                        proptest::prop_assert_eq!(closes, &answered, "{} on {}", name, d);
                    }
                }
            }
        }
    }
}
