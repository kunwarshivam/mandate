//! The `mandate-datad` process: it stores the series before it answers ready, its `exec` probe
//! answers over the probe socket, and on SIGTERM it drains and exits cleanly (DEC-994 item 5). It
//! writes only under its store root, and the recording reaches no host.

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
/// The demo instruments' series, as the chart starts the process.
const SERIES: [&str; 2] = [
    "--instruments",
    "SPY:b28f4066-5c6d-479b-a2af-85dc1a8f16fb:2026-01-02=600,\
     QQQ:eb82ffb0-efd6-4f4e-9307-8fead28e04f6:2026-01-02=500",
];
/// The symbols [`SERIES`] names, in its order.
const SYMBOLS: [&str; 2] = ["SPY", "QQQ"];

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
            "store_or_instruments_missing\n",
        ),
        (
            [&["run", "--store", "/x"][..], &SERIES].concat(),
            "store_or_instruments_missing\n",
        ),
        (
            vec!["run", "--store", "/x", "--artifacts", "/a"],
            "store_or_instruments_missing\n",
        ),
        (
            vec![
                "run",
                "--store",
                "/x",
                "--artifacts",
                "/a",
                "--instruments",
                "SPY:b28f4066-5c6d-479b-a2af-85dc1a8f16fb:2026-01-02",
            ],
            "instruments_malformed\n",
        ),
        (
            vec![
                "run",
                "--store",
                "/x",
                "--artifacts",
                "/a",
                "--instruments",
                "SPY:a:2026-01-02=600,SPY:b:2026-01-02=600",
            ],
            "instruments_malformed\n",
        ),
    ] {
        let child = Command::new(BIN)
            .args(&args)
            .arg("--probe")
            .arg(&socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let (status, stderr) = Guard(child).exited();
        assert_eq!(status.code(), Some(2), "{args:?}");
        assert_eq!(stderr, said, "{args:?}");
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
    for symbol in SYMBOLS {
        assert!(
            !stderr.contains(symbol) && !stderr.contains(&symbol.to_lowercase()),
            "{stderr}"
        );
    }
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
    for symbol in SYMBOLS {
        for kind in ["bars-1Day", "bars-1Min"] {
            let manifest = store
                .join(RECORDED_ROOT)
                .join(format!("alpaca/iex/{kind}/{symbol}/manifest.json"));
            assert!(
                manifest.is_file(),
                "ready only once stored: {}",
                manifest.display()
            );
        }
    }
    let killed = Command::new("kill")
        .args(["-TERM", &child.0.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let (status, stderr) = child.exited();
    assert_eq!(status.code(), Some(0));
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(lines.len(), SYMBOLS.len() + 1, "{stderr}");
    let mut digests = Vec::new();
    for line in &lines[..SYMBOLS.len()] {
        let digest = line
            .strip_prefix("closes_stored sha256:")
            .expect("each instrument's closes object's digest, once");
        assert_eq!(digest.len(), 64, "a digest and nothing else: {stderr}");
        let object = dir.join("artifacts/sha256").join(&digest[..2]).join(digest);
        assert!(object.is_file(), "{}", object.display());
        digests.push(digest);
    }
    assert_ne!(digests[0], digests[1], "each instrument has its own closes");
    assert_eq!(lines[SYMBOLS.len()], "drained");
    assert_eq!(
        probe(&socket, "readyz"),
        Some(1),
        "a stopped process is not ready"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// DEC-2283: the IEX source refuses to start without its arguments, on a store that is not an
/// `alpaca-iex` root, and an unknown source is refused, each before reaching any host.
#[test]
fn the_iex_source_refuses_to_start_without_what_it_needs() {
    let probe = scratch("iex-args").join("probe.sock");
    let probe = probe.to_str().unwrap().to_owned();
    let base = |extra: &[&str]| -> Vec<String> {
        let mut args = vec!["run".to_owned(), "--probe".to_owned(), probe.clone()];
        args.extend(extra.iter().map(|a| (*a).to_owned()));
        args
    };
    let run = |args: Vec<String>| {
        let output = Command::new(BIN).args(&args).output().unwrap();
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        )
    };
    assert_eq!(
        run(base(&["--source", "alpaca-iex", "--workspace", "ws"])),
        (Some(2), "iex_arguments_missing".to_owned())
    );
    assert_eq!(
        run(base(&[
            "--source",
            "alpaca-iex",
            "--workspace",
            "ws",
            "--store",
            "/nowhere/cell-recorded",
            "--artifacts",
            "/nowhere",
            "--vault-address",
            "https://127.0.0.1:1",
            "--vault-dir",
            "/nowhere",
        ])),
        (
            Some(2),
            "data_start_failed: store_not_generations".to_owned()
        )
    );
    assert_eq!(
        run(base(&[
            "--source",
            "alpaca-iex",
            "--workspace",
            "ws",
            "--store",
            "/nowhere/alpaca-iex",
            "--artifacts",
            "/nowhere",
            "--vault-address",
            "https://127.0.0.1:1",
            "--vault-dir",
            "/nowhere",
        ])),
        (Some(2), "data_start_failed: vault_ca_unreadable".to_owned()),
        "the CA is read before anything is asked of the vault"
    );
    assert_eq!(
        run(base(&["--source", "sip"])),
        (Some(2), "source_unknown".to_owned())
    );
}
