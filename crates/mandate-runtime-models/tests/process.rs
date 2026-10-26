//! The `mandate-daemon` process: it refuses without a whole deployment or a journal, its `exec`
//! probe answers ready over the probe socket, and on SIGTERM it drains and exits cleanly (E21-30
//! slice 1, DEC-1012, DEC-1013; DEC-994 item 5). It runs over the in-memory journal, so no network
//! is used; `pg.rs` runs it over Postgres.

#![allow(
    clippy::disallowed_methods,
    reason = "the tests bound their waits on a child process; no core code reads the clock"
)]

use std::ffi::OsStr;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_mandate-daemon");
const BUNDLE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../mandate-daemon/tests/fixtures/research"
);
const BOUND: Duration = Duration::from_secs(10);
const DEPLOYMENT: [&str; 8] = [
    "--workspace",
    "ws1",
    "--agent",
    "agent-a",
    "--connection",
    "conn-1",
    "--owner",
    "user-owner",
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
    fn spawn(args: &[&str], socket: &PathBuf) -> Self {
        Self(
            Command::new(BIN)
                .args(args)
                .arg("--probe")
                .arg(socket)
                .env_remove("MANDATE_JOURNAL_DSN")
                .env_remove("MANDATE_PROCESS")
                .env_remove("MANDATE_WORKSPACE")
                .env_remove("MANDATE_SUBJECT")
                .env_remove("MANDATE_ARTIFACTS")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        )
    }

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

fn socket(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("daemon-{}-{name}.sock", std::process::id()))
}

fn probe(socket: &PathBuf, word: &str) -> Option<i32> {
    Command::new(BIN)
        .args(["probe", word, "--probe"])
        .arg(socket)
        .status()
        .ok()
        .and_then(|s| s.code())
}

/// Without `--fixture` the process runs only on the journal `MANDATE_JOURNAL_DSN` names, and with
/// none it refuses rather than running on a journal of its own (DEC-1013 item 1).
#[test]
fn without_a_whole_deployment_or_a_journal_the_process_refuses() {
    let path = socket("refuse");
    let (status, stderr) = Guard::spawn(&["run", "--workspace", "ws1"], &path).exited();
    assert_eq!(status.code(), Some(2));
    assert_eq!(stderr, "deployment_missing\n");
    let mut run = vec!["run"];
    run.extend(DEPLOYMENT);
    let (status, stderr) = Guard::spawn(&run, &path).exited();
    assert_eq!(status.code(), Some(2));
    assert_eq!(stderr, "runtime_start_failed: journal_dsn_missing\n");
    assert!(!path.exists(), "a refused process serves no probe");
}

/// A pod the deployment manager starts names its runtime by variables (DEC-1015). Until the
/// connection and owner are read from the agent's mandate document, such a launch refuses before
/// it is ready, and a pod for another process is refused outright.
#[test]
fn a_manager_launch_refuses_until_its_deployment_is_resolved() {
    let path = socket("manager");
    let launch = |process: &str| {
        let mut child = Command::new(BIN)
            .args(["run", "--probe"])
            .arg(&path)
            .env_remove("MANDATE_JOURNAL_DSN")
            .env("MANDATE_WORKSPACE", "ws1")
            .env("MANDATE_PROCESS", process)
            .env("MANDATE_SUBJECT", "agent-a")
            .env("MANDATE_GENERATION", "1")
            .env_remove("MANDATE_ARTIFACTS")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.wait().unwrap();
        let mut stderr = String::new();
        child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        (child.wait().unwrap().code(), stderr)
    };
    assert_eq!(
        launch("runtime"),
        (Some(2), "deployment_unresolved\n".to_owned())
    );
    assert_eq!(
        launch("executor"),
        (Some(2), "process_not_runtime\n".to_owned())
    );
    let unreadable = Command::new(BIN)
        .args(["run", "--probe"])
        .arg(&path)
        .args(DEPLOYMENT)
        .env_remove("MANDATE_JOURNAL_DSN")
        .env_remove("MANDATE_WORKSPACE")
        .env_remove("MANDATE_SUBJECT")
        .env("MANDATE_PROCESS", OsStr::from_bytes(b"run\xfftime"))
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(unreadable.status.code(), Some(2));
    assert_eq!(
        unreadable.stderr, b"process_not_runtime\n",
        "an unreadable value is not taken as unset"
    );
    assert!(!path.exists(), "a refused process serves no probe");
}

/// A pod the deployment manager starts never runs on the in-memory journal: `--fixture` there is
/// refused before anything starts, even with the connection and owner named by flags, so the pod
/// never forgets its journal at exit (`AGENTS.md` rule 3; the executor's guard, #1754).
#[test]
fn the_fixture_journal_is_refused_on_a_managers_pod() {
    let path = socket("manager-fixture");
    let mut pod = Guard(
        Command::new(BIN)
            .args([
                "run",
                "--fixture",
                "--connection",
                "conn1",
                "--owner",
                "user-owner",
            ])
            .args(["--probe"])
            .arg(&path)
            .env_remove("MANDATE_JOURNAL_DSN")
            .env("MANDATE_WORKSPACE", "ws1")
            .env("MANDATE_PROCESS", "runtime")
            .env("MANDATE_SUBJECT", "agent-a")
            .env("MANDATE_GENERATION", "1")
            .env_remove("MANDATE_ARTIFACTS")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let (status, stderr) = pod.exited();
    assert_eq!(status.code(), Some(2));
    assert_eq!(stderr, "fixture_under_manager\n");
    assert!(!path.exists(), "a refused pod serves no probe");
}

/// Research asked for and refused stops the start before the process is ready, so asking for
/// research and getting none is never silent, and the deny-all runtime never starts in its place
/// (DEC-2215 item 2): on a deployment handed by flags, with or without `--fixture`, and on one to
/// be read from the journal whose bundle does not load, which refuses before any journal is named.
/// A bundle that loads leaves the start to its journal.
#[test]
fn a_refused_research_opt_in_stops_the_start() {
    let path = socket("research");
    let refused = |code: &str| (Some(2), format!("runtime_start_failed: {code}\n"));
    for extra in [&[][..], &["--fixture"][..]] {
        let mut run = vec!["run"];
        run.extend(DEPLOYMENT);
        run.extend(extra);
        run.extend(["--research", BUNDLE]);
        let (status, stderr) = Guard::spawn(&run, &path).exited();
        assert_eq!(
            (status.code(), stderr),
            refused("research_needs_resolved_deployment"),
            "{extra:?}"
        );
    }
    let store = std::env::temp_dir().join(format!("daemon-{}-research", std::process::id()));
    std::fs::create_dir_all(store.join("sha256")).unwrap();
    let missing = std::env::temp_dir().join(format!("daemon-{}-no-bundle", std::process::id()));
    let launch = |bundle: &Path| {
        let out = Command::new(BIN)
            .args(["run", "--research"])
            .arg(bundle)
            .arg("--probe")
            .arg(&path)
            .env_remove("MANDATE_JOURNAL_DSN")
            .env("MANDATE_WORKSPACE", "ws1")
            .env("MANDATE_PROCESS", "runtime")
            .env("MANDATE_SUBJECT", "agent-a")
            .env("MANDATE_ARTIFACTS", &store)
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        (out.status.code(), String::from_utf8(out.stderr).unwrap())
    };
    assert_eq!(launch(&missing), refused("research_bundle_unreadable"));
    assert_eq!(launch(Path::new(BUNDLE)), refused("journal_dsn_missing"));
    assert!(!path.exists(), "a refused process serves no probe");
}

/// `--research` with no bundle after it, an empty one, another flag where the bundle belongs, or
/// written `--research=` with or without a bundle, refuses before the process is ready, rather
/// than running deny-all as if research were not asked for (DEC-2215 item 2).
#[test]
fn research_asked_for_without_a_bundle_refuses() {
    let path = socket("research-missing");
    let launch = |tail: &[&str]| {
        let mut args = vec!["run"];
        args.extend(DEPLOYMENT);
        args.push("--probe");
        let out = Command::new(BIN)
            .args(&args)
            .arg(&path)
            .args(tail)
            .env_remove("MANDATE_JOURNAL_DSN")
            .env_remove("MANDATE_ARTIFACTS")
            .stdin(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        (out.status.code(), String::from_utf8(out.stderr).unwrap())
    };
    let refused = (Some(2), "research_bundle_missing\n".to_owned());
    assert_eq!(launch(&["--research"]), refused);
    assert_eq!(launch(&["--research", ""]), refused);
    assert_eq!(launch(&["--research", "--probe"]), refused);
    assert_eq!(launch(&["--research="]), refused);
    let joined = format!("--research={BUNDLE}");
    assert_eq!(launch(&[joined.as_str()]), refused);
    assert_eq!(
        launch(&["--research", BUNDLE]),
        (
            Some(2),
            "runtime_start_failed: research_needs_resolved_deployment\n".to_owned()
        )
    );
    assert!(!path.exists(), "a refused process serves no probe");
}

/// DEC-2397 item 6: `--gateway` that does not parse as `host:port`, given twice, or without
/// `--research` refuses the start `gateway_address_invalid` before the process is ready; one that
/// parses is taken without being resolved, so a name that does not resolve yet starts as far as
/// the research opt-in, as one with no `--gateway` does.
#[test]
fn a_gateway_flag_that_names_no_service_refuses_the_start() {
    let path = socket("gateway");
    let launch = |tail: &[&str]| {
        let mut args = vec!["run"];
        args.extend(DEPLOYMENT);
        args.push("--probe");
        let out = Command::new(BIN)
            .args(&args)
            .arg(&path)
            .args(tail)
            .env_remove("MANDATE_JOURNAL_DSN")
            .env_remove("MANDATE_ARTIFACTS")
            .stdin(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        (out.status.code(), String::from_utf8(out.stderr).unwrap())
    };
    let refused = (Some(2), "gateway_address_invalid\n".to_owned());
    for tail in [
        &["--research", BUNDLE, "--gateway", "nowhere"][..],
        &["--research", BUNDLE, "--gateway", "model-gateway:0"],
        &["--research", BUNDLE, "--gateway"],
        &["--research", BUNDLE, "--gateway=model-gateway:8080"],
        &["--research", BUNDLE, "--gateway", "a:1", "--gateway", "b:2"],
        &["--gateway", "model-gateway:8080"],
    ] {
        assert_eq!(launch(tail), refused, "{tail:?}");
    }
    let opted_in = (
        Some(2),
        "runtime_start_failed: research_needs_resolved_deployment\n".to_owned(),
    );
    assert_eq!(
        launch(&["--research", BUNDLE, "--gateway", "not-yet.invalid:8080"]),
        opted_in
    );
    assert_eq!(launch(&["--research", BUNDLE]), opted_in);
    assert!(!path.exists(), "a refused process serves no probe");
}

/// A launch that leaves its connection and owner to be resolved reads them from the journal and
/// the artifact store `MANDATE_ARTIFACTS` names (DEC-1016). A root that is not a store, a store with
/// no journal, a fixture run with nothing to read them from, and a launch naming only one of the two
/// each refuse before the process is ready.
#[test]
fn a_launch_that_cannot_resolve_its_deployment_refuses() {
    let path = socket("resolve");
    let store = std::env::temp_dir().join(format!("daemon-{}-store", std::process::id()));
    std::fs::create_dir_all(store.join("sha256")).unwrap();
    let not_a_store = std::env::temp_dir().join(format!("daemon-{}-empty", std::process::id()));
    std::fs::create_dir_all(&not_a_store).unwrap();
    let launch = |args: &[&str], root: &PathBuf| {
        let out = Command::new(BIN)
            .args(args)
            .arg("--probe")
            .arg(&path)
            .env_remove("MANDATE_JOURNAL_DSN")
            .env("MANDATE_WORKSPACE", "ws1")
            .env("MANDATE_PROCESS", "runtime")
            .env("MANDATE_SUBJECT", "agent-a")
            .env("MANDATE_ARTIFACTS", root)
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        (out.status.code(), String::from_utf8(out.stderr).unwrap())
    };
    let refused = |code: &str| (Some(2), format!("{code}\n"));
    assert_eq!(
        launch(&["run"], &not_a_store),
        refused("runtime_start_failed: artifact_store_unavailable")
    );
    assert!(
        !not_a_store.join("sha256").exists(),
        "a runtime never creates a store"
    );
    assert_eq!(
        launch(&["run"], &store),
        refused("runtime_start_failed: journal_dsn_missing")
    );
    assert_eq!(
        launch(&["run", "--fixture"], &store),
        refused("deployment_unresolved")
    );
    assert_eq!(
        launch(&["run", "--connection", "conn-1"], &store),
        refused("deployment_missing")
    );
    assert!(
        !store.join("tmp").exists(),
        "a runtime creates nothing in a store it reads"
    );
    assert!(!path.exists(), "a refused process serves no probe");
    std::fs::remove_dir_all(&store).ok();
    std::fs::remove_dir_all(&not_a_store).ok();
}

#[test]
fn the_ready_runtime_answers_its_probes_and_drains_on_sigterm() {
    let path = socket("drain");
    let mut run = vec!["run", "--fixture"];
    run.extend(DEPLOYMENT);
    let mut child = Guard::spawn(&run, &path);
    let deadline = Instant::now() + BOUND;
    while probe(&path, "readyz") != Some(0) {
        assert!(Instant::now() < deadline, "never ready");
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(probe(&path, "livez"), Some(0));
    let killed = Command::new("kill")
        .args(["-TERM", &child.0.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let (status, stderr) = child.exited();
    assert_eq!(status.code(), Some(0));
    assert_eq!(stderr, "drained\n");
    assert_eq!(
        probe(&path, "readyz"),
        Some(1),
        "a stopped runtime is not ready"
    );
    std::fs::remove_file(&path).ok();
}
