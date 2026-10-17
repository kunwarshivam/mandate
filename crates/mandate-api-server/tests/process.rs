//! The `mandate-api-server` binary: its start refusals, and its drain on SIGTERM (§3.11, DEC-978
//! items 1 to 4).

mod common;

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use common::{get, send};
use mandate_api_server::config::SESSION_KEY_FILE;

const BIN: &str = env!("CARGO_BIN_EXE_mandate-api-server");
const APP: &str = "https://app.owlhead.test";

static DIRS: AtomicU32 = AtomicU32::new(0);

fn credentials() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mandate-api-server-process-{}-{}",
        std::process::id(),
        DIRS.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&dir).expect("a directory");
    let file = dir.join(SESSION_KEY_FILE);
    fs::write(&file, [0x42_u8; 32]).expect("the key");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).expect("its mode");
    dir
}

/// The binary with a clean environment holding only what it needs, and `extra`.
fn command(args: &[&str], extra: &[(&str, &str)]) -> Command {
    let mut command = Command::new(BIN);
    command
        .args(args)
        .env_clear()
        .env("CREDENTIALS_DIRECTORY", credentials())
        .env("MANDATE_API_APP_ORIGIN", APP)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in extra {
        command.env(name, value);
    }
    command
}

const LOCAL: [&str; 4] = ["--bind", "127.0.0.1:0", "--probe-bind", "127.0.0.1:0"];

/// Runs to its exit and returns its status code and both outputs.
fn refused(mut command: Command) -> (Option<i32>, String) {
    let output = command.output().expect("the binary runs");
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.code(), text)
}

#[test]
fn a_public_bind_is_refused() {
    for args in [
        ["--bind", "0.0.0.0:8080", "--probe-bind", "127.0.0.1:0"],
        ["--bind", "127.0.0.1:0", "--probe-bind", "0.0.0.0:9090"],
        ["--bind", "[::]:8080", "--probe-bind", "127.0.0.1:0"],
    ] {
        let (code, text) = refused(command(&args, &[]));
        assert_eq!(code, Some(2), "{args:?}: {text}");
        assert!(text.contains("refused: the bind address"), "{text}");
    }
}

#[test]
fn a_pod_address_admits_only_itself() {
    let (code, text) = refused(command(
        &[
            "--bind",
            "10.42.0.8:8080",
            "--probe-bind",
            "127.0.0.1:0",
            "--pod-address",
            "10.42.0.7",
        ],
        &[],
    ));
    assert_eq!(code, Some(2), "{text}");
    assert!(text.contains("refused: the bind address"), "{text}");
}

#[test]
fn a_secret_in_the_environment_is_refused_and_never_printed() {
    let canary = "canary-41d9-never-printed";
    let (code, text) = refused(command(&LOCAL, &[("BROKER_TOKEN", canary)]));
    assert_eq!(code, Some(2), "{text}");
    assert!(text.contains("BROKER_TOKEN"), "{text}");
    assert!(!text.contains(canary), "{text}");
}

#[test]
fn a_password_in_the_database_url_is_refused_and_never_printed() {
    let (code, text) = refused(command(
        &LOCAL,
        &[(
            "MANDATE_API_DATABASE_URL",
            "postgresql://u:canary-pw-93c1@localhost/db",
        )],
    ));
    assert_eq!(code, Some(2), "{text}");
    assert!(text.contains("password"), "{text}");
    assert!(!text.contains("canary-pw-93c1"), "{text}");
}

#[test]
fn a_missing_key_or_origin_or_bind_is_refused() {
    let mut no_key = command(&LOCAL, &[]);
    no_key.env_remove("CREDENTIALS_DIRECTORY");
    let mut no_origin = command(&LOCAL, &[]);
    no_origin.env_remove("MANDATE_API_APP_ORIGIN");
    let plain_http = command(
        &LOCAL,
        &[("MANDATE_API_APP_ORIGIN", "http://app.owlhead.test")],
    );
    let with_path = command(
        &LOCAL,
        &[("MANDATE_API_APP_ORIGIN", "https://app.owlhead.test/")],
    );
    let no_probes = command(&["--bind", "127.0.0.1:0"], &[]);
    let unknown = command(
        &[
            "--bind",
            "127.0.0.1:0",
            "--probe-bind",
            "127.0.0.1:0",
            "--x",
        ],
        &[],
    );
    for command in [no_key, no_origin, plain_http, with_path, no_probes, unknown] {
        let (code, text) = refused(command);
        assert_eq!(code, Some(2), "{text}");
        assert!(text.starts_with("refused: "), "{text}");
    }
}

fn start() -> (Child, SocketAddr, SocketAddr) {
    let mut child = command(&LOCAL, &[]).spawn().expect("the binary starts");
    let stdout = child.stdout.take().expect("its stdout");
    let mut line = String::new();
    BufReader::new(stdout)
        .read_line(&mut line)
        .expect("a first line");
    let rest = line
        .trim()
        .strip_prefix("listening api=")
        .unwrap_or_else(|| panic!("unexpected first line: {line}"));
    let (api, probes) = rest.split_once(" probes=").expect("two addresses");
    (
        child,
        api.parse().expect("the API's address"),
        probes.parse().expect("the probes' address"),
    )
}

fn terminate(child: &Child) {
    let status = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .expect("kill runs");
    assert!(status.success());
}

fn wait(child: &mut Child, bound: Duration) -> Option<i32> {
    let poll = Duration::from_millis(20);
    for _ in 0..bound.as_millis() / poll.as_millis() {
        if let Some(status) = child.try_wait().expect("its status") {
            return status.code();
        }
        std::thread::sleep(poll);
    }
    panic!("still running after {bound:?}");
}

#[test]
fn it_serves_both_listeners_and_exits_0_on_sigterm() {
    let (mut child, api, probes) = start();
    assert_eq!(get(probes, "/livez").status, 200);
    assert_eq!(get(probes, "/readyz").status, 200);
    let reply = send(api, "GET", "/v1/workspaces/ws_1/agents", &[], b"");
    assert_eq!(reply.status, 401);
    assert_eq!(
        get(api, "/livez").status,
        401,
        "no probe on the API listener"
    );
    terminate(&child);
    assert_eq!(wait(&mut child, Duration::from_secs(10)), Some(0));
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("its stderr")
        .read_to_string(&mut stderr)
        .expect("read");
    assert!(stderr.is_empty(), "{stderr}");
}
