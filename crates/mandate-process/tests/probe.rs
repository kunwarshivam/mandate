//! The probe answers and their Unix socket, with fixed generic words only (E21-28, DEC-994 item
//! 5; `AGENTS.md` rule 6).

use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::Duration;

use mandate_process::probe::{Answer, Probe, Status, answer, query, serve};
use mandate_process::{Exit, Input, Kind, Lifecycle, Snapshot, State};
use mandate_time::UtcNanos;

const LIVENESS: Duration = Duration::from_secs(10);

fn at(secs: i64, nanos: u32) -> UtcNanos {
    UtcNanos::from_parts(secs, nanos).unwrap_or_else(|e| panic!("time: {e:?}"))
}

fn ready_service() -> Lifecycle {
    let mut lifecycle = Lifecycle::new(Kind::Service, LIVENESS, at(100, 0));
    lifecycle.observe(at(100, 0), Input::StartupPassed);
    lifecycle
}

fn pass(word: &'static str) -> Answer {
    Answer { ok: true, word }
}

fn fail(word: &'static str) -> Answer {
    Answer { ok: false, word }
}

#[test]
fn only_a_ready_live_process_answers_ready_and_a_stalled_one_is_neither() {
    let snapshot = ready_service().snapshot();
    assert_eq!(answer(Probe::Live, snapshot, at(110, 0)), pass("live"));
    assert_eq!(answer(Probe::Ready, snapshot, at(110, 0)), pass("ready"));
    assert_eq!(answer(Probe::Live, snapshot, at(110, 1)), fail("stalled"));
    assert_eq!(
        answer(Probe::Ready, snapshot, at(110, 1)),
        fail("not_ready")
    );
    for state in [
        State::Starting,
        State::Replaying,
        State::NotReady,
        State::Draining,
    ] {
        let snapshot = Snapshot::unbounded(state);
        assert_eq!(answer(Probe::Live, snapshot, at(9_999, 0)), pass("live"));
        assert_eq!(answer(Probe::Ready, snapshot, at(0, 0)), fail("not_ready"));
    }
    let exited = Snapshot::unbounded(State::Exited(Exit::Drained));
    assert_eq!(answer(Probe::Live, exited, at(0, 0)), fail("stopped"));
    assert_eq!(answer(Probe::Ready, exited, at(0, 0)), fail("not_ready"));
    assert_eq!(
        answer(
            Probe::Ready,
            Snapshot::unbounded(State::Ready),
            at(9_999, 0)
        ),
        pass("ready")
    );
    assert_eq!(Snapshot::unbounded(State::Ready).state(), State::Ready);
}

#[test]
fn probe_words_parse_and_exit_statuses_follow_the_answer() {
    for probe in [Probe::Live, Probe::Ready] {
        assert_eq!(Probe::parse(probe.word()), Some(probe));
    }
    assert_eq!(Probe::Live.word(), "livez");
    assert_eq!(Probe::Ready.word(), "readyz");
    assert_eq!(Probe::parse("healthz"), None);
    assert_eq!(pass("live").exit_status(), 0);
    assert_eq!(fail("stalled").exit_status(), 1);
}

#[test]
fn status_hands_the_socket_the_latest_snapshot() {
    let status = Status::new(Snapshot::unbounded(State::Starting));
    let shared = status.clone();
    status.publish(Snapshot::unbounded(State::Ready));
    assert_eq!(shared.snapshot().state(), State::Ready);
}

fn socket(name: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!("mp-{}-{n}-{name}.sock", std::process::id()))
}

fn ask(path: &PathBuf, request: &[u8]) -> String {
    ask_within(path, request, Duration::from_secs(5))
}

fn ask_within(path: &PathBuf, request: &[u8], wait: Duration) -> String {
    let mut stream = UnixStream::connect(path).unwrap_or_else(|e| panic!("connect: {e}"));
    stream
        .set_read_timeout(Some(wait))
        .unwrap_or_else(|e| panic!("timeout: {e}"));
    stream
        .write_all(request)
        .unwrap_or_else(|e| panic!("write: {e}"));
    let mut reply = String::new();
    stream
        .read_to_string(&mut reply)
        .unwrap_or_else(|e| panic!("read: {e}"));
    reply
}

#[test]
fn the_socket_answers_each_probe_from_the_published_snapshot() {
    let path = socket("answers");
    let status = Status::new(ready_service().snapshot());
    serve(&path, status.clone(), || at(105, 0)).unwrap_or_else(|e| panic!("serve: {e}"));
    let asked = |probe| query(&path, probe).unwrap_or_else(|e| panic!("query: {e}"));
    assert_eq!(asked(Probe::Live), pass("live"));
    assert_eq!(asked(Probe::Ready), pass("ready"));
    status.publish(Snapshot::unbounded(State::Draining));
    assert_eq!(asked(Probe::Ready), fail("not_ready"));
    status.publish(Snapshot::unbounded(State::Exited(Exit::Fenced)));
    assert_eq!(asked(Probe::Live), fail("stopped"));
    assert_eq!(ask(&path, b"livez\n"), "fail stopped\n");
    assert_eq!(ask(&path, b"metrics\n"), "fail not_found\n");
    let _removed = fs::remove_file(&path);
}

#[test]
fn the_socket_judges_liveness_at_the_clock_it_is_given() {
    let path = socket("stalled");
    let status = Status::new(ready_service().snapshot());
    serve(&path, status, || at(111, 0)).unwrap_or_else(|e| panic!("serve: {e}"));
    assert_eq!(ask(&path, b"livez\n"), "fail stalled\n");
    assert_eq!(ask(&path, b"readyz\n"), "fail not_ready\n");
    let _removed = fs::remove_file(&path);
}

#[test]
fn a_stale_socket_is_replaced_and_any_other_file_is_refused() {
    let path = socket("stale");
    let first = Status::new(Snapshot::unbounded(State::Exited(Exit::Drained)));
    serve(&path, first, || at(0, 0)).unwrap_or_else(|e| panic!("serve: {e}"));
    let second = Status::new(Snapshot::unbounded(State::Ready));
    serve(&path, second, || at(0, 0)).unwrap_or_else(|e| panic!("serve again: {e}"));
    assert_eq!(ask(&path, b"readyz\n"), "ok ready\n");
    let _removed = fs::remove_file(&path);

    let file = socket("file");
    fs::write(&file, b"not a socket").unwrap_or_else(|e| panic!("write: {e}"));
    let refused = serve(
        &file,
        Status::new(Snapshot::unbounded(State::Ready)),
        || at(0, 0),
    );
    assert_eq!(
        refused.err().map(|e| e.kind()),
        Some(ErrorKind::AlreadyExists)
    );
    assert!(file.exists(), "the file is left as it was");
    let _removed = fs::remove_file(&file);
}

#[test]
fn a_silent_or_long_winded_client_cannot_hold_the_socket() {
    let path = socket("bounds");
    serve(
        &path,
        Status::new(Snapshot::unbounded(State::Ready)),
        || at(0, 0),
    )
    .unwrap_or_else(|e| panic!("serve: {e}"));
    let silent = UnixStream::connect(&path).unwrap_or_else(|e| panic!("connect: {e}"));
    let waiting = thread::spawn({
        let path = path.clone();
        move || ask(&path, b"readyz\n")
    });
    let reply = waiting
        .join()
        .unwrap_or_else(|_| panic!("the probe thread panicked"));
    assert_eq!(reply, "ok ready\n");
    drop(silent);
    let mut long = UnixStream::connect(&path).unwrap_or_else(|e| panic!("connect: {e}"));
    let _sent = long.write_all(&[b'x'; 4096]);
    drop(long);
    assert_eq!(ask(&path, b"readyz\n"), "ok ready\n");
    let _removed = fs::remove_file(&path);
}

#[test]
fn a_query_of_nothing_fails_rather_than_passes() {
    let path = socket("absent");
    assert!(query(&path, Probe::Ready).is_err());
}

#[test]
fn a_trickling_client_holds_only_its_own_connection() {
    let path = socket("trickle");
    serve(
        &path,
        Status::new(Snapshot::unbounded(State::Ready)),
        || at(0, 0),
    )
    .unwrap_or_else(|e| panic!("serve: {e}"));
    let mut trickle = UnixStream::connect(&path).unwrap_or_else(|e| panic!("connect: {e}"));
    trickle
        .write_all(b"r")
        .unwrap_or_else(|e| panic!("write: {e}"));
    assert_eq!(
        ask_within(&path, b"readyz\n", Duration::from_millis(500)),
        "ok ready\n",
        "a probe is answered while another client trickles"
    );
    drop(trickle);
    let _removed = fs::remove_file(&path);
}

#[test]
fn connections_over_the_bound_are_closed_unanswered_and_the_socket_is_the_owners_only() {
    let path = socket("bound");
    serve(
        &path,
        Status::new(Snapshot::unbounded(State::Ready)),
        || at(0, 0),
    )
    .unwrap_or_else(|e| panic!("serve: {e}"));
    let mode = fs::metadata(&path)
        .unwrap_or_else(|e| panic!("metadata: {e}"))
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    let silent: Vec<UnixStream> = (0..8)
        .map(|_| UnixStream::connect(&path).unwrap_or_else(|e| panic!("connect: {e}")))
        .collect();
    thread::sleep(Duration::from_millis(100));
    let mut over = UnixStream::connect(&path).unwrap_or_else(|e| panic!("connect: {e}"));
    let _sent = over.write_all(b"readyz\n");
    let mut reply = String::new();
    let _read = over.read_to_string(&mut reply);
    assert_eq!(reply, "", "a connection over the bound gets no answer");
    drop(silent);
    let answered = (0..50).any(|_| {
        thread::sleep(Duration::from_millis(20));
        ask_within(&path, b"readyz\n", Duration::from_millis(500)) == "ok ready\n"
    });
    assert!(
        answered,
        "the socket answers again once the silent clients leave"
    );
    let _removed = fs::remove_file(&path);
}
