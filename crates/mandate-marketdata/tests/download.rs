//! Re-running a download is idempotent: identical data changes no file, and revised data is a
//! conflict that leaves the stored file as it was (backlog E2-1, DEC-89). A stock download also
//! records the corporate actions of the whole stored span next to the dataset (E2-4).

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use common::{
    FakeTransport, NO_ACTIONS, RecordingPause, Scratch, btc_1hour, day, ok, scenario,
    spy_sip_1hour, status,
};
use mandate_canon::{DecStr, Digest};
use mandate_marketdata::actions::{CORPORATE_ACTIONS, RecordedActions, read_actions};
use mandate_marketdata::alpaca::corporate_actions_path;
use mandate_marketdata::client::{Client, FetchError};
use mandate_marketdata::dataset::{self, DatasetError, Status, Store};
use mandate_marketdata::download::{DownloadError, StoredActions, download};
use mandate_marketdata::model::{CashDividend, CorporateActions, DayRange, Symbol};

type Snapshot = BTreeMap<PathBuf, (String, u64, SystemTime)>;

fn snapshot(root: &Path) -> Snapshot {
    let mut files = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let meta = fs::metadata(&path).unwrap();
                let hash = Digest::of(&fs::read(&path).unwrap()).to_hex();
                files.insert(path, (hash, meta.len(), meta.modified().unwrap()));
            }
        }
    }
    files
}

fn two_days() -> Vec<Vec<u8>> {
    [
        "stock-bars-sip-spy-1hour-2026-09-23",
        "stock-bars-sip-spy-1hour-2026-09-24",
    ]
    .iter()
    .flat_map(|name| scenario(name).bodies)
    .collect()
}

fn spy() -> Symbol {
    Symbol::parse("SPY").unwrap()
}

fn actions_request(first: &str, last: &str) -> String {
    let range = DayRange::new(day(first), day(last)).unwrap();
    corporate_actions_path(&spy(), range, 1_000, None).unwrap()
}

fn none_recorded(first: &str, last: &str, status: Status) -> Option<StoredActions> {
    Some(StoredActions {
        recorded: RecordedActions {
            range: DayRange::new(day(first), day(last)).unwrap(),
            actions: CorporateActions::none(spy()),
        },
        status,
    })
}

#[tokio::test(flavor = "current_thread")]
async fn second_run_changes_no_file() {
    let scratch = Scratch::new("idempotent");
    let store = Store::new(scratch.path());
    let id = spy_sip_1hour();
    let range = DayRange::new(day("2026-09-23"), day("2026-09-24")).unwrap();
    let replies = || {
        let mut bodies = two_days();
        bodies.push(NO_ACTIONS.to_vec());
        bodies
    };

    let transport = FakeTransport::serving(replies().iter().map(|b| ok(b)));
    let client = Client::new(transport.clone(), RecordingPause::default());
    let mut reported = Vec::new();
    let first = download(&client, &store, &id, range, |o| reported.push(o.clone()))
        .await
        .unwrap();
    assert_eq!(reported, first.days);
    assert_eq!(first.days.len(), 2);
    assert!(
        first
            .days
            .iter()
            .all(|o| o.status == Status::Written && o.rows == 16)
    );
    assert_eq!(
        first.corporate_actions,
        none_recorded("2026-09-23", "2026-09-24", Status::Written)
    );
    let requested = transport.requested();
    assert_eq!(
        requested,
        [
            scenario("stock-bars-sip-spy-1hour-2026-09-23").requests,
            scenario("stock-bars-sip-spy-1hour-2026-09-24").requests,
            vec![actions_request("2026-09-23", "2026-09-24")],
        ]
        .concat()
    );
    let before = snapshot(scratch.path());
    assert_eq!(
        before.len(),
        4,
        "two partitions, the manifest, and the corporate actions"
    );

    let transport = FakeTransport::serving(replies().iter().map(|b| ok(b)));
    let client = Client::new(transport, RecordingPause::default());
    let second = download(&client, &store, &id, range, |_| {}).await.unwrap();
    assert!(second.days.iter().all(|o| o.status == Status::Unchanged));
    assert_eq!(
        second.corporate_actions,
        none_recorded("2026-09-23", "2026-09-24", Status::Unchanged)
    );
    assert_eq!(
        first.days.iter().map(|o| &o.file).collect::<Vec<_>>(),
        second.days.iter().map(|o| &o.file).collect::<Vec<_>>()
    );
    assert_eq!(
        snapshot(scratch.path()),
        before,
        "no byte and no mtime changed"
    );

    for outcome in &first.days {
        let path = store
            .dataset_dir(&id)
            .join(format!("{}.parquet", outcome.day));
        let bytes = fs::read(&path).unwrap();
        let (len, digest) = outcome.file.unwrap();
        assert_eq!((len, digest), (bytes.len() as u64, Digest::of(&bytes)));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn revised_vendor_data_is_a_conflict_and_leaves_the_file_untouched() {
    let scratch = Scratch::new("conflict");
    let store = Store::new(scratch.path());
    let id = spy_sip_1hour();
    let range = DayRange::new(day("2026-09-24"), day("2026-09-24")).unwrap();
    let original = scenario("stock-bars-sip-spy-1hour-2026-09-24").bodies;

    let client = Client::new(
        FakeTransport::serving(original.iter().map(|b| ok(b)).chain([ok(NO_ACTIONS)])),
        RecordingPause::default(),
    );
    download(&client, &store, &id, range, |_| {}).await.unwrap();
    let before = snapshot(scratch.path());

    let text = String::from_utf8(original[0].clone()).unwrap();
    let revised = text.replacen("\"o\":764.53", "\"o\":764.54", 1);
    assert_ne!(revised, text);
    let client = Client::new(
        FakeTransport::serving([ok(revised.as_bytes())]),
        RecordingPause::default(),
    );
    let err = download(&client, &store, &id, range, |_| {})
        .await
        .unwrap_err();
    assert!(
        matches!(
            &err,
            DownloadError::Store {
                source: DatasetError::Conflict { .. },
                ..
            }
        ),
        "{err}"
    );
    assert_eq!(snapshot(scratch.path()), before);

    let client = Client::new(
        FakeTransport::serving([ok(br#"{"bars":{},"next_page_token":null}"#)]),
        RecordingPause::default(),
    );
    let err = download(&client, &store, &id, range, |_| {})
        .await
        .unwrap_err();
    assert!(matches!(err, DownloadError::Store { .. }), "{err}");
    assert_eq!(
        snapshot(scratch.path()),
        before,
        "a day that turns up empty keeps its file"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn empty_days_are_recorded_in_the_manifest_without_a_file() {
    let scratch = Scratch::new("empty");
    let store = Store::new(scratch.path());
    let id = spy_sip_1hour();
    let range = DayRange::new(day("2026-09-19"), day("2026-09-19")).unwrap();
    let body = scenario("stock-bars-sip-spy-1hour-2026-09-19").bodies;
    let replies = || body.iter().map(|b| ok(b)).chain([ok(NO_ACTIONS)]);

    let client = Client::new(FakeTransport::serving(replies()), RecordingPause::default());
    let first = download(&client, &store, &id, range, |_| {}).await.unwrap();
    assert_eq!(first.days.len(), 1);
    assert_eq!(
        (first.days[0].rows, first.days[0].file, first.days[0].status),
        (0, None, Status::Written)
    );
    let dir = store.dataset_dir(&id);
    assert!(!dir.join("2026-09-19.parquet").exists());
    let manifest = mandate_canon::parse(&fs::read(dir.join(dataset::MANIFEST)).unwrap()).unwrap();
    let days = manifest.get("days").and_then(|d| d.as_array()).unwrap();
    assert_eq!(days.len(), 1);
    assert_eq!(
        days[0].get("date").and_then(|d| d.as_str()),
        Some("2026-09-19")
    );
    assert_eq!(days[0].get("rows").and_then(|r| r.as_int()), Some(0));
    assert!(days[0].get("file").is_none());
    let before = snapshot(scratch.path());

    let client = Client::new(FakeTransport::serving(replies()), RecordingPause::default());
    let second = download(&client, &store, &id, range, |_| {}).await.unwrap();
    assert_eq!(second.days[0].status, Status::Unchanged);
    assert_eq!(snapshot(scratch.path()), before);
}

#[tokio::test(flavor = "current_thread")]
async fn a_manifest_of_another_dataset_is_refused() {
    let scratch = Scratch::new("foreign-manifest");
    let store = Store::new(scratch.path());
    let id = spy_sip_1hour();
    let dir = store.dataset_dir(&id);
    fs::create_dir_all(&dir).unwrap();
    let foreign = br#"{"days":[],"format":"something-else/9"}"#;
    fs::write(dir.join(dataset::MANIFEST), foreign).unwrap();
    let range = DayRange::new(day("2026-09-19"), day("2026-09-19")).unwrap();
    let body = scenario("stock-bars-sip-spy-1hour-2026-09-19").bodies;
    let client = Client::new(
        FakeTransport::serving(body.iter().map(|b| ok(b))),
        RecordingPause::default(),
    );
    let err = download(&client, &store, &id, range, |_| {})
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            DownloadError::Store {
                source: DatasetError::Manifest { .. },
                ..
            }
        ),
        "{err}"
    );
    assert_eq!(fs::read(dir.join(dataset::MANIFEST)).unwrap(), foreign);
}

const SPY_DIVIDEND: &[u8] = br#"{"corporate_actions":{"cash_dividends":[{"cusip":"78462F103","ex_date":"2026-09-24","foreign":false,"id":"spy-dividend","payable_date":"2026-10-31","process_date":"2026-10-31","rate":1.83,"record_date":"2026-09-24","special":false,"symbol":"SPY"}]},"next_page_token":null}"#;

#[tokio::test(flavor = "current_thread")]
async fn each_stock_download_records_the_actions_of_every_day_stored_so_far() {
    let scratch = Scratch::new("actions-span");
    let store = Store::new(scratch.path());
    let id = spy_sip_1hour();
    let dir = store.dataset_dir(&id);
    let one_day = |d: &str| DayRange::new(day(d), day(d)).unwrap();
    let bars = |d: &str| scenario(&format!("stock-bars-sip-spy-1hour-{d}")).bodies;

    let transport = FakeTransport::serving(
        bars("2026-09-23")
            .iter()
            .map(|b| ok(b))
            .chain([ok(SPY_DIVIDEND)]),
    );
    let client = Client::new(transport.clone(), RecordingPause::default());
    download(&client, &store, &id, one_day("2026-09-23"), |_| {})
        .await
        .unwrap();
    assert_eq!(
        transport.requested().last().unwrap(),
        &actions_request("2026-09-23", "2026-09-23")
    );
    assert_eq!(
        read_actions(&dir, &spy()).unwrap(),
        none_recorded("2026-09-23", "2026-09-23", Status::Written).map(|s| s.recorded),
        "the dividend's ex-date is after the stored day"
    );

    let transport = FakeTransport::serving(
        bars("2026-09-24")
            .iter()
            .map(|b| ok(b))
            .chain([ok(SPY_DIVIDEND)]),
    );
    let client = Client::new(transport.clone(), RecordingPause::default());
    let second = download(&client, &store, &id, one_day("2026-09-24"), |_| {})
        .await
        .unwrap();
    assert_eq!(
        transport.requested().last().unwrap(),
        &actions_request("2026-09-23", "2026-09-24"),
        "the request covers the whole stored span, not just this run's day"
    );
    let expected = RecordedActions {
        range: DayRange::new(day("2026-09-23"), day("2026-09-24")).unwrap(),
        actions: CorporateActions {
            cash_dividends: vec![CashDividend {
                id: "spy-dividend".to_owned(),
                ex_date: day("2026-09-24"),
                record_date: Some(day("2026-09-24")),
                payable_date: Some(day("2026-10-31")),
                rate: DecStr::parse("1.83").unwrap(),
                special: false,
                foreign: false,
            }],
            ..CorporateActions::none(spy())
        },
    };
    assert_eq!(
        second.corporate_actions,
        Some(StoredActions {
            recorded: expected.clone(),
            status: Status::Written,
        })
    );
    assert_eq!(read_actions(&dir, &spy()).unwrap(), Some(expected));
}

#[tokio::test(flavor = "current_thread")]
async fn a_crypto_download_asks_for_and_records_no_corporate_actions() {
    let scratch = Scratch::new("actions-crypto");
    let store = Store::new(scratch.path());
    let id = btc_1hour();
    let range = DayRange::new(day("2026-09-24"), day("2026-09-24")).unwrap();
    let recorded = scenario("crypto-bars-btcusd-1hour-2026-09-24");
    let transport = FakeTransport::serving(recorded.bodies.iter().map(|b| ok(b)));
    let client = Client::new(transport.clone(), RecordingPause::default());
    let done = download(&client, &store, &id, range, |_| {}).await.unwrap();
    assert_eq!(done.corporate_actions, None);
    assert_eq!(transport.requested(), recorded.requests);
    assert!(!store.dataset_dir(&id).join(CORPORATE_ACTIONS).exists());
}

#[tokio::test(flavor = "current_thread")]
async fn a_failed_actions_fetch_keeps_the_stored_days_and_a_rerun_records_the_actions() {
    let scratch = Scratch::new("actions-fetch-fails");
    let store = Store::new(scratch.path());
    let id = spy_sip_1hour();
    let dir = store.dataset_dir(&id);
    let range = DayRange::new(day("2026-09-24"), day("2026-09-24")).unwrap();
    let bars = scenario("stock-bars-sip-spy-1hour-2026-09-24").bodies;

    let client = Client::new(
        FakeTransport::serving(bars.iter().map(|b| ok(b)).chain([status(403)])),
        RecordingPause::default(),
    );
    let err = download(&client, &store, &id, range, |_| {})
        .await
        .unwrap_err();
    assert!(
        matches!(
            &err,
            DownloadError::FetchActions {
                source: FetchError::Status { status: 403 },
                ..
            }
        ),
        "{err}"
    );
    assert_eq!(err.code(), "fetch_actions");
    assert_eq!(
        err.to_string(),
        "SPY bars-1Hour (sip) corporate actions: the data host answered HTTP 403"
    );
    assert!(dir.join("2026-09-24.parquet").exists());
    assert!(!dir.join(CORPORATE_ACTIONS).exists());

    let client = Client::new(
        FakeTransport::serving(bars.iter().map(|b| ok(b)).chain([ok(NO_ACTIONS)])),
        RecordingPause::default(),
    );
    let rerun = download(&client, &store, &id, range, |_| {}).await.unwrap();
    assert_eq!(rerun.days[0].status, Status::Unchanged);
    assert_eq!(
        rerun.corporate_actions,
        none_recorded("2026-09-24", "2026-09-24", Status::Written)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn actions_that_cannot_be_stored_are_an_error_naming_the_dataset() {
    let scratch = Scratch::new("actions-store-fails");
    let store = Store::new(scratch.path());
    let id = spy_sip_1hour();
    fs::create_dir_all(store.dataset_dir(&id).join(CORPORATE_ACTIONS)).unwrap();
    let range = DayRange::new(day("2026-09-24"), day("2026-09-24")).unwrap();
    let bars = scenario("stock-bars-sip-spy-1hour-2026-09-24").bodies;
    let client = Client::new(
        FakeTransport::serving(bars.iter().map(|b| ok(b)).chain([ok(NO_ACTIONS)])),
        RecordingPause::default(),
    );
    let err = download(&client, &store, &id, range, |_| {})
        .await
        .unwrap_err();
    assert!(matches!(err, DownloadError::StoreActions { .. }), "{err}");
    assert_eq!(err.code(), "store_actions");
    assert!(
        err.to_string()
            .starts_with("SPY bars-1Hour (sip) corporate actions: "),
        "{err}"
    );
}
