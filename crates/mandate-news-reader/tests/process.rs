//! The `mandate-news-reader` process (E2-10, DEC-2278, DEC-2280 item 6; DEC-994 item 5): it refuses
//! a start it cannot make with a code; with `--fixture` it holds no key, so it writes its reason,
//! answers live and not ready over the probe socket, and on SIGTERM drains and exits cleanly. It
//! runs over the in-memory journal and reaches no vault and no network.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    clippy::arithmetic_side_effects,
    reason = "the tests bound their waits on a child process; no core code reads the clock"
)]

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_mandate-news-reader");
const BOUND: Duration = Duration::from_secs(10);
const ALLOWLIST: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../config/sources/allowlist-v3.toml"
);

/// A child process that is killed when the test ends, passing or not.
struct Guard(Child);

impl Drop for Guard {
    fn drop(&mut self) {
        self.0.kill().ok();
        self.0.wait().ok();
    }
}

/// The first line of `stderr`, waited for at most [`BOUND`], and the reader for the rest.
fn first_line(stderr: ChildStderr) -> (String, BufReader<ChildStderr>) {
    let (sent, received) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        reader.read_line(&mut line).ok();
        sent.send((line, reader)).ok();
    });
    received
        .recv_timeout(BOUND)
        .expect("the process wrote no line on its standard error")
}

impl Guard {
    fn exited(&mut self, rest: &mut BufReader<ChildStderr>) -> (ExitStatus, String) {
        let deadline = Instant::now() + BOUND;
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                let mut stderr = String::new();
                rest.read_to_string(&mut stderr).ok();
                return (status, stderr);
            }
            assert!(Instant::now() < deadline, "the process did not exit");
            thread::sleep(Duration::from_millis(20));
        }
    }
}

/// The process run with `args`, without `MANDATE_JOURNAL_DSN`, waited for at most [`BOUND`]: its
/// exit status and its standard error.
fn refused(args: &[String]) -> (ExitStatus, String) {
    let mut child = Guard(
        Command::new(BIN)
            .args(args)
            .env_remove("MANDATE_JOURNAL_DSN")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut stderr = BufReader::new(child.0.stderr.take().unwrap());
    child.exited(&mut stderr)
}

fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("news-reader-{}-{name}", std::process::id()))
}

fn probe(socket: &Path, word: &str) -> Option<i32> {
    Command::new(BIN)
        .args(["probe", word, "--probe"])
        .arg(socket)
        .status()
        .ok()
        .and_then(|s| s.code())
}

/// `run`'s flags for workspace `ws_1` over a fresh artifact directory, then `extra`.
fn run_args(socket: &Path, extra: &[&str]) -> Vec<String> {
    let artifacts = scratch(&format!(
        "artifacts-{}",
        extra.join("-").replace(['=', '/'], "_")
    ));
    mandate_artifacts_fs::FsArtifactStore::open(&artifacts).unwrap();
    let mut args: Vec<String> = [
        "run",
        "--workspace",
        "ws_1",
        "--allowlist",
        ALLOWLIST,
        "--user-agent",
        "Mandate tests admin@example.com",
        "--artifacts",
    ]
    .iter()
    .map(|a| (*a).to_owned())
    .collect();
    args.push(artifacts.to_string_lossy().into_owned());
    args.push("--probe".to_owned());
    args.push(socket.to_string_lossy().into_owned());
    args.extend(extra.iter().map(|a| (*a).to_owned()));
    args
}

#[test]
fn a_start_it_cannot_make_is_refused_with_its_code() {
    let none = Command::new(BIN).args(["run"]).output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&none.stderr),
        "probe_socket_missing\n"
    );
    assert_eq!(none.status.code(), Some(2));
    let socket = scratch("refused.sock");
    let unknown = Command::new(BIN)
        .args(["serve", "--probe"])
        .arg(&socket)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&unknown.stderr),
        "usage: mandate-news-reader run|probe\n"
    );
    assert_eq!(unknown.status.code(), Some(2));
    let no_workspace = Command::new(BIN)
        .args(["run", "--probe"])
        .arg(&socket)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&no_workspace.stderr),
        "news_reader_start_failed: workspace_missing\n"
    );
    assert_eq!(no_workspace.status.code(), Some(2));
    let (status, stderr) = refused(&run_args(&socket, &["--follow", "agent_a=acct_1"]));
    assert_eq!(
        stderr, "news_reader_start_failed: journal_dsn_missing\n",
        "without --fixture it runs only on the journal MANDATE_JOURNAL_DSN names"
    );
    assert_eq!(status.code(), Some(2));
    let probe_word = Command::new(BIN)
        .args(["probe", "healthz", "--probe"])
        .arg(&socket)
        .output()
        .unwrap();
    assert_eq!(
        (
            String::from_utf8_lossy(&probe_word.stderr).as_ref(),
            probe_word.status.code()
        ),
        ("", Some(2))
    );
}

/// A reader that cannot run: it writes `code` alone on its standard error, answers live and not
/// ready, and on SIGTERM drains and exits cleanly.
fn not_ready(name: &str, extra: &[&str], code: &str) {
    let socket = scratch(name);
    let mut child = Guard(
        Command::new(BIN)
            .args(run_args(&socket, extra))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let (reason, mut stderr) = first_line(child.0.stderr.take().unwrap());
    assert_eq!(reason, format!("{code}\n"), "the reason, as its code alone");
    let deadline = Instant::now() + BOUND;
    while probe(&socket, "livez") != Some(0) {
        assert!(Instant::now() < deadline, "never live");
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(probe(&socket, "readyz"), Some(1), "not ready: {code}");
    let killed = Command::new("kill")
        .args(["-TERM", &child.0.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let (status, rest) = child.exited(&mut stderr);
    assert_eq!(status.code(), Some(0));
    assert_eq!(rest, "drained\n");
    std::fs::remove_file(&socket).ok();
}

#[test]
fn with_no_key_it_says_why_answers_live_and_not_ready_and_drains_on_sigterm() {
    not_ready(
        "nokey.sock",
        &["--fixture", "--follow", "agent_a=acct_1"],
        "no_key",
    );
}

#[test]
fn with_no_follows_or_a_malformed_one_it_says_why_and_stays_not_ready() {
    not_ready("nofollow.sock", &["--fixture"], "no_follows");
    not_ready(
        "badfollow.sock",
        &[
            "--fixture",
            "--follow",
            "agent_a=acct_1",
            "--follow",
            "agent_b",
        ],
        "follow_malformed",
    );
}

/// The process run with `args` at `tier`, or with no tier set, without `MANDATE_JOURNAL_DSN`.
fn refused_at(tier: Option<&str>, args: &[String]) -> (ExitStatus, String) {
    let mut command = Command::new(BIN);
    command
        .args(args)
        .env_remove("MANDATE_JOURNAL_DSN")
        .env_remove("MANDATE_DEPLOYMENT_TIER")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if let Some(tier) = tier {
        command.env("MANDATE_DEPLOYMENT_TIER", tier);
    }
    let mut child = Guard(command.spawn().unwrap());
    let mut stderr = BufReader::new(child.0.stderr.take().unwrap());
    child.exited(&mut stderr)
}

/// DEC-2085: `--recorded` runs the reader on the recording and the fixture key at the `dev` and
/// `ci` tiers alone, with no tier set read as production; an unreadable or malformed recording
/// refuses the start with its code; and a recorded start still runs on the journal
/// `MANDATE_JOURNAL_DSN` names, never an in-memory one.
#[test]
fn a_recorded_start_runs_at_dev_and_ci_alone_on_a_readable_recording_over_the_journal() {
    let socket = scratch("recorded.sock");
    let recording = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../deploy/cell/news-reader/recorded-news.json"
    );
    let recorded = run_args(
        &socket,
        &["--recorded", recording, "--follow", "agent_a=acct_1"],
    );
    for tier in [None, Some("staging"), Some("production"), Some("demo")] {
        let (status, stderr) = refused_at(tier, &recorded);
        assert_eq!(
            stderr, "news_reader_start_failed: recorded_outside_dev_and_ci\n",
            "{tier:?}"
        );
        assert_eq!(status.code(), Some(2));
    }
    for tier in ["dev", "ci"] {
        let (status, stderr) = refused_at(Some(tier), &recorded);
        assert_eq!(
            stderr, "news_reader_start_failed: journal_dsn_missing\n",
            "{tier}: past the tier and the recording, to the journal"
        );
        assert_eq!(status.code(), Some(2));
    }
    let missing = scratch("no-recording.json");
    let missing = missing.to_string_lossy();
    let (_, stderr) = refused_at(
        Some("ci"),
        &run_args(
            &socket,
            &["--recorded", &missing, "--follow", "agent_a=acct_1"],
        ),
    );
    assert_eq!(
        stderr,
        "news_reader_start_failed: recorded_news_unreadable\n"
    );
    let malformed = scratch("malformed-recording.json");
    std::fs::write(&malformed, b"{\"news\": [{\"id\": 1}]}").unwrap();
    let (_, stderr) = refused_at(
        Some("ci"),
        &run_args(
            &socket,
            &[
                "--recorded",
                &malformed.to_string_lossy(),
                "--follow",
                "agent_a=acct_1",
            ],
        ),
    );
    assert_eq!(
        stderr,
        "news_reader_start_failed: recorded_news_malformed\n"
    );
    std::fs::remove_file(&malformed).ok();
}
