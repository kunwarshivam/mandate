//! The runtime's research artifact sink in a cell (DEC-2279 item 3): a put counts as stored only
//! when the model-gateway service answers the reference of the bytes sent and the runtime's own
//! checked read of it through its read-only store gives them back. A service that answers without
//! storing, answers another reference, refuses, stores bytes that do not re-hash, or answers
//! nothing, or whose research store is not there, is a put that stored nothing, so the writer refuses the batch and research halts
//! (DEC-2208 items 5 and 6). Each case runs against a real local socket that a test thread
//! scripts; the oracle is SHA-256 of the bytes sent and the files the script wrote.

#![allow(
    clippy::disallowed_methods,
    reason = "each case times a real socket against the host clock, as the runtime's edge does"
)]

use std::fs;
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::Digest;
use mandate_daemon::ArtifactSink;
use mandate_daemon::remote::{PUT_TIMEOUT, RemoteGateway, RemoteSink};
use mandate_gateway::frame::{MAX_REPLY_FRAME, MAX_REQUEST_FRAME, read_frame, write_frame};
use mandate_gateway::wire::{PutRefusal, PutReply, PutRequest, put_from, put_reply_bytes};
use mandate_journal::{ArtifactRef, ArtifactStore};
use mandate_time::UtcNanos;

fn host_now() -> UtcNanos {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    UtcNanos::from_parts(
        i64::try_from(since.as_secs()).unwrap(),
        since.subsec_nanos(),
    )
    .unwrap()
}

/// A fresh store, opened for writing by the scripted service, at a path of its own.
fn store(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("daemon-{}-sink-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    FsArtifactStore::open(&root).unwrap();
    root
}

/// What the scripted service does with one put, given the store it may write.
#[derive(Clone, Copy)]
enum Script {
    /// Stores the bytes and answers their reference, as the model-gateway service does.
    Honest,
    /// Answers the bytes' reference and stores nothing.
    Lies,
    /// Stores the bytes and answers another reference.
    WrongHash,
    /// Stores nothing and refuses.
    Refuses(PutRefusal),
    /// Stores other bytes under the bytes' reference, and answers that reference.
    Damages,
    /// Reads the put and closes without a reply.
    Drops,
    /// Reads the put and stores it, then answers only after three seconds.
    Stalls,
}

/// Serves one put on a local port by `script`, handing back the put it read.
fn serve(root: &Path, script: Script) -> (SocketAddr, mpsc::Receiver<PutRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let mut store = FsArtifactStore::open_existing(root).unwrap();
    let (seen, sent) = mpsc::channel();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let put = put_from(&read_frame(&mut stream, MAX_REQUEST_FRAME).unwrap()).unwrap();
        let reference = ArtifactRef::of(&put.bytes);
        let reply: Option<PutReply> = match script {
            Script::Honest => Some(Ok(store.put_artifact(&put.bytes).unwrap().digest())),
            Script::Lies => Some(Ok(reference.digest())),
            Script::WrongHash => {
                store.put_artifact(&put.bytes).unwrap();
                Some(Ok(Digest::of(b"another artifact")))
            }
            Script::Refuses(refusal) => Some(Err(refusal)),
            Script::Damages => {
                let path = store.object_path(&reference);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, b"not the bytes sent").unwrap();
                Some(Ok(reference.digest()))
            }
            Script::Drops => None,
            Script::Stalls => {
                let stored = store.put_artifact(&put.bytes).unwrap().digest();
                thread::sleep(Duration::from_secs(3));
                Some(Ok(stored))
            }
        };
        if let Some(reply) = reply {
            let _ = write_frame(&mut stream, &put_reply_bytes(&reply), MAX_REPLY_FRAME);
        }
        seen.send(put).unwrap();
    });
    (address, sent)
}

/// The sink `script`'s service backs, reading back from the research store at `root`.
fn sink(root: &Path, script: Script) -> (RemoteSink, mpsc::Receiver<PutRequest>) {
    let (address, sent) = serve(root, script);
    let sink = RemoteSink::new(RemoteGateway::new(address, host_now), "ws1", root);
    (sink, sent)
}

#[test]
fn a_put_the_service_stored_and_the_runtime_read_back_is_stored() {
    let root = store("honest");
    let (mut sink, sent) = sink(&root, Script::Honest);
    assert!(sink.put(b"thesis"));
    assert_eq!(
        sent.recv().unwrap(),
        PutRequest {
            workspace_id: "ws1".to_owned(),
            bytes: b"thesis".to_vec(),
        },
        "the put names the runtime's workspace and carries the bytes alone"
    );
    assert_eq!(sink.get(&Digest::of(b"thesis")), Some(b"thesis".to_vec()));
}

#[test]
fn an_answer_without_the_object_stored_is_a_failed_put() {
    let root = store("lies");
    let (mut sink, _) = sink(&root, Script::Lies);
    assert!(!sink.put(b"thesis"), "the read-back finds nothing");
}

#[test]
fn an_answer_naming_another_reference_is_a_failed_put() {
    let root = store("wrong-hash");
    let (mut sink, _) = sink(&root, Script::WrongHash);
    assert!(
        !sink.put(b"thesis"),
        "the bytes are stored, but the answer is not their reference"
    );
}

#[test]
fn a_refused_put_is_a_failed_put() {
    for refusal in PutRefusal::ALL {
        let root = store(refusal.code());
        let (mut sink, _) = sink(&root, Script::Refuses(refusal));
        assert!(!sink.put(b"thesis"), "{}", refusal.code());
    }
}

#[test]
fn a_read_back_that_does_not_re_hash_is_a_failed_put() {
    let root = store("damaged");
    let (mut sink, _) = sink(&root, Script::Damages);
    assert!(!sink.put(b"thesis"));
    assert_eq!(sink.get(&Digest::of(b"thesis")), None);
}

#[test]
fn a_lost_answer_is_a_failed_put() {
    let root = store("dropped");
    let (mut sink, _) = sink(&root, Script::Drops);
    assert!(!sink.put(b"thesis"));
}

#[test]
fn a_service_that_is_not_there_is_a_failed_put() {
    let root = store("absent");
    let address = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let mut sink = RemoteSink::new(RemoteGateway::new(address, host_now), "ws1", &root);
    assert!(!sink.put(b"thesis"));
}

/// DEC-2279 item 5: the runtime opens the research store at each read, never at start, so a sink
/// over a store that is not there, or not yet laid out, is made, and its puts fail closed.
#[test]
fn a_research_store_not_there_or_not_laid_out_is_a_failed_put_never_a_failed_start() {
    let root = store("unlaid");
    for research in [root.join("nowhere"), root.join("bare")] {
        if research.ends_with("bare") {
            fs::create_dir(&research).unwrap();
        }
        let (address, _) = serve(&root, Script::Honest);
        let mut sink = RemoteSink::new(RemoteGateway::new(address, host_now), "ws1", &research);
        assert!(!sink.put(b"thesis"), "{research:?}");
        assert_eq!(sink.get(&Digest::of(b"thesis")), None);
    }
}

/// DEC-2279 item 6: the runtime's research reads read the research store alone.
#[test]
fn the_sink_reads_the_research_store_alone() {
    let shared = store("shared");
    let research = store("research-alone");
    FsArtifactStore::open_existing(&shared)
        .unwrap()
        .put_artifact(b"in the shared store")
        .unwrap();
    let sink = RemoteSink::new(
        RemoteGateway::new(
            TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap(),
            host_now,
        ),
        "ws1",
        &research,
    );
    assert_eq!(sink.get(&Digest::of(b"in the shared store")), None);
}

/// DEC-2279 item 3: a service that takes the put but answers past the put's timeout is a failed put,
/// and the runtime gives up at the timeout, not at the answer.
#[test]
fn an_answer_past_the_put_timeout_is_a_failed_put() {
    let root = store("stalled");
    let (address, _) = serve(&root, Script::Stalls);
    let mut sink = RemoteSink::new(
        RemoteGateway::new(address, host_now).with_put_timeout(Duration::from_millis(300)),
        "ws1",
        &root,
    );
    let started = Instant::now();
    assert!(!sink.put(b"thesis"), "stored, but answered too late");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
}

/// DEC-2279 item 3: the put's timeout is 30 seconds unless set.
#[test]
fn the_put_timeout_is_thirty_seconds() {
    assert_eq!(PUT_TIMEOUT, Duration::from_secs(30));
}
