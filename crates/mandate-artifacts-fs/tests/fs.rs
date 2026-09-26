//! The filesystem artifact store (journal spec §6.3, backlog E5-2, DEC-107). Oracles: published
//! SHA-256 vectors for the address, and the files on disk read directly for layout, contents,
//! permissions, and leftovers.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;

use mandate_artifacts_fs::FsArtifactStore;
use mandate_journal::{
    AppendOutcome, ArtifactError, ArtifactRef, ArtifactSource, ArtifactStore, EventCheck,
    EventFailure, MemoryJournal, StreamId, TrustedStart, get_artifact, verify_events,
};
use mandate_time::UtcNanos;
use proptest::collection::vec;
use proptest::prelude::*;

/// FIPS 180-2 SHA-256 test vector for `abc`.
const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

/// A fresh, empty directory under Cargo's per-target temporary directory.
fn fresh_dir(name: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "artifacts-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap();
    }
    dir
}

fn open(name: &str) -> (PathBuf, FsArtifactStore) {
    let dir = fresh_dir(name);
    let store = FsArtifactStore::open(&dir).unwrap();
    (dir, store)
}

/// Every regular file under `dir`, recursively.
fn files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(files(&path));
        } else {
            found.push(path);
        }
    }
    found
}

/// Overwrites the object for `reference` with `bytes`, as a disk fault or an intruder would.
fn tamper(store: &FsArtifactStore, reference: &ArtifactRef, bytes: &[u8]) {
    let path = store.object_path(reference);
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    #[expect(
        clippy::permissions_set_readonly_false,
        reason = "the test plays a writer that ignores the store's read-only mode"
    )]
    permissions.set_readonly(false);
    fs::set_permissions(&path, permissions).unwrap();
    fs::write(&path, bytes).unwrap();
}

#[test]
#[ignore = "pending E5-2"]
fn objects_live_at_sha256_shard_hex_and_are_read_only() {
    let (dir, mut store) = open("layout");
    let reference = store.put_artifact(b"abc").unwrap();
    assert_eq!(reference.to_string(), format!("sha256:{ABC_SHA256}"));
    let expected = dir.join("sha256").join(&ABC_SHA256[..2]).join(ABC_SHA256);
    assert_eq!(store.object_path(&reference), expected);
    assert_eq!(fs::read(&expected).unwrap(), b"abc");
    assert!(fs::metadata(&expected).unwrap().permissions().readonly());
    assert_eq!(
        files(&dir),
        vec![expected],
        "one object and no temporary files"
    );
}

#[test]
#[ignore = "pending E5-2"]
fn an_absent_reference_is_missing() {
    let (_, store) = open("absent");
    let reference = ArtifactRef::of(b"never stored");
    assert_eq!(store.read_artifact(&reference), Err(ArtifactError::Missing));
    assert_eq!(
        get_artifact(&store, &reference),
        Err(ArtifactError::Missing)
    );
}

#[test]
#[ignore = "pending E5-2"]
fn an_unreadable_object_is_unavailable_not_missing() {
    let (_, mut store) = open("unreadable");
    let readable = store.put_artifact(b"readable").unwrap();
    let reference = ArtifactRef::of(b"blocked");
    fs::create_dir_all(store.object_path(&reference)).unwrap();
    assert_eq!(
        store.read_artifact(&reference),
        Err(ArtifactError::Unavailable)
    );
    assert_eq!(
        get_artifact(&store, &reference),
        Err(ArtifactError::Unavailable),
        "the checked read passes Unavailable through, not Missing or Corrupt"
    );
    assert_eq!(
        store.put_artifact(b"blocked"),
        Err(ArtifactError::Unavailable)
    );
    assert_eq!(
        get_artifact(&store, &readable),
        Ok(b"readable".to_vec()),
        "one unreadable object leaves the others readable"
    );
}

#[test]
#[ignore = "pending E5-2"]
fn a_name_in_place_without_an_object_is_unavailable() {
    let (_, mut store) = open("dangling");
    let reference = ArtifactRef::of(b"shadowed");
    let path = store.object_path(&reference);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(path.with_extension("gone"), &path).unwrap();
    assert_eq!(store.read_artifact(&reference), Err(ArtifactError::Missing));
    assert_eq!(
        store.put_artifact(b"shadowed"),
        Err(ArtifactError::Unavailable),
        "the link cannot replace the name, and nothing readable is behind it"
    );
}

#[test]
#[ignore = "pending E5-2"]
fn a_root_that_cannot_be_created_is_unavailable() {
    let dir = fresh_dir("root-is-a-file");
    fs::create_dir_all(dir.parent().unwrap()).unwrap();
    fs::write(&dir, b"not a directory").unwrap();
    assert_eq!(FsArtifactStore::open(&dir), Err(ArtifactError::Unavailable));
}

#[test]
#[ignore = "pending E5-2"]
fn objects_survive_reopening_the_store() {
    let (dir, mut store) = open("reopen");
    let reference = store.put_artifact(b"data snapshot").unwrap();
    drop(store);
    let reopened = FsArtifactStore::open(&dir).unwrap();
    assert_eq!(
        get_artifact(&reopened, &reference),
        Ok(b"data snapshot".to_vec())
    );
}

#[test]
#[ignore = "pending E5-2"]
fn put_never_overwrites_a_corrupt_object() {
    let (_, mut store) = open("no-overwrite");
    let reference = store.put_artifact(b"thesis").unwrap();
    tamper(&store, &reference, b"thesix");
    assert_eq!(store.put_artifact(b"thesis"), Err(ArtifactError::Corrupt));
    assert_eq!(
        fs::read(store.object_path(&reference)).unwrap(),
        b"thesix",
        "the corrupt object stays as evidence"
    );
}

#[test]
#[ignore = "pending E5-2"]
fn concurrent_puts_of_the_same_bytes_all_succeed_with_one_object() {
    let dir = fresh_dir("concurrent");
    let bytes = vec![7u8; 64 * 1024];
    let expected = ArtifactRef::of(&bytes);
    let results: Vec<_> = thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    FsArtifactStore::open(&dir).and_then(|mut store| store.put_artifact(&bytes))
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert!(results.iter().all(|r| *r == Ok(expected)), "{results:?}");
    let store = FsArtifactStore::open(&dir).unwrap();
    assert_eq!(files(&dir), vec![store.object_path(&expected)]);
    assert_eq!(get_artifact(&store, &expected), Ok(bytes));
}

#[test]
#[ignore = "pending E5-2"]
fn readers_see_a_whole_object_or_none_while_it_is_written() {
    let dir = fresh_dir("atomic");
    let reader = FsArtifactStore::open(&dir).unwrap();
    let contents: Vec<Vec<u8>> = (0u8..32).map(|n| vec![n; 2 * 1024 * 1024]).collect();
    let references: Vec<ArtifactRef> = contents.iter().map(|c| ArtifactRef::of(c)).collect();
    let done = AtomicBool::new(false);
    let (puts, seen, reads) = thread::scope(|scope| {
        let writer = scope.spawn(|| {
            let puts = FsArtifactStore::open(&dir).map(|mut writer| {
                contents
                    .iter()
                    .map(|content| writer.put_artifact(content))
                    .collect::<Vec<_>>()
            });
            done.store(true, Ordering::SeqCst);
            puts
        });
        let mut pending: Vec<_> = references.iter().zip(&contents).collect();
        let mut seen = Vec::new();
        let mut reads = 0u64;
        while !done.load(Ordering::SeqCst) && !pending.is_empty() {
            pending.retain(|(reference, content)| {
                reads += 1;
                match get_artifact(&reader, reference) {
                    Ok(bytes) => {
                        assert_eq!(&&bytes, content);
                        false
                    }
                    Err(ArtifactError::Missing) => true,
                    Err(other) => {
                        seen.push(other);
                        false
                    }
                }
            });
        }
        (writer.join().unwrap(), seen, reads)
    });
    assert_eq!(puts, Ok(references.iter().copied().map(Ok).collect()));
    assert_eq!(seen, vec![], "a partial object was visible ({reads} reads)");
    for (reference, content) in references.iter().zip(&contents) {
        assert_eq!(get_artifact(&reader, reference).as_ref(), Ok(content));
    }
}

#[test]
#[ignore = "pending E5-2"]
fn a_crashed_write_leaves_no_object() {
    let (dir, mut store) = open("crash");
    let content = b"prompt and retrieved context".to_vec();
    let reference = ArtifactRef::of(&content);
    let hex = reference.digest().to_hex();
    let torn = [
        &content[..9],
        b" and bytes a torn write left behind".as_slice(),
    ]
    .concat();
    let leftovers: Vec<PathBuf> = [
        hex.clone(),
        format!("{hex}.partial"),
        format!("{hex}.tmp"),
        format!("{hex}.{}.0", std::process::id()),
    ]
    .iter()
    .map(|name| dir.join("tmp").join(name))
    .collect();
    for path in &leftovers {
        fs::write(path, &torn).unwrap();
    }
    assert_eq!(
        get_artifact(&store, &reference),
        Err(ArtifactError::Missing),
        "a write that never finished is not an object"
    );
    assert_eq!(store.put_artifact(&content), Ok(reference));
    assert_eq!(get_artifact(&store, &reference), Ok(content));
    for path in &leftovers {
        assert_eq!(fs::read(path).unwrap(), torn, "leftovers are never reused");
    }
    let objects: Vec<PathBuf> = files(&dir)
        .into_iter()
        .filter(|p| !leftovers.contains(p))
        .collect();
    assert_eq!(objects, vec![store.object_path(&reference)]);
}

#[test]
#[ignore = "pending E5-2"]
fn verification_reads_artifacts_from_disk() {
    let (_, mut store) = open("verify");
    let content = b"model response".to_vec();
    let reference = ArtifactRef::of(&content);
    let stream = StreamId::parse("acct:ws_1:ACCT1").unwrap();
    let journal = journal_citing(&stream, &reference);
    let failed = |check| Err(EventFailure { seq: 2, check });
    assert_eq!(
        verify_events(journal.rows(&stream), TrustedStart::GENESIS, &store),
        failed(EventCheck::ArtifactMissing)
    );
    assert_eq!(store.put_artifact(&content), Ok(reference));
    assert!(verify_events(journal.rows(&stream), TrustedStart::GENESIS, &store).is_ok());
    tamper(&store, &reference, b"tampered response");
    assert_eq!(
        verify_events(journal.rows(&stream), TrustedStart::GENESIS, &store),
        failed(EventCheck::ArtifactMismatch)
    );
}

/// A journal whose seq 2 `MarkUpdated` cites `reference` in its payload and `artifact_refs`.
fn journal_citing(stream: &StreamId, reference: &ArtifactRef) -> MemoryJournal {
    let t = "2026-09-21T14:00:00.000000000Z";
    let build = "3".repeat(64);
    let envelope = |n: u32, event_type: &str, payload: &str, refs: &str| {
        format!(
            r#"{{"envelope_version":1,"environment":"paper","event_id":"01J8Z3M4{n:018}",
            "stream_id":"{stream}","event_type":"{event_type}","schema_version":1,"event_time":"{t}",
            "clock_source":"local","causation_id":null,"correlation_id":null,
            "actor":{{"kind":"system","id":"executor","version":"0.1.0","build":"sha256:{build}"}},
            "config_refs":{{}},"payload":{payload},"artifact_refs":[{refs}],"pii_refs":[]}}"#
        )
        .into_bytes()
    };
    let opened = envelope(
        0,
        "StreamOpened",
        r#"{"stream_type":"account","workspace_id":"ws_1","broker":"alpaca","account_ref":"ACCT1"}"#,
        "",
    );
    let mark = envelope(
        1,
        "MarkUpdated",
        &format!(
            r#"{{"instrument_id":"inst","price":"1","source":"{reference}","feed":"iex","risk_clock":"{t}"}}"#
        ),
        &format!("\"{reference}\""),
    );
    let mut journal = MemoryJournal::new();
    let epoch = journal.take_ownership(stream);
    let now = UtcNanos::parse(t).unwrap();
    let outcome = journal.append(stream, 0, epoch, now, &[&opened, &mark]);
    assert!(
        matches!(outcome, AppendOutcome::Committed(_)),
        "{outcome:?}"
    );
    journal
}

proptest! {
    #[test]
    #[ignore = "pending E5-2"]
    fn put_then_get_returns_the_bytes(bytes in vec(any::<u8>(), 0..2048)) {
        let (dir, mut store) = open("round-trip");
        let reference = store.put_artifact(&bytes).unwrap();
        prop_assert_eq!(reference, ArtifactRef::of(&bytes));
        prop_assert_eq!(fs::read(store.object_path(&reference)).unwrap(), bytes.clone());
        prop_assert_eq!(get_artifact(&store, &reference), Ok(bytes));
        prop_assert_eq!(files(&dir).len(), 1);
    }

    #[test]
    #[ignore = "pending E5-2"]
    fn the_same_bytes_get_the_same_address_once(
        a in vec(any::<u8>(), 0..64),
        b in vec(any::<u8>(), 0..64),
    ) {
        let (dir, mut store) = open("idempotent");
        let first = store.put_artifact(&a).unwrap();
        prop_assert_eq!(store.put_artifact(&a), Ok(first), "re-put is idempotent");
        let other = store.put_artifact(&b).unwrap();
        prop_assert_eq!(first == other, a == b);
        prop_assert_eq!(files(&dir).len(), if a == b { 1 } else { 2 });
    }

    #[test]
    #[ignore = "pending E5-2"]
    fn one_flipped_bit_on_disk_is_detected_on_read(
        bytes in vec(any::<u8>(), 1..256),
        index in any::<prop::sample::Index>(),
        bit in 0u8..8,
        truncate in any::<bool>(),
    ) {
        let (_, mut store) = open("flipped");
        let reference = store.put_artifact(&bytes).unwrap();
        let mut damaged = bytes.clone();
        let at = index.index(bytes.len());
        if truncate {
            damaged.truncate(at);
        } else {
            damaged[at] ^= 1 << bit;
        }
        tamper(&store, &reference, &damaged);
        prop_assert_eq!(store.read_artifact(&reference), Ok(damaged), "raw read");
        prop_assert_eq!(get_artifact(&store, &reference), Err(ArtifactError::Corrupt));
    }
}
