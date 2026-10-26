//! The service's artifact writer (DEC-2279 items 1, 2, 4 and 5) and its put on the serve loop: a
//! put is stored in the research store under the reference the service computes, create-only, for
//! its own workspace only, never past the cap counted from the store at open, each object counted
//! once, and crosses a real connection on the one answering thread; a server that stores nothing
//! refuses every put, and no log line carries a byte of one. Oracles: the files in the store read
//! directly, and SHA-256 of the bytes sent.

use std::fs;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::Digest;
use mandate_gateway::RequestError;
use mandate_gateway::frame;
use mandate_gateway::wire::{PutRefusal, PutReply, PutRequest};
use mandate_gatewayd::serve::{Limits, Log, Server, Stopper};
use mandate_gatewayd::writer::{ArtifactWriter, STORE_CAP};
use mandate_journal::{ArtifactRef, ArtifactSource, ArtifactStore};

const CANARY: &str = "CANARY-5d1e2b-never-logged";

/// A fresh, laid-out research store under Cargo's per-target temporary directory, and its root.
fn store(name: &str) -> (FsArtifactStore, PathBuf) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "writer-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap();
    }
    (FsArtifactStore::open(&dir).unwrap(), dir)
}

fn put(bytes: &[u8]) -> PutRequest {
    PutRequest {
        workspace_id: "ws_1".to_owned(),
        bytes: bytes.to_vec(),
    }
}

/// The bytes of every file under `dir`, by the oracle's own walk.
fn on_disk(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            if path.is_dir() {
                on_disk(&path)
            } else {
                fs::metadata(&path).unwrap().len()
            }
        })
        .sum()
}

#[test]
fn the_cap_is_512_mib() {
    assert_eq!(STORE_CAP, 536_870_912);
}

#[test]
#[ignore = "pending E21-62"]
fn a_put_is_stored_under_the_reference_the_service_computes() {
    let (reader, root) = store("stored");
    let mut writer = ArtifactWriter::open(&root, "ws_1").unwrap();
    assert_eq!(writer.put(&put(b"abc")), Ok(Digest::of(b"abc")));
    assert_eq!(
        reader.read_artifact(&ArtifactRef::of(b"abc")).unwrap(),
        b"abc",
        "a reader of the research store reads what the writer stored"
    );
    assert_eq!(writer.stored(), 3);
}

#[test]
#[ignore = "pending E21-62"]
fn another_workspaces_put_is_refused_and_stores_nothing() {
    let (_, root) = store("foreign");
    let mut writer = ArtifactWriter::open(&root, "ws_1").unwrap();
    let foreign = PutRequest {
        workspace_id: "ws_2".to_owned(),
        bytes: b"abc".to_vec(),
    };
    assert_eq!(writer.put(&foreign), Err(PutRefusal::ForeignWorkspace));
    assert_eq!(on_disk(&root), 0);
    assert_eq!(writer.stored(), 0);
}

#[test]
#[ignore = "pending E21-62"]
fn an_existing_objects_bytes_never_change_and_a_damaged_one_is_refused() {
    let (reader, root) = store("create-only");
    let reference = ArtifactRef::of(b"thesis");
    let mut writer = ArtifactWriter::open(&root, "ws_1").unwrap();
    assert_eq!(writer.put(&put(b"thesis")), Ok(reference.digest()));
    let path = reader.object_path(&reference);
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    #[expect(
        clippy::permissions_set_readonly_false,
        reason = "the test plays a writer that ignores the store's read-only mode"
    )]
    permissions.set_readonly(false);
    fs::set_permissions(&path, permissions).unwrap();
    fs::write(&path, b"thesix").unwrap();
    assert_eq!(
        writer.put(&put(b"thesis")),
        Err(PutRefusal::StoreUnavailable)
    );
    assert_eq!(
        fs::read(&path).unwrap(),
        b"thesix",
        "the damaged object stays as evidence, never replaced"
    );
}

#[test]
#[ignore = "pending E21-62"]
fn the_count_starts_from_every_file_in_the_store_temporary_leftovers_among_them() {
    let (mut earlier, root) = store("counted");
    earlier.put_artifact(b"control run").unwrap();
    fs::write(root.join("tmp").join("crashed.1.0"), b"left behind").unwrap();
    let writer = ArtifactWriter::open(&root, "ws_1").unwrap();
    assert_eq!(writer.stored(), on_disk(&root));
    assert_eq!(writer.stored(), 22);
}

#[test]
#[ignore = "pending E21-62"]
fn identical_bytes_are_counted_once() {
    let (_, root) = store("once");
    let mut writer = ArtifactWriter::open(&root, "ws_1").unwrap();
    for _ in 0..3 {
        assert_eq!(writer.put(&put(b"thesis")), Ok(Digest::of(b"thesis")));
    }
    assert_eq!(writer.stored(), 6);
    assert_eq!(writer.stored(), on_disk(&root));
}

#[test]
#[ignore = "pending E21-62"]
fn a_put_past_the_cap_is_refused_store_full_and_stores_nothing() {
    let (_, root) = store("full");
    let mut writer = ArtifactWriter::capped(&root, "ws_1", 10).unwrap();
    assert_eq!(writer.put(&put(b"0123456")), Ok(Digest::of(b"0123456")));
    assert_eq!(
        writer.put(&put(b"abc")),
        Ok(Digest::of(b"abc")),
        "at the cap"
    );
    assert_eq!(writer.put(&put(b"x")), Err(PutRefusal::StoreFull));
    assert_eq!(writer.stored(), 10);
    assert_eq!(on_disk(&root), 10, "the refused put left nothing");
    assert_eq!(
        writer.put(&put(b"abc")),
        Ok(Digest::of(b"abc")),
        "bytes already stored answer their reference however full the store is"
    );
}

/// DEC-2279 item 4: after a put the store did not take, the count is taken again from the store,
/// so bytes that came after open (here a crashed put's leftover) bind the cap from then on.
#[test]
#[ignore = "pending E21-62"]
fn a_put_the_store_did_not_take_takes_the_count_again() {
    let (mut earlier, root) = store("recount");
    let mut writer = ArtifactWriter::capped(&root, "ws_1", 30).unwrap();
    assert_eq!(writer.stored(), 0);
    fs::write(root.join("tmp").join("crashed.1.0"), b"0123456789").unwrap();
    let reference = earlier.put_artifact(b"thesis").unwrap();
    let path = earlier.object_path(&reference);
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    #[expect(
        clippy::permissions_set_readonly_false,
        reason = "the test plays a writer that ignores the store's read-only mode"
    )]
    permissions.set_readonly(false);
    fs::set_permissions(&path, permissions).unwrap();
    fs::write(&path, b"thesix").unwrap();
    assert_eq!(
        writer.put(&put(b"thesis")),
        Err(PutRefusal::StoreUnavailable)
    );
    assert_eq!(writer.stored(), on_disk(&root));
    assert_eq!(writer.stored(), 16);
    assert_eq!(
        writer.put(&put(b"fifteen bytes!!")),
        Err(PutRefusal::StoreFull),
        "16 + 15 passes 30, though 0 + 15 would not"
    );
}

#[test]
#[ignore = "pending E21-62"]
fn a_store_already_past_the_cap_takes_no_new_object() {
    let (mut earlier, root) = store("past");
    earlier.put_artifact(b"0123456789ab").unwrap();
    let mut writer = ArtifactWriter::capped(&root, "ws_1", 10).unwrap();
    assert_eq!(writer.put(&put(b"")), Err(PutRefusal::StoreFull));
}

#[test]
#[ignore = "pending E21-62"]
fn a_store_that_does_not_exist_or_is_not_laid_out_never_opens_and_is_never_made() {
    let (_, root) = store("absent");
    let nowhere = root.join("nowhere");
    assert!(ArtifactWriter::open(&nowhere, "ws_1").is_err());
    assert!(!nowhere.exists(), "a wrong mount is refused, not made");
    let bare = root.join("bare");
    fs::create_dir(&bare).unwrap();
    assert!(ArtifactWriter::open(&bare, "ws_1").is_err());
    assert!(!bare.join("sha256").exists());
}

/// The log lines a server wrote, kept for the test to read.
#[derive(Default)]
struct Lines(Mutex<Vec<String>>);

impl Log for Lines {
    fn line(&self, line: &str) {
        self.0.lock().unwrap().push(line.to_owned());
    }
}

/// What answers a served put.
type Store = Box<dyn FnMut(&PutRequest) -> PutReply + Send>;

/// Serves on loopback with `store` answering puts, or with none when `store` is `None`, every call
/// refused; stopped by the returned [`Stopper`], whose loop's end the receiver reports.
fn serving(store: Option<Store>) -> (SocketAddr, Arc<Lines>, Stopper, mpsc::Receiver<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let lines = Arc::new(Lines::default());
    let server = Server::new(
        listener,
        Limits {
            io_timeout: Duration::from_secs(2),
            max_connections: 4,
        },
        Arc::clone(&lines) as Arc<dyn Log>,
    );
    let stopper = server.stopper().unwrap();
    let (end, ended) = mpsc::channel();
    thread::spawn(move || {
        let refuse = |_| Err(RequestError::ForeignWorkspace);
        match store {
            Some(store) => server.run_storing(refuse, store),
            None => server.run(refuse),
        }
        end.send(()).unwrap();
    });
    (address, lines, stopper, ended)
}

fn connect(address: SocketAddr) -> TcpStream {
    let connection = TcpStream::connect(address).unwrap();
    connection
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    connection
}

fn stop(stopper: &Stopper, ended: &mpsc::Receiver<()>) {
    stopper.stop();
    assert_eq!(ended.recv_timeout(Duration::from_secs(5)), Ok(()));
}

#[test]
#[ignore = "pending E21-62"]
fn a_put_crosses_the_service_to_the_writer_and_no_log_line_carries_its_bytes() {
    let (reader, root) = store("served");
    let mut writer = ArtifactWriter::open(&root, "ws_1").unwrap();
    let (address, lines, stopper, ended) = serving(Some(Box::new(move |put| writer.put(put))));
    let reply = frame::put(&mut connect(address), &put(CANARY.as_bytes())).unwrap();
    assert_eq!(reply, Ok(Digest::of(CANARY.as_bytes())));
    let foreign = PutRequest {
        workspace_id: CANARY.to_owned(),
        bytes: b"x".to_vec(),
    };
    assert_eq!(
        frame::put(&mut connect(address), &foreign).unwrap(),
        Err(PutRefusal::ForeignWorkspace)
    );
    stop(&stopper, &ended);
    assert_eq!(
        reader
            .read_artifact(&ArtifactRef::of(CANARY.as_bytes()))
            .unwrap(),
        CANARY.as_bytes()
    );
    let lines = lines.0.lock().unwrap().clone();
    assert!(lines.contains(&"put stored".to_owned()), "{lines:?}");
    assert!(
        lines.contains(&"put refused: foreign_workspace".to_owned()),
        "{lines:?}"
    );
    assert!(lines.iter().all(|line| !line.contains(CANARY)), "{lines:?}");
}

#[test]
#[ignore = "pending E21-62"]
fn a_server_that_stores_nothing_refuses_every_put() {
    let (address, lines, stopper, ended) = serving(None);
    assert_eq!(
        frame::put(&mut connect(address), &put(b"abc")).unwrap(),
        Err(PutRefusal::StoreUnavailable)
    );
    stop(&stopper, &ended);
    assert!(
        lines
            .0
            .lock()
            .unwrap()
            .contains(&"put refused: store_unavailable".to_owned())
    );
}
