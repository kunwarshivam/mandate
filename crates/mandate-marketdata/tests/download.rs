//! Re-running a download is idempotent: identical data changes no file, and revised data is a
//! conflict that leaves the stored file as it was (backlog E2-1, DEC-89).

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use common::{FakeTransport, RecordingPause, Scratch, day, ok, scenario, spy_sip_1hour};
use mandate_canon::Digest;
use mandate_marketdata::client::Client;
use mandate_marketdata::dataset::{self, DatasetError, Status, Store};
use mandate_marketdata::download::{DownloadError, download};
use mandate_marketdata::model::DayRange;

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

#[tokio::test(flavor = "current_thread")]
async fn second_run_changes_no_file() {
    let scratch = Scratch::new("idempotent");
    let store = Store::new(scratch.path());
    let id = spy_sip_1hour();
    let range = DayRange::new(day("2026-09-23"), day("2026-09-24")).unwrap();

    let transport = FakeTransport::serving(two_days().iter().map(|b| ok(b)));
    let client = Client::new(transport.clone(), RecordingPause::default());
    let mut reported = Vec::new();
    let first = download(&client, &store, &id, range, |o| reported.push(o.clone()))
        .await
        .unwrap();
    assert_eq!(reported, first);
    assert_eq!(first.len(), 2);
    assert!(
        first
            .iter()
            .all(|o| o.status == Status::Written && o.rows == 16)
    );
    let requested = transport.requested();
    assert_eq!(
        requested,
        [
            scenario("stock-bars-sip-spy-1hour-2026-09-23").requests,
            scenario("stock-bars-sip-spy-1hour-2026-09-24").requests
        ]
        .concat()
    );
    let before = snapshot(scratch.path());
    assert_eq!(before.len(), 3, "two partitions and the manifest");

    let transport = FakeTransport::serving(two_days().iter().map(|b| ok(b)));
    let client = Client::new(transport, RecordingPause::default());
    let second = download(&client, &store, &id, range, |_| {}).await.unwrap();
    assert!(second.iter().all(|o| o.status == Status::Unchanged));
    assert_eq!(
        first.iter().map(|o| &o.file).collect::<Vec<_>>(),
        second.iter().map(|o| &o.file).collect::<Vec<_>>()
    );
    assert_eq!(
        snapshot(scratch.path()),
        before,
        "no byte and no mtime changed"
    );

    for outcome in &first {
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
        FakeTransport::serving(original.iter().map(|b| ok(b))),
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

    let client = Client::new(
        FakeTransport::serving(body.iter().map(|b| ok(b))),
        RecordingPause::default(),
    );
    let first = download(&client, &store, &id, range, |_| {}).await.unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(
        (first[0].rows, first[0].file, first[0].status),
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

    let client = Client::new(
        FakeTransport::serving(body.iter().map(|b| ok(b))),
        RecordingPause::default(),
    );
    let second = download(&client, &store, &id, range, |_| {}).await.unwrap();
    assert_eq!(second[0].status, Status::Unchanged);
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
