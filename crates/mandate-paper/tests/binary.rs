//! The `mandate-paper` binary as the founder runs it (E7-19 slice 5, E1a-bin, DEC-846 items 1 and
//! 6): a refusal's message alone on stderr and a non-zero exit, every refusal here before any
//! credential is read, and no DSN or key text on either stream (`AGENTS.md` rule 7). The Postgres
//! test needs `MANDATE_PG_URL` and starts with a part that needs none, so the pending gate sees
//! the stub without a database (DEC-109, DEC-510 item 3). No test reaches a broker.

#[path = "../../mandate-journal/tests/common/mod.rs"]
mod common;
#[path = "../../mandate-journal/tests/conformance/mod.rs"]
#[allow(
    unused_imports,
    unused_macros,
    reason = "only `support`'s backend trait is used here; the suite runs in `mandate-journal-pg`"
)]
mod conformance;
#[path = "../../mandate-journal-pg/tests/support/mod.rs"]
mod support;

use std::process::{Command, Output};
use std::time::Duration;

use mandate_alpaca::{KEY_ID_VAR, Pause, SECRET_VAR};
use mandate_journal::{AppendOutcome, StreamId};
use mandate_journal_pg::APP_ROLE;
use mandate_paper::SystemClock;
use mandate_time::UtcNanos;
use support::{TestDb, URL_VAR};

/// Text that must never reach stdout or stderr: a DSN's password and host, and both keys.
const SECRETS: [&str; 4] = [
    "hunter2-sentinel",
    "db-sentinel.invalid",
    "PKSENTINELKEYID",
    "secret-sentinel",
];
const DSN: &str = "postgres://paper:hunter2-sentinel@db-sentinel.invalid:1/journal";
const IDS: [&str; 10] = [
    "--workspace",
    "ws1",
    "--agent",
    "agent-a",
    "--account-ref",
    "acct-a",
    "--store",
    "store",
    "--bars",
    "bars",
];
/// `ctl:ws1`'s opening in paper, by the vectors' `control_services` opener (journal spec §2).
const OPENING: &str = r#"{"actor":{"build":"sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","id":"control_services","kind":"system","version":"0.1.0"},"artifact_refs":[],"causation_id":null,"clock_source":"local","config_refs":{},"correlation_id":null,"envelope_version":1,"environment":"paper","event_id":"01J8Y0A0A000000000000000S1","event_time":"2026-09-20T13:00:00.000000000Z","event_type":"StreamOpened","payload":{"stream_type":"control","workspace_id":"ws1"},"pii_refs":[],"schema_version":1,"stream_id":"ctl:ws1"}"#;
/// `agent-a` deployed by its owner at a version whose document no store holds.
const DEPLOYED: &str = r#"{"actor":{"build":null,"id":"user_owner_01","kind":"user","version":"1"},"artifact_refs":["sha256:6666666666666666666666666666666666666666666666666666666666666666","sha256:e2b30b998595d31df51d58b142d10191d1024db476648f30dd68841e13217d43"],"causation_id":null,"clock_source":"local","config_refs":{"mandate_version":"sha256:e2b30b998595d31df51d58b142d10191d1024db476648f30dd68841e13217d43"},"correlation_id":null,"envelope_version":1,"environment":"paper","event_id":"01J8Y0A6A000000000000000S8","event_time":"2026-09-20T13:20:00.000000000Z","event_type":"AgentDeployed","payload":{"agent_id":"agent-a","mandate_version":"sha256:e2b30b998595d31df51d58b142d10191d1024db476648f30dd68841e13217d43","record_ref":"sha256:6666666666666666666666666666666666666666666666666666666666666666"},"pii_refs":[],"schema_version":1,"stream_id":"ctl:ws1"}"#;

/// The binary run with `args` after the deployment ids, in a scratch directory, with no Alpaca
/// variable but `env`.
fn paper(args: &[&str], env: &[(&str, &str)]) -> Output {
    let dir = std::env::temp_dir().join(format!("mandate-paper-bin-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_mandate-paper"));
    command.current_dir(&dir).args(IDS).args(args);
    command.env_remove(KEY_ID_VAR).env_remove(SECRET_VAR);
    command.envs(env.iter().copied());
    command.output().unwrap()
}

/// Asserts that `output` failed with exactly `message` on stderr, printed nothing, and named no
/// secret.
fn refused(output: &Output, message: &str) {
    let (stdout, stderr) = (
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(!output.status.success(), "{stderr}");
    assert_eq!(
        (stdout.as_ref(), stderr.as_ref()),
        ("", format!("{message}\n").as_str())
    );
    let named: Vec<&str> = SECRETS.into_iter().filter(|s| stderr.contains(s)).collect();
    assert!(named.is_empty(), "named {named:?}: {stderr}");
}

const KEYS: [(&str, &str); 2] = [
    (KEY_ID_VAR, "PKSENTINELKEYID"),
    (SECRET_VAR, "secret-sentinel"),
];

/// Live: whatever the run, the binary exits non-zero when it is refused, as here, with nothing.
#[test]
fn a_bare_run_fails() {
    let output = Command::new(env!("CARGO_BIN_EXE_mandate-paper"))
        .output()
        .unwrap();
    assert!(!output.status.success());
}

/// Live: the shipping clock reads this century, never a default instant.
#[test]
fn the_system_clock_reads_a_current_instant() {
    let now = SystemClock.now();
    let (from, to) = ("2026-01-01T00:00:00Z", "2100-01-01T00:00:00Z");
    let window = [from, to].map(|at| UtcNanos::parse_rfc3339(at).unwrap());
    assert!(window[0] < now && now < window[1], "{now:?}");
}

/// Live: the shipping timer waits. On a runtime whose clock is paused, tokio's clock moves only
/// when a timer is due, so it advances by the whole pause; a future that is ready at once leaves
/// it where it was.
#[test]
fn the_system_clock_pause_waits_its_whole_duration() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap();
    let duration = Duration::from_secs(90);
    let waited = runtime.block_on(async {
        let start = tokio::time::Instant::now();
        SystemClock.pause(duration).await;
        start.elapsed()
    });
    assert!(waited >= duration, "waited {waited:?} of {duration:?}");
}

/// Rule 7, DEC-846 item 6: a usage refusal names the flag, never its value, and exits non-zero.
#[test]
fn a_usage_refusal_names_no_value() {
    let output = paper(
        &["--confirm-paper", "--journal", DSN, "--journal", DSN],
        &KEYS,
    );
    refused(&output, "--journal is given twice");
}

/// FT-10, TI-5: a variable pointing Alpaca at a host is refused before the control stream is
/// read, and the refusal names the variable only, never its value or a key. The keys are set and
/// valid here, so this does not show the host check comes before the credentials; that order is
/// pinned in-process by `run.rs`'s `every_refusal_before_the_credentials_reads_none`.
#[test]
fn a_configured_host_is_refused_before_any_credential() {
    let host = ("ALPACA_API_BASE", "https://api.alpaca.markets");
    let output = paper(
        &["--confirm-paper", "--journal", DSN],
        &[KEYS[0], KEYS[1], host],
    );
    let message = "ALPACA_API_BASE looks like a URL; the tracer reaches only the Alpaca paper host";
    refused(&output, message);
}

/// FT-3, DEC-846 item 1: with no journal, or one nobody answers on, there is no control stream,
/// and the run stops before any credential: no keys are set in either run, so a credential read
/// would refuse first. The DSN appears nowhere. Then, against a real journal
/// (`MANDATE_PG_URL`), the binary reads `ctl:ws1`: an empty stream deploys nothing, and one whose
/// `AgentDeployed` names a version no store holds refuses that document, both before any
/// credential, with the keys set, and neither names the DSN's password or a key.
#[test]
fn the_control_stream_is_read_before_any_credential() {
    let unread = "the control stream could not be read";
    refused(&paper(&["--confirm-paper"], &[]), unread);
    refused(&paper(&["--confirm-paper", "--journal", DSN], &[]), unread);
    let Some(db) = TestDb::new() else {
        return;
    };
    let url = std::env::var(URL_VAR).unwrap();
    let joiner = if url.contains('?') { '&' } else { '?' };
    let options = format!("-c%20search_path%3D{}%20-c%20role%3D{APP_ROLE}", db.schema);
    let dsn = format!("{url}{joiner}options={options}");
    let password = url
        .split_once("://")
        .and_then(|(_, rest)| rest.split_once('@'));
    let password = password
        .and_then(|(user, _)| user.split_once(':'))
        .map(|(_, p)| p);
    let run = || paper(&["--confirm-paper", "--journal", &dsn], &KEYS);
    let empty = run();
    refused(
        &empty,
        "no confirmed deployment: no AgentDeployed names the agent",
    );
    let journal = db.journal();
    let stream = StreamId::parse("ctl:ws1").unwrap();
    let epoch = db.block_on(journal.take_ownership(&stream)).unwrap();
    let at = UtcNanos::from_parts(1_790_000_000, 0).unwrap();
    let drafts = [OPENING.as_bytes(), DEPLOYED.as_bytes()];
    let outcome = db.block_on(journal.append(&stream, 0, epoch, at, &drafts));
    assert!(
        matches!(outcome, Ok(AppendOutcome::Committed(_))),
        "{outcome:?}"
    );
    let deployed = run();
    let missing =
        "no confirmed deployment: the deployed version's document is not in the artifact store";
    refused(&deployed, missing);
    for output in [&empty, &deployed] {
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(password.is_none_or(|p| !stderr.contains(p)), "{stderr}");
    }
}
