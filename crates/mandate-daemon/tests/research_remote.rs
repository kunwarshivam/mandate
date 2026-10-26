//! Research in the runtime process with its artifacts stored through the model-gateway service
//! (DEC-2279 item 3, DEC-2208 items 5 and 6): a service that stores each put and answers its
//! reference lets the run start; one that answers another reference, answers without storing, or
//! stores bytes that do not re-hash under the reference halts research with an alert, and
//! nothing is journaled. The service is a real local socket a test thread scripts.

#![allow(
    clippy::disallowed_methods,
    reason = "the runtime's put times a real socket against the host clock"
)]

mod research_rig;

use std::fs;
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::Digest;
use mandate_daemon::RuntimeProcess;
use mandate_daemon::remote::{RemoteGateway, RemoteSink};
use mandate_gateway::frame::{MAX_REPLY_FRAME, MAX_REQUEST_FRAME, read_frame, write_frame};
use mandate_gateway::wire::{PutRefusal, PutReply, put_from, put_reply_bytes};
use mandate_journal::{ArtifactRef, ArtifactStore};
use mandate_time::UtcNanos;
use research_rig::*;

fn host_now() -> UtcNanos {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    UtcNanos::from_parts(
        i64::try_from(since.as_secs()).unwrap(),
        since.subsec_nanos(),
    )
    .unwrap()
}

/// What the scripted service does with every put.
#[derive(Clone, Copy, Debug)]
enum Script {
    Honest,
    WrongHash,
    Lies,
    Damages,
}

fn store(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "daemon-{}-research-remote-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    FsArtifactStore::open(&root).unwrap();
    root
}

/// Serves every put on a local port by `script`, writing the store at `root`.
fn serve(root: &Path, script: Script) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let mut store = FsArtifactStore::open_existing(root).unwrap();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let Ok(body) = read_frame(&mut stream, MAX_REQUEST_FRAME) else {
                continue;
            };
            let put = put_from(&body).unwrap();
            let reference = ArtifactRef::of(&put.bytes);
            let reply: PutReply = match script {
                Script::Honest => store
                    .put_artifact(&put.bytes)
                    .map(|stored| stored.digest())
                    .map_err(|_| PutRefusal::StoreUnavailable),
                Script::WrongHash => {
                    store.put_artifact(&put.bytes).unwrap();
                    Ok(Digest::of(b"another artifact"))
                }
                Script::Lies => Ok(reference.digest()),
                Script::Damages => {
                    let path = store.object_path(&reference);
                    fs::create_dir_all(path.parent().unwrap()).unwrap();
                    fs::write(&path, b"not the bytes sent").unwrap();
                    Ok(reference.digest())
                }
            };
            let _ = write_frame(&mut stream, &put_reply_bytes(&reply), MAX_REPLY_FRAME);
        }
    });
    address
}

/// The rig's agentic research with its artifacts put through `script`'s service, started and
/// stepped once at its first due run; the alerts that step raised.
fn first_pass(name: &str, script: Script) -> (Rig, Vec<&'static str>) {
    first_pass_over(&store(name), script)
}

/// As [`first_pass`], over the research store at `root` as it already is.
fn first_pass_over(root: &Path, script: Script) -> (Rig, Vec<&'static str>) {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    let mut research = wired.wiring();
    research.artifacts = Box::new(RemoteSink::new(
        RemoteGateway::new(serve(root, script), host_now),
        "ws1",
        root,
    ));
    reconciled(&rig);
    established(&rig);
    let mut process = RuntimeProcess::start(
        rig.clone(),
        wiring_with(Some(research), Box::new(RunRefs)),
        &config(),
        at(NOW),
    );
    tick(&rig, NOW + 60);
    let alerts = keys(&process.step(at(NOW + 60)));
    (rig, alerts)
}

#[test]
#[ignore = "pending E21-62"]
fn a_put_the_service_stored_and_the_runtime_read_back_lets_the_run_start() {
    let (rig, alerts) = first_pass("honest", Script::Honest);
    assert!(alerts.is_empty(), "{alerts:?}");
    assert_eq!(run_started(&rig).len(), 1, "{:?}", kinds(&rig));
}

#[test]
#[ignore = "pending E21-62"]
fn a_put_whose_answer_or_read_back_fails_halts_research_and_journals_nothing() {
    for script in [Script::WrongHash, Script::Lies, Script::Damages] {
        let (rig, alerts) = first_pass(&format!("{script:?}"), script);
        assert_eq!(alerts, ["research_halted"], "{script:?}");
        assert!(
            run_started(&rig).is_empty(),
            "{script:?}: {:?}",
            kinds(&rig)
        );
    }
}

/// Every object under the store at `root`, by its path below the store.
fn objects(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for shard in fs::read_dir(root.join("sha256")).unwrap() {
        for object in fs::read_dir(shard.unwrap().path()).unwrap() {
            found.push(
                object
                    .unwrap()
                    .path()
                    .strip_prefix(root)
                    .unwrap()
                    .to_owned(),
            );
        }
    }
    found
}

/// DEC-2279 items 2 and 3: objects squatted at the hashes of the artifacts the run will put, their
/// bytes not the artifacts', are never taken as stored. The honest service's create-only put
/// refuses them, the run's first batch is never journaled, and research halts; the squatted bytes
/// stay as they were, for an operator to find. The hashes are learned from the same run against a
/// store of its own.
#[test]
#[ignore = "pending E21-62"]
fn a_squatted_object_at_a_puts_hash_halts_research_and_journals_nothing() {
    let probe = store("squat-probe");
    let (_, alerts) = first_pass_over(&probe, Script::Honest);
    assert!(alerts.is_empty(), "{alerts:?}");
    let first = objects(&probe);
    assert!(!first.is_empty(), "the run puts artifacts");
    let squatted = store("squatted");
    for path in &first {
        let target = squatted.join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, b"squatted, not the artifact").unwrap();
    }
    let (rig, alerts) = first_pass_over(&squatted, Script::Honest);
    assert_eq!(alerts, ["research_halted"]);
    assert!(run_started(&rig).is_empty(), "{:?}", kinds(&rig));
    for path in &first {
        assert_eq!(
            fs::read(squatted.join(path)).unwrap(),
            b"squatted, not the artifact"
        );
    }
}
