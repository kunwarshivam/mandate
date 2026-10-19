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
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "mandate-api-server-process-{}-{}",
        std::process::id(),
        DIRS.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = fs::remove_dir_all(&dir);
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
        .env("MANDATE_API_ISSUER", "https://issuer.test/auth/v1")
        .env("MANDATE_API_AUDIENCE", "authenticated")
        .env("MANDATE_API_JWKS_URL", "http://127.0.0.1:9/jwks")
        .env(
            "MANDATE_API_DATABASE_URL",
            "postgresql://owlhead_api@127.0.0.1:9/owlhead",
        )
        .env("MANDATE_API_WORKSPACE", "01JB3K6F0C9R2V7N4M8Q1T5W3Y")
        .env("MANDATE_API_ORG", "01JB3K6F0C9R2V7N4M8Q1T5W3X")
        .env("MANDATE_DEPLOYMENT_TIER", "ci")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in extra {
        command.env(name, value);
    }
    command
}

const LOCAL: [&str; 4] = ["--bind", "127.0.0.1:0", "--probe-bind", "127.0.0.1:0"];

/// Runs to its exit, at most ten seconds, and returns its status code and both outputs. A process
/// still running then is killed and fails the test, so a refusal that never comes fails fast.
fn refused(mut command: Command) -> (Option<i32>, String) {
    let mut child = command.spawn().expect("the binary runs");
    let code = wait(&mut child, Duration::from_secs(10));
    let mut text = String::new();
    for out in [
        child.stdout.take().map(|o| Box::new(o) as Box<dyn Read>),
        child.stderr.take().map(|e| Box::new(e) as Box<dyn Read>),
    ]
    .into_iter()
    .flatten()
    {
        let mut out = out;
        out.read_to_string(&mut text).expect("its output");
    }
    (code, text)
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

/// E21-53: the binary reads its tier itself, and in dev and CI starts with an `http` key set as
/// with an `https` one. Outside dev and CI only a release-signed build gets as far as its settings
/// (E21-35), which no test build is, so the `http` refusal at staging, production and no tier is
/// `tests/config.rs`'s `check_key_set_tier` cases, the function `main` calls.
#[test]
fn an_http_or_https_key_set_starts_in_dev_and_ci() {
    for tier in ["dev", "ci"] {
        for key_set in [
            "http://127.0.0.1:9/jwks",
            "https://issuer.test/auth/v1/.well-known/jwks.json",
        ] {
            let (mut running, api, _) = start(&[
                ("MANDATE_DEPLOYMENT_TIER", tier),
                ("MANDATE_API_JWKS_URL", key_set),
            ]);
            assert_eq!(
                get(api, "/v1/workspaces/ws_1/agents").status,
                401,
                "{tier} {key_set}"
            );
            terminate(&running.0);
            assert_eq!(
                wait(&mut running.0, Duration::from_secs(10)),
                Some(0),
                "{tier} {key_set}"
            );
        }
    }
}

/// One start of [`outside_dev_and_ci_an_unsigned_or_wrongly_signed_build_refuses_first`]: its tier
/// setting, if any, the variables it adds, and the whole of what it must print.
type StartCase<'a> = (Option<&'a str>, &'a [(&'a str, &'a str)], &'a str);

/// E21-35, DEC-971 items 2 and 3: the API server runs the release start-up check before anything
/// else, as `mandate-agentd` does. Outside dev and CI, which no tier setting means, the unsigned
/// test build refuses with the check's generic code alone and exit status 2, even with every
/// setting valid and an `https` key set, and before a secret-named variable or an `http` key set
/// could refuse it; a signature that is not cosign's is refused in every tier.
#[test]
fn outside_dev_and_ci_an_unsigned_or_wrongly_signed_build_refuses_first() {
    let garbage = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("mandate-api-server-{}.sig", std::process::id()));
    fs::write(&garbage, "not a signature").expect("the signature file");
    let garbage = garbage.display().to_string();
    let https = "https://issuer.test/auth/v1/.well-known/jwks.json";
    let cases: [StartCase; 7] = [
        (
            None,
            &[("MANDATE_API_JWKS_URL", https)],
            "release_unsigned\n",
        ),
        (
            Some("staging"),
            &[("MANDATE_API_JWKS_URL", https)],
            "release_unsigned\n",
        ),
        (
            Some("production"),
            &[("MANDATE_API_JWKS_URL", https)],
            "release_unsigned\n",
        ),
        (Some("staging"), &[], "release_unsigned\n"),
        (
            Some("production"),
            &[("BROKER_TOKEN", "canary-5e2a-never-printed")],
            "release_unsigned\n",
        ),
        (
            Some("ci"),
            &[("MANDATE_RELEASE_SIGNATURE", &garbage)],
            "release_signature_malformed\n",
        ),
        (
            Some("production"),
            &[("MANDATE_RELEASE_SIGNATURE", &garbage)],
            "release_signature_malformed\n",
        ),
    ];
    for (tier, extra, expected) in cases {
        let mut outside = command(&LOCAL, extra);
        match tier {
            Some(tier) => outside.env("MANDATE_DEPLOYMENT_TIER", tier),
            None => outside.env_remove("MANDATE_DEPLOYMENT_TIER"),
        };
        let (code, text) = refused(outside);
        assert_eq!(code, Some(2), "{tier:?} {extra:?}: {text}");
        assert_eq!(text, expected, "{tier:?} {extra:?}");
    }
    fs::remove_file(&garbage).ok();
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
    let malformed_origins = [
        "https://APP.owlhead.test",
        "https://user@app.owlhead.test",
        "https://app.owlhead.test:443",
        "https://app.owlhead.test:0",
        "https://app.owlhead.test:08443",
        "https://app.owlhead.test.",
        "https://app_owlhead.test",
        "https://",
    ]
    .map(|origin| command(&LOCAL, &[("MANDATE_API_APP_ORIGIN", origin)]));
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
    for command in [no_key, no_origin, plain_http, with_path, no_probes, unknown]
        .into_iter()
        .chain(malformed_origins)
    {
        let (code, text) = refused(command);
        assert_eq!(code, Some(2), "{text}");
        assert!(text.starts_with("refused: "), "{text}");
    }
}

/// A running binary, killed when dropped so a failing test leaks no process.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Starts the binary and reads its `listening` line, for at most ten seconds.
fn start(extra: &[(&str, &str)]) -> (Running, SocketAddr, SocketAddr) {
    start_with(&[], extra)
}

/// [`start`] with `more` arguments after the two local binds.
fn start_with(more: &[&str], extra: &[(&str, &str)]) -> (Running, SocketAddr, SocketAddr) {
    let mut args = LOCAL.to_vec();
    args.extend_from_slice(more);
    let mut child = Running(command(&args, extra).spawn().expect("the binary starts"));
    let stdout = child.0.stdout.take().expect("its stdout");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(stdout).read_line(&mut line);
        let _ = tx.send(line);
    });
    let line = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("a listening line within ten seconds");
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
    let _ = child.kill();
    let _ = child.wait();
    panic!("still running after {bound:?}");
}

#[test]
fn it_serves_both_listeners_and_exits_0_on_sigterm() {
    let (mut child, api, probes) = start(&[]);
    assert_eq!(get(probes, "/livez").status, 200);
    assert_eq!(get(probes, "/readyz").status, 200);
    let reply = send(api, "GET", "/v1/workspaces/ws_1/agents", &[], b"");
    assert_eq!(reply.status, 401);
    assert_eq!(
        get(api, "/livez").status,
        401,
        "no probe on the API listener"
    );
    terminate(&child.0);
    assert_eq!(wait(&mut child.0, Duration::from_secs(10)), Some(0));
    let mut stderr = String::new();
    child
        .0
        .stderr
        .take()
        .expect("its stderr")
        .read_to_string(&mut stderr)
        .expect("read");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn a_port_other_than_443_is_a_valid_app_origin() {
    let (mut child, _, probes) =
        start(&[("MANDATE_API_APP_ORIGIN", "https://app.owlhead.test:8443")]);
    assert_eq!(get(probes, "/livez").status, 200);
    terminate(&child.0);
    assert_eq!(wait(&mut child.0, Duration::from_secs(10)), Some(0));
}

#[test]
fn a_connection_that_never_finishes_its_request_cannot_hold_the_drain_past_its_bound() {
    let (mut child, api, _) = start_with(&["--drain-bound-s", "1"], &[]);
    let mut stalled = std::net::TcpStream::connect(api).expect("connect");
    std::io::Write::write_all(
        &mut stalled,
        b"GET /v1/workspaces/ws_1/agents HTTP/1.1\r\nHost: a",
    )
    .expect("half a request");
    std::thread::sleep(Duration::from_millis(100));
    terminate(&child.0);
    assert_eq!(wait(&mut child.0, Duration::from_secs(10)), Some(1));
    let mut stderr = String::new();
    child
        .0
        .stderr
        .take()
        .expect("its stderr")
        .read_to_string(&mut stderr)
        .expect("read");
    assert!(stderr.contains("drain bound"), "{stderr}");
    drop(stalled);
}

#[test]
fn the_drain_bound_may_only_be_shortened() {
    for bound in ["0", "91", "3600", "-1", "x"] {
        let mut args = LOCAL.to_vec();
        args.extend(["--drain-bound-s", bound]);
        let (code, text) = refused(command(&args, &[]));
        assert_eq!(code, Some(2), "{bound}: {text}");
    }
}

#[test]
fn the_internal_paper_setting_starts_only_on_a_journal_shown_paper() {
    let setting = "MANDATE_API_INTERNAL_PAPER_WORKSPACE";
    let (code, text) = refused(command(&LOCAL, &[(setting, "true")]));
    assert_eq!(
        code,
        Some(2),
        "an unreachable journal shows nothing: {text}"
    );
    assert!(
        text.contains(&format!("refused: {setting} is on")),
        "{text}"
    );
    let (code, text) = refused(command(&LOCAL, &[(setting, "yes")]));
    assert_eq!(code, Some(2), "{text}");
    assert!(text.contains(&format!("malformed: {setting}")), "{text}");
    for off in ["false", ""] {
        let (mut child, _, probes) = start(&[(setting, off)]);
        assert_eq!(get(probes, "/livez").status, 200, "{off:?}");
        terminate(&child.0);
        assert_eq!(wait(&mut child.0, Duration::from_secs(10)), Some(0));
    }
}
