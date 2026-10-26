//! The `mandate-datad` process: it stores the series before it answers ready, its `exec` probe
//! answers over the probe socket, and on SIGTERM it drains and exits cleanly (DEC-994 item 5). It
//! writes only under its store root and reaches no host.

#![allow(
    clippy::disallowed_methods,
    reason = "the tests bound their waits on a child process; no core code reads the clock"
)]

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use mandate_datad::series::RECORDED_ROOT;

const BIN: &str = env!("CARGO_BIN_EXE_mandate-datad");
const BOUND: Duration = Duration::from_secs(20);
/// The demo instrument's series, as the chart starts the process.
const SERIES: [&str; 6] = [
    "--symbol",
    "SPY",
    "--asset-id",
    "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415",
    "--anchor",
    "2026-01-02=600",
];

/// A child process that is killed when the test ends, passing or not.
struct Guard(Child);

impl Drop for Guard {
    fn drop(&mut self) {
        self.0.kill().ok();
        self.0.wait().ok();
    }
}

impl Guard {
    fn exited(&mut self) -> (ExitStatus, String) {
        let deadline = Instant::now() + BOUND;
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                let mut stderr = String::new();
                if let Some(mut pipe) = self.0.stderr.take() {
                    pipe.read_to_string(&mut stderr).ok();
                }
                return (status, stderr);
            }
            assert!(Instant::now() < deadline, "the process did not exit");
            thread::sleep(Duration::from_millis(20));
        }
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("datad-{}-{name}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn probe(socket: &PathBuf, word: &str) -> Option<i32> {
    Command::new(BIN)
        .args(["probe", word, "--probe"])
        .arg(socket)
        .status()
        .ok()
        .and_then(|s| s.code())
}

#[test]
fn without_its_arguments_the_process_refuses() {
    let output = Command::new(BIN).args(["run"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let dir = scratch("refuse");
    let socket = dir.join("p.sock");
    for (args, said) in [
        (
            [&["run", "--artifacts", "/a"][..], &SERIES].concat(),
            "store_symbol_or_anchor_missing\n",
        ),
        (
            [&["run", "--store", "/x"][..], &SERIES].concat(),
            "store_symbol_or_anchor_missing\n",
        ),
        (
            [
                &["run", "--store", "/x", "--artifacts", "/a"][..],
                &SERIES[..4],
                &["--anchor", "2026-01-02"],
            ]
            .concat(),
            "anchor_malformed\n",
        ),
    ] {
        let output = Command::new(BIN)
            .args(&args)
            .arg("--probe")
            .arg(&socket)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert_eq!(String::from_utf8_lossy(&output.stderr), said, "{args:?}");
    }
}

/// A store it cannot write is a failed start-up check: it exits refused, never ready, and logs a
/// code only, no path or symbol.
#[test]
fn a_store_it_cannot_write_refuses_the_start() {
    let dir = scratch("unwritable");
    let blocked = dir.join("file");
    std::fs::write(&blocked, b"not a directory").unwrap();
    let child = Command::new(BIN)
        .arg("run")
        .args(SERIES)
        .arg("--artifacts")
        .arg(dir.join("artifacts"))
        .arg("--store")
        .arg(&blocked)
        .arg("--probe")
        .arg(dir.join("p.sock"))
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let (status, stderr) = Guard(child).exited();
    assert_ne!(status.code(), Some(0));
    assert!(
        stderr.starts_with("series_not_stored: store_refused\n"),
        "{stderr}"
    );
    assert!(
        !stderr.contains("SPY") && !stderr.contains("spy"),
        "{stderr}"
    );
}

#[test]
fn the_ready_process_has_stored_the_series_and_drains_on_sigterm() {
    let dir = scratch("drain");
    let socket = dir.join("p.sock");
    let store = dir.join("store");
    let mut child = Guard(
        Command::new(BIN)
            .arg("run")
            .args(SERIES)
            .arg("--artifacts")
            .arg(dir.join("artifacts"))
            .arg("--store")
            .arg(&store)
            .arg("--probe")
            .arg(&socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + BOUND;
    while probe(&socket, "readyz") != Some(0) {
        assert!(Instant::now() < deadline, "never ready");
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(probe(&socket, "livez"), Some(0));
    let manifest = store
        .join(RECORDED_ROOT)
        .join("alpaca/iex/bars-1Day/SPY/manifest.json");
    assert!(
        manifest.is_file(),
        "ready only once stored: {}",
        manifest.display()
    );
    let killed = Command::new("kill")
        .args(["-TERM", &child.0.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let (status, stderr) = child.exited();
    assert_eq!(status.code(), Some(0));
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(lines.len(), 2, "{stderr}");
    let digest = lines[0]
        .strip_prefix("closes_stored sha256:")
        .expect("the closes object's digest, once");
    assert_eq!(digest.len(), 64, "a digest and nothing else: {stderr}");
    let object = dir.join("artifacts/sha256").join(&digest[..2]).join(digest);
    assert!(object.is_file(), "{}", object.display());
    assert_eq!(lines[1], "drained");
    assert_eq!(
        probe(&socket, "readyz"),
        Some(1),
        "a stopped process is not ready"
    );
    std::fs::remove_dir_all(&dir).ok();
}
